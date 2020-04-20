use assert_cmd::prelude::*;
use predicates::prelude::*;
use predicates::str::contains;

#[allow(dead_code)]
mod utils;
use utils::*;

fn restart_attempts(should_contain: bool, attempts: u32) {
    let (mut cmd, temp_dir) = get_cli();
    let failing_once_script = format!(
        r#"#!/usr/bin/env bash
        echo starting
if [ ! -f {0} ]; then
    echo "I'm in!"
    touch {0} && exit 1
    echo "Done O.o"
fi
echo "File is there!:D"
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
        cmd.assert().stdout(contains("File is there!"));
    } else {
        cmd.assert().stdout(contains("File is there!").not());
    }
}

#[test]
fn test_restart_attempts() {
    restart_attempts(false, 0);
}

#[test]
fn test_restart_attempts_succeed() {
    restart_attempts(true, 1);
}
