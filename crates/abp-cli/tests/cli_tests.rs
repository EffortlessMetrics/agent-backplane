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

// ── Validate subcommand ─────────────────────────────────────────────

#[test]
fn validate_valid_work_order_succeeds() {
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("valid work_order"));
}

#[test]
fn validate_invalid_json_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json at all").unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn validate_wrong_schema_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("wrong.json");
    std::fs::write(&path, r#"{"foo": "bar"}"#).unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn validate_missing_file_fails() {
    abp()
        .args(["validate", "/nonexistent/file.json"])
        .assert()
        .failure();
}

// ── Schema subcommand ───────────────────────────────────────────────

#[test]
fn schema_work_order_prints_valid_json() {
    let output = abp()
        .args(["schema", "work-order"])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        v.get("properties").is_some() || v.get("$defs").is_some(),
        "schema should have properties or definitions"
    );
}

#[test]
fn schema_receipt_prints_valid_json() {
    let output = abp()
        .args(["schema", "receipt"])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
}

#[test]
fn schema_config_prints_valid_json() {
    let output = abp()
        .args(["schema", "config"])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
}

#[test]
fn schema_invalid_kind_fails() {
    abp()
        .args(["schema", "bogus"])
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

// ── Inspect subcommand ──────────────────────────────────────────────

#[test]
fn inspect_valid_receipt_shows_valid_hash() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("VALID"))
        .stdout(contains("outcome"))
        .stdout(contains("backend: mock"));
}

#[test]
fn inspect_tampered_receipt_shows_invalid_hash() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();

    receipt.receipt_sha256 = Some("0000dead".into());

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("INVALID"));
}

#[test]
fn inspect_missing_file_fails() {
    abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

// ── New run flags ───────────────────────────────────────────────────

#[test]
fn run_output_flag_writes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = tmp.path().join("output-receipt.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "output test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--output",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt_path.exists(), "--output should write receipt");
    let content = std::fs::read_to_string(&receipt_path).unwrap();
    let _: serde_json::Value = serde_json::from_str(&content).expect("valid receipt JSON");
}

#[test]
fn run_events_flag_writes_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = tmp.path().join("receipt.json");
    let events_path = tmp.path().join("events.jsonl");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "events test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
            "--events",
            events_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(events_path.exists(), "--events should write events file");
    let content = std::fs::read_to_string(&events_path).unwrap();
    for line in content.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("each line should be JSON: {e}\n{line}"));
    }
}

#[test]
fn run_policy_flag_loads_policy_file() {
    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("policy.json");
    let receipt_path = tmp.path().join("receipt.json");

    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    std::fs::write(&policy_path, serde_json::to_string(&policy).unwrap()).unwrap();

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "policy test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--policy",
            policy_path.to_str().unwrap(),
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_invalid_policy_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("bad-policy.json");
    std::fs::write(&policy_path, "not json").unwrap();

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "bad policy",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--policy",
            policy_path.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

// ── Help text for new subcommands ───────────────────────────────────

#[test]
fn help_lists_new_subcommands() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("validate"))
        .stdout(contains("schema"))
        .stdout(contains("inspect"))
        .stdout(contains("config"))
        .stdout(contains("receipt"));
}

#[test]
fn validate_help_shows_usage() {
    abp()
        .args(["validate", "--help"])
        .assert()
        .success()
        .stdout(contains("JSON file"));
}

#[test]
fn schema_help_shows_usage() {
    abp()
        .args(["schema", "--help"])
        .assert()
        .success()
        .stdout(contains("schema"));
}

#[test]
fn inspect_help_shows_usage() {
    abp()
        .args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(contains("receipt"));
}

// ── Exit code tests ─────────────────────────────────────────────────

#[test]
fn runtime_error_exits_with_code_1() {
    // Invalid file for inspect should give exit code 1
    let output = abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .output()
        .expect("execute abp");

    assert!(!output.status.success());
    // code() works on all platforms; no ExitStatusExt needed
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn usage_error_exits_with_code_2() {
    // Missing required subcommand → clap exits with 2
    let output = abp().output().expect("execute abp");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

// ── Validate auto-detect ────────────────────────────────────────────

#[test]
fn validate_receipt_file_succeeds() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("valid receipt"));
}

// ── Config check subcommand ─────────────────────────────────────────

#[test]
fn config_check_with_valid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("backplane.toml");
    std::fs::write(
        &config_path,
        r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();

    abp()
        .args(["config", "check", "--config", config_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("ok"));
}

#[test]
fn config_check_with_invalid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("bad.toml");
    std::fs::write(&config_path, "not [valid toml =").unwrap();

    abp()
        .args(["config", "check", "--config", config_path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("error"));
}

#[test]
fn config_check_with_missing_file() {
    abp()
        .args(["config", "check", "--config", "/nonexistent/backplane.toml"])
        .assert()
        .failure()
        .stdout(contains("error"));
}

#[test]
fn config_check_defaults_without_flag() {
    let tmp = tempfile::tempdir().unwrap();
    // Run from a directory with no backplane.toml; should use defaults.
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(contains("ok"));
}

#[test]
fn config_check_help_shows_usage() {
    abp()
        .args(["config", "check", "--help"])
        .assert()
        .success()
        .stdout(contains("config"));
}

// ── Receipt verify subcommand ───────────────────────────────────────

#[test]
fn receipt_verify_valid_hash() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("VALID"));
}

#[test]
fn receipt_verify_invalid_hash() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("tampered".into());

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("INVALID"));
}

#[test]
fn receipt_verify_missing_file() {
    abp()
        .args(["receipt", "verify", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

// ── Receipt diff subcommand ─────────────────────────────────────────

#[test]
fn receipt_diff_identical_receipts() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let p1 = tmp.path().join("r1.json");
    let p2 = tmp.path().join("r2.json");
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    std::fs::write(&p1, &json).unwrap();
    std::fs::write(&p2, &json).unwrap();

    abp()
        .args([
            "receipt",
            "diff",
            p1.to_str().unwrap(),
            p2.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("no differences"));
}

#[test]
fn receipt_diff_different_receipts() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = abp_core::ReceiptBuilder::new("other-backend")
        .outcome(abp_core::Outcome::Failed)
        .with_hash()
        .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let p1 = tmp.path().join("r1.json");
    let p2 = tmp.path().join("r2.json");
    std::fs::write(&p1, serde_json::to_string_pretty(&r1).unwrap()).unwrap();
    std::fs::write(&p2, serde_json::to_string_pretty(&r2).unwrap()).unwrap();

    abp()
        .args([
            "receipt",
            "diff",
            p1.to_str().unwrap(),
            p2.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("outcome"))
        .stdout(contains("backend"));
}

#[test]
fn receipt_diff_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p1 = tmp.path().join("r1.json");
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    std::fs::write(&p1, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args([
            "receipt",
            "diff",
            p1.to_str().unwrap(),
            "/nonexistent/r2.json",
        ])
        .assert()
        .failure();
}

// ── Schema contains expected fields ─────────────────────────────────

#[test]
fn schema_work_order_contains_task_field() {
    let output = abp()
        .args(["schema", "work-order"])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("task"),
        "work_order schema should mention 'task'"
    );
}

#[test]
fn schema_receipt_contains_outcome_field() {
    let output = abp()
        .args(["schema", "receipt"])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("outcome"),
        "receipt schema should mention 'outcome'"
    );
}

#[test]
fn schema_config_contains_backends_field() {
    let output = abp()
        .args(["schema", "config"])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("backends"),
        "config schema should mention 'backends'"
    );
}
