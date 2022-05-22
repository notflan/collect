//! Extensions
use super::*;
use std::{
    mem::{
	self,
	ManuallyDrop,
    },
    marker::PhantomData,
    ops,
};

/// Essentially equivelant bound as `eyre::StdError` (private trait)
///
/// Useful for when using traits that convert a generic type into an `eyre::Report`.
pub trait EyreError: std::error::Error + Send + Sync + 'static{}
impl<T: ?Sized> EyreError for T
where T: std::error::Error + Send + Sync + 'static{}

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
