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
use std::any::type_name;
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
#[cfg_attr(feature="logging", instrument(err(Debug)))]
pub fn parse_args() -> Result<Options, ArgParseError>
{
    let iter = std::env::args_os();
    if_trace!(trace!("argc == {}, argv == {iter:?}", iter.len()));
    
    parse_from(iter.skip(1))
}

#[inline(always)] 
pub fn type_name_short<T: ?Sized>() -> &'static str
{
    let mut s = std::any::type_name::<T>();
    if let Some(idx) = memchr::memrchr(b':', s.as_bytes()) {
	s = &s[idx.saturating_sub(1)..];
	if s.len() >= 2 && &s[..2] == "::" {
	    s = &s[2..];
	}
    }
    if s.len() > 0 && (s.as_bytes()[s.len()-1] == b'>' && s.as_bytes()[0] != b'<') {
	s = &s[..(s.len()-1)];
    }
    s
}

#[cfg_attr(feature="logging", instrument(level="debug", skip_all, fields(args = ?type_name_short::<I>())))]
fn parse_from<I, T>(args: I) -> Result<Options, ArgParseError>
where I: IntoIterator<Item = T>,
      T: Into<OsString>
{   
    let mut args = args.into_iter().map(Into::into);
    let mut output = Options::default();
    let mut idx = 0;
    //XXX: When `-exec{}` is provided, but no `{}` arguments are found, maybe issue a warning with `if_trace!(warning!())`? There are valid situations to do this in, but they are rare...
    let mut parser = || -> Result<_, ArgParseError> {
	while let Some(mut arg) = args.next() {
	    idx += 1;
	    macro_rules! try_parse_for {
		(@ assert_parser_okay $parser:path) => {
		    const _:() = {
			const fn _assert_is_parser<P: TryParse + ?Sized>() {}
			const fn _assert_is_result<P: TryParse + ?Sized>(res: P::Output) -> P::Output { res }
			
			_assert_is_parser::<$parser>();
		    };
		};
		($parser:path => $then:expr) => {
		    {
			try_parse_for!(@ assert_parser_okay $parser);
			//_assert_is_closure(&$then); //XXX: There isn't a good way to tell without having to specify some bound on return type...
			if let Some(result) = parsers::try_parse_with::<$parser>(&mut arg, &mut args) {
			    
			    // Result succeeded on visitation, use this parser for this argument and then continue to the next one.
			    $then(result?);
			    continue;
			}
			// Result failed on visitation, so continue the control flow to the next `try_parse_for!()` visitation attempt.
		    }
		};
		/*($parser:path => $then:expr) => {
		    $then(try_parse_for!(try $parser => std::convert::identity)?)
		}*/
	    }	    
	    //TODO: Add `impl TryParse` struct for `--help` and add it at the *top* of the visitation stack (it will most likely appear there.)
	    // This may require a re-work of the `Options` struct, or an enum wrapper around it should be returned instead of options directly, for special modes (like `--help` is, etc.) Perhaps `pub enum Mode { Normal(Options), Help, }` or something should be returned, and `impl From<Options>` for it, with the caller of this closure (below) 
	    try_parse_for!(parsers::ExecMode => |result| output.exec.push(result));
	    
	    //Note: try_parse_for!(parsers::SomeOtherOption => |result| output.some_other_option.set(result.something)), etc, for any newly added arguments.
	    
	    if_trace!(debug!("reached end of parser visitation for argument #{idx} {arg:?}! Failing now with `UnknownOption`"));
	    return Err(ArgParseError::UnknownOption(arg));
	}
	Ok(())
    };
    parser()
	.with_index(idx)
	.map(move |_| output.into()) //XXX: This is `output.into()`, because when successful result return type is changed from directly `Options` to `enum Mode` (which will `impl From<Options>`), it will allow any `impl Into<Mode>` to be returned. (Boxed dynamic dispatch with a trait `impl FromMode<T: ?Sized> (for Mode) { fn from(val: Box<T>) -> Self { IntoMode::into(val) } }, auto impl trait IntoMode { fn into(self: Box<Self>) -> Mode }` may be required if different types are returned from the closure, this is okay, as argument parsed struct can get rather large.)
}

#[derive(Debug)]
pub enum ArgParseError
{
    /// With an added argument index.
    WithIndex(usize, Box<ArgParseError>),
    /// Returned when an invalid or unknown argument is found
    UnknownOption(OsString),
    /// Returned when the argument, `argument`, is passed in an invalid context by the user.
    InvalidUsage { argument: String, message: String, inner: Option<Box<dyn error::Error + Send + Sync + 'static>> },
    //VisitationFailed,
    
}

trait ArgParseErrorExt<T>: Sized
{
    fn with_index(self, idx: usize) -> Result<T, ArgParseError>;
}
impl ArgParseError
{
    #[inline] 
    pub fn wrap_index(self, idx: usize) -> Self {
	Self::WithIndex(idx, Box::new(self))
    }
}
impl<T, E: Into<ArgParseError>> ArgParseErrorExt<T> for Result<T, E>
{
    #[inline(always)] 
    fn with_index(self, idx: usize) -> Result<T, ArgParseError> {
	self.map_err(Into::into)
	    .map_err(move |e| e.wrap_index(idx))
    }
}

impl error::Error for ArgParseError
{
    #[inline] 
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
	match self {
	    Self::InvalidUsage { inner, .. } => inner.as_ref().map(|x| -> &(dyn error::Error + 'static) {  x.as_ref() }),
	    Self::WithIndex(_, inner) => inner.source(),
	    _ => None,
	}
    }
}
impl fmt::Display for ArgParseError
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
	match self {
	    Self::WithIndex(index, inner) => write!(f, "Argument #{index}: {inner}"),
	    Self::UnknownOption(opt) => {
		f.write_str("Invalid/unknown argument: `")?;
		f.write_str(String::from_utf8_lossy(opt.as_bytes()).as_ref())?;
		f.write_str("`")
	    },
	    Self::InvalidUsage { argument, message, .. } => write!(f, "Invalid usage for argument `{argument}`: {message}")
	}
    }
}

trait ArgError: error::Error + Send + Sync + 'static
{
    fn into_invalid_usage(self) -> (String, String, Box<dyn error::Error + Send + Sync + 'static>)
    where Self: Sized;
}

trait TryParse: Sized
{
    type Error: ArgError;
    type Output;
    
    #[inline(always)] 
    fn visit(argument: &OsStr) -> Option<Self> { let _ = argument;  None }
    fn parse<I: ?Sized>(self, argument: OsString, rest: &mut I) -> Result<Self::Output, Self::Error>
    where I: Iterator<Item = OsString>;
}

impl<E: error::Error + Send + Sync + 'static> From<(String, String, E)> for ArgParseError
{
    #[inline] 
    fn from((argument, message, inner): (String, String, E)) -> Self
    {
	Self::InvalidUsage { argument, message, inner: Some(Box::new(inner)) }
    }
}

impl<E: ArgError> From<E> for ArgParseError
{
    #[inline(always)] 
    fn from(from: E) -> Self
    {
	let (argument, message, inner) = from.into_invalid_usage();
	Self::InvalidUsage { argument, message, inner: Some(inner) }
    }
}

#[inline(always)] 
fn extract_last_pathspec<'a>(s: &'a str) -> &'a str
{
    //#[cfg_attr(feature="logging", feature(instrument(ret)))]
    #[allow(dead_code)]
    fn string_diff<'a>(a: &'a str, b: &'a str) -> usize
    {
	#[cold]
	#[inline(never)]
	fn _panic_non_inclusive(swap: bool) -> !
	{
	    let a = swap.then(|| "b").unwrap_or("a");
	    let b = swap.then(|| "a").unwrap_or("b");
	    panic!("String {a} was not inside string {b}")
	}
	let a_addr = a.as_ptr() as usize;
	let b_addr = b.as_ptr() as usize;
	let (a_addr, b_addr, sw) = 
	    if !(a_addr + a.len() > b_addr + b.len() && b_addr + b.len() < a_addr + a.len()) {
		(b_addr, a_addr, true)
	    } else {
		(a_addr, a_addr, false)
	    };
	
	if b_addr < a_addr /*XXX || (b_addr + b.len()) > (a_addr + a.len())*/ {
	    _panic_non_inclusive(sw)
	}
	return a_addr.abs_diff(b_addr);
    }
    s.rsplit_once("::")
	.map(|(_a, b)| /*XXX: This doesn't work...match _a.rsplit_once("::") {
	     Some((_, last)) => &s[string_diff(s, last)..],
	     _ => b
	}*/ b)
	.unwrap_or(s)
}

mod parsers {
    use super::*;

    #[inline(always)]
    #[cfg_attr(feature="logging", instrument(level="debug", skip(rest), fields(parser = %extract_last_pathspec(type_name::<P>()))))]
    pub(super) fn try_parse_with<P>(arg: &mut OsString, rest: &mut impl Iterator<Item = OsString>) -> Option<Result<P::Output, ArgParseError>>
    where P: TryParse
    {
	#[cfg(feature="logging")] 
	let _span = tracing::warn_span!("parse", parser= %extract_last_pathspec(type_name::<P>()), ?arg);
	P::visit(arg.as_os_str()).map(move |parser| {
	    #[cfg(feature="logging")]
	    let _in = _span.enter();
	    parser.parse(/*if_trace!{true arg.clone(); */std::mem::replace(arg, OsString::default()) /*}*/, rest).map_err(Into::into) //This clone is not needed, the argument is captured by `try_parse_with` (in debug) and `parse` (in warning) already.
	}).map(|res| {
	    #[cfg(feature="logging")]
	    match res.as_ref() {
		Err(err) => {
		    ::tracing::event!(::tracing::Level::ERROR, ?err, "Attempted parse failed with error")
		},
		_ => ()
	    }
	    res
	}).or_else(|| {
	    #[cfg(feature="logging")]
	    ::tracing::event!(::tracing::Level::TRACE, "no match for this parser with this arg, continuing visitation.");
	    None
	})
    }

    /// Parser for `ExecMode`
    ///
    /// Parses `-exec` / `-exec{}` modes.
    #[derive(Debug, Clone, Copy)]
    pub enum ExecMode {
	Stdin,
	Postional,
    }
    impl ExecMode {
	#[inline(always)] 
	fn is_positional(&self) -> bool
	{
	    match self {
		Self::Postional => true,
		_ => false
	    }
	}
	#[inline(always)] 
	fn command_string(&self) -> &'static str
	{
	    if self.is_positional() {
		"-exec{}"
	    } else {
		"-exec"
	    }
	}
	
    }
    
    #[derive(Debug)]
    pub struct ExecModeParseError(ExecMode);
    impl error::Error for ExecModeParseError{}
    impl fmt::Display for ExecModeParseError
    {
	#[inline(always)]
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
	{
	    write!(f, "{} needs at least a command", self.0.command_string())
	}
    }

    impl ArgError for ExecModeParseError
    {
	fn into_invalid_usage(self) -> (String, String, Box<dyn error::Error + Send + Sync + 'static>)
	where Self: Sized {
	    (self.0.command_string().to_owned(), "Expected a command file-path to execute.".to_owned(), Box::new(self))
	}
    }

    impl TryParse for ExecMode
    {
	type Error = ExecModeParseError;
	type Output = super::ExecMode;
	#[inline(always)] 
	fn visit(argument: &OsStr) -> Option<Self> {
	    
	    if argument == OsStr::from_bytes(b"-exec") {
		Some(Self::Stdin)
	    } else if argument == OsStr::from_bytes(b"-exec{}") {
		Some(Self::Postional)
	    } else {
		None
	    }
	}

	#[inline] 
	fn parse<I: ?Sized>(self, _argument: OsString, rest: &mut I) -> Result<Self::Output, Self::Error>
	where I: Iterator<Item = OsString> {
	    mod warnings {
		use super::*;
		/// Issue a warning when `-exec{}` is provided as an argument, but no positional arguments (`{}`) are specified in the argument list to the command.
		#[cold]
		#[cfg_attr(feature="logging", inline(never))]
		#[cfg_attr(not(feature="logging"), inline(always))]
		pub fn execp_no_positional_replacements()
		{
		    if_trace!(warn!("-exec{{}} provided with no positional arguments `{}`, there will be no replacement with the data. Did you mean `-exec`?", POSITIONAL_ARG_STRING));
		}
		/// Issue a warning if the user apparently meant to specify two `-exec/{}` arguments to `collect`, but seemingly is accidentally is passing the `-exec/{}` string as an argument to the first.
		#[cold]
		#[cfg_attr(feature="logging", inline(never))]
		#[cfg_attr(not(feature="logging"), inline(always))]
		pub fn exec_apparent_missing_terminator(first_is_positional: bool, second_is_positional: bool, command: &OsStr, argument_number: usize)
		{
		    if_trace! {
			warn!("{} provided, but argument to command {command:?} number #{argument_number} is `{}`. Are you missing the terminator '{}' before this argument?", if first_is_positional {"-exec{}"} else {"-exec"}, if second_is_positional {"-exec{}"} else {"-exec"}, EXEC_MODE_STRING_TERMINATOR)
		    }
		}

		/// Issue a warning if the user apparently missed a command to `-exec{}`, and has typed `-exec{} {}`...
		#[cold]
		#[cfg_attr(feature="logging", inline(never))]
		#[cfg_attr(not(feature="logging"), inline(always))]
		//TODO: Do we make this a feature in the future? Being able to `fexecve()` the input received?
		pub fn execp_command_not_substituted()
		{
		    if_trace! {
			warn!("-exec{{}} provided with a command as the positional replacement string `{}`. Commands are not substituted. Are you missing a command? (Note: Currently, `fexecve()`ing the input is not supported.)", POSITIONAL_ARG_STRING)
		    }
		}
		
		/// Issue a warning if the user apparently attempted to terminate a `-exec/{}` argument, but instead made the command of the `-exec/{}` itself the terminator.
		#[cold]
		#[cfg_attr(feature="logging", inline(never))]
		#[cfg_attr(not(feature="logging"), inline(always))]
		pub fn exec_terminator_as_command(exec_arg_str: &str)
		{
		    if_trace! {
			warn!("{exec_arg_str} provided with a command that is the -exec/-exec{{}} terminator character `{}`. The sequence is not terminated, and instead the terminator character itself is taken as the command to execute. Did you miss a command before the terminator?", EXEC_MODE_STRING_TERMINATOR)
		    }
		}
	    }
	    
	    let command = rest.next().ok_or_else(|| ExecModeParseError(self))?;
	    if command == EXEC_MODE_STRING_TERMINATOR {
		warnings::exec_terminator_as_command(self.command_string());
	    }
	    let test_warn_missing_term = |(idx , string) : (usize, OsString)| {
		if let Some(val) = Self::visit(&string) {
		    warnings::exec_apparent_missing_terminator(self.is_positional(), val.is_positional(), &command, idx + 1);
		}
		string
	    };
	    Ok(match self {
		Self::Stdin => {
		    super::ExecMode::Stdin {
			args: rest
			    .take_while(|argument| argument.as_bytes() != EXEC_MODE_STRING_TERMINATOR.as_bytes())
			    .enumerate().map(&test_warn_missing_term)
			    .collect(),
			command,
		    }
		},
		Self::Postional => {
		    let mut repl_warn = true;
		    if command == POSITIONAL_ARG_STRING {
			warnings::execp_command_not_substituted();
		    }
		    let res = super::ExecMode::Positional {
			args: rest
			    .take_while(|argument| argument.as_bytes() != EXEC_MODE_STRING_TERMINATOR.as_bytes())
			    .enumerate().map(&test_warn_missing_term)
			    .map(|x| if x.as_bytes() == POSITIONAL_ARG_STRING.as_bytes() {
				repl_warn = false;
				None
			    } else {
				Some(x)
			    })
			    .collect(),
			command,
		    };
		    if repl_warn { warnings::execp_no_positional_replacements(); }
		    res
		},
	    })
	}
    }
}
