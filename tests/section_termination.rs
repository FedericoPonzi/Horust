use nix::sys::signal::{kill, Signal};
use std::time::Duration;

pub mod utils;
use utils::*;

// Test termination section
// TODO: add a test for termination / signal
#[test]
fn test_termination_wait() {
    // A signal handler will capture our gentle signal,
    // So horust will use the force to stop it:
    let (mut cmd, temp_dir) = get_cli();
    // this script captures traps SIGINT / SIGTERM / SIGEXIT
    let script = r#"#!/usr/bin/env bash
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
    let script = r#"#!/usr/bin/env bash
while true ; do
    sleep 1
done
"#;
    let service = r#"[termination]
wait = "0s"
die-if-failed = ["a.toml"]"#;

    store_service(temp_dir.path(), script, Some(service), None);
    let script = r#"#!/usr/bin/env bash
sleep 1
exit 1
"#;
    store_service(temp_dir.path(), script, None, Some("a"));
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(10));
}
