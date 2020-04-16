use assert_cmd::prelude::*;
use libc::pid_t;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use predicates::prelude::*;
use predicates::str::contains;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std;
use std::path::Path;
use std::process::Command;
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
    let rnd_name = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .collect::<String>();
    let service_name = format!("{}.toml", service_name.unwrap_or_else(|| rnd_name.as_str()));
    let script_name = format!("{}.sh", rnd_name);
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
    //.stdout(Stdio::from(std::fs::File::create("/tmp/stdout").unwrap()))
    //.stderr(std::process::Stdio::from(std::fs::File::create("/tmp/stderr").unwrap(),));
    (cmd, temp_dir)
}

/// Run the cmd and send a message on receiver when it's done.
/// This allows for ensuring termination of a test.
fn run_async(cmd: &mut Command, should_succeed: bool) -> RecvWrapper {
    println!("Cmd: {:?}", cmd);
    let mut child = cmd.spawn().unwrap();
    thread::sleep(Duration::from_millis(500));

    let pid = pid_from_id(child.id());
    let (sender, receiver) = mpsc::sync_channel(0);

    let _handle = thread::spawn(move || {
        sender
            .send(child.wait().expect("wait").success())
            .expect("test didn't terminate in time, so chan is closed!");
    });
    RecvWrapper {
        receiver,
        pid,
        should_succeed,
    }
}

/// A simple wrapper for the recv, used for ease of running multi-threaded tests
struct RecvWrapper {
    receiver: mpsc::Receiver<bool>,
    pid: Pid,
    should_succeed: bool,
}

impl RecvWrapper {
    fn recv_or_kill(self, sleep: Duration) {
        match self.receiver.recv_timeout(sleep) {
            Ok(is_success) => assert_eq!(is_success, self.should_succeed),
            Err(_err) => {
                println!("Test didn't terminate on time, going to kill horust...");
                kill(self.pid, Signal::SIGKILL).expect("horust kill");
                panic!("Test didn't terminate on time: Horust killed, test failed.");
            }
        }
    }
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

#[test]
fn test_cwd() {
    let (mut cmd, temp_dir) = get_cli();
    let another_dir = TempDir::new("another").unwrap();
    let displ = another_dir.path().display().to_string();
    let service = format!(r#"working-directory = "{}""#, displ);
    let script = r#"#!/bin/bash
pwd"#;
    store_service(temp_dir.path(), script, Some(service.as_str()), None);
    cmd.assert().success().stdout(contains(displ.as_str()));
}

#[test]
fn test_start_after() {
    let (mut cmd, temp_dir) = get_cli();
    let script_first = r#"#!/bin/bash
echo "a""#;
    store_service(temp_dir.path(), script_first, None, Some("a"));

    let service_second = r#"start-delay = "500millis" 
    start-after = ["a.toml"]
    "#;
    let script_second = r#"#!/bin/bash
echo "b""#;
    store_service(
        temp_dir.path(),
        script_second,
        Some(service_second),
        Some("b"),
    );

    let service_c = r#"start-delay = "500millis"
    start-after = ["b.toml"]"#;
    let script_c = r#"#!/bin/bash
echo "c""#;
    store_service(temp_dir.path(), script_c, Some(service_c), None);

    cmd.assert().success().stdout(contains("a\nb\nc"));
}

// Test termination section
// TODO: add a test for termination / signal
#[test]
fn test_termination_wait() {
    // A signal handler will capture our gentle signal,
    // So horust will use the force to stop it:
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
    :
}
trap_with_arg func_trap INT TERM EXIT
while true ; do
    sleep 1 
done
"#;
    let service = r#"[termination]
wait = "1s""#;
    store_service(temp_dir.path(), script, Some(service), None);

    let recv = run_async(&mut cmd, true);
    kill(recv.pid, Signal::SIGINT).expect("kill");
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
fn test_termination_die_if_failed() {
    let (mut cmd, temp_dir) = get_cli();
    let script = r#"#!/bin/bash
while true ; do
    sleep 1
done
"#;
    let service = r#"[termination]
wait = "0s"
die-if-failed = ["a.toml"]"#;

    store_service(temp_dir.path(), script, Some(service), None);
    let script = r#"#!/bin/bash
sleep 1
exit 1
"#;
    store_service(temp_dir.path(), script, None, Some("a"));
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(10));
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
static ENVIRONMENT_SCRIPT: &str = r#"#!/bin/bash
printenv"#;

// Test environment section
#[test]
fn test_environment_additional() {
    let (mut cmd, temp_dir) = get_cli();

    store_service(temp_dir.path(), ENVIRONMENT_SCRIPT, None, None);
    cmd.assert().success().stdout(contains("bar").not());

    let service = r#"[environment]
keep-env = true
re-export = [ "TERM" ]
additional = { TERM = "bar" }
"#;
    // Additional should overwrite TERM
    store_service(temp_dir.path(), ENVIRONMENT_SCRIPT, Some(service), None);
    cmd.assert().success().stdout(contains("bar"));
}

#[test]
fn test_environment_keep_env() {
    let (mut cmd, temp_dir) = get_cli();
    // keep-env should keep the env :D
    let service = r#"[environment]
keep-env = true
"#;
    store_service(temp_dir.path(), ENVIRONMENT_SCRIPT, Some(service), None);
    cmd.env("DB_PASS", "MyPassword")
        .assert()
        .success()
        .stdout(contains("MyPassword"));
}

#[test]
fn test_environment_re_export() {
    let (mut cmd, temp_dir) = get_cli();
    // If keep env is false, we can choose variables to export:
    let service = r#"[environment]
keep-env = false
re-export = [ "DB_PASS" ]
"#;
    store_service(temp_dir.path(), ENVIRONMENT_SCRIPT, Some(service), None);
    cmd.env("DB_PASS", "MyPassword")
        .assert()
        .success()
        .stdout(contains("MyPassword"));
}

// Test failure strategies
fn test_failure_strategy(strategy: &str) {
    //debug!("running test: {}", strategy);
    let (mut cmd, temp_dir) = get_cli();
    let failing_service = format!(
        r#"[failure]
strategy = "{}"
"#,
        strategy
    );
    let failing_script = r#"#!/bin/bash
# Let's give horust some time to spinup the other service as well.
sleep 1
exit 1"#;
    store_service(
        temp_dir.path(),
        failing_script,
        Some(failing_service.as_str()),
        Some("a"),
    );

    let sleep_service = r#"start-after = ["a.toml"]
[termination]
wait = "500millis"
"#;
    let sleep_script = r#"#!/bin/bash
sleep 30"#;

    //store_service(temp_dir.path(), sleep_script, None, None);
    store_service(temp_dir.path(), sleep_script, Some(sleep_service), None);
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
fn test_failure_shutdown() {
    test_failure_strategy("shutdown");
}

#[test]
fn test_failure_kill_dependents() {
    test_failure_strategy("kill-dependents");
}

fn restart_backoff(should_contain: bool, attempts: u32) {
    let (mut cmd, temp_dir) = get_cli();
    let failing_once_script = format!(
        r#"#!/bin/bash
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
    restart_backoff(false, 0);
}

#[test]
fn test_restart_attempts_succeed() {
    restart_backoff(true, 1);
}

#[test]
fn test_config_unsuccessful_exit_finished_failed() {
    let (mut cmd, temp_dir) = get_cli();
    let failing_script = r#"#!/bin/bash
exit 1
"#;
    store_service(temp_dir.path(), failing_script, None, None);
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(15));
    let mut cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    let recv = run_async(&mut cmd, false);
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
fn test_command_not_found() {
    let (mut cmd, temp_dir) = get_cli();
    let dir = temp_dir.path();
    let rnd_name = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .collect::<String>();
    let service_name = format!("{}.toml", rnd_name.as_str());
    let service = format!(r#"command = ",sorry_not_found{}""#, rnd_name);
    std::fs::write(dir.join(&service_name), service).unwrap();
    cmd.assert().success();
    let cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    cmd.assert().failure();
}

#[test]
fn test_single_command() {
    let (mut cmd, _temp_dir) = get_cli();
    cmd.args(vec!["--", "/bin/bash -c 'echo hello world'"]);
    cmd.assert().success().stdout(contains("hello world"));
}

#[test]
fn test_stress_test_chained_services() {
    let (mut cmd, temp_dir) = get_cli();
    let script = r#"#!/bin/bash 
:"#;

    let max = 10;

    for i in 1..max {
        let service = format!(r#"start-after = ["{}.toml"]"#, i - 1);
        store_service(
            temp_dir.path(),
            script,
            Some(service.as_str()),
            Some(format!("{}", i).as_str()),
        );
    }
    store_service(
        temp_dir.path(),
        script,
        None,
        Some(format!("{}", 0).as_str()),
    );
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(max * 2));
}
