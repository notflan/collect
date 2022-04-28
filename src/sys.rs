//! System interactions
//!
//! Basic system interactions.
use super::*;

/// Attempt to get the size of any stream that is backed by a file-descriptor.
///
/// If one cannot be determined (or the fd is unsized), `None` is returned.
#[cfg_attr(feature="logging", instrument(level="info", skip(reader), ret, fields(reader = std::any::type_name::<R>())))]
#[inline]
//TODO: XXX: What if the size of `reader` really *is* 0. We shouldn't use `NonZeroUsize` here, we should just use `usize`. I think `st_size` can be `-1` if `fstat64()` fails to find a size...
pub fn try_get_size<R: ?Sized>(reader: &R) -> Option<NonZeroUsize>
where R: AsRawFd
{
    let fd = reader.as_raw_fd();
    use libc::{
	fstat64,
	stat64,
    };
    if fd < 0 {
	return None;
    }
    let mut st: MaybeUninit<stat64> = MaybeUninit::uninit();
    unsafe {
	match fstat64(fd, st.as_mut_ptr()) {
	    0 => {
		NonZeroUsize::new(st.assume_init().st_size as usize)
	    },
	    _ => None,
	}
    }
}

/// Get the current stream position of any seekable stream.
#[inline(always)] 
pub fn tell_file<T>(file: &mut T) -> io::Result<u64>
where T: io::Seek + ?Sized
{
    file.stream_position()
}
