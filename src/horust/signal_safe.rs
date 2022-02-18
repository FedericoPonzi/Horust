//! This module is composed by a set of signal safe functions's wrapper.
//!
//! You can find a list of signal safe functions here: https://man7.org/linux/man-pages/man7/signal-safety.7.html
//! For a usage example: in a multithreaded program, after calling `fork()`
//! only [async-signal-safe] functions like `pause`
//! and `_exit` may be called by the child (the parent isn't restricted).

use libc::{_exit, STDERR_FILENO};
use nix::errno::Errno;
use nix::unistd::write;

/// Async-signal-safe panic. Prints s to stderr, and exit with status as code.
pub(crate) fn panic_ssafe(s: &str, errno: Errno, status: i32) {
    eprint_ssafe(s);
    eprint_ssafe_errno(errno);
    exit_ssafe(status);
}

const NEW_LINE: &str = "\n";

/// Async-signal-safe stderr print
#[allow(unused_must_use)]
pub(crate) fn eprint_ssafe(s: &str) {
    write(STDERR_FILENO, s.as_bytes());
}

/// Async-signal-safe stderr print
#[allow(unused_must_use)]
pub(crate) fn eprint_ssafe_errno(s: Errno) {
    eprint_ssafe(NEW_LINE);
    eprint_ssafe("Errno: ");
    // TODO: it won't work as these bytes will be interpreted as string...
    //write(STDERR_FILENO, &(s as i32).to_be_bytes());
    //write(STDERR_FILENO, " ".as_bytes());
    eprint_ssafe(s.desc());
    eprint_ssafe(NEW_LINE);
}

/// Async-signal-safe exit
fn exit_ssafe(status: i32) {
    unsafe {
        _exit(status);
    }
}
