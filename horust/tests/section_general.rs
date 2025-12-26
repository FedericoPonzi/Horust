use assert_cmd::prelude::*;
use predicates::str::{contains, is_empty};
use tempfile::TempDir;

mod utils;
use nix::sys::signal::{Signal, kill};
use std::thread::sleep;
use std::time::Duration;
use utils::*;

fn test_single_output_redirection(stream: &str, to: &str) {
    let (mut cmd, temp_dir) = get_cli();
    let pattern = "Hello".to_string();
    let to = if to == "FILE" {
        let name = format!("{}.log", stream);
        let path = temp_dir.path().join(name);
        path.display().to_string()
    } else {
        to.into()
    };
    let redir = if stream == "stderr" { "1>&2" } else { "" };
    let script = format!(
        r#"#!/usr/bin/env bash
printf "{}" {}"#,
        pattern, redir
    );
    let service = format!(r#"{}="{}""#, stream, to);
    store_service_script(
        temp_dir.path(),
        script.as_str(),
        Some(service.as_str()),
        None,
    );
    if to == "STDOUT" {
        cmd.assert()
            .success()
            .stdout(contains(pattern))
            .stderr(is_empty());
    } else if to == "STDERR" {
        cmd.assert()
            .success()
            .stdout(is_empty())
            .stderr(contains(pattern));
    } else {
        cmd.assert().success().stdout(is_empty()).stdout(is_empty());
        let content = std::fs::read_to_string(&to).unwrap();
        assert_eq!(content, pattern);
    }
}

#[test]
fn test_output_redirection() {
    let from = ["stdout", "stderr"];
    let to = ["STDOUT", "STDERR", "FILE"];
    from.iter()
        .flat_map(|fr| to.iter().map(move |t| (fr, t)))
        .for_each(|(stream, to)| test_single_output_redirection(stream, to));
}

#[test]
fn test_output_log_rotation() {
    // Used as arg for printf. Each number will have up to this number of leading zeros.
    let pattern_len = 4 + "\n".len(); // + 1 for the new line.
    let patterns_per_file = 10;

    let max_log_size = patterns_per_file * pattern_len;
    let num_logs = 10;
    let (mut cmd, temp_dir) = get_cli();
    let output = temp_dir.path().join("out.log").display().to_string();

    // How many patterns do we need to repeat to reach the required file size.
    let total_iterations = (max_log_size * num_logs) / (pattern_len);

    let script = format!(
        r#"#!/usr/bin/env bash
        set -x
for i in {{1..{total_iterations}}}; do printf "%0{}d\n" "$i" ; done
sync
"#,
        pattern_len - 1 // - 1 is the new line.
    );
    let service = [
        format!(r#"stdout="{}""#, output),
        format!(r#"stdout-rotate-size="{}""#, max_log_size),
    ]
    .join("\n");
    store_service_script(
        temp_dir.path(),
        script.as_str(),
        Some(service.as_str()),
        None,
    );
    cmd.assert().success().stdout(is_empty());
    let last_output = temp_dir.path().join(format!("out.log.{}", num_logs - 1));
    assert!(last_output.exists());

    // it is effectively the last output
    assert!(
        temp_dir
            .path()
            .join(format!("out.log.{}", num_logs))
            .exists()
    );

    let content = std::fs::read_to_string(&last_output).unwrap();
    let last_file_first_number = total_iterations - (patterns_per_file);
    let mut expected_content = String::new();
    // the last patterns_file number, say from (90, 100]
    for i in last_file_first_number + 1..=total_iterations {
        let mut number = i.to_string();
        number = format!("{:0>4}", number);
        expected_content.push_str(&number);
        expected_content.push('\n');
    }
    assert_eq!(content, expected_content);
}

#[test]
fn test_search_path_not_found() {
    let (mut cmd, temp_dir) = get_cli();
    store_service(temp_dir.path(), "command = \"non-existent-command\"", None);
    cmd.assert().success().stderr(contains(
        "Program \"non-existent-command\" not found in any of the PATH directories",
    ));
}

#[test]
fn test_search_path_found() {
    let (mut cmd, temp_dir) = get_cli();
    store_service(temp_dir.path(), "command = \"echo kilroy was here\"", None);
    cmd.assert().success().stdout(contains("kilroy was here"));
}

#[test]
fn test_cwd() {
    let (mut cmd, temp_dir) = get_cli();
    let another_dir = TempDir::with_prefix("another").unwrap();
    let displ = another_dir.path().display().to_string();
    let service = format!(r#"working-directory = "{}""#, displ);
    let script = r#"#!/usr/bin/env bash
pwd"#;
    store_service_script(temp_dir.path(), script, Some(service.as_str()), None);
    cmd.assert().success().stdout(contains(displ.as_str()));
}

#[test]
fn test_cwd_default() {
    let (mut cmd, temp_dir) = get_cli();
    let script = r#"#!/usr/bin/env bash
pwd"#;
    store_service_script(temp_dir.path(), script, None, Some("a"));
    cmd.assert()
        .success()
        .stdout(contains(temp_dir.path().display().to_string()));
}

#[test]
fn test_start_after() {
    let (mut cmd, temp_dir) = get_cli();
    let script_first = r#"#!/usr/bin/env bash
echo "a""#;
    store_service_script(temp_dir.path(), script_first, None, Some("a"));

    let service_second = r#"start-delay = "500millis" 
    start-after = ["a.toml"]
    "#;
    let script_second = r#"#!/usr/bin/env bash
echo "b""#;
    store_service_script(
        temp_dir.path(),
        script_second,
        Some(service_second),
        Some("b"),
    );

    let service_c = r#"start-delay = "500millis"
    start-after = ["b.toml"]"#;
    let script_c = r#"#!/usr/bin/env bash
echo "c""#;
    store_service_script(temp_dir.path(), script_c, Some(service_c), None);

    cmd.assert().success().stdout(contains("a\nb\nc"));
}

// Test user
#[test]
#[ignore]
fn test_user() {
    //TODO: figure how to run this test. ( Sys(EPERM)) maybe in docker?
    let (mut cmd, temp_dir) = get_cli();
    let service = r#"user = "games""#;
    let script = r#"#!/usr/bin/env bash
whoami"#;
    store_service_script(temp_dir.path(), script, Some(service), None);
    store_service_script(temp_dir.path(), script, None, None);
    cmd.assert().success().stdout(contains("games"));
}

#[test]
fn test_termination_with_pending_thread() {
    // start-delay should not interfere with the shutting down.
    let (mut cmd, temp_dir) = get_cli();
    let script = r#"#!/usr/bin/env bash
while true ; do
    sleep 1
done
"#;
    let service = r#"
start-delay = "10s"
"#;
    store_service_script(temp_dir.path(), script, Some(service), None);
    let recv = run_async(&mut cmd, true);
    sleep(Duration::from_secs(1));
    kill(recv.pid, Signal::SIGINT).expect("kill");
    recv.recv_or_kill(Duration::from_secs(5));
}
