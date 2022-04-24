
#[macro_use] extern crate cfg_if;
#[cfg(feature="logging")] 
#[macro_use] extern crate tracing;

#[macro_use] extern crate lazy_static;
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

#[cfg_attr(tracing, instrument(err))]
fn main() -> eyre::Result<()> {
    init()?;
    if_trace!(debug!("initialised"));
    
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
