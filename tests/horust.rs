use assert_cmd::prelude::*;
use libc::pid_t;
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, thread};
use tempdir::TempDir;

fn get_cli() -> (Command, TempDir) {
    let temp_dir = TempDir::new("horust").unwrap();
    let mut cmd = Command::cargo_bin("horust").unwrap();
    cmd.current_dir(&temp_dir)
        .stdout(Stdio::from(fs::File::create("/tmp/stdout").unwrap()))
        .stderr(Stdio::from(fs::File::create("/tmp/stderr").unwrap()));
    (cmd, temp_dir)
}
// `horust` with no args should exit with a non-zero code.
#[test]
fn client_cli_no_args() {
    get_cli().0.assert().success();
}

#[test]
fn test_cli_help() {
    let (mut cmd, _temp_dir) = get_cli();
    cmd.args(vec!["--help"]).assert().success();
}

fn store_service(dir: &Path) {
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
    let script_path = dir.join("main.sh");
    std::fs::write(&script_path, script).unwrap();
    let service = format!(
        r#"
command = "/bin/bash {}"
[termination]
wait = "1s"
    "#,
        script_path.display()
    );
    std::fs::write(dir.join("my-first-service.toml"), service).unwrap();
}

fn pid_from_id(id: u32) -> Pid {
    let id: pid_t = id as i32;
    Pid::from_raw(id)
}

pub fn list_files<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut paths = std::fs::read_dir(path)?;
    paths.try_fold(vec![], |mut ret, p| match p {
        Ok(entry) => {
            ret.push(entry.path());
            Ok(ret)
        }
        Err(err) => Err(err),
    })
}

// Test termination section
#[test]
fn test_termination() {
    let (mut cmd, temp_dir) = get_cli();
    store_service(temp_dir.path());
    println!("{:?}", list_files(temp_dir.path()));

    let mut child = cmd
        .args(vec![
            "--services-path",
            temp_dir.path().display().to_string().as_str(),
        ])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
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
