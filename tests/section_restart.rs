use assert_cmd::prelude::*;
use predicates::prelude::*;
use predicates::str::contains;

#[allow(dead_code)]
mod utils;
use std::time::Duration;
use utils::*;

//TODO: remove stdout check, and use unsuccessful_exit_finished_failed instead
// with assert.success() and assert.failure()
fn restart_attempts(should_contain: bool, attempts: u32) {
    let (mut cmd, temp_dir) = get_cli();
    let failing_once_script = format!(
        r#"#!/usr/bin/env bash
if [ ! -f {0} ]; then
    touch {0} && exit 1
fi
echo "File is there"
"#,
        temp_dir.path().join("file.temp").display()
    );
    let service = format!(
        r#"
[restart]
attempts = {}
"#,
        attempts
    );
    store_service(
        temp_dir.path(),
        failing_once_script.as_str(),
        Some(service.as_str()),
        None,
    );
    if should_contain {
        cmd.assert().stdout(contains("File is there"));
    } else {
        cmd.assert().stdout(contains("File is there").not());
    }
}

#[test]
fn test_restart_attempts() {
    restart_attempts(false, 0);
    restart_attempts(true, 1);
}

#[test]
fn test_restart_strategy_on_failure() {
    let (mut cmd, temp_dir) = get_cli();

    let failing_once_script = format!(
        r#"#!/usr/bin/env bash
if [ ! -f {0} ]; then
    touch {0} && exit 1
fi
"#,
        temp_dir.path().join("file.temp").display()
    );
    let service = format!(
        r#"
[restart]
attempts = 0
strategy = "on-failure"
"#,
    );
    store_service(
        temp_dir.path(),
        failing_once_script.as_str(),
        Some(service.as_str()),
        None,
    );
    let mut cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(15));
}
