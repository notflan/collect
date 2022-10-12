//! Used for implementation of `-exec[{}]`
use super::*;
use args::Options;
use std::{
    fs,
    process,
    path::{
	Path,
	PathBuf,
    },
    ffi::{
	OsStr,
	OsString,
    }
};

/// Get a path to the file-descriptor refered to by `file`.
fn proc_file<F: ?Sized + AsRawFd>(file: &F) -> PathBuf
{
    let fd = file.as_raw_fd();
    let pid = process::id();
    format!("/proc/{pid}/fd/{fd}").into()
}

/// Attempt to `dup()` a file descriptor into a `RawFile`.
#[inline] 
fn dup_file<F: ?Sized + AsRawFd>(file: &F) -> io::Result<memfile::RawFile>
{
    let fd = file.as_raw_fd();
    debug_assert!(fd >= 0, "Bad input file descriptor from {} (value was {fd})", std::any::type_name::<F>());
    let fd = unsafe {
	let res = libc::dup(fd);
	if res < 0 {
	    return Err(io::Error::last_os_error());
	} else {
	    res
	}
    };
    Ok(memfile::RawFile::take_ownership_of_unchecked(fd))
}

fn run_stdin<I>(file: impl Into<fs::File>, filename: impl AsRef<OsStr>, args: I) -> io::Result<process::Child>
where I: IntoIterator<Item = OsString>,
{
    let file = {
	let mut file: fs::File = file.into();
	//TODO: Do we need to fcntl() this to make it (the fd) RW?
	file
    };
    let child = process::Command::new(filename)
        .args(args)
        .stdin(process::Stdio::from(file))
        .stdout(process::Stdio::inherit())
        .spawn()?;
    
    if_trace!(info!("Spawned child process: {}", child.id()));
    /*Ok(child.wait()?
    .code()
    .unwrap_or(-1)) //XXX: What should we do if the process terminates without a code (i.e. by a signal?)
     */
    Ok(child)
}

/// Run a single `-exec` / `-exec{}` and return the (possibly still running) child process if succeeded in spawning.
///
/// The caller must wait for all child processes to exit before the parent does.
#[inline]
pub fn run_single<F: ?Sized + AsRawFd>(file: &F, opt: args::ExecMode) -> io::Result<process::Child>
{
    let input = dup_file(file)?;
    
    match opt {
	args::ExecMode::Positional { command, args } => {
	    todo!("implement this with `proc_file(&file)`")
	},
	args::ExecMode::Stdin { command, args } => {
	    run_stdin(input, command, args)
	}
    }
}

/// Spawn all `-exec/{}` commands and return all running children.
///
/// # Returns
/// An iterator of each (possibly running) spawned child, or the error that occoured when trying to spawn that child from the `exec` option in `opt`.
pub fn spawn_from<F: ?Sized + AsRawFd>(file: &F, opt: Options) -> io::Result<impl IntoIterator<Item = io::Result<process::Child> + 'static>>
{
    todo!("Loop through `opt.into_exec()`, map the call to `|x| run_single(file, x)`, and return that iterator")
}

#[inline] 
pub fn spawn_from<F: ?Sized + AsRawFd>(file: &F, opt: Options) -> io::Result<impl IntoIterator<Item = io::Result<i32> + 'static>>
{
    todo!("Map `spawn_from(...)` and wait for each child to terminate concurrently. Then return an iterator or the return codes or spawning errors for that now terminated child.")
}
