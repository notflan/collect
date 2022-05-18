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
    ops,
    fmt,
};
use libc::{
    c_uint, c_int,
    MFD_HUGETLB,
    MAP_HUGE_SHIFT,
};

/// The location in the kernel's API which shows all valid huge-page sizes.
///
/// This is a directory, and its subdirectories' *names* will contain the size in this format: `hugepages-(\d+)kB` (where the first capture-group is the size in kB.)
/// The contents of those subdirectories themselves are irrelevent for our purpose.
pub const HUGEPAGE_SIZES_LOCATION: &'static str = "/sys/kernel/mm/hugepages";

/// Should creation of `Mask`s from extracted kernel information be subject to integer conversion checks?
///
/// This is `true` on debug builds or if the feature `hugepage-checked-masks` is enabled.
const CHECKED_MASK_CREATION: bool = if cfg!(feature="hugepage-checked-masks") || cfg!(debug_assertions) { true } else { false };


/// Find all `Mask`s defined within this specific directory.
///
/// This is usually only useful when passed `HUGEPAGE_SIZES_LOCATION` unless doing something funky with it.
/// For most use-cases, `get_masks()` should be fine.
#[cfg_attr(feature="logging", instrument(err, skip_all, fields(path = ?path.as_ref())))]
#[inline] 
pub fn get_masks_in<P>(path: P) -> eyre::Result<impl Iterator<Item=eyre::Result<Mask>> + 'static>
where P: AsRef<Path>
{
    let path = path.as_ref();
    let root_path = {
	let path = path.to_owned();
	move || path
    };
    let root_path_section = {
	let root_path = root_path.clone();
	move ||
	    root_path().to_string_lossy().into_owned().header("Root path was")
    };
    

    let dir = path.read_dir()
	.wrap_err(eyre!("Failed to enumerate directory")
		  .with_section(root_path_section.clone()))?;
    Ok(dir
       .map(|x| x.map(|n| n.file_name()))
       .map(|name| name.map(|name| (find_size_bytes(&name), name)))
       .map(move |result| match result {
	   Ok((Some(ok), path)) => {
	       if CHECKED_MASK_CREATION {
		   Mask::new_checked(ok)
		       .wrap_err(eyre!("Failed to create mask from extracted bytes")
				 .with_section(|| ok.header("Bytes were"))
				 .with_section(move || format!("{path:?}").header("Checked path was"))
				 .with_section(root_path_section.clone()))
	       } else {
		   Ok(Mask::new(ok))
	       }
	   },
	   Ok((None, path)) => Err(eyre!("Failed to extract bytes from path"))
	       .with_section(move || format!("{path:?}").header("Checked path was"))
	       .with_section(root_path_section.clone()),
	   Err(e) => Err(e).wrap_err(eyre!("Failed to read path from which to extract bytes")
				     .with_section(root_path_section.clone()))
       }))
}

/// Find all `Mask`s on this system.
#[cfg_attr(feature="logging", instrument(level="trace"))]
    #[inline] 
pub fn get_masks() -> eyre::Result<impl Iterator<Item=eyre::Result<Mask>> + 'static>
{
    get_masks_in(HUGEPAGE_SIZES_LOCATION)
}

/// A huge-page mask that can be bitwise OR'd with `HUGETLB_MASK`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
#[repr(transparent)]
pub struct Mask(c_uint);

impl fmt::Display for Mask
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "{}", self.raw())
    }
}

impl fmt::LowerHex for Mask
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "0x{:x}", self.raw())
    }   
}

impl fmt::UpperHex for Mask
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "0x{:X}", self.raw())
    }   
}

impl fmt::Binary for Mask
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	write!(f, "0b{:b}", self.raw())
    }
}

#[inline]
const fn log2_usize(x: usize) -> usize {
    const BITS: usize = std::mem::size_of::<usize>() * 8usize; //XXX Is this okay to be hardcoded? I can't find CHAR_BIT in core, so...

    BITS - (x.leading_zeros() as usize) - 1
}

impl Mask {
    /// The shift mask used to calculate huge-page masks
    pub const SHIFT: c_int = MAP_HUGE_SHIFT;
    
    /// The raw bitmask applied to make the `MAP_HUGE_` mask available via `raw()` valid for `memfd_create()` in `mask()`
    pub const HUGETLB_MASK: c_uint = MFD_HUGETLB;
    
    #[cfg_attr(feature="logging", instrument(level="debug", err))]
    #[inline]
    pub fn new_checked(bytes: usize) -> eyre::Result<Self>
    {
	Ok(Self(c_uint::try_from(log2_usize(bytes))?
		.checked_shl(Self::SHIFT as u32).ok_or(eyre!("Left shift would overflow"))?))
    }

    /// Create a new mask from a number of bytes.
    ///
    /// This is unchecked and may overflow if the number of bytes is so large (in which case, there is likely a bug), for a checked version, use `new_checked()`.
    #[inline]
    pub const fn new(bytes: usize) -> Self
    {
	Self((log2_usize(bytes) as c_uint) << Self::SHIFT)
    }

    /// Create from a raw `MAP_HUGE_` mask.
    ///
    /// # Safety
    /// The caller **must** guarantee that `mask` is a valid `MAP_HUGE_` mask.
    #[inline] 
    pub const unsafe fn from_raw(mask: c_uint) -> Self
    {
	Self(mask)
    }

    /// Get the raw `MAP_HUGE_` mask.
    #[inline]
    pub const fn raw(self) -> c_int
    {
	self.0 as c_int
    }

    /// Get a HUGETLB mask suitable for `memfd_create()` from this value.
    #[inline] 
    pub const fn mask(self) -> c_uint
    {
	(self.raw() as c_uint) | Self::HUGETLB_MASK
    }
    
    /// Create a function that acts as `memfd_create()` with *only* this mask applied to it.
    ///
    /// The `flags` argument is erased. To pass arbitrary flags to `memfd_create()`, use `memfd_create_wrapper_flags()`
    pub const fn memfd_create_wrapper(self) -> impl Fn (*const libc::c_char) -> c_int
    {
	use libc::memfd_create;
	move |path| {
	    unsafe {
		memfd_create(path, self.mask())
	    }
	}
    }

    /// Create a function that acts as `memfd_create()` with this mask applied to it.
    pub const fn memfd_create_wrapper_flags(self) -> impl Fn (*const libc::c_char, c_uint) -> c_int
    {
	use libc::memfd_create;
	move |path, flag| {
	    unsafe {
		memfd_create(path, flag | self.mask())
	    }
	}
    }
}

impl TryFrom<usize> for Mask
{
    type Error = eyre::Report;

    #[cfg_attr(feature="logging", instrument(level="trace", skip_all))]
    #[inline(always)] 
    fn try_from(from: usize) -> Result<Self, Self::Error>
    {
	Self::new_checked(from)
    }
}


impl ops::BitOr<c_uint> for Mask
{
    type Output= c_uint;
    #[inline] 
    fn bitor(self, rhs: c_uint) -> Self::Output {
	self.mask() | rhs
    }
}
impl ops::BitOr for Mask
{
    type Output= Self;
    #[inline] 
    fn bitor(self, rhs: Self) -> Self::Output {
	Self(self.0 | rhs.0)
    }
}

impl ops::BitOrAssign for Mask
{
    #[inline] 
    fn bitor_assign(&mut self, rhs: Self) {
	self.0 |= rhs.0;
    }   
}

//TODO: add test for `Mask::new_checked()` above, and `.memfd_create_wrapper{,_flags}()` usage, too with some `MAP_HUGE_` constants as sizes

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
    /*if !path.is_dir() {
    // These don't count as directories for some reason
    return None;
} */

    let dir_name = path.file_name()?;
    let dir_bytes = dir_name.as_bytes();
    if_trace!(trace!("dir_name: {dir_name:?}"));
    
    // location of the b'-' in the dir name
    let split_loc = memchr::memchr(b'-', dir_bytes)?;
    
    
    // The rest of the string including the b'-' seperator. (i.e. '-(\d+)kB$')
    let split_bytes = &dir_bytes[split_loc..];
    if_trace!(debug!("split_bytes (from `-'): {:?}", std::ffi::OsStr::from_bytes(split_bytes)));

    // location of the IEC tag (in `KMAP_TAGS`, expected to be b'k') after the number of kilobytes
    let (k_loc, k_chr) = 'lookup: loop  {
	for &tag in KMAP_TAGS {
	    if_trace!(trace!("attempting check for `{}' ({tag}) in {split_bytes:?}", tag as char));
	    if let Some(k_loc) = memchr::memchr(tag, split_bytes) {
		break 'lookup (k_loc, tag);
	    } else {
		if_trace!(warn!("lookup failed"));
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
	
	if_trace!(trace!("kb_str (raw): {:?}", std::ffi::OsStr::from_bytes(kb_str)));
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
    
    if_trace!(debug!("kb_str (extracted): {kb_str}"));

    kb_str.parse::<usize>().ok().map(move |sz| {
	if_trace!(debug!("found raw size {sz}, looking up in table for byte result of suffix `{}'", k_chr as char));
	kmap_lookup(sz, k_chr)
    })
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[inline] 
    fn get_bytes<'a, P: 'a>(from: P) -> eyre::Result<impl Iterator<Item=eyre::Result<usize>> +'a>
    where P: AsRef<Path>
    {
	let dir = from.as_ref().read_dir()?;
	Ok(dir
	   .map(|x| x.map(|n| n.file_name()))
	   .map(|name| name.map(|name| super::find_size_bytes(name)))
	   .map(|result| result.flatten()))
    }
    
    #[test]
    fn find_size_bytes() -> eyre::Result<()>
    {
	//crate::init()?; XXX: Make `find_size_bytes` return eyre::Result<usize> instead of Option<usize>
	let dir = Path::new(super::HUGEPAGE_SIZES_LOCATION).read_dir()?;
	for result in dir
	    .map(|x| x.map(|n| n.file_name()))
	    .map(|name| name.map(|name| super::find_size_bytes(name)))
	{
	    println!("size: {}", result
		     .wrap_err(eyre!("Failed to extract name"))?
		     .ok_or(eyre!("Failed to find size"))?);
	}
	
	
	Ok(())
    }

    mod map_huge {
	use super::*;
	/// Some `MAP_HUGE_` constants provided by libc.
	const CONSTANTS: &[c_int] = &[
	    libc::MAP_HUGE_1GB,
	    libc::MAP_HUGE_1MB,
	    libc::MAP_HUGE_2MB,
	];

	#[inline] 
	fn find_constants_in(path: impl AsRef<Path>, checked: bool) -> eyre::Result<usize>
	{
	    let mut ok = 0usize;
	    for bytes in get_bytes(path)? {

		let bytes = bytes?;
		let flag = if checked {
		    super::Mask::new_checked(bytes)
			.wrap_err(eyre!("Failed to create mask from bytes").with_section(|| bytes.header("Number of bytes was")))?
		} else {
		    super::Mask::new(bytes)
		};
		if CONSTANTS.contains(&flag.raw()) {
		    println!("Found pre-set MAP_HUGE_ flag: {flag:X} ({flag:b}, {bytes} bytes)");
		    ok +=1;
		}
	    }
	    Ok(ok)
	}

	#[test]
	fn find_map_huge_flags_checked() -> eyre::Result<()>
	{
	    eprintln!("Test array contains flags: {:#?}", CONSTANTS.iter().map(|x| format!("0x{x:X} (0b{x:b})")).collect::<Vec<String>>());
	    let ok = find_constants_in(super::HUGEPAGE_SIZES_LOCATION, true).wrap_err("Failed to find constants (checked mask creation)")?;
	    if ok>0 {
		println!("Found {ok} / {} of test flags set.", CONSTANTS.len());
		Ok(())
	    } else {
		println!("Found none of the test flags set...");
		Err(eyre!("Failed to find any matching map flags in test array of `MAP_HUGE_` flags: {:?}", CONSTANTS))
	    }
	}
	
	#[test]
	fn find_map_huge_flags() -> eyre::Result<()>
	{
	    eprintln!("Test array contains flags: {:#?}", CONSTANTS.iter().map(|x| format!("0x{x:X} (0b{x:b})")).collect::<Vec<String>>());
	    let ok = find_constants_in(super::HUGEPAGE_SIZES_LOCATION, false).wrap_err("Failed to find constants (unchecked mask creation)")?;
	    if ok>0 {
		println!("Found {ok} / {} of test flags set.", CONSTANTS.len());
		Ok(())
	    } else {
		println!("Found none of the test flags set...");
		Err(eyre!("Failed to find any matching map flags in test array of `MAP_HUGE_` flags: {:?}", CONSTANTS))
	    }
	}

	//TODO: test `get_masks()`
    }
}
