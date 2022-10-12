//! Errors and helpers for errors.
use super::*;
use std::{
    fmt,
    error,
};

pub const DEFAULT_USE_ENV: bool = std::option_env!("NO_RT_ERROR_CTL").is_none();

pub type DispersedResult<T, const USE_ENV: bool = DEFAULT_USE_ENV> = Result<T, Dispersed<USE_ENV>>;

pub const ENV_NAME: &'static str = "RUST_VERBOSE";
const DEFAULT_ENV_VERBOSE: DispersedVerbosity = match std::option_env!("DEFAULT_ERROR") {
    Some("1") |
    Some("V") |
    Some("verbose") |
    Some("VERBOSE") |
    Some("v") => DispersedVerbosity::Verbose,
    Some("0") |
    _ => DispersedVerbosity::static_default(),
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
#[repr(u8)]
pub enum DispersedVerbosity
{
    Simple = 0,
    Verbose = 1,
}

impl From<DispersedVerbosity> for bool
{
    #[inline] 
    fn from(from: DispersedVerbosity) -> Self
    {
	from.is_verbose()
    }
}


impl Default for DispersedVerbosity
{
    #[inline]
    fn default() -> Self
    {
	DEFAULT_ENV_VERBOSE
    }
}


impl DispersedVerbosity
{
    #[inline(always)] 
    const fn static_default() -> Self
    {
	Self::Simple
    }

    #[inline(always)] 
    pub const fn is_verbose(self) -> bool
    {
	(self as u8) != 0
    }
}

fn get_env_value() -> DispersedVerbosity
{
    match std::env::var_os(ENV_NAME) {
	Some(mut value) => {
	    value.make_ascii_lowercase();
	    match value {
		"1" |
		"v" |
		"verbose" => DispersedVerbosity::Verbose,
		"0" |
		"s" |
		"simple" => DispersedVerbosity::Simple,
		_ => DispersedVerbosity::default(),
	    }
	},
	None => Default::default(),
    }
}

#[inline] 
pub fn dispersed_env_verbosity() -> DispersedVerbosity
{
    lazy_static! {
	static ref VALUE: DispersedVerbosity = get_env_value();
    }
    *VALUE
}

/// A simpler error message when returning an `eyre::Report` from main.
pub struct Dispersed<const USE_ENV: bool = DEFAULT_USE_ENV>(eyre::Report);

impl<const E: bool> From<eyre::Report> for Dispersed<E>
{
    #[inline] 
    fn from(from: eyre::Report) -> Self
    {
	Self(from)
    }
}

impl<const E: bool> Dispersed<E>
{
    #[inline] 
    pub fn into_inner(self) -> eyre::Report
    {
	self.0
    }
}

impl Dispersed<false>
{
    #[inline(always)] 
    pub const fn obey_env(self) -> Dispersed<true>
    {
	Dispersed(self.0)
    }
}

impl Dispersed<true>
{
    #[inline(always)]
    pub const fn ignore_env(self) -> Dispersed<false>
    {
	Dispersed(self.1)
    }
}

impl<const E: bool> Dispersed<E>
{
    #[inline(always)] 
    pub const fn set_env<const To: bool>(self) -> Dispersed<To>
    {
	Dispersed(self.0)
    }
}

impl error::Error for Dispersed<true>
{
    #[inline] 
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
	self.0
    }
}
impl error::Error for Dispersed<false>
{
    #[inline] 
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
	self.0
    }
}

impl fmt::Debug for Dispersed<false>
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	fmt::Display::fmt(&self.0, f)
    }
}


impl fmt::Display for Dispersed<false>
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	fmt::Debug::fmt(&self.0, f)
    }
}


impl fmt::Debug for Dispersed<true>
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	if dispersed_env_verbosity().is_verbose() {
	    fmt::Debug::fmt(&self.0, f)
	} else {
	    fmt::Display::fmt(&self.0, f)
	}
    }
}


impl fmt::Display for Dispersed<true>
{
    #[inline] 
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	if dispersed_env_verbosity().is_verbose() {
	    fmt::Display::fmt(&self.0, f)
	} else {
	    fmt::Debug::fmt(&self.0, f)
	}
    }
}

