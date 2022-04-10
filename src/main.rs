
#[macro_use] extern crate tracing;

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

mod buffers;
use buffers::prelude::*;

#[cfg(feature="bytes")]
use bytes::{
    Buf,
    BufMut,
};

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
    fn install_tracing()
    {
	//! Install spantrace handling
	
	use tracing_error::ErrorLayer;
	use tracing_subscriber::prelude::*;
	use tracing_subscriber::{fmt, EnvFilter};

	let fmt_layer = fmt::layer().with_target(false);
	let filter_layer = EnvFilter::try_from_default_env()
	    .or_else(|_| EnvFilter::try_new("info"))
	    .unwrap();

	tracing_subscriber::registry()
	    .with(filter_layer)
	    .with(fmt_layer)
	    .with(ErrorLayer::default())
	    .init();
    }
    
    install_tracing();
    
    color_eyre::install()
}

#[instrument]
fn main() -> eyre::Result<()> {
    init()?;
    //info!("Initialised");
    
    let (bytes, read) = {
	let stdin = io::stdin();
	let mut bytes: buffers::DefaultMut = try_get_size(&stdin).create_buffer();
	
	let read = io::copy(&mut stdin.lock(), &mut bytes.writer())
	    .with_section(|| bytes.len().header("Buffer size is"))
	    .with_section(|| bytes.capacity().header("Buffer cap is"))
	    .with_section(|| format!("{:?}", bytes).header("Buffer is"))
	    .wrap_err("Failed to read into buffer")?;
	
	(bytes.freeze(), read as usize)
    };

    let written = 
	io::copy(&mut (&bytes[..read]).reader() , &mut io::stdout().lock())
	.with_section(|| read.header("Bytes read"))
	.with_section(|| bytes.len().header("Buffer length (frozen)"))
	.with_section(|| format!("{:?}", &bytes[..read]).header("Read Buffer"))
	.with_section(|| format!("{:?}", bytes).header("Full Buffer"))
	.wrap_err("Failed to write from buffer")?;

    if read != written as usize {
	return Err(io::Error::new(io::ErrorKind::BrokenPipe, format!("read {read} bytes, but only wrote {written}")))
	    .wrap_err("Writing failed: size mismatch");
    }
    
    Ok(())
}
