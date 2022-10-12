
#[macro_use] extern crate cfg_if;
#[cfg(feature="logging")] 
#[macro_use] extern crate tracing;

//#[cfg(feature="memfile")] 
#[macro_use] extern crate lazy_static;

/// Run this statement only if `tracing` is enabled
macro_rules! if_trace {
    (? $expr:expr) => {
	cfg_if! {
	    if #[cfg(all(feature="logging", debug_assertions))] {
		$expr;
	    }
	}
    };
    ($expr:expr) => {
	cfg_if! {
	    if #[cfg(feature="logging")] {
		$expr;
	    }
	}
    };
    (true $yes:expr$(; $no:expr)?) => {
	{
	    #[allow(unused_variables)]
	    {
		let val = cfg!(feature="logging");
		#[cfg(feature="logging")]
		let val = { $yes };
		$(
		    #[cfg(not(feature="logging"))]
		    let val = { $no };
		    )?
		val
	    }
	}
    };
}

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

#[allow(unused_imports)]
use color_eyre::{
    eyre::{
	self,
	eyre,
	WrapErr,
    },
    Section,
    SectionExt, Help,
};

/// Get an `&'static str` of the current function name.
macro_rules! function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            ::std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }}
}

mod ext; use ext::*;
mod errors;
mod sys;
use sys::{
    try_get_size,
    tell_file,
};

mod exec;

mod buffers;
use buffers::prelude::*;

#[cfg(feature="memfile")] mod memfile;

#[cfg(feature="bytes")]
use bytes::{
    Buf,
    BufMut,
};

/* TODO: Allow `collect -exec <command>` /proc/self/fds/<memfd OR STDOUT_FILENO>, `collect -exec{} <command> {/proc/self/fds/<memfd OR STDOUT_FILENO>} <other args>`
struct Options {
/// If the arguments vector contains `None`, that `None` shall be replaced with the string referring to: If in memfd mode: The `memfd_create()` buffer fd, set to RW, truncated to size, seeked to 0. In this mode, the file will remain open when `ecec` is not `None`, and will instead be returned below, as the mode methods will all be modified to return `Option<Box<dyn ModeReturn + 'static>>::Some(<private struct: impl ModeReturn>)`, which will contain the memfd object so it is dropped *after* the child process has exited. If the mode is *not* memfd, then `STDOUT_FILENO` itself will be used; also set to RW, truncated correctly, and seeked to 0. The return code of this process shall be the return code of the child process once it has terminated.
/// Execution of commands (if passed) **always** happens *after* the copy to `stdout`, but *before* the **close** of `stdout`. If the copy to `stdout` fails, the exec will not be executed regardless of if the mode required is actually using `stdout`.
/// The process shall always wait for the child to terminate before exiting. If the child daemon forks, that fork is not followed, and the process exists anyway.
/// Ideally, A `SIGHUP` handler should be registered, which tells the parent to stop waiting on the child and exit now. TODO: The behaviour of the child is unspecified if this happens. It may be killed, or re-attached to `init`. But the return code of the parent should always be `0` in this case. 
exec: Option<(OSString, Vec<Option<OSString>>)> 
}
trait ModeReturn: Send {
fn get_fd_path(&self) -> &Path;
}
struct BufferedReturn;
impl ModeReturn for BufferedReturn { fn get_fd_str(&self) -> &OsStr{ static_assert(STDOUT_FILENO == 1);  "/proc/self/fds/1"  }} /* XXX: In the case where the (compile time) check of STDOUT_FILENO == 0 fails, another boxed struct containing the OSString with the correct path that `impl ModeReturn` can be returned, this path will be removed by the compiler if `STDOUT_FILENO != 1`, allowing for better unboxing analysis. */
 */

mod args;

trait ModeReturn: Send {
    
}

fn init() -> eyre::Result<()>
{
    cfg_if!{ if #[cfg(feature="logging")] {
	fn install_tracing()
	{
	    //! Install spantrace handling
	    
	    use tracing_error::ErrorLayer;
	    use tracing_subscriber::prelude::*;
	    use tracing_subscriber::{fmt, EnvFilter};

	    let fmt_layer = fmt::layer()
		.with_target(false)
		.with_writer(io::stderr);
	    
	    let filter_layer = EnvFilter::try_from_default_env()
		.or_else(|_| EnvFilter::try_new(if cfg!(debug_assertions) {
		    "debug"
		} else {
		    "info"
		}))
		.unwrap();

	    tracing_subscriber::registry()
		.with(fmt_layer)
		.with(filter_layer)
		.with(ErrorLayer::default())
		.init();
	}

	if !cfg!(feature="disable-logging") {
	    install_tracing();
	    if_trace!(trace!("installed tracing"));
	}
    } }
    
    color_eyre::install()
}

#[cfg_attr(feature="logging", instrument(err))]
#[inline]
fn feature_check() -> eyre::Result<()>
{
    if cfg!(feature="memfile") && cfg!(feature="mode-buffered") {
	if_trace!(warn!("This is an incorrectly compiled binary! Compiled with `mode: buffered` and the `memfile` feature; `memfile` stragery will be used and the mode selection will be ignored."));
    }

    Ok(())
}

mod work {
    use super::*;
    #[cfg_attr(feature="logging", instrument(err))]
    #[inline] 
    pub(super) fn buffered() -> eyre::Result<()>
    {
	if_trace!(info!("strategy: allocated buffer"));
	
	let (bytes, read) = {
	    let stdin = io::stdin();
	    let mut bytes: buffers::DefaultMut = try_get_size(&stdin).create_buffer();
	    
	    let read = io::copy(&mut stdin.lock(), &mut (&mut bytes).writer())
		.with_section(|| bytes.len().header("Buffer size is"))
		.with_section(|| bytes.capacity().header("Buffer cap is"))
		.with_section(|| format!("{:?}", bytes).header("Buffer is"))
		.wrap_err("Failed to read into buffer")?;
	    (bytes.freeze(), read as usize)
	};
	if_trace!(info!("collected {read} from stdin. starting write."));

	let written = 
	    io::copy(&mut (&bytes[..read]).reader() , &mut io::stdout().lock())
	    .with_section(|| read.header("Bytes read"))
	    .with_section(|| bytes.len().header("Buffer length (frozen)"))
	    .with_section(|| format!("{:?}", &bytes[..read]).header("Read Buffer"))
	    .with_section(|| format!("{:?}", bytes).header("Full Buffer"))
	    .wrap_err("Failed to write from buffer")?;
	if_trace!(info!("written {written} to stdout."));

	if read != written as usize {
	    return Err(io::Error::new(io::ErrorKind::BrokenPipe, format!("read {read} bytes, but only wrote {written}")))
		.wrap_err("Writing failed: size mismatch");
	}
	
	Ok(())
    }

    #[cfg_attr(feature="logging", instrument(err))]
    #[inline]
    #[cfg(feature="memfile")]
    //TODO: We should establish a max memory threshold for this to prevent full system OOM: Output a warning message if it exceeeds, say, 70-80% of free memory (not including used by this program (TODO: How do we calculate this efficiently?)), and fail with an error if it exceeds 90% of memory... Or, instead of using free memory as basis of the requirement levels on the max size of the memory file, use max memory? Or just total free memory at the start of program? Or check free memory each time (slow!! probably not this one...). Umm... I think basing it off total memory would be best; perhaps make the percentage levels user-configurable at compile time (and allow the user to set the memory value as opposed to using the total system memory at runtime.) or runtime (compile-time preffered; use that crate that lets us use TOML config files at comptime (find it pretty easy by looking through ~/work's rust projects, I've used it before.))
    pub(super) fn memfd() -> eyre::Result<()>
    {
	const DEFAULT_BUFFER_SIZE: fn () -> Option<std::num::NonZeroUsize> = || {
	    cfg_if!{ 
		if #[cfg(feature="memfile-preallocate")]  {
		    extern "C" {
			fn getpagesize() -> libc::c_int;
		    }
		    unsafe { std::num::NonZeroUsize::new(getpagesize() as usize * 8) }
		} else {
		    std::num::NonZeroUsize::new(0)
		}
	    }
	};
	
	if_trace!(info!("strategy: mapped memory file"));

	use std::borrow::Borrow;

	#[inline(always)] 
	fn unwrap_int_string<T, E>(i: impl Borrow<Result<T, E>>) -> String
	where T: std::fmt::Display,
	      E: std::fmt::Display
	{
	    i.borrow().as_ref().map(ToString::to_string)
		.unwrap_or_else(|e| format!("<unknown: {e}>"))
	}

	
	#[cfg_attr(feature="logging", instrument(skip_all, err, fields(i = ?i.as_raw_fd())))]
	    #[inline]
	fn truncate_file<S>(i: impl AsRawFd, to: S) -> eyre::Result<()>
	where S: TryInto<u64>,
	<S as TryInto<u64>>::Error: EyreError
	{
	    truncate_file_raw(i, to.try_into().wrap_err(eyre!("Size too large"))?)?;
	    Ok(())
	}
	
	fn truncate_file_raw(i: impl AsRawFd, to: impl Into<u64>) -> io::Result<()>
	{
	    use libc::ftruncate;
	    let fd = i.as_raw_fd();
	    let to = {
		let to = to.into();
		#[cfg(feature="logging")]
		let span_size_chk = debug_span!("chk_size", size = ?to);
		#[cfg(feature="logging")]
		let _span = span_size_chk.enter();
		
		if_trace!{
		    if to > i64::MAX as u64 {
			error!("Size too large (over max by {}) (max {})", to - (i64::MAX as u64), i64::MAX);
		    } else {
			trace!("Setting {fd} size to {to}");
		    }
		}
		
		if cfg!(debug_assertions) {
		    i64::try_from(to).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Size too large for ftruncate() offset"))?
		} else {
		    to as i64
		}
	    };
	    
	    match unsafe { ftruncate(fd, to) } {
		-1 => Err(io::Error::last_os_error()),
		_ => Ok(())
	    }
	}
	
	//TODO: How to `ftruncate()` stdout only once... If try_get_size succeeds, we want to do it then. If it doesn't, we want to do it when `stdin` as been consumed an we know the size of the memory-file... `RunOnce` won't work unless we can give it an argument....
	#[allow(unused_mut)]
	let mut set_stdout_len = {
	    cfg_if! {
		if #[cfg(feature="memfile-size-output")] {
		    if_trace!(warn!("Feature `memfile-size-output` is not yet stable and will cause crash."));
		    
		    const STDOUT: memfile::fd::RawFileDescriptor = unsafe { memfile::fd::RawFileDescriptor::new_unchecked(libc::STDOUT_FILENO) }; //TODO: Get this from `std::io::Stdout.as_raw_fd()` instead.
		    
		    use std::sync::atomic::{self, AtomicUsize};
		    #[cfg(feature="logging")]
		    let span_ro = debug_span!("run_once", stdout = ?STDOUT);

		    static LEN_HOLDER: AtomicUsize = AtomicUsize::new(0);
		    
		    let mut set_len = RunOnce::new(move || {
			#[cfg(feature="logging")]
			let _span = span_ro.enter();

			let len =  LEN_HOLDER.load(atomic::Ordering::Acquire);

			if_trace!(debug!("Attempting single `ftruncate()` on `STDOUT_FILENO` -> {len}"));
			truncate_file(STDOUT, len)
			    .wrap_err(eyre!("Failed to set length of stdout ({STDOUT}) to {len}"))
			    
		    });
		    
		    move |len: usize| {
			#[cfg(feature="logging")]
			let span_ssl = info_span!("set_stdout_len", len = ?len);
			
			#[cfg(feature="logging")]
			let _span = span_ssl.enter();

			if_trace!(trace!("Setting static-stored len for RunOnce"));
			
			LEN_HOLDER.store(len, atomic::Ordering::Release);
			
			if_trace!(trace!("Calling RunOnce for `set_stdout_len`"));
			match set_len.try_run() {
			    Some(result) => result
				.with_section(|| len.header("Attempted length set was"))
				.with_warning(|| libc::off_t::MAX.header("Max length is"))
				.with_note(|| STDOUT.header("STDOUT_FILENO is")),
			    None => {
				if_trace!(warn!("Already called `set_stdout_len()`"));
				Ok(())
			    },
			}
		    }
		} else {
		    |len: usize| -> Result<(), std::convert::Infallible> {
			#[cfg(feature="logging")]
			let span_ssl = info_span!("set_stdout_len", len = ?len);
			#[cfg(feature="logging")]
			let _span = span_ssl.enter();
			
			if_trace!(info!("Feature `memfile-size-output` is disabled; ignoring."));
			let _ = len;
			Ok(())
		    }
		}
	    }
	};

	let (mut file, read) = {
	    let stdin = io::stdin();

	    let buffsz = try_get_size(&stdin);
	    if_trace!(debug!("Attempted determining input size: {:?}", buffsz));
	    let buffsz = if cfg!(feature="memfile-size-output") {
		//TODO: XXX: Even if this actually works, is it safe to do this? Won't the consumer try to read `value` bytes before we've written them? Perhaps remove pre-setting entirely...
		match buffsz {
		    y @ Some(ref value) => {
			let value = value.get();
			
			set_stdout_len(value).wrap_err("Failed to set stdout len to that of stdin")
			    .with_section(|| value.header("Stdin len was calculated as"))
			    .with_warning(|| "This is a pre-setting")?;
			
			y
		    },
		    n => n,
		}
	    } else { buffsz }.or_else(DEFAULT_BUFFER_SIZE);
	    
	    if_trace!(if let Some(buf) = buffsz.as_ref() {
		trace!("Failed to determine input size: preallocating to {}", buf);
	    } else {
		trace!("Failed to determine input size: alllocating on-the-fly (no preallocation)");
	    });
	    
	    let mut file = memfile::create_memfile(Some("collect-buffer"), 
						   buffsz.map(|x| x.get()).unwrap_or(0))	    
		.with_section(|| format!("{:?}", buffsz).header("Deduced input buffer size"))
		.wrap_err(eyre!("Failed to create in-memory buffer"))?;

	    let read = io::copy(&mut stdin.lock(), &mut file)
		.with_section(|| format!("{:?}", file).header("Memory buffer file"))?;
	    
	    let read =  {
		use io::*;
		use std::borrow::Cow;

		let (read, sp, sl) = if cfg!(any(feature="memfile-preallocate", debug_assertions)) {
		    let sp = file.stream_position(); //TODO: XXX: Is this really needed?
		    let sl = memfile::stream_len(&file);
		    
		    if_trace!(trace!("Stream position after read: {:?}", sp));
		    if_trace!(trace!("Stream length after read: {:?}", sp));
		    
		    let read = match sp.as_ref() {
			Ok(&v) if v != read  => {
			    if_trace!(warn!("Reported read value not equal to memfile stream position: expected from `io::copy()`: {v}, got {read}"));
			    v
			},
			Ok(&x) => {
			    if_trace!(trace!("Reported memfile stream position and copy result equal: {x} == {}", read));
			    x
			},
			Err(e) => {
			    if_trace!(error!("Could not report memfile stream position, ignoring check on {read}: {e}"));
			    read
			},
		    };

		    let truncate_stream = |bad: u64, good: u64| {
			use std::num::NonZeroU64;
			file.set_len(good)
			    .map(|_| good)
			    .with_section(|| match NonZeroU64::new(bad) {Some (b) => Cow::Owned(b.get().to_string()), None => Cow::Borrowed("<unknown>") }.header("Original (bad) length"))
			    .with_section(|| good.header("New (correct) length"))
			    .wrap_err(eyre!("Failed to truncate stream to correct length")
				      .with_section(|| format!("{:?}", file).header("Memory buffer file")))
		    };
		    
		    let read = match sl.as_ref() {
			Ok(&v) if v != read  => {
			    if_trace!(warn!("Reported read value not equal to memfile stream length: expected from `io::copy()`: {read}, got {v}"));
			    if_trace!(debug!("Attempting to correct memfile stream length from {v} to {read}"));
			    
			    truncate_stream(v, read)?
			},
			Ok(&v) => {
			    if_trace!(trace!("Reported memfile stream length and copy result equal: {v} == {}", read));
			    v
			},
			Err(e) => {
			    if_trace!(error!("Could not report memfile stream length, ignoring check on {read}: {e}"));
			    if_trace!(warn!("Attempting to correct memfile stream length anyway"));
			    if let Err(e) = truncate_stream(0, read) {
				if_trace!(error!("Truncate failed: {e}"));
			    }
			    
			    read
			}
		    };
		    (read, Some(sp), Some(sl))
		} else {
		    (read, None, None)
		};

		file.seek(SeekFrom::Start(0))
		    .with_section(|| read.header("Actual read bytes"))
		    .wrap_err(eyre!("Failed to seek back to start of memory buffer file for output")
			      .with_section(move || if let Some(sp) = sp { Cow::Owned(unwrap_int_string(sp)) }
					    else { Cow::Borrowed("<unknown>")  }.header("Memfile position"))
			      .with_section(move || if let Some(sp) = sl { Cow::Owned(unwrap_int_string(sp)) }
					    else { Cow::Borrowed("<unknown>")  }.header("Memfile full length"))
			      /*.with_section(|| file.stream_len().map(|x| x.to_string())
			      .unwrap_or_else(|e| format!("<unknown: {e}>")).header("Memfile full length"))*/)?;
		
		read
	    };
	    
	    (file, usize::try_from(read)
	     .wrap_err(eyre!("Failed to convert read bytes to `usize`")
		       .with_section(|| read.header("Number of bytes was"))
		       .with_section(|| u128::abs_diff(read.into(), usize::MAX as u128).header("Difference between `read` and `usize::MAX` is"))
		       .with_suggestion(|| "It is likely you are running on a 32-bit ptr width machine and this input exceeds that of the maximum 32-bit unsigned integer value")
		       .with_note(|| usize::MAX.header("Maximum value of `usize`")))?)
	};
	if_trace!(info!("collected {} from stdin. starting write.", read));

	// TODO: XXX: Currently causes crash. But if we can get this to work, leaving this in is definitely safe (as opposed to the pre-setting (see above.))
	set_stdout_len(read)
	    .wrap_err(eyre!("Failed to `ftruncate()` stdout after collection of {read} bytes"))
	    .with_note(|| "Was not pre-set")?;	

	let written =
	    io::copy(&mut file, &mut io::stdout().lock())
	    .with_section(|| read.header("Bytes read from stdin"))
	    .with_section(|| unwrap_int_string(tell_file(&mut file)).header("Current buffer position"))
	    .wrap_err("Failed to write buffer to stdout")?;
	if_trace!(info!("written {written} to stdout."));

	if read != written as usize {
	    return Err(io::Error::new(io::ErrorKind::BrokenPipe, format!("read {read} bytes, but only wrote {written}")))
		.wrap_err("Writing failed: size mismatch");
	}
	
	Ok(())
    }
}

#[cfg_attr(feature="logging", instrument(err))]
#[inline(always)]
unsafe fn close_raw_fileno(fd: RawFd) -> io::Result<()>
{
    match libc::close(fd) {
	0 => Ok(()),
	_ => Err(io::Error::last_os_error()),
    }
}

#[inline]
#[cfg_attr(feature="logging", instrument(skip_all, fields(T = ?std::any::type_name::<T>())))]
fn close_fileno<T: IntoRawFd>(fd: T) -> eyre::Result<()>
{
    let fd = fd.into_raw_fd();
    if fd < 0 {
	return Err(eyre!("Invalid fd").with_note(|| format!("fds begin at 0 and end at {}", RawFd::MAX)));
    } else {
	if_trace!(debug!("closing consumed fd {fd}"));
	unsafe {
	    close_raw_fileno(fd)
	}.wrap_err("Failed to close fd")
	    .with_section(move || fd.header("Fileno was"))
	    .with_section(|| std::any::type_name::<T>().header(""))
    }
}

fn parse_args() -> eyre::Result<args::Options>
{
    args::parse_args()
	.wrap_err("Parsing arguments failed")
	.with_section(|| std::env::args_os().skip(1)
		      .map(|x| std::borrow::Cow::Owned(format!("{x:?}")))
		      .join_by_clone(std::borrow::Cow::Borrowed(" ")) //XXX: this can be replaced by `flat_map() -> [x, " "]` really... Dunno which will be faster...
		      .collect::<String>()
		      .header("Program arguments (argv+1) were"))
	.with_section(|| args::program_name().header("Program name (*argv) was"))
	.with_section(|| std::env::args_os().len().header("Total numer of arguments, including program name (argc) was"))
	.with_suggestion(|| "Try passing `--help`")
}

#[cfg_attr(feature="logging", instrument(err))]
fn main() -> errors::DispersedResult<()> {
    init()?;
    feature_check()?;
    if_trace!(debug!("initialised"));

    //TODO: How to cleanly feature-gate `args`?
    let opt = {
	#[cfg(feature="logging")]
	let _span = debug_span!("args");
	#[cfg(feature="logging")]
	let _in_span = _span.enter();
	let parsed = parse_args()?;
	if_trace!(debug!("Parsed arguments: {parsed:?}"));
	parsed
    };

    //TODO: maybe look into fd SEALing? Maybe we can prevent a consumer process from reading from stdout until we've finished the transfer. The name SEAL sounds like it might have something to do with that?
    cfg_if!{ 
	if #[cfg(feature="memfile")] {
	    work::memfd()
		.wrap_err("Operation failed").with_note(|| "Stragery was `memfd`")?;
	} else {
	    work::buffered()
		.wrap_err("Operation failed").with_note(|| "Strategy was `buffered`")?;
	}
    }
    // Transfer complete

    // Now that transfer is complete from buffer to `stdout`, close `stdout` pipe before exiting process.
    if_trace!(info!("Transfer complete, closing `stdout` pipe"));
    {
	let stdout_fd = libc::STDOUT_FILENO; // (io::Stdout does not impl `IntoRawFd`, just use the raw fd directly; using the constant from libc may help in weird cases where STDOUT_FILENO is not 1...)
	debug_assert_eq!(stdout_fd, std::io::stdout().as_raw_fd(), "STDOUT_FILENO and io::stdout().as_raw_fd() are not returning the same value.");
	close_fileno(/*std::io::stdout().as_raw_fd()*/ stdout_fd) // SAFETY: We just assume fd 1 is still open. If it's not (i.e. already been closed), this will return error. 
            .with_section(move || stdout_fd.header("Attempted to close this fd (STDOUT_FILENO)"))
            .with_warning(|| format!("It is possible fd {} (STDOUT_FILENO) has already been closed; if so, look for where that happens and prevent it. `stdout` should be closed here.", stdout_fd).header("Possible bug"))
    }.wrap_err(eyre!("Failed to close stdout"))?;

    Ok(())
}
