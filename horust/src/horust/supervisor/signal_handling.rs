use nix::sys::signal::{SIGINT, SIGTERM, SaFlags, SigAction, SigHandler, SigSet, sigaction};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::horust::signal_safe::panic_ssafe;

static PENDING_SIGNALS: AtomicU32 = AtomicU32::new(0);

pub(crate) fn take_pending_signals() -> u32 {
    PENDING_SIGNALS.swap(0, Ordering::SeqCst)
}

/// Setup the signal handlers
#[inline]
pub(crate) fn init() {
    // To allow auto restart on some syscalls,
    // for example: `waitpid`.
    let flags = SaFlags::SA_RESTART;
    let sig_action = SigAction::new(SigHandler::Handler(handle_sigterm), flags, SigSet::empty());

    if let Err(err) = unsafe { sigaction(SIGTERM, &sig_action) } {
        panic_ssafe(
            "signal_handling: sigaction() SIGTERM failed.",
            None,
            err,
            103,
        );
    };

    if let Err(err) = unsafe { sigaction(SIGINT, &sig_action) } {
        panic_ssafe(
            "signal_handling: sigaction() SIGINT failed .",
            None,
            err,
            104,
        );
    };
}

extern "C" fn handle_sigterm(_signal: libc::c_int) {
    // Two pending signals already mean forceful shutdown; saturate to avoid overflow.
    let mut pending = PENDING_SIGNALS.load(Ordering::SeqCst);
    while pending < 2 {
        match PENDING_SIGNALS.compare_exchange_weak(
            pending,
            pending + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return,
            Err(current) => pending = current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horust::bus::Bus;
    use crate::horust::formats::{Event, ShuttingDown};
    use crate::horust::supervisor::Supervisor;
    use std::sync::{Barrier, Mutex};

    static SIGNAL_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn second_signal_survives_queued_shutdown() {
        let _guard = SIGNAL_TEST.lock().unwrap_or_else(|err| err.into_inner());
        take_pending_signals();
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![]);

        handle_sigterm(libc::SIGTERM);
        supervisor.handle_signals(take_pending_signals());
        // The supervisor has observed the first signal but its bus event is still queued.
        handle_sigterm(libc::SIGINT);
        supervisor.handle_event(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        assert_eq!(
            take_pending_signals(),
            1,
            "queued shutdown erased the second signal"
        );
        assert_eq!(take_pending_signals(), 0);
    }

    #[test]
    fn pending_signals_are_consumed_once() {
        let _guard = SIGNAL_TEST.lock().unwrap_or_else(|err| err.into_inner());
        take_pending_signals();
        handle_sigterm(libc::SIGTERM);
        assert_eq!(take_pending_signals(), 1);
        assert_eq!(take_pending_signals(), 0);
        handle_sigterm(libc::SIGINT);
        handle_sigterm(libc::SIGTERM);
        assert_eq!(take_pending_signals(), 2);
        assert_eq!(take_pending_signals(), 0);
        for _ in 0..100_000 {
            handle_sigterm(libc::SIGTERM);
        }
        assert_eq!(take_pending_signals(), 2);
    }

    #[test]
    fn concurrent_signal_handoff() {
        let _guard = SIGNAL_TEST.lock().unwrap_or_else(|err| err.into_inner());
        take_pending_signals();
        let start = Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                for _ in 0..100_000 {
                    handle_sigterm(libc::SIGTERM);
                }
            });
            start.wait();
            for _ in 0..100_000 {
                std::hint::black_box(take_pending_signals());
            }
        });
        take_pending_signals();
    }
}
