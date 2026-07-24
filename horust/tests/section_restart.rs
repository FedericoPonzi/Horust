use std::time::Duration;

use assert_cmd::cmd::Command;
#[cfg(target_os = "linux")]
use libc::SIGPOLL;
use libc::{
    SIGABRT, SIGBUS, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGPIPE, SIGPROF, SIGQUIT, SIGSEGV,
    SIGSYS, SIGTERM, SIGTRAP, SIGUSR1, SIGUSR2, SIGVTALRM, SIGXCPU, SIGXFSZ, c_int,
};
use predicates::prelude::predicate;
use utils::*;

#[allow(dead_code)]
mod utils;

fn restart_attempts(should_contain: bool, attempts: u32) {
    let (mut cmd, temp_dir) = get_cli();

    let failing_once_script = format!(
        r#"#!/usr/bin/env sh
if [ ! -f {0} ]; then
    touch {0} && exit 1
fi
echo "File is there"
"#,
        temp_dir.path().join("file.temp").display()
    );
    let service = format!(
        r#"
[healthiness]
file-path = "{}"
[restart]
attempts = {}
"#,
        temp_dir
            .path()
            .join("valid-path-but-shouldnt-exists.temp")
            .display(),
        attempts
    );
    store_service_script(
        temp_dir.path(),
        failing_once_script.as_str(),
        Some(service.as_str()),
        None,
    );
    let cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    let recv = run_async(cmd, should_contain);
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
fn test_restart_attempts() {
    // Should try to check for the presence of a file, since it's not there it will fail.
    restart_attempts(false, 0);
    // Now we have a second shot, since the file was created the first time this will succeed.
    restart_attempts(true, 1);
}

#[test]
fn test_restart_strategy_on_failure() {
    let (mut cmd, temp_dir) = get_cli();

    let failing_once_script = format!(
        r#"#!/usr/bin/env bash
if [ ! -f {0} ]; then
    touch {0} && sleep 1 && exit 1
fi
"#,
        temp_dir.path().join("file.temp").display()
    );
    let service = r#"
[restart]
attempts = 0
strategy = "on-failure"
"#
    .to_string();
    store_service_script(
        temp_dir.path(),
        failing_once_script.as_str(),
        Some(service.as_str()),
        None,
    );
    let cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    let recv = run_async(cmd, true);
    recv.recv_or_kill(Duration::from_secs(15));
}

/// Waits for horust to exit on its own, panicking with `on_timeout` if it keeps running.
fn wait_for_exit(
    child: &mut std::process::Child,
    on_timeout: impl FnOnce() -> String,
) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            let msg = on_timeout();
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{msg}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Runs an always-failing service and returns (horust exit status, number of launches).
fn run_bounded_crash_loop(strategy: &str, attempts: u32) -> (std::process::ExitStatus, usize) {
    let (mut cmd, temp_dir) = get_cli();
    let attempts_log = temp_dir.path().join("attempts.log");
    let always_failing_script = format!(
        r#"#!/usr/bin/env bash
printf 'run\n' >> "{}"
exit 1
"#,
        attempts_log.display()
    );
    // healthy-after keeps the service in the not-yet-green state long enough that each
    // immediate crash counts against the restart budget, making the bound deterministic.
    let service = format!(
        r#"
[restart]
attempts = {attempts}
backoff = "1ms"
strategy = "{strategy}"

[healthiness]
healthy-after = "10s"

[failure]
successful-exit-code = [0]
strategy = "ignore"
"#
    );
    store_service_script(
        temp_dir.path(),
        &always_failing_script,
        Some(&service),
        None,
    );

    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .spawn()
        .unwrap();
    let count_launches = || {
        std::fs::read_to_string(&attempts_log)
            .map(|contents| contents.lines().count())
            .unwrap_or(0)
    };
    let status = wait_for_exit(&mut child, || {
        format!("Horust did not stop after {} launches", count_launches())
    });
    (status, count_launches())
}

/// `never` may restart a service that fails *rapidly* (the escape hatch enabled by
/// `attempts`), but once the service has become stable it must stay dead: such failures
/// never consume the budget, so restarting them would loop forever.
#[test]
fn test_restart_strategy_never_does_not_restart_after_stable() {
    let (mut cmd, temp_dir) = get_cli();
    let attempts_log = temp_dir.path().join("attempts.log");
    let script = format!(
        r#"#!/usr/bin/env bash
printf 'run\n' >> "{}"
sleep 1
exit 1
"#,
        attempts_log.display()
    );
    let service = r#"
[restart]
attempts = 3
backoff = "1ms"
strategy = "never"

[healthiness]
healthy-after = "100ms"

[failure]
successful-exit-code = [0]
strategy = "ignore"
"#;
    store_service_script(temp_dir.path(), &script, Some(service), None);
    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .spawn()
        .unwrap();
    let count_launches = || {
        std::fs::read_to_string(&attempts_log)
            .map(|contents| contents.lines().count())
            .unwrap_or(0)
    };
    wait_for_exit(&mut child, || {
        format!(
            "`never` kept restarting after the service became stable ({} launches)",
            count_launches()
        )
    });
    assert_eq!(count_launches(), 1, "the service must not be restarted");
}

/// A service whose command cannot be spawned at all (not found in PATH) fails before the
/// process ever exists, so there is no exit to observe. It must still consume the restart
/// budget, otherwise it would be restarted forever.
#[test]
fn test_spawn_failure_exhausts_attempts() {
    let (mut cmd, temp_dir) = get_cli();
    let service = r#"command = "definitely-not-a-real-binary-xyz"
[restart]
attempts = 2
backoff = "1ms"
strategy = "on-failure"
"#;
    std::fs::write(temp_dir.path().join("svc.toml"), service).unwrap();
    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .spawn()
        .unwrap();
    let status = wait_for_exit(&mut child, || {
        "Horust kept restarting a service that could never be spawned".to_string()
    });
    assert_eq!(status.code(), Some(101));
}

#[test]
fn test_restart_strategy_on_failure_exhausts_attempts() {
    let (status, launches) = run_bounded_crash_loop("on-failure", 3);
    assert_eq!(status.code(), Some(101));
    assert_eq!(launches, 4, "initial launch plus three retries");
}

#[test]
fn test_restart_strategy_always_exhausts_attempts() {
    let (status, launches) = run_bounded_crash_loop("always", 3);
    assert_eq!(status.code(), Some(101));
    assert_eq!(launches, 4, "initial launch plus three retries");
}

/// A service that survives past `healthy-after` becomes stable, resetting its restart
/// budget. It must therefore keep restarting on failure rather than being bounded.
#[test]
fn test_healthy_after_resets_restart_budget() {
    let (mut cmd, temp_dir) = get_cli();
    let attempts_log = temp_dir.path().join("attempts.log");
    // Sleeps past healthy-after (so it reaches Running and resets the budget) before
    // failing. With attempts = 2 this would exhaust quickly if the budget never reset.
    let script = format!(
        r#"#!/usr/bin/env bash
count=0
[ -f "{0}" ] && count=$(wc -l < "{0}")
printf 'run\n' >> "{0}"
sleep 0.5
if [ "$count" -lt 4 ]; then
    exit 1
fi
sleep 60
"#,
        attempts_log.display()
    );
    let service = r#"
[restart]
attempts = 2
backoff = "1ms"
strategy = "on-failure"

[healthiness]
healthy-after = "100ms"

[failure]
successful-exit-code = [0]
strategy = "ignore"
"#;
    store_service_script(temp_dir.path(), &script, Some(service), None);

    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .spawn()
        .unwrap();

    // Give it time to fail several times and then stabilize. If the budget were not
    // reset on reaching a stable state, Horust would have exited (FinishedFailed) after
    // 3 launches; instead it should keep restarting past the budget and still be running.
    std::thread::sleep(Duration::from_secs(6));
    let still_running = child.try_wait().unwrap().is_none();
    let launches = std::fs::read_to_string(&attempts_log)
        .map(|c| c.lines().count())
        .unwrap_or(0);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        still_running,
        "Horust exited early after {launches} launches; the budget was not reset on reaching a stable state"
    );
    assert!(
        launches > 3,
        "expected more than the 3 allowed launches (initial + 2 retries), got {launches}"
    );
}

/// The respawn delay is `backoff * restart_attempts`, and `restart_attempts` is only reset
/// once a service becomes stable. A service that never stabilizes would therefore be
/// restarted increasingly slowly forever, so the multiplier is capped: check the observed
/// delay plateaus rather than growing with every failure.
#[test]
fn test_restart_backoff_stops_growing() {
    const BACKOFF_MS: u128 = 100;
    // Enough launches to exhaust MAX_BACKOFF_MULTIPLIER (10) and settle well past it.
    const LAUNCHES: usize = 19;
    // The delays are quantised by the supervisor's 300ms loop, so a capped run repeats the
    // very same delay while a growing one steps up by `backoff` on every launch.
    const PLATEAU_TOLERANCE_MS: u128 = 150;

    let (mut cmd, temp_dir) = get_cli();
    let attempts_log = temp_dir.path().join("attempts.log");
    let script = format!(
        r#"#!/usr/bin/env bash
date +%s%N >> "{}"
exit 1
"#,
        attempts_log.display()
    );
    // healthy-after keeps the service from ever becoming stable, so restart_attempts (and
    // hence the multiplier) keeps growing for the whole run.
    let service = format!(
        r#"
[restart]
attempts = 0
backoff = "{BACKOFF_MS}ms"
strategy = "always"

[healthiness]
healthy-after = "60s"

[failure]
successful-exit-code = [0]
strategy = "ignore"
"#
    );
    store_service_script(temp_dir.path(), &script, Some(&service), None);
    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .spawn()
        .unwrap();

    let read_timestamps = || -> Vec<u128> {
        std::fs::read_to_string(&attempts_log)
            .map(|contents| {
                contents
                    .lines()
                    .filter_map(|l| l.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    while read_timestamps().len() < LAUNCHES && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    child.wait().unwrap();

    let timestamps = read_timestamps();
    let gaps: Vec<u128> = timestamps
        .windows(2)
        .map(|w| (w[1] - w[0]) / 1_000_000)
        .collect();
    assert!(
        timestamps.len() >= LAUNCHES,
        "only {} launches, delays: {gaps:?}",
        timestamps.len()
    );
    let settled = &gaps[gaps.len() - 6..];
    let spread = settled.iter().max().unwrap() - settled.iter().min().unwrap();
    assert!(
        spread <= PLATEAU_TOLERANCE_MS,
        "restart delay was still growing after {LAUNCHES} launches: {gaps:?}"
    );
}

/// With restart strategy set to always, the child service should be always restarted regardless of
/// the reason why it exited.
fn test_restart_always_signal(signal: i32) -> Result<(), std::io::Error> {
    let (cmd, temp_dir) = get_cli();
    let mut cmd = Command::from_std(cmd);

    let suicide_script = format!(
        r#"#!/usr/bin/env bash
echo "restarting"
kill -{} $$
"#,
        signal
    );
    let service = r#"
[restart]
strategy = "always"
"#;
    store_service_script(
        temp_dir.path(),
        suicide_script.as_str(),
        Some(service),
        None,
    );
    cmd.timeout(Duration::from_millis(2000))
        .assert()
        .failure()
        .stdout(predicate::function(|x: &str| {
            x.matches("restarting").count() >= 2
        }));

    Ok(())
}

#[test]
fn test_restart_always_killed_by_signals() -> Result<(), std::io::Error> {
    #[cfg(target_os = "linux")]
    const DEFAULT_TERMINATE: [c_int; 20] = [
        SIGABRT, SIGBUS, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGPIPE, SIGPOLL, SIGPROF,
        SIGQUIT, SIGSEGV, SIGSYS, SIGTERM, SIGTRAP, SIGUSR1, SIGUSR2, SIGVTALRM, SIGXCPU, SIGXFSZ,
    ];
    #[cfg(not(target_os = "linux"))]
    const DEFAULT_TERMINATE: [c_int; 19] = [
        SIGABRT, SIGBUS, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGPIPE, SIGPROF, SIGQUIT,
        SIGSEGV, SIGSYS, SIGTERM, SIGTRAP, SIGUSR1, SIGUSR2, SIGVTALRM, SIGXCPU, SIGXFSZ,
    ];
    for sig in DEFAULT_TERMINATE {
        test_restart_always_signal(sig)?;
    }
    Ok(())
}

#[test]
fn test_restart_always_normal_exit() -> Result<(), std::io::Error> {
    let (cmd, temp_dir) = get_cli();
    let mut cmd = Command::from_std(cmd);

    let suicide_script = r#"#!/usr/bin/env bash
echo "restarting"
sleep 0.5
"#;
    let service = r#"
[restart]
strategy = "always"
"#;
    store_service_script(temp_dir.path(), suicide_script, Some(service), None);
    cmd.timeout(Duration::from_millis(2000))
        .assert()
        .failure()
        .stdout(predicate::function(|x: &str| {
            x.matches("restarting").count() >= 2
        }));

    Ok(())
}
