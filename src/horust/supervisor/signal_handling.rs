use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGINT, SIGTERM};

use crate::horust::signal_safe::panic_ssafe;

static mut SIGTERM_RECEIVED: bool = false;

pub(crate) fn is_sigterm_received() -> bool {
    unsafe { SIGTERM_RECEIVED }
}

pub(crate) fn clear_sigtem() {
    unsafe {
        SIGTERM_RECEIVED = false;
    }
}

/// Setup the signal handlers
#[inline]
pub(crate) fn init() {
    // To allow auto restart on some syscalls,
    // for example: `waitpid`.
    let flags = SaFlags::SA_RESTART;
    let sig_action = SigAction::new(SigHandler::Handler(handle_sigterm), flags, SigSet::empty());

    if let Err(err) = unsafe { sigaction(SIGTERM, &sig_action) } {
        panic_ssafe("signal_handling: sigaction() SIGTERM failed.", err, 103);
    };

    if let Err(err) = unsafe { sigaction(SIGINT, &sig_action) } {
        panic_ssafe("signal_handling: sigaction() SIGINT failed .", err, 104);
    };
}

extern "C" fn handle_sigterm(_signal: libc::c_int) {
    unsafe {
        SIGTERM_RECEIVED = true;
    }
}
