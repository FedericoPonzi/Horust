use libc::STDOUT_FILENO;
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGINT, SIGTERM};
use std::ffi::{c_void, CStr, CString, OsStr};

static mut SIGTERM_RECEIVED: bool = false;

pub(crate) fn is_sigterm_received() -> bool {
    unsafe { SIGTERM_RECEIVED }
}

struct SignalSafe;

impl SignalSafe {
    fn print(s: &str) {
        unsafe {
            libc::write(STDOUT_FILENO, s.as_ptr() as *const c_void, s.len());
        }
    }
}

/// Setup the signal handlers
pub(crate) fn init() {
    // To allow auto restart on some syscalls,
    // for example: `waitpid`.
    let flags = SaFlags::SA_RESTART;
    let sig_action = SigAction::new(SigHandler::Handler(handle_sigterm), flags, SigSet::empty());

    if let Err(err) = unsafe { sigaction(SIGTERM, &sig_action) } {
        panic!("sigaction() failed: {}", err);
    };
    if let Err(err) = unsafe { sigaction(SIGINT, &sig_action) } {
        panic!("sigaction() failed: {}", err);
    };
}
extern "C" fn handle_sigterm(_: libc::c_int) {
    SignalSafe::print("Received SIGTERM.\n");
    unsafe {
        SIGTERM_RECEIVED = true;
    }
}
