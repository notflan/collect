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

mod fd;
pub mod error;

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct RawFile(fd::RawFileDescriptor);

impl RawFile
{
    /// Get the raw fd for this raw file
    #[inline(always)] 
    pub const fn fileno(&self) -> RawFd
    {
	self.0.get()
    }

    #[inline(always)] 
    pub fn into_fileno(self) -> RawFd
    {
	self.into_raw_fd()
    }

    #[inline(always)] 
    pub unsafe fn from_fileno(fd: RawFd) -> Self
    {
	Self::from_raw_fd(fd)
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
	    libc::dup2(self.fileno(), other.as_raw_fd())
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
    
    /// Consume into a managed file
    #[inline(always)] 
    pub fn into_file(self) -> fs::File
    {
	unsafe {
	    fs::File::from_raw_fd(self.into_raw_fd())
	}
    }

    /// Attempt to open a new raw file with these options
    #[inline] 
    pub fn open(path: impl AsRef<Path>, opt: impl Borrow<fs::OpenOptions>) -> io::Result<Self>
    {
	opt.borrow().open(path).map(Into::into)
    }
}

impl io::Write for RawFile
{
    #[inline] 
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
	match unsafe {
	    libc::write(self.fileno(), buf.as_ptr() as *const _, buf.len())
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
	    libc::writev(self.fileno(), bufs.as_ptr() as *const _, bufs.len() as i32)
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
	    libc::read(self.fileno(), buf.as_mut_ptr() as *mut _, buf.len())
	} {
	    -1 =>  Err(io::Error::last_os_error()),
	    wr => Ok(wr as usize)
	}
    }
    
    #[inline] 
    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
	// SAFETY: IoSlice is guaranteed to be ABI-compatible with `struct iovec`
	match unsafe {
	    libc::readv(self.fileno(), bufs.as_mut_ptr() as *mut _, bufs.len() as i32)
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
