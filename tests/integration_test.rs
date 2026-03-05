use std::process::Command;

#[test]
fn test_cli_help() {
    let output = Command::new("cargo")
        .args(["run", "--release", "--", "--help"])
        .output()
        .expect("Failed to run command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("findex"));
    assert!(stdout.contains("index"));
    assert!(stdout.contains("search"));
}

#[test]
fn test_cli_index_help() {
    let output = Command::new("cargo")
        .args(["run", "--release", "--", "index", "--help"])
        .output()
        .expect("Failed to run command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Index files"));
    assert!(stdout.contains("--batch-size"));
}

#[test]
fn test_cli_search_help() {
    let output = Command::new("cargo")
        .args(["run", "--release", "--", "search", "--help"])
        .output()
        .expect("Failed to run command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Search indexed files"));
    assert!(stdout.contains("--short"));
}
