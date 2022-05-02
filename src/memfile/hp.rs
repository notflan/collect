//! # Huge pages
//! Calculates available huge page sizes, and creates `memfd_create()` flag masks for a `MFD_HUGETLB` fd.
//!
//! ## Method of calculating HUGETLB masks
//! * Enumerate and find the numbers matching on the subdirectories in dir `/sys/kernel/mm/hugepages/hugepages-(\d+)kB/`
//! * Multiply that number by `1024`.
//! * Calculate the base-2 logorithm (`log2()`) of the resulting number.
//! * Left shift that number by `MAP_HUGE_SHIFT`. This produces a valid `MAP_HUGE_...`-useable flag the same as the builtin `sys/mman.h` flags (`MAP_HUGE_2MB`, `MAP_HUGE_1GB`, etc..)
//! * The resulting number is a valid flag to pass to `memfd_create()` if it is `|`d with `MFD_HUGETLB`
//!
//! All `MAP_HUGE_...` flags must be bitwise OR'd with that flag in `memfd_create()`, however other memory handling syscalls that support hugepages will also accept the constructed `MAP_HUGE_...` flag as valid as per their own specification.

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
//TODO: Maybe make this `Result` instead? So we can know what part of the lookup is failing?
#[cfg_attr(feature="logging", instrument(ret, skip_all, fields(path = ?path.as_ref())))]
fn find_size_bytes(path: impl AsRef<Path>) -> Option<usize>
{
    const KMAP_TAGS: &[u8] = b"kmgbB"; //"bB";
    const KMAP_SIZES: &[usize] = &[1024, 1024*1024, 1024*1024*1024, 0, 0]; // Having `b` and `B` (i.e. single bytes) be 0 means `sz` will be passed unmodified: the default multiplier is 1 and never 0. Making these two values 0 instead of 1 saves a single useless `* 1` call, but still allows for them to be looked up in `dir_bytes` when finding `k_loc` below.

    /// Lookup the correct multiplier for `sz` to get the number of individual bytes from the IEC bytes-suffix `chr`.
    /// Then, return the number of individual bytes of `sz` multiplied by the appropriate number dictated by the IEC suffix `chr` (if there is one.)
    ///
    /// The lookup table is generated at compile-time from the constants `KMAP_TAGS` and `KMAP_SIZES`.
    /// The indecies of `KMAP_TAGS` of `KMAP_SIZES` should should correspond the suffix to the multiplier.
    /// If a suffix is not found, a default multipler of `1` is used (i.e. `sz` is returned un-multipled.)
    ///
    /// # Examples
    ///  * Where `sz = 10`, and `chr = b'k'` -> `10 * 1024` -> `10240` bytes (10 kB)
    ///  * Where `sz = 100`, and `chr = b'B'` -> `100` -> `100` bytes (100 bytes)
    const fn kmap_lookup(sz: usize, chr: u8) -> usize {
	const fn gen_kmap(tags: &[u8], sizes: &[usize]) -> [Option<NonZeroUsize>; 256] {
	    let mut output = [None; 256];
	    let mut i=0;
	    let len = if tags.len() < sizes.len() { tags.len() } else { sizes.len() };
	    while i < len {
		output[tags[i] as usize] = NonZeroUsize::new(sizes[i]);
		i += 1;
	    }
	    output
	}
	const KMAP: [Option<NonZeroUsize>; 256] = gen_kmap(KMAP_TAGS, KMAP_SIZES);

	match KMAP[chr as usize] {
	    Some(mul) => sz * mul.get(),
	    None => sz,
	}
    }
    
    let path= path.as_ref();
    if !path.is_dir() {
	return None;
    } 

    let dir_name = path.file_name()?;
    let dir_bytes = dir_name.as_bytes();
    
    // location of the b'-' in the dir name
    let split_loc = memchr::memchr(b'-', dir_bytes)?;
    
    // The rest of the string including the b'-' seperator. (i.e. '-(\d+)kB$')
    let split_bytes = &dir_bytes[split_loc..];

    // location of the IEC tag (in `KMAP_TAGS`, expected to be b'k') after the number of kilobytes
    let (k_loc, k_chr) = 'lookup: loop  {
	for &tag in KMAP_TAGS {
	    if let Some(k_loc) = memchr::memchr(tag, split_bytes) {
		break 'lookup (k_loc, tag);
	    } else {
		continue 'lookup;
	    }
	}
	// No suffixes in `KMAP_TAGS` found.
	if_trace!(error!("No appropriate suffix ({}) found in {:?}", unsafe { std::str::from_utf8_unchecked(KMAP_TAGS) }, split_bytes));
	return None;
    };
    

    // The number of kilobytes in this hugepage as a base-10 string
    let kb_str = {
	let kb_str = &split_bytes[..k_loc];// &dir_bytes[split_loc..k_loc];
	if kb_str.len() <= 1 {
	    // There is no number between the digits and the `kB` (unlikely)
	    if_trace!(error!("Invalid format of hugepage kB size in pathname `{:?}': Extracted string was `{}'", dir_name, String::from_utf8_lossy(kb_str)));
	    return None;
	}
	match std::str::from_utf8(&kb_str[1..]) {
	    Ok(v) => v,
	    Err(e) => {
		if_trace!(error!("Kilobyte string number (base-10) in pathname `{:?}' is not valid utf8: {e}", kb_str));
		drop(e);
		return None;
	    }
	}
    };

    kb_str.parse::<usize>().ok().map(move |sz| kmap_lookup(sz, k_chr))
}
