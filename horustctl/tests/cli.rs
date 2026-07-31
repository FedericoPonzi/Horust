#[expect(
    deprecated,
    reason = "false alert: https://github.com/rust-lang/rust/issues/148426"
)]
use assert_cmd::cargo::cargo_bin;
use assert_cmd::prelude::*;
use predicates::boolean::PredicateBooleanExt;
use predicates::str::contains;
use rand::RngExt;
use rand::distr::Alphanumeric;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// Creates script and service file, and stores them in dir.
/// It will append a `command` at the top of the service, with a reference to script.
/// Returns the service name.
/// if service_name is None a random name will be used.
pub fn store_service_script(
    dir: &Path,
    script: &str,
    service_content: Option<&str>,
    filename: Option<&str>,
) -> String {
    let rng = rand::rng();
    let rnd_name = rng
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(|x| x as char)
        .collect::<String>();
    let service_name = format!("{}.toml", filename.unwrap_or(rnd_name.as_str()));
    let script_name = format!("{}.sh", rnd_name);
    let script_path = dir.join(script_name);
    std::fs::write(&script_path, script).unwrap();
    let service = format!(
        r#"command = "/usr/bin/env bash {}"
{}"#,
        script_path.display(),
        service_content.unwrap_or("")
    );
    std::fs::write(dir.join(&service_name), service).unwrap();
    service_name
}

/// Wait until a file appears (created by the running script), with a timeout.
fn wait_for_file(dir: &Path, filename: &str, max_ms: u64) {
    let mut waited = 0;
    while !dir.join(filename).exists() && waited < max_ms {
        thread::sleep(Duration::from_millis(50));
        waited += 50;
    }
}

/// Build the horust binary once via escargot (cached across calls).
fn build_horust_cmd(temp_dir: &TempDir) -> Command {
    let mut cmd = escargot::CargoBuild::new()
        .package("horust")
        .current_release()
        .current_target()
        .run()
        .expect("Building Horust binary")
        .command();

    cmd.current_dir(temp_dir).args(vec![
        "--services-path",
        temp_dir.path().display().to_string().as_str(),
        "--uds-folder-path",
        temp_dir.path().display().to_string().as_str(),
    ]);
    cmd
}

/// Build a horustctl Command pointing at the temp_dir socket folder.
fn horustctl_cmd(temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(cargo_bin!("horustctl"));
    cmd.current_dir(temp_dir).args(vec![
        "--uds-folder-path",
        temp_dir.path().display().to_string().as_str(),
    ]);
    cmd
}

/// A long-running script that creates a marker file and then sleeps.
static LONG_RUNNING_SCRIPT: &str = r#"#!/usr/bin/env bash
touch marker
i=0
while [ "$i" -lt 30 ]; do
    sleep 1
    i=$((i+1))
done"#;

#[test]
fn test_cli_help() {
    Command::new(cargo_bin!("horustctl"))
        .args(vec!["--help"])
        .assert()
        .success();
}

static ENVIRONMENT_SCRIPT: &str = r#"#!/usr/bin/env bash
printenv"#;

#[test]
fn test_cli_status() {
    let temp_dir = TempDir::with_prefix("horustctl").unwrap();
    let mut horust_cmd = escargot::CargoBuild::new()
        .package("horust")
        .current_release()
        .current_target()
        .run()
        .expect("Building Horust binary")
        .command();

    horust_cmd.current_dir(&temp_dir).args(vec![
        "--services-path",
        temp_dir.path().display().to_string().as_str(),
        "--uds-folder-path",
        temp_dir.path().display().to_string().as_str(),
    ]);

    store_service_script(
        temp_dir.path(),
        ENVIRONMENT_SCRIPT,
        None,
        Some("terminated"),
    );
    horust_cmd.assert().success().stdout(contains("bar").not());
    // Exit after 5 seconds.
    store_service_script(
        temp_dir.path(),
        r#"#!/usr/bin/env bash
    trap 'quit=1' USR1
    touch file
i=0;
while [ "$i" -lt 5 ]; do
    sleep 1
done"#,
        None,
        Some("running"),
    );

    thread::spawn(move || {
        horust_cmd.assert().success().stdout(contains("bar"));
    });
    let mut total_wait = 0;
    const MAX_WAIT_TIME: u32 = 1000;
    // created by running script
    while !temp_dir.path().join("file").exists() && total_wait < MAX_WAIT_TIME {
        total_wait += 50;
        thread::sleep(Duration::from_millis(50));
    }
    Command::new(cargo_bin!("horustctl"))
        .current_dir(&temp_dir)
        .args(vec![
            "--uds-folder-path",
            temp_dir.path().display().to_string().as_str(),
            "status",
            "terminated.toml",
        ])
        .assert()
        .success()
        .stdout(contains("terminated"));

    Command::new(cargo_bin!("horustctl"))
        .current_dir(&temp_dir)
        .args(vec![
            "--uds-folder-path",
            temp_dir.path().display().to_string().as_str(),
            "status",
            "running.toml",
        ])
        .assert()
        .success()
        .stdout(contains("running"));
}

// ============================================================================
// Integration tests for new horustctl commands
// ============================================================================

/// Test: `horustctl status` (no args) shows all services.
#[test]
fn test_status_all_services() {
    let temp_dir = TempDir::with_prefix("horustctl_statall").unwrap();
    let mut horust_cmd = build_horust_cmd(&temp_dir);

    store_service_script(temp_dir.path(), LONG_RUNNING_SCRIPT, None, Some("svc_a"));
    store_service_script(
        temp_dir.path(),
        "#!/usr/bin/env bash\nexit 0",
        None,
        Some("svc_b"),
    );

    thread::spawn(move || {
        horust_cmd.assert().success();
    });
    wait_for_file(temp_dir.path(), "marker", 5000);
    // Give a moment for the short-lived service to finish too
    thread::sleep(Duration::from_millis(500));

    // Status with no service_name should list both services
    let mut cmd = horustctl_cmd(&temp_dir);
    cmd.arg("status");
    cmd.assert()
        .success()
        .stdout(contains("svc_a").and(contains("svc_b")));
}

/// Test: `horustctl stop <service>` stops one service while the other keeps running.
#[test]
fn test_stop_service() {
    let temp_dir = TempDir::with_prefix("horustctl_stop").unwrap();
    let mut horust_cmd = build_horust_cmd(&temp_dir);

    store_service_script(
        temp_dir.path(),
        LONG_RUNNING_SCRIPT,
        Some("[restart]\nstrategy = \"never\""),
        Some("longsvc"),
    );
    // Second service that must remain running after we stop the first.
    store_service_script(
        temp_dir.path(),
        "#!/usr/bin/env bash\ntouch other_marker\nsleep 30",
        Some("[restart]\nstrategy = \"never\""),
        Some("othersvc"),
    );

    thread::spawn(move || {
        horust_cmd.assert().success();
    });
    wait_for_file(temp_dir.path(), "marker", 5000);
    wait_for_file(temp_dir.path(), "other_marker", 5000);

    // Stop only longsvc
    let mut cmd = horustctl_cmd(&temp_dir);
    cmd.args(["stop", "longsvc.toml"]);
    cmd.assert().success().stdout(contains("accepted"));

    // Wait for the service to actually stop
    thread::sleep(Duration::from_secs(2));

    // Verify longsvc is no longer RUNNING
    let mut cmd = horustctl_cmd(&temp_dir);
    cmd.args(["status", "longsvc.toml"]);
    cmd.assert().success().stdout(contains("RUNNING").not());

    // Verify othersvc is still RUNNING
    let mut cmd = horustctl_cmd(&temp_dir);
    cmd.args(["status", "othersvc.toml"]);
    cmd.assert().success().stdout(contains("RUNNING"));
}

/// Test: `horustctl restart <service>` restarts one service without affecting the other.
#[test]
fn test_restart_service() {
    let temp_dir = TempDir::with_prefix("horustctl_restart").unwrap();
    let mut horust_cmd = build_horust_cmd(&temp_dir);

    // Script that writes its PID to a file (so we can detect restart)
    let restart_script = r#"#!/usr/bin/env bash
echo $$ >> pids
touch marker
i=0
while [ "$i" -lt 30 ]; do
    sleep 1
    i=$((i+1))
done"#;

    store_service_script(
        temp_dir.path(),
        restart_script,
        Some("[restart]\nstrategy = \"never\""),
        Some("rsvc"),
    );

    // Second service that must NOT be restarted.
    let other_script = r#"#!/usr/bin/env bash
echo $$ >> other_pids
touch other_marker
sleep 30"#;
    store_service_script(
        temp_dir.path(),
        other_script,
        Some("[restart]\nstrategy = \"never\""),
        Some("stable"),
    );

    thread::spawn(move || {
        horust_cmd.assert().success();
    });
    wait_for_file(temp_dir.path(), "marker", 5000);
    wait_for_file(temp_dir.path(), "other_marker", 5000);

    // Snapshot the other service's PID count before restart
    let other_pids_before = std::fs::read_to_string(temp_dir.path().join("other_pids")).unwrap();
    let other_pid_count_before = other_pids_before.lines().count();

    // Read initial PID count for rsvc
    let pids_before = std::fs::read_to_string(temp_dir.path().join("pids")).unwrap();
    let pid_count_before = pids_before.lines().count();

    // Remove marker so we can detect when the restarted instance creates it
    std::fs::remove_file(temp_dir.path().join("marker")).ok();

    // Restart only rsvc
    let mut cmd = horustctl_cmd(&temp_dir);
    cmd.args(["restart", "rsvc.toml"]);
    cmd.assert().success().stdout(contains("accepted"));

    // Wait for rsvc to restart and write a new PID
    wait_for_file(temp_dir.path(), "marker", 10000);
    thread::sleep(Duration::from_millis(500));

    // Verify rsvc was restarted (new PID appended)
    let pids_after = std::fs::read_to_string(temp_dir.path().join("pids")).unwrap();
    let pid_count_after = pids_after.lines().count();
    assert!(
        pid_count_after > pid_count_before,
        "Expected a new PID after restart. Before: {pid_count_before}, After: {pid_count_after}"
    );

    // Verify the other service was NOT restarted (same PID count)
    let other_pids_after = std::fs::read_to_string(temp_dir.path().join("other_pids")).unwrap();
    let other_pid_count_after = other_pids_after.lines().count();
    assert_eq!(
        other_pid_count_before, other_pid_count_after,
        "Other service should not have been restarted. PIDs before: {other_pid_count_before}, after: {other_pid_count_after}"
    );
}
