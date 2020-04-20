use assert_cmd::prelude::*;
use predicates::str::contains;
use tempdir::TempDir;

#[allow(dead_code)]
mod utils;
use utils::*;

#[test]
fn test_cwd() {
    let (mut cmd, temp_dir) = get_cli();
    let another_dir = TempDir::new("another").unwrap();
    let displ = another_dir.path().display().to_string();
    let service = format!(r#"working-directory = "{}""#, displ);
    let script = r#"#!/usr/bin/env bash
pwd"#;
    store_service(temp_dir.path(), script, Some(service.as_str()), None);
    cmd.assert().success().stdout(contains(displ.as_str()));
}

#[test]
fn test_start_after() {
    let (mut cmd, temp_dir) = get_cli();
    let script_first = r#"#!/usr/bin/env bash
echo "a""#;
    store_service(temp_dir.path(), script_first, None, Some("a"));

    let service_second = r#"start-delay = "500millis" 
    start-after = ["a.toml"]
    "#;
    let script_second = r#"#!/usr/bin/env bash
echo "b""#;
    store_service(
        temp_dir.path(),
        script_second,
        Some(service_second),
        Some("b"),
    );

    let service_c = r#"start-delay = "500millis"
    start-after = ["b.toml"]"#;
    let script_c = r#"#!/usr/bin/env bash
echo "c""#;
    store_service(temp_dir.path(), script_c, Some(service_c), None);

    cmd.assert().success().stdout(contains("a\nb\nc"));
}

// Test user
#[test]
#[ignore]
fn test_user() {
    //TODO: figure how to run this test. ( Sys(EPERM))
    let (mut cmd, temp_dir) = get_cli();
    let service = r#"user = "games""#;
    let script = r#"#!/usr/bin/env bash
whoami"#;
    store_service(temp_dir.path(), script, Some(service), None);
    store_service(temp_dir.path(), script, None, None);
    cmd.assert().success().stdout(contains("games"));
}
