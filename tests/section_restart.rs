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
