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

    #[cfg_attr(feature="logging", instrument(skip_all, fields(fd = ?file.as_raw_fd())))]
fn proc_file<F: ?Sized + AsRawFd>(file: &F) -> PathBuf
{
    let fd = file.as_raw_fd();
    let pid = process::id();
    //process::Command::new("/bin/ls").arg("-l").arg(format!("/proc/{pid}/fd/")).spawn().unwrap().wait().unwrap();
    format!("/proc/{pid}/fd/{fd}").into()
    //format!("/dev/fd/{fd}").into()
}

/// Attempt to `dup()` a file descriptor into a `RawFile`.
#[inline]

    #[cfg_attr(feature="logging", instrument(skip_all, err, fields(fd = ?file.as_raw_fd())))]
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

    #[cfg_attr(feature="logging", instrument(skip_all, fields(has_stdin = ?file.is_some(), filename = ?filename.as_ref())))]
fn run_stdin<I>(file: Option<impl Into<fs::File>>, filename: impl AsRef<OsStr>, args: I) -> io::Result<(process::Child, Option<fs::File>)>
where I: IntoIterator<Item = OsString>,
{
    let file = {
	let file: Option<fs::File> = file.map(Into::into);
	//TODO: Do we need to fcntl() this to make it (the fd) RW?
	match file {
	    None => None,
	    Some(mut file) => {
		use std::io::Seek;
		if let Err(err) = file.seek(io::SeekFrom::Start(0)) {
		    if_trace!(warn!("Failed to seed to start: {err}"));
		}
		let _ = try_seal_size(&file);
		Some(file)
	    },
	}
    };
    
    let child = process::Command::new(filename)
        .args(args)
        .stdin(file.as_ref().map(|file| process::Stdio::from(fs::File::from(dup_file(file).unwrap()))).unwrap_or_else(|| process::Stdio::null())) //XXX: Maybe change to `piped()` and `io::copy()` from begining (using pread()/send_file()/copy_file_range()?)
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .spawn()?;
    //TODO: XXX: Why does `/proc/{pid}/fd/{fd}` **and** `/dev/fd/{fd}` not work for -exec{}, and why foes `Stdio::from(file)` not work for stdin even *afer* re-seeking the file???
    /*
    if let Some((mut input, mut output)) = file.as_mut().zip(child.stdin.take()) {
	io::copy(&mut input, &mut output)
	    /*.wrap_err("Failed to pipe file into stdin for child")*/?;
    }*/
    
    if_trace!(info!("Spawned child process: {}", child.id()));
    /*Ok(child.wait()?
    .code()
    .unwrap_or(-1)) //XXX: What should we do if the process terminates without a code (i.e. by a signal?)
     */
    Ok((child, file))
}

/// Run a single `-exec` / `-exec{}` and return the (possibly still running) child process if succeeded in spawning.
///
/// The caller must wait for all child processes to exit before the parent does.
#[inline]
    #[cfg_attr(feature="logging", instrument(skip(file), err))]
pub fn run_single<F: ?Sized + AsRawFd>(file: &F, opt: args::ExecMode) -> io::Result<(process::Child, Option<fs::File>)>
{
    let input: std::mem::ManuallyDrop<memfile::RawFile> = std::mem::ManuallyDrop::new(dup_file(file)?);
    
    match opt {
	args::ExecMode::Positional { command, args } => {
	    run_stdin(None::<fs::File>, command, args.into_iter().map(|x| x.unwrap_or_else(|| proc_file(&*input).into())))
	},
	args::ExecMode::Stdin { command, args } => {
	    run_stdin(Some(std::mem::ManuallyDrop::into_inner(input)), command, args)
	}
    }
}

/// Spawn all `-exec/{}` commands and return all running children.
///
/// # Returns
/// An iterator of each (possibly running) spawned child, or the error that occoured when trying to spawn that child from the `exec` option in `opt`.
    #[cfg_attr(feature="logging", instrument(skip(file)))]
pub fn spawn_from<'a, F: ?Sized + AsRawFd>(file: &'a F, opt: Options) -> impl IntoIterator<Item = io::Result<(process::Child, Option<fs::File>)>> + 'a
{
    opt.into_opt_exec().map(|x| run_single(file, x))
}

/// Spawn all `-exec/{}` commands and wait for all children to complete.
///
/// # Returns
/// An iterator of the result of spawning each child and its exit status (if one exists)
///
/// If the child exited via a signal termination, or another method that does not return a status, the iterator's result will be `Ok(None)`
#[inline] 
    #[cfg_attr(feature="logging", instrument(skip(file)))]
pub fn spawn_from_sync<'a, F: ?Sized + AsRawFd>(file: &'a F, opt: Options) -> impl IntoIterator<Item = eyre::Result<Option<i32>>> + 'a
{
    spawn_from(file, opt).into_iter().zip(0..).map(move |(child, idx)| -> eyre::Result<_> {
	
	let idx = move || idx.to_string().header("The child index");
	match child {
	    Ok(mut child) => {
		Ok(child.0.wait()
		   .wrap_err("Failed to wait on child")
		   .with_note(|| "The child may have detached itself")
		   .with_section(idx)?
		   .code())
	    },
	    Err(err) => {
		if_trace!(error!("Failed to spawn child: {err}"));
		Err(err)
		    .wrap_err("Failed to spawn child")
	    }
	}.with_section(idx)
    })
}
