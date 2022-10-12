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
#[cfg(feature="hugetlb")] 
mod hp;


/// Flags passed to `memfd_create()` when used in this module
const MEMFD_CREATE_FLAGS: libc::c_uint = libc::MFD_CLOEXEC;

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct RawFile(fd::RawFileDescriptor);

/// Attempt to get the length of a stream's file descriptor
#[inline]
#[cfg_attr(feature="logging", instrument(level="debug", err, skip_all, fields(from_fd = from.as_raw_fd())))]
pub fn stream_len(from: &(impl AsRawFd + ?Sized)) -> io::Result<u64>
{
    let mut stat = std::mem::MaybeUninit::uninit();
    match unsafe { libc::fstat(from.as_raw_fd(), stat.as_mut_ptr()) } {
	-1 => Err(io::Error::last_os_error()),
	_ => {
	    let stat = unsafe { stat.assume_init() };
	    debug_assert!(stat.st_size >= 0, "bad stat size");
	    Ok(stat.st_size as u64)
	},
    }
}

/// Create an in-memory `File`, with an optional name
#[cfg_attr(feature="logging", instrument(level="info", err))]
pub fn create_memfile(name: Option<&str>, size: usize) -> eyre::Result<fs::File>
{
    if_trace!(debug!("Attempting to allocate {size} bytes of contiguous physical memory for memory file named {:?}", name.unwrap_or("<unbound>")));
    RawFile::open_mem(name, size).map(Into::into)
	.wrap_err(eyre!("Failed to open in-memory file")
		  .with_section(move || format!("{:?}", name).header("Proposed name"))
		  .with_section(|| size.header("Requested physical memory buffer size")))
}

impl Clone for RawFile
{
    #[inline]
    #[cfg_attr(feature="logging", instrument(skip_all))]
    fn clone(&self) -> Self {
	self.try_clone().expect("failed to duplicate raw fd")
    }

    #[inline]
    fn clone_from(&mut self, source: &Self)
    {
	if (!cfg!(debug_assertions)) || !std::ptr::eq(self, source) {
	    #[cfg(feature="logging")]
	    let span = trace_span!("clone_from()", self = ?self, source= ?source);
	    #[cfg(feature="logging")]
	    let _span = span.enter();
	    
	    self.try_link_from(source).expect("failed to duplicate raw fd into self");
	} else {
	    #[cfg(feature="logging")]
	    let span = trace_span!("clone_from()", self = ?format!("0x{:x}", self as *mut _ as usize), source = ?format!("0x{:x}", source as *const _ as usize));
	    #[cfg(feature="logging")]
	    let _span = span.enter();
	    
	    if_trace!(error!("`self` and `source` are the same variable. This should never happen!"));
	    #[cfg(not(feature="logging"))] 
	    panic!("Mutable reference and shared reference point to the same location in memory")
	}
    }
}

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
    pub(crate) const fn take_ownership_of_unchecked(fd: RawFd) -> Self
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
    /// If `T` is a buffered container (e.g. `std::io::BufWriter<T: AsRawFd>`), make sure the buffer is flushed *before* calling this method on it, or the buffered data will be lost.
    #[cfg_attr(feature="logging", instrument(err, skip(other), fields(other = ?other.as_raw_fd())))]
    pub fn try_link_to<'o, T: ?Sized>(&self, other: &'o mut T) -> Result<&'o mut T, error::DuplicateError>
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

    /// Attempt to link `other`'s contained file descriptor to this instance's fd.
    ///
    /// This is a safe wrapper around `dup2()`, and an analogue of `try_link_to()`.
    ///
    /// # Note
    /// After this call succeeds, writing to `self` will have the same effect of writing directly to `other`'s contained file descriptor. If `other` is a buffered stream, you must ensure that `other` has been flushed *before* writing anything to `self`.
    #[cfg_attr(feature="logging", instrument(err, skip(other), fields(other = ?other.as_raw_fd())))]
    pub fn try_link_from<'i, T: ?Sized>(&mut self, other: &'i T) -> Result<&'i T, error::DuplicateError>
    where T: AsRawFd
    {
	if unsafe {
	    libc::dup2(other.as_raw_fd(), self.0.get())
	} < 0 {
	    Err(error::DuplicateError::new_dup2(other, self))
	} else {
	    Ok(other)
	}
    }

    /// Link `other`'s contained file descriptor to this instance's fd.
    ///
    /// # Panics
    /// If the call to `dup2()` fails.
    ///
    /// # Note
    /// This is a panicking version of `try_link_from()`. See that function for more information on how to safely use `self` after this call.
    #[inline]
	#[cfg_attr(feature="logging", instrument(skip_all))]
    pub fn link_from<'i, T: ?Sized>(&mut self, other: &'i T) -> &'i T
    where T: AsRawFd
    {
	self.try_link_from(other).expect("failed to duplicate file descriptor from another container")
    }
    
    /// Attempt to link this instance's fd to another container over an fd
    ///
    /// # Panics
    /// If the call to `dup2()` fails.
    ///
    /// # Note
    /// This is a panicking version of `try_link_to()`. See that function for more information on how to safely use `self` after this call.
    #[inline]
	#[cfg_attr(feature="logging", instrument(skip_all))]
    pub fn link_to<'o, T: ?Sized>(&self, other: &'o mut T) -> &'o mut T
    where T: AsRawFd
    {
	self.try_link_to(other).expect("failed to duplicate file descriptor into another container")
    }
    
    /// Attempt to duplicate this raw file
    #[cfg_attr(feature="logging", instrument(err))]
    pub fn try_clone(&self) -> Result<Self, error::DuplicateError>
    {
	match unsafe { libc::dup(self.0.get()) }
	{
	    -1 => Err(error::DuplicateError::new_dup(self)),
	    fd => Ok(Self::take_ownership_of_unchecked(fd))
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
    #[cfg_attr(feature="logging", instrument(level="debug"))]
    pub fn try_from_file_synced(file: fs::File, metadata: bool) -> Result<Self, (fs::File, io::Error)>
    {
	if_trace!(trace!("syncing file data"));
	match if metadata {
	    file.sync_all()
	} else {
	    file.sync_data()
	} {
	    Ok(()) => unsafe {
		if_trace!(debug!("sync succeeded, consumeing fd"));
		Ok(Self::from_raw_fd(file.into_raw_fd()))
	    },
	    Err(ioe) => {
		if_trace!({
		    #[cfg(feature="logging")]
		    let span = warn_span!("failed_path", file = ?file, error = ?ioe);
		    #[cfg(feature="logging")]
		    let _spen = span.enter();
		    error!("sync failed: {ioe}")
		});
		Err((file, ioe))
	    },
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

    /// Allocates `size` bytes for this file.
    ///
    /// # Note
    /// This does not *extend* the file's capacity, it is instead similar to `fs::File::set_len()`.
    #[cfg_attr(feature="logging", instrument(err))]
    #[inline] 
    pub fn allocate_size(&mut self, size: u64) -> io::Result<()>
    {
	use libc::{ fallocate, off_t};
	if_trace!(trace!("attempting fallocate({}, 0, 0, {size}) (max offset: {})", self.0.get(), off_t::MAX));
	match unsafe { fallocate(self.0.get(), 0, 0, if cfg!(debug_assertions) {
	    size.try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Offset larger than max offset size"))?
	} else { size as off_t }) } { //XXX is this biteise AND check needed? fallocate() should already error if the size is negative with these parameters, no?
	    -1 => Err(io::Error::last_os_error()),
	    _ => Ok(())
	}
    }

    /// Sets the size of this file.
    ///
    /// The only real difference is that this will work on a `hugetlbfs` file, whereas `allocate_size()` will not.
    /// # Note
    /// This is essentially `fs::File::set_len()`.
    #[cfg_attr(feature="logging", instrument(err))]
    #[inline] 
    pub fn truncate_size(&mut self, size: u64) -> io::Result<()>
    {
	use libc::{ ftruncate, off_t};
	if_trace!(trace!("attempting ftruncate({}, {size}) (max offset: {})", self.0.get(), off_t::MAX));
	match unsafe { ftruncate(self.0.get(), if cfg!(debug_assertions) {
	    size.try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Offset larger than max offset size"))?
	} else { size as off_t }) } {
	    -1 => Err(io::Error::last_os_error()),
	    _ => Ok(())
	}
    }

    /// Open a new in-memory (W+R) file with an optional name and a fixed size.
    #[cfg_attr(feature="logging", instrument(level="debug", skip_all, err))]
    pub fn open_mem(name: Option<&str>, len: usize) -> Result<Self, error::MemfileError>
    {
	use std::{
	    ffi::CString,
	    borrow::Cow,
	};
	lazy_static! {
	    static ref DEFAULT_NAME: CString = CString::new(format!(concat!("<memfile@", file!(), "->", "{}", ":", line!(), "-", column!(), ">"), function!())).unwrap();
	}

	use libc::{
	    memfd_create,
	    fallocate,
	};
	use error::MemfileCreationStep::*;

	let bname: Cow<CString> = match name {
	    Some(s) => Cow::Owned(CString::new(Vec::from(s)).expect("Invalid name")),
	    None => Cow::Borrowed(&DEFAULT_NAME),
	};

	let bname = bname.as_bytes_with_nul();
	if_trace!(trace!("created nul-terminated buffer for name `{:?}': ({})", std::str::from_utf8(bname), bname.len()));
	
	macro_rules! attempt_call
	{
	    ($errcon:literal, $expr:expr, $step:expr) => {
		//if_trace!(debug!("attempting systemcall"));
		match unsafe {
		    $expr
		} {
		    $errcon => {
			if_trace!(warn!("systemcall failed: {}", error::raw_errno()));
			Err($step)
		    },
		    x => Ok(x)
		}
	    }
	}
	
	let fd = attempt_call!(-1, memfd_create(bname.as_ptr() as *const _, MEMFD_CREATE_FLAGS), Create(name.map(str::to_owned), MEMFD_CREATE_FLAGS))
	    .map(Self::take_ownership_of_unchecked)?; // Ensures `fd` is dropped if any subsequent calls fail

	#[cfg(feature="logging")] 
	let using_memfile = debug_span!("setup_memfd", fd = ?fd.0.get());
	{
	    #[cfg(feature="logging")]
	    let _span = using_memfile.enter();
	    
	    if len > 0 {
		attempt_call!(-1
			      , fallocate(fd.0.get(), 0, 0, len.try_into()
					  .map_err(|_| Allocate(None, len))?)
			      , Allocate(Some(fd.fileno().clone()), len))?;
		if cfg!(debug_assertions) {
		    if_trace!(trace!("Allocated {len} bytes to memory buffer"));
		    let seeked;
		    assert_eq!(attempt_call!(-1
					     , { seeked = libc::lseek(fd.0.get(), 0, libc::SEEK_CUR); seeked }
					     , io::Error::last_os_error())
			       .expect("Failed to check seek position in fd")
			       , 0, "memfd seek position is non-zero after fallocate()");
		    if_trace!(if seeked != 0 { warn!("Seek offset is non-zero: {seeked}") } else { trace!("Seek offset verified ok") });
		}
	    } else {
		if_trace!(trace!("No length provided, skipping fallocate() call"));
	    }
	}
	Ok(fd)
	    
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

#[cfg(test)]
mod tests
{
    use super::*;
    #[test]
    fn memory_mapping() -> eyre::Result<()>
    {
	use std::io::*;
	const STRING: &[u8] = b"Hello world!";
	let mut file = {
	    let mut file = RawFile::open_mem(None, 4096)?;
	    file.write_all(STRING)?;
	    let mut file = fs::File::from(file);
	    file.seek(SeekFrom::Start(0))?;
	    file
	};
	let v = {
	    let mut buf = vec![0; STRING.len()];
	    file.read_exact(&mut buf[..])?;
	    buf
	};

	assert_eq!(v.len(), STRING.len(), "Invalid read size.");
	assert_eq!(&v[..], &STRING[..], "Invalid read data.");
	Ok(())
    }
}
