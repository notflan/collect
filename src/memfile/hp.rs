//! # Huge pages
//! Calculates available huge page sizes, and creates `memfd_create()` flag masks for a `MFD_HUGETLB` fd.
use super::*;
use std::{
    path::Path,
};
use libc::{
    MFD_HUGETLB,
    MAP_HUGE_SHIFT,
};

/// The location in the kernel's API which shows all valid huge-page sizes.
///
/// This is a directory, and its subdirectories' *names* will contain the size in this format: `hugepages-(\d+)kB` (where the first capture-group is the size in kB.)
/// The contents of those subdirectories themselves are irrelevent for our purpose.
pub const HUGEPAGE_SIZES_LOCATION: &'static str = "/sys/kernel/mm/hugepages";

/// Take a directory path and try to parse the hugepage size from it.
///
/// All subdirectories from `HUGEPAGE_SIZES_LOCATION` should be passed to this, and the correct system-valid hugepage size will be returned for that specific hugepage.
fn find_size_bytes(path: impl AsRef<Path>) -> Option<usize>
{
    let path= path.as_ref();
    if !path.is_dir() {
	return None;
    } 

    let dir_name = path.file_name()?;
    // location of the `-` in the dir name
    let split_loc = memchr::memchr(b'-', dir_name.as_bytes())?;

    //TODO: find the `k` (XXX: Is it always in kB? Or do we have to find the last non-digit byte instead?) For now, we can just memchr('k') I think -- look into kernel spec for this later.
    
    None
}
