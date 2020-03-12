use assert_cmd::prelude::*;
use libc::pid_t;
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use predicates::prelude::*;
use predicates::str::contains;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempdir::TempDir;

/// Creates script and service file, and stores them in dir.
/// It will append a `command` at the top of the service, with a reference to script.
/// Returns the service name.
fn store_service(
    dir: &Path,
    script: &str,
    service: Option<&str>,
    service_name: Option<&str>,
) -> String {
    let rnd_name = || {
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(5)
            .collect::<String>()
    };
    let service_name = format!(
        "{}{}.toml",
        service_name
            .map(|name| format!("{}-", name))
            .unwrap_or_else(|| "".into()),
        rnd_name()
    );
    let script_name = rnd_name();
    let script_path = dir.join(script_name);
    std::fs::write(&script_path, script).unwrap();
    let service = format!(
        r#"command = "/bin/bash {}"
{}"#,
        script_path.display(),
        service.unwrap_or("")
    );
    std::fs::write(dir.join(&service_name), service).unwrap();
    service_name
}

fn get_cli() -> (Command, TempDir) {
    let temp_dir = TempDir::new("horust").unwrap();
    let mut cmd = Command::cargo_bin("horust").unwrap();
    cmd.current_dir(&temp_dir).args(vec![
        "--services-path",
        temp_dir.path().display().to_string().as_str(),
    ]);
    //.stdout(Stdio::from(fs::File::create("/tmp/stdout").unwrap()))
    //.stderr(Stdio::from(fs::File::create("/tmp/stderr").unwrap()));
    (cmd, temp_dir)
}

#[test]
fn test_cli_help() {
    let (mut cmd, _temp_dir) = get_cli();
    cmd.args(vec!["--help"]).assert().success();
}

fn pid_from_id(id: u32) -> Pid {
    let id: pid_t = id as i32;
    Pid::from_raw(id)
}

// Test termination section
#[test]
fn test_termination() {
    let (mut cmd, temp_dir) = get_cli();
    // this script captures traps SIGINT / SIGTERM / SIGEXIT
    let script = r#"#!/bin/bash

trap_with_arg() {
    func="$1" ; shift
    for sig ; do
        trap "$func $sig" "$sig"
    done
}
func_trap() {
    echo "Trapped: $1"
}
trap_with_arg func_trap INT TERM EXIT
echo "Send signals to PID $$ and type [enter] when done."
while true ; do
sleep 1 
done 
# Wait so the script doesn't exit.
"#;
    let service = r#"[termination]
wait = "1s""#;
    store_service(temp_dir.path(), script, Some(service), None);

    let mut child = cmd.stdin(Stdio::null()).spawn().unwrap();
    thread::sleep(Duration::from_millis(500));

    let pid = pid_from_id(child.id());
    let (sender, receiver) = mpsc::sync_channel(0);

    let _handle = thread::spawn(move || {
        child.wait().unwrap().success();
        sender.send(123).unwrap();
    });

    kill(pid, Signal::SIGINT).unwrap();
    thread::sleep(Duration::from_secs(3));
    receiver.try_recv().unwrap();
}

// Test user
#[test]
#[ignore]
fn test_user() {
    //TODO: figure how to run this test. ( Sys(EPERM))
    let (mut cmd, temp_dir) = get_cli();
    let service = r#"user = "games""#;
    let script = r#"#!/bin/bash
whoami"#;
    store_service(temp_dir.path(), script, Some(service), None);
    store_service(temp_dir.path(), script, None, None);
    cmd.assert().success().stdout(contains("games"));
}

// Test environment section
#[test]
fn test_environment() {
    let (mut cmd, temp_dir) = get_cli();
    let service = r#"[environment]
foo = "bar""#;
    let script = r#"#!/bin/bash
printenv"#;

    store_service(temp_dir.path(), script, None, None);
    cmd.assert().success().stdout(contains("bar").not());

    store_service(temp_dir.path(), script, Some(service), None);
    cmd.assert().success().stdout(contains("bar"));
}
