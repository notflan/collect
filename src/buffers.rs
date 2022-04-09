//! Buffers and helpers
use super::*;
use std::num::NonZeroUsize;

#[cfg(feature="bytes")]
/// Default mutable buffer
#[allow(dead_code)]
pub type DefaultMut = bytes::BytesMut;

#[cfg(not(feature="bytes"))] 
/// Default mutable buffer
#[allow(dead_code)]
pub type DefaultMut = Vec<u8>;

/// Default immutable buffer
#[allow(dead_code)]
pub type Default = <DefaultMut as MutBuffer>::Frozen;


/// Reader from a mutable reference of a `Buffer`.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BufferReader<'a, B: ?Sized>(&'a mut B, usize);


/// Writer to a mutable reference of a `MutBuffer`.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BufferWriter<'a, B: ?Sized>(&'a mut B, usize);

#[allow(dead_code)]
const _: () = {
    impl<'a, B: ?Sized + Buffer> BufferReader<'a, B>
    {
	#[inline(always)] 
	pub fn get(&self) -> &B
	{
	    &self.0
	}
	#[inline(always)] 
	pub fn get_mut(&mut self) -> &B
	{
	    &mut self.0
	}
	#[inline(always)] 
	pub fn amount_read(&self) -> usize
	{
	    self.1
	}
    }
    impl<'a, 'b: 'a, B: Buffer + 'b> BufferReader<'a, B>
    {
	#[inline] 
	pub fn unsize(self) -> BufferReader<'a, (dyn Buffer + 'b)>
	{
	    BufferReader(self.0, self.1)
	}
    }

    impl<'a, B: ?Sized + Buffer> BufferWriter<'a, B>
    {
	#[inline(always)] 
	pub fn get(&self) -> &B
	{
	    &self.0
	}
	#[inline(always)] 
	pub fn get_mut(&mut self) -> &B
	{
	    &mut self.0
	}
	#[inline(always)] 
	pub fn amount_written(&self) -> usize
	{
	    self.1
	}
    }
    impl<'a, 'b: 'a, B: Buffer + 'b> BufferWriter<'a, B>
    {
	#[inline] 
	pub fn unsize(self) -> BufferWriter<'a, (dyn Buffer + 'b)>
	{
	    BufferWriter(self.0, self.1)
	}
    }
};

impl<'a, B: ?Sized + Buffer> io::Read for BufferReader<'a, B>
{
    #[inline] 
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
	let adv = self.0.copy_to_slice(self.1, buf);
	self.1 += adv;
	Ok(adv)
    }
}

impl<'a, B: ?Sized + MutBuffer> io::Write for BufferWriter<'a, B>
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
	let adv = self.0.copy_from_slice(self.1, buf);
	self.1 += adv;
	Ok(adv)
    }
    #[inline(always)] 
    fn flush(&mut self) -> io::Result<()> {
	Ok(())
    }
}

/// An immutable contiguous buffer
pub trait Buffer: AsRef<[u8]>
{
    #[inline] 
    fn copy_to_slice(&self, st: usize, slice: &mut [u8]) -> usize
    {
	let by = self.as_ref();
	if st >= by.len() {
	    return 0;
	}
	
	let by = &by[st..];
	let len = std::cmp::min(by.len(), slice.len());
	// SAFETY: We know `self`'s AsRef impl cannot overlap with `slice`, since `slice` is a mutable reference.
	if len > 0 {
	    unsafe {
		std::ptr::copy_nonoverlapping(by.as_ptr(), slice.as_mut_ptr(), len)
	    }
	}
	len
    }

}
pub trait BufferExt: Buffer
{
    #[inline(always)] 
    fn reader_from(&mut self, st: usize) -> BufferReader<'_, Self>
    {
	BufferReader(self, st)
    }
    #[inline]
    fn reader(&mut self) -> BufferReader<'_, Self>
    {
	self.reader_from(0)
    }
}
impl<B: Buffer> BufferExt for B{}

impl<T: ?Sized> Buffer for T
where T: AsRef<[u8]>
{}

/// A mutable contiguous buffer
pub trait MutBuffer: AsMut<[u8]>
{
    type Frozen: Sized + Buffer;
    
    /// Make immutable
    fn freeze(self) -> Self::Frozen;

    #[inline] 
    fn copy_from_slice(&mut self, st: usize, slice: &[u8]) -> usize
    {
	let by = self.as_mut();
	if st >= by.len() {
	    return 0;
	}

	let by = &mut by[st..];
	let len = std::cmp::min(by.len(), slice.len());
	
	if len > 0 {
	    // SAFETY: We know `self`'s AsRef impl cannot overlap with `slice`, since `slice` is a mutable reference.
	    unsafe {
		std::ptr::copy_nonoverlapping(slice.as_ptr(), by.as_mut_ptr(), len);
	    }
	}
	len
    }
}

pub trait MutBufferExt: MutBuffer
{
    #[inline(always)] 
    fn writer_from(&mut self, st: usize) -> BufferWriter<'_, Self>
    {
	BufferWriter(self, st)
    }
    #[inline] 
    fn writer(&mut self) -> BufferWriter<'_, Self>
    {
	self.writer_from(0)
    }
}
impl<B: ?Sized + MutBuffer> MutBufferExt for B{}

#[cfg(feature="bytes")]
impl MutBuffer for bytes::BytesMut
{
    type Frozen = bytes::Bytes;
    #[inline(always)] 
    fn freeze(self) -> Self::Frozen {
	bytes::BytesMut::freeze(self)
    }
}

impl MutBuffer for Vec<u8>
{
    type Frozen = Box<[u8]>;
    #[inline]
    fn freeze(self) -> Self::Frozen {
	self.into_boxed_slice()
    }
}

/// A trait for buffers that can be allocated with a capacity
pub trait WithCapacity: Sized
{
    fn wc_new() -> Self;
    fn wc_with_capacity(_: usize) -> Self;
}

impl WithCapacity for Box<[u8]>
{
    #[inline(always)] 
    fn wc_new() -> Self {
	Vec::wc_new().into_boxed_slice()
    }
    #[inline(always)] 
    fn wc_with_capacity(cap: usize) -> Self {
	Vec::wc_with_capacity(cap).into_boxed_slice()
    }
}

pub trait WithCapExt: WithCapacity
{
    fn maybe_with_capacity(maybe: Option<NonZeroUsize>) -> Self;
    #[inline(always)]
    fn try_with_capacity(cap: usize) -> Self
    {
	Self::maybe_with_capacity(NonZeroUsize::new(cap))
    }
}

/// A type that can be used as a size for creating a `WithCapacity` buffer
pub trait TryCreateBuffer
{
    fn create_buffer<T: WithCapacity>(&self) -> T;
}

impl TryCreateBuffer for Option<NonZeroUsize>
{
    #[inline(always)] 
    fn create_buffer<T: WithCapacity>(&self) -> T {
	T::maybe_with_capacity(*self)
    }
}

impl TryCreateBuffer for usize
{
    #[inline(always)]
    fn create_buffer<T: WithCapacity>(&self) -> T {
	T::try_with_capacity(*self)
    }
}

impl<T: WithCapacity> WithCapExt for T
{
    #[inline] 
    fn maybe_with_capacity(maybe: Option<NonZeroUsize>) -> Self {
	match maybe {
	    Some(sz) => Self::wc_with_capacity(sz.into()),
	    None => Self::wc_new()
	}
    }
    
}

/// Implement `WithCapacity` for a type that supports it.
macro_rules! cap_buffer  {
    ($name:ty) => {
	impl $crate::buffers::WithCapacity for $name
	{
	    #[inline(always)] 
	    fn wc_new() -> Self
	    {
		Self::new()
	    }
	    #[inline(always)] 
	    fn wc_with_capacity(cap: usize) -> Self
	    {
		Self::with_capacity(cap)
	    }
	}
    };
}

pub mod prelude
{
    /// Export these items anonymously.
    macro_rules! export_anon {
	($($name:ident),+ $(,)?) => {
	    $(
		pub use super::$name as _;
	    )*
	};
    }

    // Causes conflicts for `.writer()`, so remove them from prelude.
    #[cfg(feature="bytes")]
    export_anon!{
	WithCapExt,
	//BufferExt,
	//MutBufferExt,
	WithCapExt,
    }
    
    #[cfg(not(feature="bytes"))] 
    export_anon!{
	WithCapExt,
	BufferExt,
	MutBufferExt,
	WithCapExt,
    }
    
    pub use super::{
	WithCapacity,
	TryCreateBuffer,
	MutBuffer,
	Buffer,
    };
}

pub(crate) use cap_buffer;

#[cfg(feature="bytes")] buffers::cap_buffer!(bytes::BytesMut);
cap_buffer!(Vec<u8>);
