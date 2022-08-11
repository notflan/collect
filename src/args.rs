//! For handling arguments.
use super::*;
use std::ffi::{
    OsStr,
    OsString,
};
use std::{
    iter,
    fmt, error,
    borrow::Cow,
};
//TODO: When added, the `args` comptime feature will need to enable `lazy_static`.
use ::lazy_static::lazy_static;

/// The string used for positional argument replacements in `-exec{}`.
pub const POSITIONAL_ARG_STRING: &'static str = "{}";

/// The token that terminates adding arguments for `-exec` / `-exec{}`.
///
/// # Usage
/// If the user wants multiple `-exec/{}` parameters, they must be seperated with this token. e.g. `sh$ collect -exec c a b c \; -exec{} c2 d {} e f {} g`
///
/// It is not required for the user to provide the terminator when the `-exec/{}` is the final argument passed, but they can if they wish. e.g. `sh$ collect -exec command a b c` is valid, and `sh$ collect -exec command a b c \;` is *also* valid. 
pub const EXEC_MODE_STRING_TERMINATOR: &'static str = ";";

/// Mode for `-exec` / `-exec{}`
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExecMode
{
    Stdin{command: OsString, args: Vec<OsString>},
    Positional{command: OsString, args: Vec<Option<OsString>>},
}

impl fmt::Display for ExecMode
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	#[inline] 
	fn quote_into<'a, const QUOTE: u8>(string: &'a [u8], f: &mut (impl fmt::Write + ?Sized)) -> fmt::Result
	{
	    let data = if let Some(mut location) = memchr::memchr(QUOTE, string) {
		let mut data = Vec::with_capacity(string.len() * 2);
		Cow::Owned(loop {
		    data.extend_from_slice(&string[..location]);
		    data.extend([b'\\', QUOTE]);
		    location += match memchr::memchr(QUOTE, &string[location..]) {
			Some(x) if !&string[(location + x)..].is_empty() => x,
			_ => break data,
		    };
		})
	    } else {
		Cow::Borrowed(string)
	    };
	    let string = String::from_utf8_lossy(data.as_ref());
	    
	    if string.split_whitespace().take(2).count() == 1
	    {
		f.write_char(QUOTE as char)?;
		f.write_str(string.as_ref())?;
		f.write_char(QUOTE as char)
	    } else {
		f.write_str(string.as_ref())
	    }
	}
	match self {
	    Self::Stdin { command, args } => {
		quote_into::<b'\''>(command.as_bytes(), f)?;
		args.iter().map(move |arg| {
		    use fmt::Write;
		    f.write_char(' ').and_then(|_| quote_into::<b'"'>(arg.as_bytes(), f))
		}).collect()
	    },
	    Self::Positional { command, args } => {	
		quote_into::<b'\''>(command.as_bytes(), f)?;
		args.iter().map(move |arg| {
		    use fmt::Write;
		    f.write_char(' ').and_then(|_| match arg.as_ref() {
			Some(arg) => quote_into::<b'"'>(arg.as_bytes(), f),
			None => f.write_str(POSITIONAL_ARG_STRING),
		    })
		}).collect()
	    },
	}
    }
}


impl ExecMode {
    #[inline(always)] 
    pub fn is_positional(&self) -> bool
    {
	if let Self::Positional { .. } = &self {
	    true
	} else {
	    false
	}
    }
    #[inline(always)] 
    pub fn is_stdin(&self) -> bool
    {
	!self.is_positional()
    }
    
    #[inline(always)] 
    pub fn command(&self) -> &OsStr
    {
	match self {
	    Self::Positional { command, .. } |
	    Self::Stdin { command, .. } =>
		command.as_os_str()
	}
    }

    /// Returns an iterator over the arguments.
    ///
    /// Its output type is `Option<&OsStr>`, because the variant may be `Positional`. If it is instead `Stdin`, all values yielded will be `Some()`.
    #[inline] 
    pub fn arguments(&self) -> impl Iterator<Item = Option<&'_ OsStr>>
    {
	#[derive(Debug, Clone)]
	struct ArgIter<'a>(Result<std::slice::Iter<'a, Option<OsString>>, std::slice::Iter<'a, OsString>>);
	

	impl<'a> Iterator for ArgIter<'a>
	{
	    type Item = Option<&'a OsStr>;
	    
	    #[inline(always)] 
	    fn next(&mut self) -> Option<Self::Item>
	    {
		Some(match &mut self.0 {
		    Err(n) => Some(n.next()?.as_os_str()),
		    Ok(n) => n.next().map(|x| x.as_ref().map(|x| x.as_os_str()))?
		})
	    }

	    #[inline(always)] 
	    fn size_hint(&self) -> (usize, Option<usize>) {
		match &self.0 {
		    Err(n) => n.size_hint(),
		    Ok(n) => n.size_hint()
		}
	    }
	}
	impl<'a> ExactSizeIterator for ArgIter<'a>{}
	impl<'a> iter::FusedIterator for ArgIter<'a>{}

	ArgIter(match self {
	    Self::Positional { args, .. } => Ok(args.iter()),
	    Self::Stdin {  args, .. } => Err(args.iter())
	})
    }

    /// Returns a tuple of `(command, args)`.
    ///
    /// # Modes
    /// * When invariant is `Stdin`, `positional` is ignored and can be `iter::empty()` or an empty array. If it is not, it is still ignored.
    /// * When invariant is `Positional`, `positional` is iterated on for every instance a positional argument should appear.
    ///   If the iterator completes and there are positional arguments left, they are removed from the iterator's output, and the next argument is shifted along. `iter::repeat(arg)` can be used to insert the same argument into each instance where a positional argument is expected.
    #[inline] 
    pub fn into_process_info<T, I>(self, positional: I) -> (OsString, ExecModeArgIterator<I>)
    where I: IntoIterator<Item=OsString>,
    {
	
	match self {
	    Self::Stdin { command, args } => (command, ExecModeArgIterator::Stdin(args.into_iter())),
	    Self::Positional { command, args } => (command,
						   ExecModeArgIterator::Positional(ArgZippingIter(args.into_iter(),
												  positional.into_iter().fuse()))),
	}
    }

    /// # Panics
    /// If the invariant of the enum was `Positional`.
    #[inline] 
    pub fn into_process_info_stdin(self) -> (OsString, ExecModeArgIterator<NoPositionalArgs>)
    {
	#[cold]
	#[inline(never)] 
	fn _panic_invalid_invariant() -> !
	{
	    panic!("Invalid invariant for ExecMode: Expected `Stdin`, was `Positional`.")
	}
	match self {
	    Self::Stdin { command, args } => (command, ExecModeArgIterator::Stdin(args.into_iter())),
	    _ => _panic_invalid_invariant()
	}
    }
}

pub struct ArgZippingIter<T>(std::vec::IntoIter<Option<OsString>>, iter::Fuse<T::IntoIter>)
where T: IntoIterator<Item = OsString>;

/// Private trait used to mark an instantiation of `ExecModeArgIterator<T>` as not ever being the `Positional` invariant.
unsafe trait NoPositional{}
pub enum NoPositionalArgs{}
impl Iterator for NoPositionalArgs
{
    type Item = OsString;
    fn next(&mut self) -> Option<Self::Item>
    {
	match *self{}
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
	(0, Some(0))
    }
}
unsafe impl NoPositional for NoPositionalArgs{}
unsafe impl NoPositional for std::convert::Infallible{}
impl ExactSizeIterator for NoPositionalArgs{}
impl DoubleEndedIterator for NoPositionalArgs
{
    fn next_back(&mut self) -> Option<Self::Item> {
	match *self{}
    }
}
impl iter::FusedIterator for NoPositionalArgs{}
impl From<std::convert::Infallible> for NoPositionalArgs
{
    fn from(from: std::convert::Infallible) -> Self
    {
	match from{}
    }
}

pub enum ExecModeArgIterator<T: IntoIterator<Item = OsString>> {
    Stdin(std::vec::IntoIter<OsString>),
    Positional(ArgZippingIter<T>),
}

impl<I> Iterator for ExecModeArgIterator<I>
where I: IntoIterator<Item = OsString>
{
    type Item = OsString;
    #[inline] 
    fn next(&mut self) -> Option<Self::Item>
    {
	loop {
	    break match self {
		Self::Stdin(vec) => vec.next(),
		Self::Positional(ArgZippingIter(ref mut vec, ref mut pos)) => {
		    match vec.next()? {
			None => {
			    match pos.next() {
				None => continue,
				replace => replace,
			    }
			},
			set => set,
		    }
		},
	    }
	}
    }
    #[inline(always)] 
    fn size_hint(&self) -> (usize, Option<usize>) {
	match self {
	    Self::Stdin(vec, ..) => vec.size_hint(),
	    Self::Positional(ArgZippingIter(vec, ..)) => vec.size_hint(),
	}
    }
}
impl<I> iter::FusedIterator for ExecModeArgIterator<I>
where I: IntoIterator<Item = OsString>{}
// ExecModeArgIterator can never be FixedSizeIterator if it is *ever* `Positional`
impl<I: NoPositional> ExactSizeIterator for ExecModeArgIterator<I>
where I: IntoIterator<Item = OsString>{}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Options {
    /// For `-exec` (stdin exec) and `-ecec{}` (positional exec)
    exec: Vec<ExecMode>,
}

impl Options
{
    #[inline(always)] 
    fn count_exec(&self) -> (usize, usize)
    {
	self.exec.is_empty().then(|| (0, 0))
	    .or_else(move ||
		     self.exec.iter().map(|x| {
			 x.is_positional().then(|| (0, 1)).unwrap_or((1, 0))
		     })
		     .reduce(|(s, p), (s1, p1)| (s + s1, p + p1)))
	    .unwrap_or((0,0))
    }
    /// Has `-exec` (stdin) or `-exec{}` (positional)
    ///
    /// Tuple element 1 is for `-exec`; element 2 is for `-exec{}`.
    #[inline(always)] 
    pub fn has_exec(&self) -> (bool, bool)
    {
	self.exec.is_empty().then(|| (false, false))
	    .or_else(move || 
		     self.exec.iter().map(|x| {
			 let x = x.is_positional();
			 (!x, x)
		     })
		     .reduce(|(s, p), (s1, p1)| (s || s1, p || p1)))
	    .unwrap_or((false, false))
    }
    #[inline] 
    pub fn has_positional_exec(&self) -> bool
    {
	self.has_exec().1
    }
    #[inline]
    pub fn has_stdin_exec(&self) -> bool
    {
	self.has_exec().0
    }

    #[inline] 
    pub fn opt_exec(&self) -> impl Iterator<Item= &'_ ExecMode> + ExactSizeIterator + iter::FusedIterator + DoubleEndedIterator
    {
	self.exec.iter()
    }
    #[inline] 
    pub fn into_opt_exec(self) -> impl Iterator<Item=ExecMode> + ExactSizeIterator + iter::FusedIterator
    {
	self.exec.into_iter()
    }
}

/// The executable name of this program.
///
/// # Returns
/// * If the program's executable name is a valid UTF8 string, that string.
/// * If it is not, then that string is lossily-converted to a UTF8 string, with invalid characters replaced accordingly. This can be checked by checking if the return value is `Cow::Owned`, if it is, then this is not a reliable indication of the exetuable path's basename.
/// * If there is no program name provided, i.e. if `argc == 0`, then an empty string is returned.
#[inline(always)] 
pub fn program_name() -> Cow<'static, str>
{
    lazy_static! {
	static ref NAME: OsString = std::env::args_os().next().unwrap_or(OsString::from_vec(Vec::new()));
    }
    String::from_utf8_lossy(NAME.as_bytes())
}

/// Parse the program's arguments into an `Options` array.
/// If parsing fails, an `ArgParseError` is returned detailing why it failed.
#[inline] 
#[cfg_attr(feature="logging", instrument(err))]
pub fn parse_args() -> Result<Options, ArgParseError>
{
    parse_from(std::env::args_os().skip(1))
}

#[cfg_attr(feature="logging", instrument(level="debug", skip_all, fields(args = ?std::any::type_name::<I>())))]
fn parse_from<I, T>(args: I) -> Result<Options, ArgParseError>
where I: IntoIterator<Item = T>,
      T: Into<OsString>
{
    mod warnings {
	use super::*;
	/// Issue a warning when `-exec{}` is provided as an argument, but no positional arguments (`{}`) are specified in the argument list to the command.
	#[cold]
	#[cfg_attr(feature="logging", inline(never), instrument(level="trace"))]
	#[cfg_attr(not(feature="logging"), inline(always))]
	pub fn execp_no_positional_replacements()
	{
	    if_trace!(warn!("-exec{{}} provided with no positional arguments ({}), there will be no replacement with the data. Did you mean `-exec`?", POSITIONAL_ARG_STRING));
	}
	/// Issue a warning if the user apparently meant to specify two `-exec/{}` arguments to `collect`, but seemingly is accidentally is passing the `-exec/{}` string as an argument to the first.
	#[cold]
	#[cfg_attr(feature="logging", inline(never), instrument(level="trace"))]
	#[cfg_attr(not(feature="logging"), inline(always))]
	pub fn exec_apparent_missing_terminator(first_is_positional: bool, second_is_positional: bool, command: &str, argument_number: usize)
	{
	    if_trace! {
		warn!("{} provided, but argument to command {command:?} number {argument_number} is {}. Are you missing the terminator before '{}' before this argument?", if first_is_positional {"-exec{{}}"} else {"-exec"}, if second_is_positional {"-exec{{}}"} else {"-exec"}, EXEC_MODE_STRING_TERMINATOR)
	    }
	}	
    }
    
    let mut args = args.into_iter().map(Into::into);
    //XXX: When `-exec{}` is provided, but no `{}` arguments are found, maybe issue a warning with `if_trace!(warning!())`? There are valid situations to do this in, but they are rare...
    todo!("//TODO: Parse `args` into `Options`")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArgParseError
{
    /// Returned when an invalid or unknown argument is found
    UnknownOption(OsString),
    /// Returned when the argument, `argument`, is passed in an invalid context by the user.
    InvalidUsage { argument: String, message: String },
}

impl error::Error for ArgParseError{}
impl fmt::Display for ArgParseError
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	match self {
	    Self::UnknownOption(opt) => f.write_str(String::from_utf8_lossy(opt.as_bytes()).as_ref()),
	    Self::InvalidUsage { argument, message } => write!(f, "Invalid usage for argument `{argument}`: {message}")
	}
    }
}
