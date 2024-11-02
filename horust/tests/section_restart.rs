use std::time::Duration;

use assert_cmd::cmd::Command;
#[cfg(target_os = "linux")]
use libc::SIGPOLL;
use libc::{
    c_int, SIGABRT, SIGBUS, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGPIPE, SIGPROF, SIGQUIT,
    SIGSEGV, SIGSYS, SIGTERM, SIGTRAP, SIGUSR1, SIGUSR2, SIGVTALRM, SIGXCPU, SIGXFSZ,
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
        test_restart_always_signal(sig as i32)?;
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
