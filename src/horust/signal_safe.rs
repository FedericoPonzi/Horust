//! This module is composed by a set of signal safe functions's wrapper.
//!
//! You can find a list of signal safe functions here: https://man7.org/linux/man-pages/man7/signal-safety.7.html
//! For a usage example: in a multithreaded program, after calling `fork()`
//! only [async-signal-safe] functions like `pause`
//! and `_exit` may be called by the child (the parent isn't restricted).

use libc::{_exit, STDERR_FILENO};
use nix::unistd::write;

/// Async-signal-safe panic. Prints s to stderr, and exit with status as code.
pub(crate) fn panic_ssafe(s: &str, status: i32) {
    eprint_ssafe(s);
    exit_ssafe(status);
}

/// Async-signal-safe stderr print
#[allow(unused_must_use)]
pub(crate) fn eprint_ssafe(s: &str) {
    write(STDERR_FILENO, s.as_bytes());
    // No allocation is allowed, so let's print the new line with a second call
    let new_line = "\n";
    write(STDERR_FILENO, new_line.as_bytes());
}

/// Async-signal-safe exit
fn exit_ssafe(status: i32) {
    unsafe {
        _exit(status);
    }
}
