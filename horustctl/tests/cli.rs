#[expect(
    deprecated,
    reason = "false alert: https://github.com/rust-lang/rust/issues/148426"
)]
use assert_cmd::cargo::cargo_bin;
use assert_cmd::prelude::*;
use predicates::boolean::PredicateBooleanExt;
use predicates::str::contains;
use rand::Rng;
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
