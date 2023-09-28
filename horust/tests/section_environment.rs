use assert_cmd::prelude::*;
use predicates::prelude::*;
use predicates::str::contains;

#[allow(dead_code)]
mod utils;
use utils::{get_cli, store_service_script};

static ENVIRONMENT_SCRIPT: &str = r#"#!/usr/bin/env bash
printenv"#;

// Test environment section
#[test]
fn test_environment_additional() {
    let (mut cmd, temp_dir) = get_cli();

    store_service_script(temp_dir.path(), ENVIRONMENT_SCRIPT, None, None);
    cmd.assert().success().stdout(contains("bar").not());

    let service = r#"[environment]
keep-env = true
re-export = [ "TERM" ]
additional = { TERM = "bar" }
"#;
    // Additional should overwrite TERM
    store_service_script(temp_dir.path(), ENVIRONMENT_SCRIPT, Some(service), None);
    cmd.assert().success().stdout(contains("bar"));
}

#[test]
fn test_environment_keep_env() {
    let (mut cmd, temp_dir) = get_cli();
    // keep-env should keep the env :D
    let service = r#"[environment]
keep-env = true
"#;
    store_service_script(temp_dir.path(), ENVIRONMENT_SCRIPT, Some(service), None);
    cmd.env("DB_PASS", "MyPassword")
        .assert()
        .success()
        .stdout(contains("MyPassword"));
}

#[test]
fn test_environment_re_export() {
    let (mut cmd, temp_dir) = get_cli();
    // If keep env is false, we can choose variables to export:
    let service = r#"[environment]
keep-env = false
re-export = [ "DB_PASS" ]
"#;
    store_service_script(temp_dir.path(), ENVIRONMENT_SCRIPT, Some(service), None);
    cmd.env("DB_PASS", "MyPassword")
        .assert()
        .success()
        .stdout(contains("MyPassword"));
}
