mod utils;
use utils::*;

use std::time::Duration;
use std::time::Instant;

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
    let failing_script = r#"#!/usr/bin/env bash
# Let's give horust some time to spinup the other service as well.
sleep 1
exit 1"#;
    store_service_script(
        temp_dir.path(),
        failing_script,
        Some(failing_service.as_str()),
        Some("a"),
    );

    let sleep_service = r#"start-after = ["a.toml"]
[termination]
wait = "500millis"
"#;
    let sleep_script = r#"#!/usr/bin/env bash
sleep 30"#;

    //store_service(temp_dir.path(), sleep_script, None, None);
    store_service_script(temp_dir.path(), sleep_script, Some(sleep_service), None);
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

#[test]
fn test_failure_shutdown_preserves_failure_with_retries_remaining() {
    let (mut cmd, temp_dir) = get_cli();
    let launches_path = temp_dir.path().join("launches.log");
    let script = format!("printf 'run\\n' >> '{}'\nexit 7\n", launches_path.display());
    let service = r#"
[failure]
strategy = "shutdown"
successful-exit-code = [0]

[restart]
strategy = "on-failure"
attempts = 3

[healthiness]
healthy-after = "60s"
"#;
    store_service_script(temp_dir.path(), &script, Some(service), Some("failing"));
    let log_path = temp_dir.path().join("horust.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = cmd
        .arg("--unsuccessful-exit-finished-failed")
        .env("HORUST_LOG", "debug")
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "Horust did not finish shutdown:\n{}",
                std::fs::read_to_string(&log_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let logs = std::fs::read_to_string(log_path).unwrap();
    assert_eq!(status.code(), Some(101), "{logs}");
    assert_eq!(
        std::fs::read_to_string(launches_path)
            .unwrap()
            .lines()
            .count(),
        1,
        "unexpected restart during shutdown:\n{logs}"
    );
}
