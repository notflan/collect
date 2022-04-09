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

fn main() -> io::Result<()> {
    let (bytes, read) = {
	let stdin = io::stdin();
	let mut bytes: buffers::DefaultMut = try_get_size(&stdin).create_buffer();
	
	let read = io::copy(&mut stdin.lock(), &mut (&mut bytes).writer())?;
	(bytes.freeze(), read as usize)
    };

    let written = 
	io::copy(&mut (&bytes[..read]).reader() , &mut io::stdout().lock())?;

    if read != written as usize {
	return Err(io::Error::new(io::ErrorKind::BrokenPipe, format!("read {read} bytes, but only wrote {written}")));
    }
    
    Ok(())
}
