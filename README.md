# `collect` - Collect all input until it's closed, then output it all at once.

This small tool can be used to ensure all data between pipes is synchronised, and/or to ensure the 2nd program in the pipe doesn't start processing before the first one has finished outputting her data.

## Usage
For example, in the pipeline `x | collect | y`, where `x` is a program who's output is sporadic (something like a network connection, reading and processing a segmented file, etc) `y` will receive all of `x`s output at once as soon as `x` closes her standard output pipe. So `y` will not start processing until `x` has completed hers.


There are no runtime flags (unless logging is enabled, in which case, see below), it simply reads from `stdin` and writes to `stdout`. (When logging is enabled, and the log-level is set to a level that will enabled common info logging, it is written to `stderr` **only** to not interfere with the data collected from `stdin`.)


### Logging
When compiled with the `logging` feature (default), you can control the log level with the `RUST_LOG` environment variable (the default for release builds is `info`, for debug builds, `debug`.)

#### Available levels
To set the level, run with `RUST_LOG=` one of the below values:
* `trace` - The lowest level of logging, all information will be printed.
* `debug` - The 2nd lowest level, debugging-relevent information (such as buffer sizes, file descriptor numbers/names, read/write segment sizes, allocations, etc.) will be printed. (default for `debug` builds.)
* `info` - Will print information when collection has started, finished, and output is over. (default for `release` builds.)
* `warn` - Will print only warnings. Most of these that will be seen will be related to additionally required syscalls for fd-size truncation, which are only efficiency-related and not warnings to the user herself's use of the program. But some will be.
* `error` - Only print error messages.
* `off` - Print no messages at all.

## Building
Building requires `rust` and `Cargo`.

To build with the default configuration:
``` shell
$ cargo build --release
```
Will build the binary into `./target/release/collect`.

### Debug builds
To create a debug build:
``` shell
$ cargo build
```
Will build the binary into `./target/debug/collect`. 
*NOTE*: when `logging` feature is enabled, the default logging level will be `debug` instead of `info`.

To create a release build that is not symbol-stripped:
``` shell
$ cargo build --profile symbols
```
Will build the binary into `./target/symbols/collect`.

### Modes & features
There are two major operative modes: `mode-memfile` (default [+`logging`]) and `mode-buffered`. 
These are collections of features specific to each operating mode.

#### Modes
Each mode feature can be chosen by building with a `Cargo` incantation in the following format:
``` shell
$ cargo build --release --no-default-features --features mode-<name>[,logging]
```

* `mode-memfile` - This is the default used mode, which will use the feature `memfile-preallocate`. *NOTE*: The default enabled features chooses this mode and the `logging` feature.
* `mode-buffered` - This will use `jemalloc` and `bytes`-allocated buffers instead of file-descriptors.

*NOTE*: If both modes are specified at once, `mode-memfile` will take precidence by the program, and `mode-buffered` will not be used.

#### Features
The user can also compile the program with individual features specific to her needs.

They can be specified as such:

| Feature name          | Description                                                                                                                                                                                                            | Notes                                                                                                                                                                                                                                                                                      |
|-----------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `memfile`             | Use an in-memory file-descriptor pointing to unmapped physical pages of memory. This will allow the program to make use of the more efficient `splice()` and `send_file()` syscalls where possible.                    | **WARNING**: Can potentially cause a *full system OOM* if used incorrectly or irresponsibly. (See below)                                                                                                                                                                                   |
| `memfile-preallocate` | `memfile`, but when unable to determine the size of `stdin`, will pre-allocate it to a multiple of the system page size.                                                                                               | *NOTE*: Requires `int getpagesize()` to be availble in your used `libc` implementation. (It ususally will exist unless you're using some meme implementation of `libc`.) This is enabled by default with the `memfile` mode.                                                               |
| `jemalloc`            | Use `jemalloc` instead of system `malloc()` when allocating memory. This is only really helpful when *not* using `memfile`, but the program heap is still used for error propagating and log reporting in either mode. | `jemalloc` incorporates a lot of redundant (in this case) locking mechanisms, but causes a generally lower used memory profile than system malloc, however it does allocate far more *virtual memory* pages than is generally needed. This is enabled by default with the `buffered` mode. |
| `bytes`               | Use the `bytes` crate to manage memory allocations in `buffered` mode instead of native vector implementations, this can *potentially* save on *some* copying operations.                                              | Some crude benchmarks have shown this to be mildly more efficient in `buffered` mode than without it.                                                                                                                                                                                      |
| `disable-logging`     | Removes all **runtime** logging code. Span-traces are still captured, however, they just are never used.                                                                                                               | This won't save you much compared to just disabling the `logging` feature (below.)                                                                                                                                                                                                         |
| `logging`             | Enable the capture and reporting of span-traces and events. (See the section on logging above.)                                                                                                                        | This does cause a slowdown, but can provide useful information to the user about error locations, warnings, when and where input and output have finished and the sizes of both, etc. If you're only using it in scripts however, it'd be better to disable. (*default enabled*)           |

##### Notes about `memfile` feature/mode
If `memfile` is enabled, and the input size can be determined by the program, it will preallocate the required space for the input.
If this input were to exceed the amount of physical memory available (since this is unpaged memory being allocated,) it could hang and/or then cause the kernel to OOMkill basically everything *except* `collect`. 

Please note however, this would only typically happen in instances where a *file* is passed as input (where the length can be determined, the source it *usually* not segmented at all); in which case `collect` is just going to slow down your pipe. (It is still worth using for scripts where the script doesn't *know* if the standatd input is a file or not.)

In the current version, this is not yet accounted for, so passing massive files, for example:
``` shell
$ collect <10-gb-file | wc -c
```
Will try to allocate 10GB of *physical* memory for the collection.

In future versions, a warning for large known-size inputs will be displayed, and an error for known-size inputs so large they would cause an OOM. (Same for unknown-sized inputs that grow the backing memfd to a size that would start to become an issue or would use too much physical memory.)
But currently, this is a pitfall of the `memfile` mode that, while very unlikely to ever be encountered, could still bite the user if it is encountered.

If something like this may be a concern for your usecase, please fall-back to using the `buffered` mode instead, which, while significantly slower, will only OOM *itself* if the input is too large and cannot eat *physical* memory directly, only its already-large VM page maps which are, for most instances, mostly empty.

# License
CPL'd with <3


