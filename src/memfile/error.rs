//! Errors
use super::*;
use std::{fmt, error};

/// The kind of duplicate fd syscall that was attempted
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
pub enum DuplicateKind
{
    /// A `dup()` call failed
    Duplicate,
    /// A `dup2(fd)` call failed
    Link(RawFd),
}

/// Error returned when duplicating a file descriptor fails
#[derive(Debug)]
pub struct DuplicateError {
    pub(super) from: RawFd,
    pub(super) to: DuplicateKind,
    pub(super) inner: io::Error,
}

impl DuplicateError
{
    #[inline(always)] 
    pub fn new_dup<T: ?Sized + AsRawFd>(from: &T) -> Self
    {
	Self{
	    inner: io::Error::last_os_error(),
	    from: from.as_raw_fd(),
	    to: DuplicateKind::Duplicate,
	}
    }

    #[inline(always)]
    pub fn new_dup2<T: ?Sized + AsRawFd, U: ?Sized+ AsRawFd>(from: &T, to: &U) -> Self
    {
	Self {
	    inner: io::Error::last_os_error(),
	    from: from.as_raw_fd(),
	    to: DuplicateKind::Link(to.as_raw_fd()),
	}
    }

    #[inline] 
    pub fn new<T: ?Sized + AsRawFd>(from: &T, kind: DuplicateKind, reason: impl Into<io::Error>) -> Self
    {
	Self {
	    from: from.as_raw_fd(),
	    to: kind,
	    inner: reason.into()
	}
    }

    #[inline(always)] 
    pub fn reason(&self) -> &io::Error
    {
	&self.inner
    }

    #[inline(always)] 
    pub fn kind(&self) -> &DuplicateKind
    {
	&self.to
    }

    #[inline(always)] 
    pub fn source_fileno(&self) -> RawFd
    {
	self.from
    }
}

impl fmt::Display for DuplicateKind
{
    #[inline(always)] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	match self {
	    Self::Duplicate => f.write_str("dup()"),
	    Self::Link(fd) => write!(f, "dup2({fd})"),
	}
    }
}

impl error::Error for DuplicateError
{
    #[inline] 
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
	Some(&self.inner)
    }
}

impl std::borrow::Borrow<io::Error> for DuplicateError
{
    #[inline] 
    fn borrow(&self) -> &io::Error
    {
	self.reason()
    }
}


impl fmt::Display for DuplicateError
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "failed to {} fd {}", self.to, self.from)
    }
}

