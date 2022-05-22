//! Managing raw `fd`s
use super::*;
use std::num::NonZeroU32;
use libc::{
    c_int,
};
use std::{
    fmt, error
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
#[repr(transparent)]
struct NonNegativeI32(NonZeroU32);

impl NonNegativeI32
{
    pub const MASK: u32 = c_int::MIN as u32; //0b10000000_00000000_00000000_00000000;

    #[inline(always)] 
    pub const fn new(from: i32) -> Option<Self>
    {
	if from < 0 {
	    None
	} else {
	    Some(unsafe {
		Self::new_unchecked(from)
	    })
	}
    }

    #[inline(always)] 
    pub const unsafe fn new_unchecked(from: i32) -> Self
    {
	Self(NonZeroU32::new_unchecked( (from as u32) | Self::MASK ))
    }
    
    #[inline(always)]
    pub const fn get(self) -> i32
    {
	(self.0.get() & (!Self::MASK)) as i32
    }
}

impl PartialEq<i32> for NonNegativeI32
{
    #[inline] 
    fn eq(&self, other: &i32) -> bool
    {
	self.get() == *other
    }
}

impl PartialOrd<i32> for NonNegativeI32
{
    #[inline] 
    fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
	self.get().partial_cmp(other)
    }
}

impl Default for NonNegativeI32
{
    #[inline(always)]
    fn default() -> Self
    {
	unsafe {
	    Self::new_unchecked(0)
	}
    }
}

impl From<NonNegativeI32> for i32
{
    #[inline(always)] 
    fn from(from: NonNegativeI32) -> Self
    {
	from.get()
    }
}

impl TryFrom<i32> for NonNegativeI32
{
    type Error = std::num::TryFromIntError;

    #[inline(always)] 
    fn try_from(from: i32) -> Result<Self, Self::Error>
    {
	NonZeroU32::try_from((!from as u32) & Self::MASK)?;
	debug_assert!(from >= 0, "Bad check");
	unsafe {
	    Ok(Self::new_unchecked(from))
	}
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BadFDError(());

impl error::Error for BadFDError{} 
impl fmt::Display for BadFDError
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	f.write_str("invalid file descriptor")
    }
}


pub type FileNo = RawFd;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct RawFileDescriptor(NonNegativeI32);

impl fmt::Debug for RawFileDescriptor
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "RawFileDescriptor({})", self.0.get())
    }
}

impl RawFileDescriptor
{
    pub const STDIN: Self = Self(unsafe { NonNegativeI32::new_unchecked(0) });
    pub const STDOUT: Self = Self(unsafe { NonNegativeI32::new_unchecked(1) });
    pub const STDERR: Self = Self(unsafe { NonNegativeI32::new_unchecked(2) });
    
    #[inline(always)] 
    pub fn try_new(fd: FileNo) -> Result<Self, BadFDError>
    {
	NonNegativeI32::new(fd).ok_or(BadFDError(())).map(Self)
    }

    #[inline] 
    pub fn new(fd: FileNo) -> Self
    {
	Self::try_new(fd).expect("Invalid fileno")
    }

    #[inline(always)] 
    pub const unsafe fn new_unchecked(fd: FileNo) -> Self
    {
	Self(NonNegativeI32::new_unchecked(fd))
    }

    #[inline(always)] 
    pub const fn get(&self) -> FileNo
    {
	self.0.get()
    }

    #[inline(always)] 
    pub(super) const fn clone_const(&self) -> Self
    {
	//! **Internal**: `clone()` but useable in `memfile`-local `const fn`s
	//! : since this type is essentially a `Copy` type, but without implicit copying.
	Self(self.0)
    }
}

impl fmt::Display for RawFileDescriptor
{
    #[inline(always)] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "{}", self.get())
    }
}


impl PartialEq<FileNo> for RawFileDescriptor
{
    #[inline] 
    fn eq(&self, other: &FileNo) -> bool
    {
	self.get() == *other
    }
}

impl PartialOrd<FileNo> for RawFileDescriptor
{
    #[inline] 
    fn partial_cmp(&self, other: &FileNo) -> Option<std::cmp::Ordering> {
	self.get().partial_cmp(other)
    }
}

impl From<NonNegativeI32> for RawFileDescriptor
{
    #[inline(always)] 
    fn from(from: NonNegativeI32) -> Self
    {
	Self(from)
    }
}

impl TryFrom<FileNo> for RawFileDescriptor
{
    type Error = BadFDError;

    #[inline(always)] 
    fn try_from(from: FileNo) -> Result<Self, Self::Error>
    {
	Self::try_new(from)
    }
}

impl From<RawFileDescriptor> for FileNo
{
    #[inline(always)] 
    fn from(from: RawFileDescriptor) -> Self
    {
	from.get()
    }
}

impl AsRawFd for RawFileDescriptor
{
    fn as_raw_fd(&self) -> RawFd {
	self.get()
    }
}
