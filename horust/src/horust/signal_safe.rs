//! This module is composed by a set of signal safe functions's wrapper.
//!
//! You can find a list of signal safe functions here: https://man7.org/linux/man-pages/man7/signal-safety.7.html
//! For a usage example: in a multithreaded program, after calling `fork()`
//! only [async-signal-safe] functions like `pause`
//! and `_exit` may be called by the child (the parent isn't restricted).

use nix::errno::Errno;
use nix::libc::{_exit, STDERR_FILENO};
use nix::unistd::write;

/// Async-signal-safe panic. Prints s to stderr, and exit with status as code.
pub(crate) fn panic_ssafe(message: &str, service: Option<&str>, errno: Errno, status: i32) {
    if let Some(serv_name) = service {
        eprint_ssafe(serv_name);
        eprint_ssafe(": ");
    }
    eprint_ssafe(message);
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
    eprint_ssafe("Errno: (");
    let (bytes, digits) = i32_to_str_bytes(s as i32);
    write(STDERR_FILENO, &bytes[digits..]);
    eprint_ssafe(") ");
    eprint_ssafe(s.desc());
    eprint_ssafe(NEW_LINE);
}

/// Async-signal-safe exit
fn exit_ssafe(status: i32) {
    unsafe {
        _exit(status);
    }
}

/// usage:
/// let (res, digits) = i32_to_str_bytes(10);
/// // use it as:
/// &res[digits..]
///
fn i32_to_str_bytes(num: i32) -> ([u8; 11], usize) {
    // need i64 because the limits are : [2147483647, -2147483648].
    // when I do num *= -1, the i32::MIN cannot be represented anymore (because is larger than MAX)
    // and fails with `attempt to multiply with overflow.`
    let mut num = num as i64;
    let mut digits = 0;
    let mut ret = [0u8; 11];
    const I32_STR_LEN: usize = 11;
    const BASE_ASCII: u8 = b'0';
    const MINUS_SIGN: u8 = b'-';

    let is_negative = num < 0;
    if is_negative {
        // need to convert because the module returns a negative number.
        num *= -1;
    }

    if num == 0 {
        let index = I32_STR_LEN - 1 - digits;
        ret[index] = BASE_ASCII;
        digits += 1;
    }

    while num != 0 {
        let n = (num % 10) as i32 as u8;
        let index = I32_STR_LEN - 1 - digits;
        ret[index] = n + BASE_ASCII;
        num = (num - n as i64) / 10;
        digits += 1;
    }
    if is_negative {
        let index = I32_STR_LEN - 1 - digits;
        ret[index] = MINUS_SIGN;
        digits += 1;
    }
    (ret, I32_STR_LEN - digits)
}

#[cfg(test)]
mod test {
    use crate::horust::signal_safe::i32_to_str_bytes;

    #[test]
    fn test_int_to_string_conversion() {
        let test = |i| {
            let (res, digits) = i32_to_str_bytes(i);
            assert_eq!(&res[digits..], format!("{}", i).as_bytes());
        };

        for _ in 0..100 {
            test(rand::random());
        }
        for i in [0, -1, 1, 10, -10, i32::MAX, i32::MIN] {
            test(i)
        }
    }
}
