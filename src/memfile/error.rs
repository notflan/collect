//! Errors
use super::*;
use std::{fmt, error};

pub(super) fn raw_errno() -> libc::c_int
{
    unsafe { *libc::__errno_location() }
}

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
    #[inline] 
    pub fn new_dup<T: ?Sized + AsRawFd>(from: &T) -> Self
    {
	Self{
	    inner: io::Error::last_os_error(),
	    from: from.as_raw_fd(),
	    to: DuplicateKind::Duplicate,
	}
    }

    #[inline]
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


#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MemfileCreationStep
{
    /// `memfd_create()` call
    Create(Option<String>, libc::c_uint),
    /// `fallocate()` call
    Allocate(Option<fd::RawFileDescriptor>, usize),
    /// `mmap()` call
    Map {
	addr: usize,
	size: usize,
	prot: map::MapProtection,
	flags: libc::c_int,
	fd: Option<fd::RawFileDescriptor>,
	offset: libc::off_t,
    },
}

#[derive(Debug)]
pub struct MemfileError
{
    step: MemfileCreationStep,
    inner: io::Error,
}

impl fmt::Display for MemfileCreationStep
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	match self {
	    Self::Create(None, 0 | MEMFD_CREATE_FLAGS) => f.write_str("memfd_create()"),
	    Self::Create(None, flags) => write!(f, "memfd_create(<unbound>, {flags})"),
	    Self::Create(Some(name), flag) => write!(f, "memfd_create({name}, {flag})"),
	    Self::Allocate(None, size) => write!(f, "checked_cast<off_t>({size})"),
	    Self::Allocate(Some(fd), size) => write!(f, "fallocate({fd}, 0, 0, {size})"),
	    Self::Map{ addr: 0, size, prot, flags, fd: Some(fd), offset } => write!(f, "mmap(NULL, {size}, {prot:?}, {flags}, {fd}, {offset})"),
	    Self::Map{ addr: 0, size, prot, flags, fd: None, offset } => write!(f, "mmap(NULL, {size}, {prot:?}, {flags}, -1, {offset})"),
	    Self::Map{ addr, size, prot, flags, fd: Some(fd), offset } => write!(f, "mmap(0x{addr:x}, {size}, {prot:?}, {flags}, {fd}, {offset})"),
	    Self::Map{ addr, size, prot, flags, fd: None, offset } => write!(f, "mmap(0x{addr:x}, {size}, {prot:?}, {flags}, -1, {offset})"),
	}
    }
}

impl error::Error for MemfileError
{
    #[inline] 
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
	Some(&self.inner)
    }
}
impl fmt::Display for MemfileError
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "failed to create in-memory file: `{}` failed", self.step)
    }
}

impl MemfileError
{
    #[inline] 
    pub fn from_step(step: MemfileCreationStep) -> Self
    {
	Self {
	    step,
	    inner: io::Error::last_os_error()
	}
    }
}

impl From<MemfileCreationStep> for MemfileError
{
    #[inline] 
    fn from(from: MemfileCreationStep) -> Self
    {
	Self::from_step(from)
    }
}

