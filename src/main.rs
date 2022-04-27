
#[macro_use] extern crate cfg_if;
#[cfg(feature="logging")] 
#[macro_use] extern crate tracing;

#[cfg(feature="memfile")] 
#[macro_use] extern crate lazy_static;
#[cfg(feature="memfile")] 
#[macro_use] extern crate stackalloc;

/// Run this statement only if `tracing` is enabled
macro_rules! if_trace {
    (? $expr:expr) => {
	cfg_if! {
	    if #[cfg(all(feature="logging", debug_assertions))] {
		$expr;
	    }
	}
    };
    ($expr:expr) => {
	cfg_if! {
	    if #[cfg(feature="logging")] {
		$expr;
	    }
	}
    };
}

#[cfg(feature="jemalloc")] 
extern crate jemallocator;

#[cfg(feature="jemalloc")]
const _:() = {
    #[global_allocator]
    static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
};

use std::{
    io,
    mem::MaybeUninit,
    os::unix::prelude::*,
    num::NonZeroUsize,
};

#[allow(unused_imports)]
use color_eyre::{
    eyre::{
	self,
	eyre,
	WrapErr,
    },
    Section,
    SectionExt, Help,
};

/// Get an `&'static str` of the current function name.
macro_rules! function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            ::std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }}
}

mod buffers;
use buffers::prelude::*;

#[cfg(feature="memfile")] mod memfile;

#[cfg(feature="bytes")]
use bytes::{
    Buf,
    BufMut,
};

/* TODO: XXX: For colouring buffer::Perc
#[derive(Debug)]
struct StackStr<const MAXLEN: usize>(usize, std::mem::MaybeUninit<[u8; MAXLEN]>);

impl<const SZ: usize> StackStr<SZ>
{
#[inline] 
pub const fn new() -> Self
{
Self(0, std::mem::MaybeUninit::uninit())
    }
    
    #[inline(always)] 
    pub const unsafe fn slice_mut(&mut self) -> &mut [u8]
    {
	&mut self.1[self.0..]
    }
    #[inline] 
    pub const fn slice(&self) -> &[u8]
    {
	&self.1[self.0..]
    }
    
    #[inline] 
    pub const unsafe fn as_str_unchecked(&self) -> &str
    {
	std::str::from_utf8_unchecked(&self.1[self.0..])
    }

    #[inline] 
    pub const unsafe fn as_mut_str_unchecked(&mut self) -> &mut str
    {
	std::str::from_utf8_unchecked_mut(&mut self.1[..self.0])
    }

    #[inline]
    #[cfg_attr(feature="logging", instrument(level="debug"))]
    pub fn as_str(&self) -> &str
    {
	std::str::from_utf8(self.slice()).expect("Invalid string")
    }

    #[inline(always)]
    const fn left(&self) -> usize {
	SZ - self.0
    }

    #[inline(always)] 
    pub fn write_bytes(&mut self, s: &[u8]) -> usize {
	let b = &s[..std::cmp::min(match self.left() {
	    0 => return 0,
	    x => x,
	}, s.len())];
	unsafe { &mut self.slice_mut() [self.0..] }.copy_from_slice(b);
	let v = b.len();
	self.0 += v;
	v
    }
}

impl<const SZ: usize> std::fmt::Write for StackStr<SZ>
{
    #[inline] 
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
	self.write_bytes(s.as_bytes());
	Ok(())
    }
    #[inline] 
    fn write_char(&mut self, c: char) -> std::fmt::Result {
	let l = c.len_utf8();
	if l > self.left() {
	    return Ok(())
	} 
	self.write_bytes(c.encode_utf8(unsafe { &mut self.slice_mut() [self.0..] }));
	self.0 += l;

	Ok(())
    }
}
*/

#[cfg_attr(feature="logging", instrument(level="info", skip(reader), fields(reader = ?std::any::type_name::<R>())))]
fn try_get_size<R: ?Sized>(reader: &R) -> Option<NonZeroUsize>
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


fn init() -> eyre::Result<()>
{
    cfg_if!{ if #[cfg(feature="logging")] {
	fn install_tracing()
	{
	    //! Install spantrace handling
	    
	    use tracing_error::ErrorLayer;
	    use tracing_subscriber::prelude::*;
	    use tracing_subscriber::{fmt, EnvFilter};

	    let fmt_layer = fmt::layer()
		.with_target(false)
		.with_writer(io::stderr);
	    
	    let filter_layer = EnvFilter::try_from_default_env()
		.or_else(|_| EnvFilter::try_new(if cfg!(debug_assertions) {
		    "debug"
		} else {
		    "info"
		}))
		.unwrap();

	    tracing_subscriber::registry()
		.with(fmt_layer)
		.with(filter_layer)
		.with(ErrorLayer::default())
		.init();
	}

	if !cfg!(feature="disable-logging") {
	    install_tracing();
	    if_trace!(trace!("installed tracing"));
	}
    } }
    
    color_eyre::install()
}

#[cfg_attr(feature="logging", instrument(err))]
#[inline] 
fn non_map_work() -> eyre::Result<()>
{
    if_trace!(trace!("strategy: allocated buffer"));
    
    let (bytes, read) = {
	let stdin = io::stdin();
	let mut bytes: buffers::DefaultMut = try_get_size(&stdin).create_buffer();
	
	let read = io::copy(&mut stdin.lock(), &mut (&mut bytes).writer())
	    .with_section(|| bytes.len().header("Buffer size is"))
	    .with_section(|| bytes.capacity().header("Buffer cap is"))
	    .with_section(|| format!("{:?}", bytes).header("Buffer is"))
	    .wrap_err("Failed to read into buffer")?;
	(bytes.freeze(), read as usize)
    };
    if_trace!(info!("collected {read} from stdin. starting write."));

    let written = 
	io::copy(&mut (&bytes[..read]).reader() , &mut io::stdout().lock())
	.with_section(|| read.header("Bytes read"))
	.with_section(|| bytes.len().header("Buffer length (frozen)"))
	.with_section(|| format!("{:?}", &bytes[..read]).header("Read Buffer"))
	.with_section(|| format!("{:?}", bytes).header("Full Buffer"))
	.wrap_err("Failed to write from buffer")?;
    if_trace!(info!("written {written} to stdout."));

    if read != written as usize {
	return Err(io::Error::new(io::ErrorKind::BrokenPipe, format!("read {read} bytes, but only wrote {written}")))
	    .wrap_err("Writing failed: size mismatch");
    }
    
    Ok(())
}

#[cfg_attr(feature="logging", instrument(err))]
#[inline]
#[cfg(feature="memfile")] 
fn map_work() -> eyre::Result<()>
{        
    const DEFAULT_BUFFER_SIZE: fn () -> Option<std::num::NonZeroUsize> = || {
	cfg_if!{ 
	    if #[cfg(feature="memfile-preallocate")]  {
		extern "C" {
		    fn getpagesize() -> libc::c_int;
		}
		unsafe { std::num::NonZeroUsize::new(getpagesize() as usize * 8) }
	    } else {
		std::num::NonZeroUsize::new(0)
	    }
	}
    };
    
    if_trace!(trace!("strategy: mapped memory file"));

    use std::borrow::Borrow;

    #[inline(always)] 
    fn tell_file<T>(file: &mut T) -> io::Result<u64>
	where T: io::Seek + ?Sized
    {
	file.stream_position()
    }

    #[inline(always)] 
    fn unwrap_int_string<T, E>(i: impl Borrow<Result<T, E>>) -> String
    where T: std::fmt::Display,
    E: std::fmt::Display
    {
	i.borrow().as_ref().map(ToString::to_string)
	    .unwrap_or_else(|e| format!("<unknown: {e}>"))
    }

    let (mut file, read) = {
	let stdin = io::stdin();

	let buffsz = try_get_size(&stdin);
	if_trace!(debug!("Attempted determining input size: {:?}", buffsz));
	let buffsz = buffsz.or_else(DEFAULT_BUFFER_SIZE);
	if_trace!(if let Some(buf) = buffsz.as_ref() {
	    trace!("Failed to determine input size: preallocating to {}", buf);
	} else {
	    trace!("Failed to determine input size: alllocating on-the-fly (no preallocation)");
	});
	
	let mut file = memfile::create_memfile(Some("collect-buffer"), 
					       buffsz.map(|x| x.get()).unwrap_or(0))	    
	    .with_section(|| format!("{:?}", buffsz).header("Deduced input buffer size"))
	    .wrap_err(eyre!("Failed to create in-memory buffer"))?;

	let read = io::copy(&mut stdin.lock(), &mut file)
	    .with_section(|| format!("{:?}", file).header("Memory buffer file"))?;
	
	let read =  {
	    use io::*;
	    use std::borrow::Cow;

	    let (read, sp, sl) = if cfg!(any(feature="memfile-preallocate", debug_assertions)) {
		let sp = file.stream_position();
		let sl = memfile::stream_len(&file);
		
		if_trace!(trace!("Stream position after read: {:?}", sp));
		if_trace!(trace!("Stream length after read: {:?}", sp));
		
		let read = match sp.as_ref() {
		    Ok(&v) if v != read  => {
			if_trace!(warn!("Reported read value not equal to memfile stream position: expected from `io::copy()`: {v}, got {read}"));
			v
		    },
		    Ok(&x) => {
			if_trace!(trace!("Reported memfile stream position and copy result equal: {x} == {}", read));
			x
		    },
		    Err(e) => {
			if_trace!(error!("Could not report memfile stream position, ignoring check on {read}: {e}"));
			read
		    },
		};

		let truncate_stream = |bad: u64, good: u64| {
		    use std::num::NonZeroU64;
		    file.set_len(good)
			.map(|_| good)
			.with_section(|| match NonZeroU64::new(bad) {Some (b) => Cow::Owned(b.get().to_string()), None => Cow::Borrowed("<unknown>") }.header("Original (bad) length"))
			.with_section(|| good.header("New (correct) length"))
			.wrap_err(eyre!("Failed to truncate stream to correct length")
				  .with_section(|| format!("{:?}", file).header("Memory buffer file")))
		};
		
		let read = match sl.as_ref() {
		    Ok(&v) if v != read  => {
			if_trace!(warn!("Reported read value not equal to memfile stream length: expected from `io::copy()`: {read}, got {v}"));
			if_trace!(debug!("Attempting to correct memfile stream length from {v} to {read}"));
			
			truncate_stream(v, read)?
		    },
		    Ok(&v) => {
			if_trace!(trace!("Reported memfile stream length and copy result equal: {v} == {}", read));
			v
		    },
		    Err(e) => {
			if_trace!(error!("Could not report memfile stream length, ignoring check on {read}: {e}"));
			if_trace!(warn!("Attempting to correct memfile stream length anyway"));
			if let Err(e) = truncate_stream(0, read) {
			    if_trace!(error!("Truncate failed: {e}"));
			}
			
			read
		    }
		};
		(read, Some(sp), Some(sl))
	    } else {
		(read, None, None)
	    };

	    file.seek(SeekFrom::Start(0))
		.with_section(|| read.header("Actual read bytes"))
		.wrap_err(eyre!("Failed to seek back to start of memory buffer file for output")
			  .with_section(move || if let Some(sp) = sp { Cow::Owned(unwrap_int_string(sp)) }
					else { Cow::Borrowed("<unknown>")  }.header("Memfile position"))
			  .with_section(move || if let Some(sp) = sl { Cow::Owned(unwrap_int_string(sp)) }
					else { Cow::Borrowed("<unknown>")  }.header("Memfile full length"))
			  /*.with_section(|| file.stream_len().map(|x| x.to_string())
			  .unwrap_or_else(|e| format!("<unknown: {e}>")).header("Memfile full length"))*/)?;
	    
	    read
	};
	
	(file, usize::try_from(read)
	 .wrap_err(eyre!("Failed to convert read bytes to `usize`")
		   .with_section(|| read.header("Number of bytes was"))
		   .with_section(|| u128::abs_diff(read.into(), usize::MAX as u128).header("Difference between `read` and `usize::MAX` is"))
		   .with_suggestion(|| "It is likely you are running on a 32-bit ptr width machine and this input exceeds that of the maximum 32-bit unsigned integer value")
		   .with_note(|| usize::MAX.header("Maximum value of `usize`")))?)
    };
    if_trace!(info!("collected {} from stdin. starting write.", read));

    let written =
	io::copy(&mut file, &mut io::stdout().lock())
	.with_section(|| read.header("Bytes read from stdin"))
	.with_section(|| unwrap_int_string(tell_file(&mut file)).header("Current buffer position"))
	.wrap_err("Failed to write buffer to stdout")?;
    if_trace!(info!("written {written} to stdout."));

    if read != written as usize {
	return Err(io::Error::new(io::ErrorKind::BrokenPipe, format!("read {read} bytes, but only wrote {written}")))
	    .wrap_err("Writing failed: size mismatch");
    }
    
    Ok(())
}

#[cfg_attr(feature="logging", instrument(err))]
fn main() -> eyre::Result<()> {
    init()?;
    if_trace!(debug!("initialised"));

    cfg_if!{ 
	if #[cfg(feature="memfile")] {
	    map_work()
		.wrap_err(eyre!("Operation failed").with_note(|| "With mapped memfd algorithm"))?;
	} else {
	    non_map_work()
		.wrap_err(eyre!("Operation failed").with_note(|| "With alloc-buf (non-mapped) algorithm"))?;
	}
    }

    Ok(())
}
