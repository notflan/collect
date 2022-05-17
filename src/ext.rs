//! Extensions
use super::*;

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
