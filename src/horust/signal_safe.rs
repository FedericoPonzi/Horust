use libc::{_exit, write, STDERR_FILENO};
use std::ffi::c_void;

/// Async-signal-safe panic. Prints s to stderr, and exit with status as code.
pub(crate) fn ss_panic(s: &str, status: i32) {
    eprint_safe(s);
    exit_safe(status);
}

/// Async-signal-safe stderr print
pub(crate) fn eprint_safe(s: &str) {
    unsafe {
        write(STDERR_FILENO, s.as_ptr() as *const c_void, s.len());
        let new_line = "\n";
        write(STDERR_FILENO, new_line.as_ptr() as *const c_void, s.len());
    }
}

/// Async-signal-safe exit
fn exit_safe(status: i32) {
    unsafe {
        _exit(status);
    }
}
