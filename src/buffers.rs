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
    #[cfg_attr(feature="logging", instrument(level="trace", skip_all, fields(buf = ?buf.len())))]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
	let adv = self.0.copy_to_slice(self.1, buf);
	self.1 += adv;
	if_trace!(? trace!(" -> reading one buffer +{adv}"));
	Ok(adv)
    }
}

impl<'a, B: ?Sized + MutBuffer> io::Write for BufferWriter<'a, B>
{
    #[inline]
    #[cfg_attr(feature="logging", instrument(level="trace", skip_all, fields(buf = ?buf.len())))]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
	let adv = self.0.copy_from_slice(self.1, buf);
	
	self.1 += adv;
	
	if_trace!(? trace!(" <- writing one buffer {adv}"));
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
    #[cfg_attr(feature="logging", instrument(level="trace", skip_all, fields(buf = ?slice.len())))]
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
    #[cfg_attr(feature="logging", instrument(level="debug", skip_all, fields(st, buflen = ?slice.len())))]
    fn copy_from_slice(&mut self, st: usize, slice: &[u8]) -> usize
    {
	let by = self.as_mut();
	dbg!(&by);

	if st >= by.len() {
	    return 0;
	}
	dbg!(st);

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
    #[cfg_attr(feature="logging", instrument(level="info", skip(self)))]
    fn writer_from(&mut self, st: usize) -> BufferWriter<'_, Self>
    {
	if_trace!(debug!("creating writer at start {st}"));
	BufferWriter(self, st)
    }
    #[inline]
    //#[instrument(level="info", skip(self))]
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
    #[cfg_attr(feature="logging", instrument(level="trace"))]
    fn freeze(self) -> Self::Frozen {
	bytes::BytesMut::freeze(self)
    }
    //TODO: XXX: Impl copy_from_slice() as is done in impl for Vec<u8>? Or change how `.writer()` works for us to return the BytesMut writer which seems more efficient.
    /*#[instrument]
    fn copy_from_slice(&mut self, st: usize, buf: &[u8]) -> usize
    {
    //TODO: Special case for `st == 0` maybe? No slicing of the BytesMut might increase perf? Idk.
    if  (st + buf.len()) <= self.len() {
    // We can put `buf` in st..buf.len()
    self[st..].copy_from_slice(buf); 
} else if  st < self.len() {
    // The start is lower but the end is not
    let rem = self.len() - st;
    self[st..].copy_from_slice(&buf[..rem]);
    self.extend_from_slice(&buf[rem..]);
} else {
    // it is past the end, extend.
    self.extend_from_slice(buf);
}
    buf.len()
}*/
}

#[cfg(feature="recolored")] 
mod perc { 
    #[deprecated = "this is absolutely retardedly unsafe and unsound... fuck this shit man lole"]
    pub(super) unsafe fn gen_perc_boring(low: f64, high: f64) -> std::pin::Pin<&'static (impl std::fmt::Display + ?Sized + 'static)>
    {
	use std::{
	    cell::RefCell,
	    mem::MaybeUninit,
	    pin::Pin,
	    
	};
	thread_local! {
	    static STRING_BUFFER: RefCell<MaybeUninit<[u8; 16]>> = RefCell::new(MaybeUninit::uninit());
	}
	STRING_BUFFER.try_with(|buffer| -> Result<std::pin::Pin<&'static str>, Box<dyn std::error::Error + 'static>>{
	    let mut buffer = buffer.try_borrow_mut()?;
	    use std::io::Write;
	    write!(unsafe {&mut buffer.assume_init_mut()[..]}, "{:0.2}", (low / high) * 100f64)?;
	    let s_ref = unsafe {
		#[derive(Debug)]
		struct FindFailed;
		impl std::error::Error for FindFailed{}
		impl std::fmt::Display for FindFailed {
		    #[inline(always)] 
		    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
		    {
			f.write_str("boring perc: failed to write whole string into buffer of size 16")
		    }
		}
		let buf = buffer.assume_init_mut();
		let spl = memchr::memchr(b'%', &buf[..]).ok_or(FindFailed)?;
		std::str::from_utf8_mut(&mut buf[..=spl])?
	    };
	    unsafe {
		Ok(Pin::new(std::mem::transmute::<_, &'static _>(s_ref)))
	    }
	}).expect("bad static memory access").expect("failed to calc")
    }

    #[inline]
    //XXX::: WHY::: TRACING IGNORES MY COLOURS!!!
    #[deprecated(note="my colouring is ignored. we'll have to either: figure out why. or, use a different method to highlight abnormal (above 100) percentages")]
    pub(super) fn gen_perc(low: f64, high: f64) -> impl std::fmt::Display
    {
	use std::fmt;
	let f = low / match high {
	    0f64 => if low != 0f64 {
		return Perc::Invalid
	    } else {
		0f64
	    }
	    x => x,
	};
	enum Perc {
	    Normal(f64),
	    Goal(String),
	    High(String),
	    Zero(String),
	    Low(String),
	    
	    Invalid,
	}
	
	macro_rules! fmt_str {
	    (%) => ("{:0.2}%");
	    () => ("{:0.2}")
	}
	impl fmt::Display for Perc
	{
	    #[inline(always)] 
	    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
	    {
		use recolored::Colorize;

		write!(f, "{}", match self {
		    Self::Normal(p) => return write!(f, fmt_str!(%), p),
		    Self::Goal(p) => p.green(),
		    Self::High(p) => p.red(),
		    Self::Zero(p) => p.purple().bold(),
		    Self::Low(p) => p.on_red().white().underline(),
		    Self::Invalid => return write!(f, fmt_str!(%), ("0.00%".on_bright_red().white().strikethrough())),
		})?;
		{
		    use fmt::Write;
		    f.write_char('%')
		}
	    }
	}

	//TODO: StackStr instead of String
	(match f {
	    0f64 => Perc::Zero,
	    1f64 => Perc::Goal,
	    0f64..=1f64 => return Perc::Normal(f * 100f64),
	    1f64.. => Perc::High,
	    _ => Perc::Low,
	})(format!(fmt_str!(), f * 100f64))
    }
}

impl MutBuffer for Vec<u8>
{
    type Frozen = Box<[u8]>;
    
    #[inline]
    #[cfg_attr(feature="logging", instrument(level="trace"))]
    fn freeze(self) -> Self::Frozen {
	self.into_boxed_slice()
    }

    #[cfg_attr(feature="logging", instrument(level="trace", skip(buf, self), fields(st = ?st, self = ?self.len(), alloc= ?self.capacity())))]
    fn copy_from_slice(&mut self, st: usize, buf: &[u8]) -> usize
    {
	if  (st + buf.len()) <= self.len() {
	    // We can put `buf` in st..buf.len()
	    self[st..].copy_from_slice(buf);
	} else if  st < self.len() {
	    // The start is lower but the end is not
	    let rem = self.len() - st;
	    self[st..].copy_from_slice(&buf[..rem]);
	    if_trace!(trace!("extending buffer (partial, +{})", buf[rem..].len()));
	    self.extend_from_slice(&buf[rem..]);
	} else {
	    // it is past the end, extend.
	    if_trace!(trace!("extending buffer (whole, self + buf = {} / {}: {})"
			     ,self.len() + buf.len()
			     , self.capacity()
			     , {
				 cfg_if! {
				     if #[cfg(feature="recolored")] {
					 use perc::*;
					 (if cfg!(feature="recolored") {
					     |x,y| -> Box<dyn std::fmt::Display> { Box::new(gen_perc(x,y)) }
					 } else {
					     |x,y| -> Box<dyn std::fmt::Display> { Box::new(unsafe {gen_perc_boring(x,y)}.get_ref()) }
					 })((self.len() + buf.len()) as f64, self.capacity() as f64)
				     } else {
					 let t= self.len();
					 let c= self.capacity();
					 let b = buf.len();
					 lazy_format::lazy_format!("{:0.2}", ((t + b) as f64 / c as f64) * 100f64)
				     }
				 }
			     }));
	    self.extend_from_slice(buf);
	}
	buf.len()
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
    #[cfg_attr(feature="logging", instrument(level="info", fields(cap = "(unbound)")))]
    fn wc_new() -> Self {
	if_trace!(debug!("creating new boxed slice with size 0"));
	Vec::wc_new().into_boxed_slice()
    }
    #[inline(always)]
    #[cfg_attr(feature="logging", instrument(level="info"))]
    fn wc_with_capacity(cap: usize) -> Self {
	if_trace!(debug!("creating new boxed slice with size {cap}"));
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
	    #[cfg_attr(feature="logging", instrument(level="info", fields(cap = "(unbound)")))]
	    fn wc_new() -> Self
	    {
		if_trace! (debug!("creating {} with no cap", std::any::type_name::<Self>()));
		Self::new()
	    }
	    #[inline(always)]
	    #[cfg_attr(feature="logging", instrument(level="info"))]
	    fn wc_with_capacity(cap: usize) -> Self
	    {
		if_trace!(debug!("creating {} with {cap}", std::any::type_name::<Self>()));
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

// cap_buffer impls

#[cfg(feature="bytes")] buffers::cap_buffer!(bytes::BytesMut);
cap_buffer!(Vec<u8>);
