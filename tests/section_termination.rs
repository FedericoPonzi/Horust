use nix::sys::signal::{kill, Signal};
use std::time::Duration;

pub mod utils;

use utils::*;

// Test termination section
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
    store_service_script(temp_dir.path(), script, Some(service), None);

    let recv = run_async(&mut cmd, true);
    kill(recv.pid, Signal::SIGINT).expect("kill");
    recv.recv_or_kill(Duration::from_secs(5));
}

fn test_termination_custom_signal(friendly_name: &str) {
    let (mut cmd, temp_dir) = get_cli();
    // this script captures traps signals
    let script = format!(
        r#"#!/usr/bin/env bash
trap_with_arg() {{
    func="$1" ; shift
    for sig ; do
        trap "$func $sig" "$sig"
    done
}}
func_trap() {{
    if [ "$1" == "{0}" ] ; then
        exit 0
    fi
}}
trap_with_arg func_trap {0}
while true ; do
    sleep 0.3
done
"#,
        friendly_name
    );
    let service = format!(
        r#"[termination]
signal = "{}"
wait = "10s""#,
        friendly_name
    ); // wait is higher than the test duration.

    store_service_script(
        temp_dir.path(),
        script.as_str(),
        Some(service.as_str()),
        None,
    );
    let recv = run_async(&mut cmd, true);
    kill(recv.pid, Signal::SIGTERM).expect("kill");
    recv.recv_or_kill(Duration::from_secs(20));
}

/// User can set a custom termination signal, this test will ensure we're sending the correct one.
#[test]
fn test_termination_all_custom_signals() {
    #[cfg(target_os = "linux")]
    let signals = vec![
        "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "USR1", "SEGV", "USR2", "PIPE",
        "ALRM", "TERM", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU", "URG", "XCPU", "XFSZ",
        "VTALRM", "PROF", "WINCH", "IO", "SYS",
    ];
    #[cfg(not(target_os = "linux"))]
    let signals = vec![
        "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "USR1", "SEGV", "USR2", "PIPE",
        "ALRM", "TERM", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU", "URG", "XCPU", "XFSZ",
        "VTALRM", "PROF", "WINCH", "IO", "SYS",
    ];
    signals.into_iter().for_each(|friendly_name| {
        eprintln!("Testing: {}", friendly_name);
        test_termination_custom_signal(friendly_name);
    })
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

    store_service_script(temp_dir.path(), script, Some(service), None);
    let script = r#"#!/usr/bin/env bash
sleep 1
exit 1
"#;
    store_service_script(temp_dir.path(), script, None, Some("a"));
    let recv = run_async(&mut cmd, true);
    recv.recv_or_kill(Duration::from_secs(10));
}
