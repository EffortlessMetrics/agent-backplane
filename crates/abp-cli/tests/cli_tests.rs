// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the `abp` CLI binary.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

// ── Help & version ──────────────────────────────────────────────────

#[test]
fn help_flag_prints_usage() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Agent Backplane CLI"))
        .stdout(contains("backends"))
        .stdout(contains("run"));
}

#[test]
fn version_flag_prints_version() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

// ── Subcommands ─────────────────────────────────────────────────────

#[test]
fn backends_subcommand_lists_backends() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("mock"))
        .stdout(contains("sidecar:node"))
        .stdout(contains("sidecar:python"))
        .stdout(contains("sidecar:claude"));
}

#[test]
fn run_with_mock_backend_succeeds() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "hello world",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt.exists(), "receipt file should be written");
    let content = std::fs::read_to_string(&receipt).unwrap();
    let receipt_json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(receipt_json.get("receipt_sha256").is_some());
}

#[test]
fn run_with_json_flag_emits_json() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");

    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "hello",
            "--json",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    // Each stdout line should be valid JSON (event stream).
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
    }
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn unknown_subcommand_gives_error() {
    abp()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(contains("unrecognized subcommand"));
}

#[test]
fn run_missing_required_task_flag() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(contains("--task"));
}

// ── Debug flag ──────────────────────────────────────────────────────

#[test]
fn debug_flag_is_accepted() {
    abp()
        .args(["--debug", "backends"])
        .assert()
        .success()
        .stdout(contains("mock"));
}

// ── Config file loading ─────────────────────────────────────────────

#[test]
fn config_file_registers_backends() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Write a minimal TOML config with a custom mock backend.
    let config_path = tmp.path().join("backplane.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(
        f,
        r#"[backends.custom-mock]
type = "mock"
"#
    )
    .unwrap();

    let receipt = tmp.path().join("receipt.json");

    // Run from the temp dir so it picks up backplane.toml.
    abp()
        .current_dir(tmp.path())
        .args([
            "run",
            "--backend",
            "custom-mock",
            "--task",
            "config-test",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt.exists(), "receipt file should be written");
}
