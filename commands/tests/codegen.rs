#![cfg(feature = "codegen")]

use std::process::Command;

#[test]
fn generated_messages_match_checked_in_protocol() {
    let output = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_horust-generate-proto"))
        .arg(output.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(output.path().join("messages.rs")).unwrap(),
        include_str!("../src/proto/messages.rs")
    );
}
