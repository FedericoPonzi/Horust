mod utils;
use utils::*;

use std::time::Duration;

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
