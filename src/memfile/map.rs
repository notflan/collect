//! Memory mapping
use super::*;
use libc::{
    c_int,
    
    PROT_NONE,
    PROT_READ,
    PROT_WRITE,
    PROT_EXEC,
};

//TODO: Make this a `bitflags` struct.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
#[repr(i32)]
pub enum MapProtection
{
    None = PROT_NONE,
    Read = PROT_READ,
    Write = PROT_WRITE,
    Execute = PROT_EXEC,
}

impl Default for MapProtection
{
    #[inline(always)]
    fn default() -> Self
    {
	Self::None
    }
} 
