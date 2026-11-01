use nix::sys::signal::{Signal, kill};
use std::time::Duration;

pub mod utils;

use utils::*;

#[test]
fn test_late_run_does_not_delay_shutdown() {
    late_run_replay(
        "test_late_run_does_not_delay_shutdown",
        Duration::from_secs(30),
    );
}

#[test]
fn test_late_run_does_not_fail_unstarted_service() {
    late_run_replay(
        "test_late_run_does_not_fail_unstarted_service",
        Duration::from_secs(2),
    );
}

fn late_run_replay(test_name: &str, delay: Duration) {
    const CHILD_ENV: &str = "HORUST_LATE_RUN_TEST_CHILD";
    const UDS_ENV: &str = "HORUST_LATE_RUN_TEST_SOCKET";
    if let Some(mode) = std::env::var_os(CHILD_ENV) {
        use horust::Horust;
        use horust::horust::formats::{Event, ExitStatus, Service, ServiceStatus, ShuttingDown};

        let shutdown = match mode.to_str().expect("shutdown mode") {
            "graceful" => ShuttingDown::Gracefully,
            "forceful" => ShuttingDown::Forcefully,
            other => panic!("unexpected shutdown mode: {other}"),
        };
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Debug)
            .init();
        let mut service = Service::from_command("/bin/true".into());
        service.name = "svc".into();
        service.start_delay = delay;
        let mut horust = Horust::from_services(
            vec![service],
            std::env::var_os(UDS_ENV)
                .expect("replay socket path")
                .into(),
        );
        let bus = horust.join_bus();
        bus.send_event(Event::ShuttingDownInitiated(shutdown));
        bus.send_event(Event::Run("svc".into()));
        let observer = std::thread::spawn(move || {
            let mut seen = vec![];
            loop {
                let events = bus.try_get_events();
                let finished = events.iter().any(|event| {
                    matches!(event, Event::StatusChanged(name,
                        ServiceStatus::Finished | ServiceStatus::FinishedFailed)
                        if name == "svc")
                });
                seen.extend(events);
                if finished {
                    return seen;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let result = horust.run();
        let events = observer.join().unwrap();
        assert_eq!(result, ExitStatus::Successful, "{shutdown:?}: {events:?}");
        assert!(
            !events.iter().any(|event| matches!(
                event,
                Event::StatusChanged(_, ServiceStatus::Starting) | Event::PidChanged(_, _)
            )),
            "late Run admitted: {events:?}"
        );
        return;
    }

    // Isolate signal handlers and waitpid(-1) from other tests.
    for mode in ["graceful", "forceful"] {
        let logs = tempfile::tempdir().unwrap();
        let path = logs.path().join("run.log");
        let log = std::fs::File::create(&path).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env(CHILD_ENV, mode)
            .env(UDS_ENV, logs.path().join("horust.sock"))
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "{mode} replay failed:\n{}",
                    std::fs::read_to_string(&path).unwrap()
                );
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!(
                    "{mode} replay timed out:\n{}",
                    std::fs::read_to_string(&path).unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

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
    recv.recv_or_kill(Duration::from_secs(15));
}

#[test]
fn test_force_kill_preserves_successful_exit_classification() {
    let (mut cmd, temp_dir) = get_cli();
    let logs = tempfile::Builder::new()
        .prefix("horust-forcekill-")
        .tempdir()
        .unwrap()
        .keep();
    let log_path = logs.join("run.log");
    let ready = temp_dir.path().join("ready");
    let script = format!(
        // Stop the fixture if timeout cleanup kills Horust; SIGTERM remains ignored.
        "trap '' TERM\nparent=$PPID\nprintf 'ready\\n' > '{}'\nwhile kill -0 \"$parent\" 2>/dev/null; do sleep 0.1; done\n",
        ready.display()
    );
    store_service_script(
        temp_dir.path(),
        &script,
        Some(
            r#"[termination]
wait = "0s"
[failure]
successful-exit-code = [-137]
strategy = "ignore""#,
        ),
        Some("forcekill"),
    );
    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .env("HORUST_LOG", "debug")
        .stderr(std::fs::File::create(&log_path).unwrap())
        .spawn()
        .unwrap();
    let result = (|| -> anyhow::Result<std::process::ExitStatus> {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let output = std::fs::read_to_string(&log_path)?;
            if ready.exists() && output.contains("StatusChanged(\"forcekill.toml\", Running)") {
                break;
            }
            anyhow::ensure!(
                child.try_wait()?.is_none(),
                "Horust exited before readiness"
            );
            anyhow::ensure!(std::time::Instant::now() < deadline, "readiness timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
        kill(
            nix::unistd::Pid::from_raw(child.id() as i32),
            Signal::SIGTERM,
        )?;
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            anyhow::ensure!(std::time::Instant::now() < deadline, "shutdown timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let status = result.unwrap_or_else(|error| panic!("{error}; logs: {logs:?}"));
    let output = std::fs::read_to_string(&log_path).unwrap();
    assert!(output.contains("Going to send SIGKILL"), "logs: {logs:?}");
    assert!(
        status.success(),
        "configured SIGKILL exit failed; logs: {logs:?}"
    );
    assert!(
        output.contains("Service '\"forcekill.toml\"' has exited."),
        "service was not reaped; logs: {logs:?}"
    );
    assert!(
        output.contains("StatusChanged(\"forcekill.toml\", Success)"),
        "observed exit was not classified successfully; logs: {logs:?}"
    );
    let reaped = output
        .find("Service '\"forcekill.toml\"' has exited.")
        .unwrap();
    assert!(
        !output[reaped..].contains("Going to send SIGKILL"),
        "SIGKILL was sent after the PID was reaped; logs: {logs:?}"
    );
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

/// Test that a service with shutdown-after waits for its dependency to stop first.
/// Service "a" has no shutdown-after and stops immediately on SIGTERM.
/// Service "b" has shutdown-after = ["a.toml"] and should only stop after "a" finishes.
#[test]
fn test_shutdown_after_basic() {
    let (mut cmd, temp_dir) = get_cli();
    let output_file = temp_dir.path().join("shutdown_order.txt");

    // Service A: writes "a" to shared file on SIGTERM, then exits
    let script_a = format!(
        r#"#!/usr/bin/env bash
trap 'echo "a" >> {0}; exit 0' TERM
while true; do sleep 0.1; done
"#,
        output_file.display()
    );
    store_service_script(temp_dir.path(), &script_a, None, Some("a"));

    // Service B: writes "b" on SIGTERM, has shutdown-after = ["a.toml"]
    let script_b = format!(
        r#"#!/usr/bin/env bash
trap 'echo "b" >> {0}; exit 0' TERM
while true; do sleep 0.1; done
"#,
        output_file.display()
    );
    let service_b = r#"shutdown-after = ["a.toml"]"#;
    store_service_script(temp_dir.path(), &script_b, Some(service_b), Some("b"));

    let recv = run_async(&mut cmd, true);
    // Give services time to start
    std::thread::sleep(Duration::from_secs(1));
    kill(recv.pid, Signal::SIGTERM).expect("kill");
    recv.recv_or_kill(Duration::from_secs(10));

    let output = std::fs::read_to_string(&output_file).expect("read output file");
    assert_eq!(output.trim(), "a\nb", "Service a should stop before b");
}

/// Test a chain of shutdown-after dependencies: a → b → c
/// Expected shutdown order: c stops first, then b, then a.
#[test]
fn test_shutdown_after_chain() {
    let (mut cmd, temp_dir) = get_cli();
    let output_file = temp_dir.path().join("shutdown_order.txt");

    // Service A: shutdown-after = ["b.toml"] — stops last
    let script_a = format!(
        r#"#!/usr/bin/env bash
trap 'echo "a" >> {0}; exit 0' TERM
while true; do sleep 0.1; done
"#,
        output_file.display()
    );
    let service_a = r#"shutdown-after = ["b.toml"]"#;
    store_service_script(temp_dir.path(), &script_a, Some(service_a), Some("a"));

    // Service B: shutdown-after = ["c.toml"] — stops second
    let script_b = format!(
        r#"#!/usr/bin/env bash
trap 'echo "b" >> {0}; exit 0' TERM
while true; do sleep 0.1; done
"#,
        output_file.display()
    );
    let service_b = r#"shutdown-after = ["c.toml"]"#;
    store_service_script(temp_dir.path(), &script_b, Some(service_b), Some("b"));

    // Service C: no shutdown-after — stops first
    let script_c = format!(
        r#"#!/usr/bin/env bash
trap 'echo "c" >> {0}; exit 0' TERM
while true; do sleep 0.1; done
"#,
        output_file.display()
    );
    store_service_script(temp_dir.path(), &script_c, None, Some("c"));

    let recv = run_async(&mut cmd, true);
    std::thread::sleep(Duration::from_secs(1));
    kill(recv.pid, Signal::SIGTERM).expect("kill");
    recv.recv_or_kill(Duration::from_secs(15));

    let output = std::fs::read_to_string(&output_file).expect("read output file");
    assert_eq!(output.trim(), "c\nb\na", "Shutdown order should be c, b, a");
}
