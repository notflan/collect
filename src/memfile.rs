//! Memory file handling
use super::*;
use std::os::unix::io::*;
use std::{
    mem,
    ops,
    fs,
    io,
    path::Path,
    borrow::{
	Borrow,
	BorrowMut,
    },
};

pub mod fd;
pub mod error;
mod map;

/// Flags passed to `memfd_create()` when used in this module
const MEMFD_CREATE_FLAGS: libc::c_uint = libc::MFD_CLOEXEC;

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct RawFile(fd::RawFileDescriptor);

impl RawFile
{
    /// Get the raw fd for this raw file
    #[inline(always)] 
    pub const fn fileno(&self) -> &fd::RawFileDescriptor
    {
	&self.0//.clone_const()
    }

    #[inline(always)] 
    pub fn into_fileno(self) -> fd::RawFileDescriptor
    {
	// SAFETY: We know this is safe since we are just converting the released (valid) fd from `self`
	unsafe {
	    fd::RawFileDescriptor::new_unchecked(self.into_raw_fd())
	}
    }

    #[inline(always)] 
    pub unsafe fn from_fileno(fd: fd::RawFileDescriptor) -> Self
    {
	Self::from_raw_fd(fd.get())
    }

    #[inline(always)] 
    const fn take_ownership_of_unchecked(fd: RawFd) -> Self
    {
	//! **Internal**: Non-`unsafe` and `const` version of `take_ownership_of_raw_unchecked()`
	//! : assumes `fd` is `>= 0`
	//!
	//! For use in `memfile` functions where `fd` has already been checked for validation (since `unsafe fn`s aren't first-class :/)
	unsafe {
	    Self(fd::RawFileDescriptor::new_unchecked(fd))
	}
    }

    #[inline] 
    pub fn take_ownership_of(fd: impl Into<fd::RawFileDescriptor>) -> Self
    {
	Self(fd.into())
    }

    #[inline] 
    pub fn take_ownership_of_raw(fd: impl Into<RawFd>) -> Result<Self, RawFd>
    {
	let fd = fd.into();
	Ok(Self(fd.try_into().map_err(|_| fd)?))
    }
    
    #[inline] 
    pub unsafe fn take_ownership_of_raw_unchecked(fd: impl Into<RawFd>) -> Self
    {
	Self(fd::RawFileDescriptor::new_unchecked(fd.into()))
    }

    /// Attempt to link this instance's fd to another container over an fd
    ///
    /// This is a safe wrapper around `dup2()`, as `clone()` is a safe wrapper around `dup()`.
    ///
    /// # Note
    /// If `T` is a buffered container (e.g. `std::fs::File`), make sure the buffer is flushed *before* calling this method on it, or the buffered data will be lost.
    pub fn try_link<'o, T: ?Sized>(&self, other: &'o mut T) -> Result<&'o mut T, error::DuplicateError>
    where T: AsRawFd
    {
	if unsafe {
	    libc::dup2(self.0.get(), other.as_raw_fd())
	} < 0 {
	    Err(error::DuplicateError::new_dup2(self, other))
	} else {
	    Ok(other)
	}
    }

    /// Consume a managed file into a raw file, attempting to synchronise it first.
    ///
    /// # Note
    /// This method attempts to sync the file's data.
    /// To also attempt to sync the file's metadata, set `metadata` to true.
    ///
    /// # Returns
    /// If the sync should fail, the original file is returned, along with the error from the sync.
    #[inline(always)] 
    pub fn try_from_file_synced(file: fs::File, metadata: bool) -> Result<Self, (fs::File, io::Error)>
    {
	match if metadata {
	    file.sync_all()
	} else {
	    file.sync_data()
	} {
	    Ok(()) => unsafe {
		Ok(Self::from_raw_fd(file.into_raw_fd()))
	    },
	    Err(ioe) => Err((file, ioe))
	}
    }
    
    /// Consume a managed fd type into a raw file
    #[inline(always)] 
    pub fn from_file(file: impl IntoRawFd) -> Self
    {
	unsafe {
	    Self::from_raw_fd(file.into_raw_fd())
	}
    }
    
    /// Consume into another managed file type container
    #[inline(always)] 
    pub fn into_file<T: FromRawFd>(self) -> T
    {
	unsafe {
	    T::from_raw_fd(self.into_raw_fd())
	}
    }

    /// Attempt to open a new raw file with these options
    #[inline] 
    pub fn open(path: impl AsRef<Path>, opt: impl Borrow<fs::OpenOptions>) -> io::Result<Self>
    {
	opt.borrow().open(path).map(Into::into)
    }

    /// Open a new in-memory (W+R) file with an optional name and a fixed size.
    pub fn open_mem(name: Option<&str>, len: usize) -> Result<Self, error::MemfileError>
    {
	lazy_static! {
	    static ref DEFAULT_NAME: String = format!(concat!("<memfile@", file!(), "->", "{}", ":", line!(), "-", column!(), ">"), function!()); //TODO: If it turns out memfd_create() requires an `&'static str`; remove the use of stackalloc, and have this variable be a nul-terminated CString instead.
	}

	use libc::{
	    memfd_create,
	    fallocate,
	};
	use error::MemfileCreationStep::*;

	let rname = name.unwrap_or(&DEFAULT_NAME);
	
	stackalloc::alloca_zeroed(rname.len()+1, move |bname| { //XXX: Isn't the whole point of making `name` `&'static` that I don't know if `memfd_create()` requires static-lifetime name strings? TODO: Check this
	    macro_rules! attempt_call
	    {
		($errcon:literal, $expr:expr, $step:expr) => {
		    match unsafe {
			$expr
		    } {
			$errcon => Err($step),
			x => Ok(x)
		    }
		}
	    }

	    let bname = {
		unsafe {
		    std::ptr::copy_nonoverlapping(rname.as_ptr(), bname.as_mut_ptr(), rname.len());
		}
		debug_assert_eq!(bname[rname.len()], 0, "Copied name string not null-terminated?");
		bname.as_ptr()
	    };

	    let fd = attempt_call!(-1, memfd_create(bname as *const _, MEMFD_CREATE_FLAGS), Create(name.map(str::to_owned), MEMFD_CREATE_FLAGS))
		.map(Self::take_ownership_of_unchecked)?; // Ensures `fd` is dropped if any subsequent calls fail
	    
	    attempt_call!(-1
			  , fallocate(fd.0.get(), 0, 0, len.try_into()
				      .map_err(|_| Allocate(fd.fileno().clone(), len))?)
			  , Allocate(fd.fileno().clone(), len))?;

	    Ok(fd)
		
	})
    }
}



impl io::Write for RawFile
{
    #[inline] 
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
	match unsafe {
	    libc::write(self.0.get(), buf.as_ptr() as *const _, buf.len())
	}  {
	    -1 =>  Err(io::Error::last_os_error()),
	    wr => Ok(wr as usize)
	}
    }
    #[inline] 
    fn flush(&mut self) -> io::Result<()> {
	// Not buffered
	Ok(())
    }

    #[inline] 
    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
	// SAFETY: IoSlice is guaranteed to be ABI-compatible with `struct iovec`
	match unsafe {
	    libc::writev(self.0.get(), bufs.as_ptr() as *const _, bufs.len() as i32)
	} {
	    -1 =>  Err(io::Error::last_os_error()),
	    wr => Ok(wr as usize)
	}
    }
}

impl io::Read for RawFile
{
    #[inline] 
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
	match unsafe {
	    libc::read(self.0.get(), buf.as_mut_ptr() as *mut _, buf.len())
	} {
	    -1 =>  Err(io::Error::last_os_error()),
	    wr => Ok(wr as usize)
	}
    }
    
    #[inline] 
    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
	// SAFETY: IoSlice is guaranteed to be ABI-compatible with `struct iovec`
	match unsafe {
	    libc::readv(self.0.get(), bufs.as_mut_ptr() as *mut _, bufs.len() as i32)
	} {
	    -1 =>  Err(io::Error::last_os_error()),
	    wr => Ok(wr as usize)
	}
    }
}

impl From<fs::File> for RawFile
{
    #[inline] 
    fn from(from: fs::File) -> Self
    {
	Self::from_file(from)
    }
}

impl From<RawFile> for fs::File
{
    #[inline] 
    fn from(from: RawFile) -> Self
    {
	from.into_file()
    }
}


impl Clone for RawFile
{
    #[inline] 
    fn clone(&self) -> Self {
	unsafe { Self::from_raw_fd(libc::dup(self.0.get())) }
    }
}
impl ops::Drop for RawFile
{
    #[inline] 
    fn drop(&mut self) {
	unsafe {
	    libc::close(self.0.get());
	}
    }
}

impl AsRawFd for RawFile
{
    #[inline] 
    fn as_raw_fd(&self) -> RawFd {
	self.0.get()
    }
}

impl FromRawFd for RawFile
{
    #[inline] 
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
	Self(fd::RawFileDescriptor::new(fd))
    }
}

impl IntoRawFd for RawFile
{
    #[inline] 
    fn into_raw_fd(self) -> RawFd {
	let fd = self.0.get();
	mem::forget(self); // prevent close
	fd
    }
}
