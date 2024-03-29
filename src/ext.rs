//! Extensions
use super::*;
use std::{
    mem::{
	self,
	ManuallyDrop,
    },
    marker::PhantomData,
    ops,
    iter,
};

/// Essentially equivelant bound as `eyre::StdError` (private trait)
///
/// Useful for when using traits that convert a generic type into an `eyre::Report`.
pub trait EyreError: std::error::Error + Send + Sync + 'static{}
impl<T: ?Sized> EyreError for T
where T: std::error::Error + Send + Sync + 'static{}

#[derive(Debug, Clone)]
pub struct Joiner<I, F>(I, F, bool);

#[derive(Debug, Clone, Copy)]
pub struct CloneJoiner<T>(T);

impl<I, F> Joiner<I, F>
{
    #[inline(always)]
    fn size_calc(low: usize) -> usize
    {
	match low {
	    0 | 1 => low,
	    2 => 4,
	    x if x % 2 == 0 => x * 2,
	    odd => (odd * 2) - 1
	}
    }
}
type JoinerExt = Joiner<std::convert::Infallible, std::convert::Infallible>;

impl<I, F> Iterator for Joiner<I, F>
where I: Iterator, F: FnMut() -> I::Item
{
    type Item = I::Item;

    #[inline] 
    fn next(&mut self) -> Option<Self::Item> {
	let val = match self.2 {
	    false => self.0.next(),
	    true => Some(self.1())
	};
	if val.is_some() {
	    self.2 ^= true;
	}
	val
    }

    #[inline] 
    fn size_hint(&self) -> (usize, Option<usize>) {
	let (low, high) = self.0.size_hint();
	(Self::size_calc(low), high.map(Self::size_calc))
    }
}

impl<I, T> Iterator for Joiner<I, CloneJoiner<T>>
where I: Iterator<Item = T>, T: Clone
{
    type Item = I::Item;

    #[inline] 
    fn next(&mut self) -> Option<Self::Item> {
	let val = match self.2 {
	    false => self.0.next(),
	    true => Some(self.1.0.clone())
	};
	if val.is_some() {
	    self.2 ^= true;
	}
	val
    }

    #[inline] 
    fn size_hint(&self) -> (usize, Option<usize>) {
	let (low, high) = self.0.size_hint();
	(Self::size_calc(low), high.map(Self::size_calc))
    }
}

impl<I, F> iter::FusedIterator for Joiner<I, F>
where Joiner<I,F>: Iterator,
      I: iter::FusedIterator{}
impl<I, F> ExactSizeIterator for Joiner<I, F>
where Joiner<I,F>: Iterator,
      I: ExactSizeIterator {}

pub trait IterJoinExt<T>: Sized
{    
    fn join_by<F: FnMut() -> T>(self, joiner: F) -> Joiner<Self, F>;
    fn join_by_default(self) -> Joiner<Self, fn () -> T>
    where T: Default;
    fn join_by_clone(self, value: T) -> Joiner<Self, CloneJoiner<T>>
    where T: Clone;

}

impl<I, T> IterJoinExt<T> for I
where I: Iterator<Item = T>
{
    #[inline] 
    fn join_by<F: FnMut() -> T>(self, joiner: F) -> Joiner<Self, F> {
	Joiner(self, joiner, false)
    }
    #[inline] 
    fn join_by_default(self) -> Joiner<Self, fn () -> T>
    where T: Default
    {
	Joiner(self, T::default, false)
    }
    #[inline] 
    fn join_by_clone(self, value: T) -> Joiner<Self, CloneJoiner<T>>
    where T: Clone {
	Joiner(self, CloneJoiner(value), false)
    }
}

pub trait IntoEyre<T>
{
    fn into_eyre(self) -> eyre::Result<T>;
}

impl<T, E: EyreError> IntoEyre<T> for Result<T, E>
{
    #[inline(always)] 
    fn into_eyre(self) -> eyre::Result<T> {
	match self {
	    Err(e) => Err(eyre::Report::from(e)),
	    Ok(y) => Ok(y),
	}
    }
}

pub trait FlattenReports<T>
{
    /// Flatten a `eyre::Result<eyre::Result<T>>` into an `eyre::Result<T>`
    fn flatten(self) -> eyre::Result<T>;
}

pub trait FlattenEyreResult<T, E>
where E: EyreError
{
    /// Flatten a `Result<Result<T, IE>, OE>` into an `eyre::Result<E>`
    fn flatten(self) -> eyre::Result<T>;
}

pub trait FlattenResults<T, E>
{
    /// Flatten a `Result<Result<T, IE>, E>` into a `Result<T, E>`.
    fn flatten(self) -> Result<T, E>;
}

impl<T, E, IE: Into<E>> FlattenResults<T, E> for Result<Result<T, IE>, E>
{
    /// Flatten a `Result<Result<T, impl Into<E>>, E>` into a `Result<T, E>`
    ///
    /// This will convert the inner error into the type of the outer error.
    #[inline] 
    fn flatten(self) -> Result<T, E> {
	match self {
	    Err(e) => Err(e),
	    Ok(Ok(e)) => Ok(e),
	    Ok(Err(e)) => Err(e.into())
	}
    }
}

impl<T, IE: EyreError, E: EyreError> FlattenEyreResult<T, E> for Result<Result<T, IE>, E>
{
    #[inline] 
    fn flatten(self) -> eyre::Result<T> {
	match self {
	    Err(e) => Err(e).with_note(|| "Flattened report (outer)"),
	    Ok(Err(e)) => Err(e).with_warning(|| "Flattened report (inner)"),
	    Ok(Ok(a)) => Ok(a),
	}	
    }
}

impl<T> FlattenReports<T> for eyre::Result<eyre::Result<T>>
{
    #[inline] 
    fn flatten(self) -> eyre::Result<T> {
	match self {
	    Err(e) => Err(e.with_note(|| "Flattened report (outer)")),
	    Ok(Err(e)) => Err(e.with_warning(|| "Flattened report (inner)")),
	    Ok(Ok(a)) => Ok(a),
	}
    }
}

impl<T> FlattenReports<T> for eyre::Result<Option<T>>
{
    #[inline] 
    fn flatten(self) -> eyre::Result<T> {
	match self {
	    Err(e) => Err(e.with_note(|| "Flattened report (outer)")),
	    Ok(None) => Err(eyre!("Value expected, but not found").with_section(|| format!("Option<{}>", std::any::type_name::<T>()).header("Option type was")).with_warning(|| "Flattened report (inner)")),
	    Ok(Some(a)) => Ok(a),
	}
    }
}

impl<T, E: EyreError> FlattenEyreResult<T, E> for Result<Option<T>, E>
{
    #[inline] 
    fn flatten(self) -> eyre::Result<T> {
	match self {
	    Err(e) => Err(e).with_note(|| "Flattened report (outer)"),
	    Ok(None) => Err(eyre!("Value expected, but not found")
			    .with_section(|| format!("Option<{}>", std::any::type_name::<T>())
					  .header("Option type was"))
			    .with_warning(|| "Flattened report (inner)")),
	    Ok(Some(a)) => Ok(a),
	}
    }
}

#[derive(Debug)]
enum RunOnceInternal<F>
{
    Live(ManuallyDrop<F>),
    Dead,
}

impl<F: Clone> Clone for RunOnceInternal<F>
{
    #[inline] 
    fn clone(&self) -> Self {
	match self {
	    Self::Live(l) => Self::Live(l.clone()),
	    _ => Self::Dead
	}
    }
}

impl<F> RunOnceInternal<F>
{
    /// Take `F` now, unless it doesn't need to be dropped.
    ///
    /// # Returns
    /// * if `!needs_drop::<F>()`, `None` is always returned.
    /// * if `self` is `Dead`, `None` is returned.
    /// * if `self` is `Live(f)`, `Some(f)` is returned, and `self` is set to `Dead`.
    #[inline(always)] 
    fn take_now_for_drop(&mut self) -> Option<F>
    {
	if mem::needs_drop::<F>() {
	    self.take_now()
	} else {
	    None
	}
    }

    /// If `Live`, return the value inside and set to `Dead`.
    /// Otherwise, return `None`.
    #[inline(always)] 
    fn take_now(&mut self) -> Option<F>
    {
	if let Self::Live(live) = self {
	    let val = unsafe {
		ManuallyDrop::take(live)
	    };
	    *self = Self::Dead;
	    Some(val)
	} else {
	    None
	}
    }
}

impl<F> ops::Drop for RunOnceInternal<F>
{
    #[inline]
    fn drop(&mut self) {
	if mem::needs_drop::<F>() {
	    if let Self::Live(func) = self {
		unsafe { ManuallyDrop::drop(func) };
	    }
	}
    }
}

/// Holds a 0 argument closure that will only be ran *once*.
#[derive(Debug, Clone)]
pub struct RunOnce<F, T>(PhantomData<fn () -> T>, RunOnceInternal<F>);

unsafe impl<T, F> Send for RunOnce<F, T>
where F: FnOnce() -> T + Send {}

impl<F, T> RunOnce<F, T>
where F: FnOnce() -> T
{
    pub const fn new(func: F) -> Self
    {
	Self(PhantomData, RunOnceInternal::Live(ManuallyDrop::new(func)))
    }
    
    pub const fn never() -> Self
    {
	Self(PhantomData, RunOnceInternal::Dead)
    }

    #[inline] 
    pub fn try_take(&mut self) -> Option<F>
    {
	match &mut self.1 {
	    RunOnceInternal::Live(func) => {
		Some(unsafe { ManuallyDrop::take(func) })
	    },
	    _ => None
	}
    }

    #[inline] 
    pub fn try_run(&mut self) -> Option<T>
    {
	self.try_take().map(|func| func())
    }

    #[inline] 
    pub fn run(mut self) -> T
    {
	self.try_run().expect("Function has already been consumed")
    }

    #[inline] 
    pub fn take(mut self) -> F
    {
	self.try_take().expect("Function has already been consumed")
    }

    #[inline] 
    pub fn is_runnable(&self) -> bool
    {
	if let RunOnceInternal::Dead = &self.1 {
	    false
	} else {
	    true
	}
    }
}

#[inline(always)] 
pub(crate) fn map_bool<T>(ok: bool, value: T) -> T
where T: Default
{
    if ok {
	value
    } else {
	T::default()
    }
}
pub trait SealExt
{
    fn try_seal(&self, shrink: bool, grow: bool, write: bool) -> io::Result<()>;

    #[inline] 
    fn sealed(self, shrink: bool, grow: bool, write: bool) -> Self
    where Self: Sized {
	if let Err(e) = self.try_seal(shrink, grow, write) {
	    panic!("Failed to apply seals: {}", io::Error::last_os_error())
	}
	self
    }
}
#[cfg(any(feature="memfile", feature="exec"))]
const _: () = {
    impl<T: AsRawFd + ?Sized> SealExt for T
    {
	#[cfg_attr(feature="logging", instrument(skip(self)))] 
	fn sealed(self, shrink: bool, grow: bool, write: bool) -> Self
	where Self: Sized {
	    use libc::{
		F_SEAL_GROW, F_SEAL_SHRINK, F_SEAL_WRITE,
		F_ADD_SEALS,
		fcntl
	    };
	    let fd = self.as_raw_fd();
	    if unsafe {
		fcntl(fd, F_ADD_SEALS
		      , map_bool(shrink, F_SEAL_SHRINK)
		      | map_bool(grow, F_SEAL_GROW)
		      | map_bool(write, F_SEAL_WRITE))
	    } < 0 {
		panic!("Failed to apply seals to file descriptor {fd}: {}", io::Error::last_os_error())
	    } 
	    self	
	}
	
	#[cfg_attr(feature="logging", instrument(skip(self), err))] 
	fn try_seal(&self, shrink: bool, grow: bool, write: bool) -> io::Result<()> {
	    use libc::{
		F_SEAL_GROW, F_SEAL_SHRINK, F_SEAL_WRITE,
		F_ADD_SEALS,
		fcntl
	    };
	    let fd = self.as_raw_fd();
	    if unsafe {
		fcntl(fd, F_ADD_SEALS
		      , map_bool(shrink, F_SEAL_SHRINK)
		      | map_bool(grow, F_SEAL_GROW)
		      | map_bool(write, F_SEAL_WRITE))
	    } < 0 {
		Err(io::Error::last_os_error())
	    } else {
		Ok(())
	    }
	}
    }
};
