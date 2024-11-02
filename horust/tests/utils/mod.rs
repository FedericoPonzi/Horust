use assert_cmd::prelude::*;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempdir::TempDir;

/// Create a random name
pub fn create_random_name() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(|x| x as char)
        .collect::<String>()
}

/// Stores a service
pub fn store_service(dir: &Path, service: &str, service_name: Option<&str>) -> String {
    let service_name = match service_name {
        Some(name) => name.to_string(),
        None => format!("{}.toml", create_random_name()),
    };
    std::fs::write(dir.join(&service_name), service).unwrap();
    service_name
}

/// Creates script and service file, and stores them in dir.
/// It will append a `command` at the top of the service, with a reference to script.
/// Returns the service name.
pub fn store_service_script(
    dir: &Path,
    script: &str,
    service: Option<&str>,
    service_name: Option<&str>,
) -> String {
    let rnd_name = create_random_name();
    let service_name = format!("{}.toml", service_name.unwrap_or(rnd_name.as_str()));
    let script_name = format!("{}.sh", rnd_name);
    let script_path = dir.join(script_name);
    std::fs::write(&script_path, script).unwrap();
    let service = format!(
        r#"command = "/usr/bin/env bash {}"
{}"#,
        script_path.display(),
        service.unwrap_or("")
    );
    store_service(dir, &service, Some(&service_name))
}

#[allow(dead_code)]
pub fn get_cli_multiple() -> (Command, TempDir, TempDir) {
    let temp_dir = TempDir::new("horust").unwrap();
    let temp_dir_2 = TempDir::new("horust_2").unwrap();
    let mut cmd = Command::cargo_bin("horust").unwrap();
    cmd.current_dir(&temp_dir).args(vec![
        "--services-path",
        temp_dir.path().display().to_string().as_str(),
        "--services-path",
        temp_dir_2.path().display().to_string().as_str(),
    ]);

    (cmd, temp_dir, temp_dir_2)
}

pub fn get_cli() -> (Command, TempDir) {
    let temp_dir = TempDir::new("horust").unwrap();
    let mut cmd = Command::cargo_bin("horust").unwrap();
    cmd.current_dir(&temp_dir).args(vec![
        "--services-path",
        temp_dir.path().display().to_string().as_str(),
    ]);
    //.stdout(std::process::Stdio::from(std::fs::File::create("/tmp/stdout").unwrap()))
    //.stderr(std::process::Stdio::from(std::fs::File::create("/tmp/stderr").unwrap(),));
    (cmd, temp_dir)
}

/// Run the cmd in a new process and send a message on receiver when it's done.
/// This allows for ensuring termination of a test.
pub fn run_async(cmd: &mut Command, should_succeed: bool) -> RecvWrapper {
    println!("Cmd: {:?}", cmd);
    let mut child = cmd.spawn().unwrap();
    thread::sleep(Duration::from_millis(500));

    let pid = Pid::from_raw(child.id() as i32);
    let (sender, receiver) = mpsc::sync_channel(0);

    let _handle = thread::spawn(move || {
        sender
            .send(child.wait().expect("wait failed").success())
            .expect("test didn't terminate in time, so chan is closed!");
    });
    RecvWrapper {
        receiver,
        pid,
        should_succeed,
    }
}

/// A simple wrapper for the recv, used for ease of running multi-threaded tests
pub struct RecvWrapper {
    receiver: mpsc::Receiver<bool>,
    pub(crate) pid: Pid,
    should_succeed: bool,
}

impl RecvWrapper {
    pub fn recv_or_kill(self, sleep: Duration) {
        match self.receiver.recv_timeout(sleep) {
            Ok(is_success) => assert_eq!(
                is_success, self.should_succeed,
                "Wrong exit status, right=expected."
            ),
            Err(_err) => {
                println!("Test didn't terminate on time, going to kill horust...");
                kill(self.pid, Signal::SIGKILL).expect("horust kill");
                panic!("Test didn't terminate on time: Horust killed, test failed.");
            }
        }
    }
}
