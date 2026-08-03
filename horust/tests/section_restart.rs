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
