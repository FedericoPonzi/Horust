use assert_cmd::prelude::*;
use predicates::str::contains;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::time::Duration;

#[allow(dead_code)]
mod utils;
use utils::*;

#[test]
fn test_cli_help() {
    let (mut cmd, _temp_dir) = get_cli();
    cmd.args(vec!["--help"]).assert().success();
}

#[test]
fn test_config_unsuccessful_exit_finished_failed() {
    let (mut cmd, temp_dir) = get_cli();
    let failing_script = r#"#!/usr/bin/env bash
exit 1
"#;
    store_service_script(temp_dir.path(), failing_script, None, None);
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(15));
    let cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    let recv = run_async(cmd, false);
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
/// This tests prints a couple of log lines:
/// Error spawning process: NixError: ENOENT: No such file or directory
/// it's fine
fn test_command_not_found() {
    let (mut cmd, temp_dir) = get_cli();
    let dir = temp_dir.path();
    let rnd_name = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(|x| x as char)
        .collect::<String>();
    let service_name = format!("{}.toml", rnd_name.as_str());
    let service = format!(r#"command = ",sorry_not_found{}""#, rnd_name);
    std::fs::write(dir.join(&service_name), service).unwrap();
    let recv = run_async(&mut cmd, true);
    // Error spawning process: NixError: ENOENT: No such file or directory
    // It's fine
    recv.recv_or_kill(Duration::from_secs(15));
    let cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    let recv = run_async(cmd, false);
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
fn test_single_command() {
    let (mut cmd, _temp_dir) = get_cli();
    cmd.args(vec!["--", "/usr/bin/env bash -c 'echo hello world'"]);
    cmd.assert().success().stdout(contains("hello world"));

    // Invalid command
    let (mut cmd, _temp_dir) = get_cli();
    cmd.args(vec!["--unsuccessful-exit-finished-failed", "--", "\""]);
    cmd.assert()
        .failure()
        .stderr(contains("Failed spawning the process: Invalid command:"));
}

#[test]
fn test_multiple() {
    let (mut cmd, temp_dir, temp_dir_2) = get_cli_multiple();

    let script = r#"#!/usr/bin/env bash
:"#;

    let max = 10;

    for i in 1..max {
        let service = format!(r#"start-after = ["{}.toml"]"#, i - 1);
        let t_dir = if i % 2 == 0 {
            temp_dir.path()
        } else {
            temp_dir_2.path()
        };
        store_service_script(
            t_dir,
            script,
            Some(service.as_str()),
            Some(i.to_string().as_str()),
        );
    }
    store_service_script(temp_dir.path(), script, None, Some("0"));
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(max * 2));
}

#[test]
fn test_stress_test_chained_services() {
    let (mut cmd, temp_dir) = get_cli();
    let script = r#"#!/usr/bin/env bash 
:"#;

    let max = 10;

    for i in 1..max {
        let service = format!(r#"start-after = ["{}.toml"]"#, i - 1);
        store_service_script(
            temp_dir.path(),
            script,
            Some(service.as_str()),
            Some(i.to_string().as_str()),
        );
    }
    store_service_script(temp_dir.path(), script, None, Some("0"));
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(max * 2));
}
