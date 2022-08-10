//! For handling arguments.
use super::*;
use std::ffi::{
    OsStr,
    OsString,
};
use std::iter;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExecMode
{
    Stdin{command: OsString, args: Vec<OsString>},
    Positional{command: OsString, args: Vec<Option<OsString>>},
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
	self.exec.iter().map(|x| {
	    x.is_positional().then(|| (0, 1)).unwrap_or((1, 0))
	})
	    .reduce(|(s, p), (s1, p1)| (s + s1, p + p1))
	    .unwrap_or((0,0))
    }
    /// Has `-exec` (stdin) or `-exec{}` (positional)
    ///
    /// Tuple element 1 is for `-exec`; element 2 is for `-exec{}`.
    #[inline(always)] 
    pub fn has_exec(&self) -> (bool, bool)
    {
	self.exec.iter().map(|x| {
	    let x = x.is_positional();
	    (!x, x)
	})
	    .reduce(|(s, p), (s1, p1)| (s || s1, p || p1))
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

