// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the ABP CLI binary and library API.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

/// Create a temp dir with a valid work order JSON file inside.
fn write_work_order(dir: &std::path::Path) -> std::path::PathBuf {
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let path = dir.join("wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();
    path
}

/// Create a temp dir with a valid receipt (with hash) JSON file inside.
fn write_receipt_with_hash(dir: &std::path::Path) -> std::path::PathBuf {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let path = dir.join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    path
}

/// Create a temp dir with a receipt (no hash) JSON file inside.
fn write_receipt_no_hash(dir: &std::path::Path) -> std::path::PathBuf {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let path = dir.join("receipt_nohash.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    path
}

/// Create a minimal backplane.toml config.
fn write_config(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
    let path = dir.join("backplane.toml");
    std::fs::write(&path, content).unwrap();
    path
}

/// Run `abp run --backend mock` with pass-through workspace in the given dir
/// and write receipt to `receipt.json`.
fn run_mock_passthrough(tmp: &tempfile::TempDir) -> assert_cmd::assert::Assert {
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "comprehensive test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
}

// ===========================================================================
// 1. Help text generation
// ===========================================================================

#[test]
fn help_flag_shows_about() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Agent Backplane CLI"));
}

#[test]
fn help_flag_lists_subcommands() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("backends"))
        .stdout(contains("run"))
        .stdout(contains("validate"))
        .stdout(contains("schema"))
        .stdout(contains("inspect"))
        .stdout(contains("config"))
        .stdout(contains("receipt"));
}

#[test]
fn help_short_flag_works() {
    abp()
        .arg("-h")
        .assert()
        .success()
        .stdout(contains("Agent Backplane CLI"));
}

#[test]
fn run_help_shows_flags() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(contains("--backend"))
        .stdout(contains("--task"))
        .stdout(contains("--model"))
        .stdout(contains("--root"))
        .stdout(contains("--workspace-mode"))
        .stdout(contains("--lane"));
}

#[test]
fn run_help_shows_new_flags() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(contains("--param"))
        .stdout(contains("--env"))
        .stdout(contains("--max-budget-usd"))
        .stdout(contains("--max-turns"))
        .stdout(contains("--policy"))
        .stdout(contains("--output"))
        .stdout(contains("--events"));
}

#[test]
fn backends_help_shows_description() {
    abp()
        .args(["backends", "--help"])
        .assert()
        .success()
        .stdout(contains("List available backends"));
}

#[test]
fn validate_help_shows_file_arg() {
    abp()
        .args(["validate", "--help"])
        .assert()
        .success()
        .stdout(contains("JSON file"));
}

#[test]
fn schema_help_shows_kind() {
    abp()
        .args(["schema", "--help"])
        .assert()
        .success()
        .stdout(contains("schema"));
}

#[test]
fn inspect_help_shows_receipt() {
    abp()
        .args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(contains("receipt"));
}

#[test]
fn config_check_help_shows_usage() {
    abp()
        .args(["config", "check", "--help"])
        .assert()
        .success()
        .stdout(contains("config"));
}

#[test]
fn receipt_verify_help_shows_usage() {
    abp()
        .args(["receipt", "verify", "--help"])
        .assert()
        .success();
}

#[test]
fn receipt_diff_help_shows_usage() {
    abp().args(["receipt", "diff", "--help"]).assert().success();
}

// ===========================================================================
// 2. Version output
// ===========================================================================

#[test]
fn version_flag_prints_version() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_short_flag_works() {
    abp()
        .arg("-V")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

// ===========================================================================
// 3. Backends subcommand
// ===========================================================================

#[test]
fn backends_lists_mock() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("mock"));
}

#[test]
fn backends_lists_sidecar_node() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:node"));
}

#[test]
fn backends_lists_sidecar_python() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:python"));
}

#[test]
fn backends_lists_sidecar_claude() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:claude"));
}

#[test]
fn backends_lists_sidecar_copilot() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:copilot"));
}

#[test]
fn backends_lists_sidecar_kimi() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:kimi"));
}

#[test]
fn backends_lists_sidecar_gemini() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:gemini"));
}

#[test]
fn backends_lists_sidecar_codex() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(contains("sidecar:codex"));
}

#[test]
fn backends_lists_short_aliases() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for alias in &[
        "node", "python", "claude", "copilot", "kimi", "gemini", "codex",
    ] {
        assert!(
            stdout.lines().any(|l| l.trim() == *alias),
            "expected alias '{alias}' in backends output"
        );
    }
}

#[test]
fn backends_with_debug_flag() {
    abp()
        .args(["--debug", "backends"])
        .assert()
        .success()
        .stdout(contains("mock"));
}

// ===========================================================================
// 4. Run subcommand
// ===========================================================================

#[test]
fn run_mock_backend_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_passthrough(&tmp).success();
    assert!(tmp.path().join("receipt.json").exists());
}

#[test]
fn run_receipt_is_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_passthrough(&tmp).success();
    let content = std::fs::read_to_string(tmp.path().join("receipt.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("receipt_sha256").is_some());
}

#[test]
fn run_receipt_has_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_passthrough(&tmp).success();
    let content = std::fs::read_to_string(tmp.path().join("receipt.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("outcome").is_some());
}

#[test]
fn run_receipt_has_backend_id() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_passthrough(&tmp).success();
    let content = std::fs::read_to_string(tmp.path().join("receipt.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let backend = v.get("backend").and_then(|b| b.get("id"));
    assert!(backend.is_some());
}

#[test]
fn run_with_json_flag_emits_json_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json test",
            "--json",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
    }
}

#[test]
fn run_with_output_flag_writes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = tmp.path().join("out-receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "output flag test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--output",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt_path.exists());
}

#[test]
fn run_with_events_flag_writes_jsonl() {
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
    assert!(events_path.exists());
    let content = std::fs::read_to_string(&events_path).unwrap();
    for line in content.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("invalid JSON in events file: {e}\n{line}"));
    }
}

#[test]
fn run_with_lane_workspace_first() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "lane test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--lane",
            "workspace-first",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_model_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "model test",
            "--model",
            "gpt-4o",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_include_exclude_globs() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "glob test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--include",
            "*.rs",
            "--exclude",
            "target/**",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_multiple_includes() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "multi-include",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--include",
            "*.rs",
            "--include",
            "*.toml",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_param_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "param test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--param",
            "stream=true",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_env_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "env test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--env",
            "MY_KEY=my_value",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_max_budget_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "budget test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--max-budget-usd",
            "5.0",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_max_turns_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "turns test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--max-turns",
            "10",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_policy_file() {
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
fn run_creates_receipt_dir_automatically() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("deep").join("nested").join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "nested dir test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            nested.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(nested.exists());
}

// ===========================================================================
// 5. Error handling for invalid arguments
// ===========================================================================

#[test]
fn no_subcommand_fails_with_code_2() {
    let output = abp().output().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_subcommand_fails() {
    abp()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(contains("unrecognized subcommand"));
}

#[test]
fn run_missing_task_flag_fails() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(contains("--task"));
}

#[test]
fn run_unknown_flag_fails() {
    abp()
        .args(["run", "--task", "test", "--nonexistent-flag", "value"])
        .assert()
        .failure();
}

#[test]
fn run_invalid_workspace_mode_fails() {
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--workspace-mode",
            "invalid-mode",
        ])
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

#[test]
fn run_invalid_lane_fails() {
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--lane",
            "bogus-lane",
        ])
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

#[test]
fn run_invalid_policy_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let policy = tmp.path().join("bad.json");
    std::fs::write(&policy, "not json").unwrap();
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
            policy.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

#[test]
fn run_nonexistent_policy_file_fails() {
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "missing policy",
            "--workspace-mode",
            "pass-through",
            "--policy",
            "/nonexistent/policy.json",
        ])
        .assert()
        .failure();
}

#[test]
fn schema_invalid_kind_fails() {
    abp()
        .args(["schema", "bogus"])
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

#[test]
fn validate_missing_file_fails() {
    abp()
        .args(["validate", "/nonexistent/file.json"])
        .assert()
        .failure();
}

#[test]
fn inspect_missing_file_fails() {
    abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

#[test]
fn receipt_verify_missing_file_fails() {
    abp()
        .args(["receipt", "verify", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

#[test]
fn receipt_diff_missing_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let p1 = write_receipt_with_hash(tmp.path());
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

#[test]
fn runtime_error_exits_with_code_1() {
    let output = abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

// ===========================================================================
// 6. Validate subcommand
// ===========================================================================

#[test]
fn validate_detects_work_order() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_work_order(tmp.path());
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("valid work_order"));
}

#[test]
fn validate_detects_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("valid receipt"));
}

#[test]
fn validate_rejects_invalid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn validate_rejects_unknown_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("random.json");
    std::fs::write(&path, r#"{"foo": "bar"}"#).unwrap();
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

// ===========================================================================
// 7. Schema subcommand
// ===========================================================================

#[test]
fn schema_work_order_is_valid_json() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
}

#[test]
fn schema_work_order_has_properties() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v.get("properties").is_some() || v.get("$defs").is_some());
}

#[test]
fn schema_work_order_mentions_task() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("task"));
}

#[test]
fn schema_receipt_is_valid_json() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).unwrap();
}

#[test]
fn schema_receipt_mentions_outcome() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("outcome"));
}

#[test]
fn schema_config_is_valid_json() {
    let output = abp().args(["schema", "config"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).unwrap();
}

#[test]
fn schema_config_mentions_backends() {
    let output = abp().args(["schema", "config"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("backends"));
}

// ===========================================================================
// 8. Inspect subcommand
// ===========================================================================

#[test]
fn inspect_valid_receipt_shows_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("VALID"));
}

#[test]
fn inspect_valid_receipt_shows_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("backend: mock"));
}

#[test]
fn inspect_valid_receipt_shows_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("outcome"));
}

#[test]
fn inspect_valid_receipt_shows_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("sha256:"));
}

#[test]
fn inspect_tampered_receipt_shows_invalid() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("0000dead".into());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tampered.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("INVALID"));
}

#[test]
fn inspect_receipt_no_hash_shows_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_no_hash(tmp.path());
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("INVALID"));
}

// ===========================================================================
// 9. Config subcommand
// ===========================================================================

#[test]
fn config_check_with_valid_toml_says_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_config(
        tmp.path(),
        r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"
"#,
    );
    abp()
        .args(["config", "check", "--config", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("ok"));
}

#[test]
fn config_check_invalid_toml_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_config(tmp.path(), "not [valid toml =");
    abp()
        .args(["config", "check", "--config", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("error"));
}

#[test]
fn config_check_missing_file_shows_error() {
    abp()
        .args(["config", "check", "--config", "/nonexistent/backplane.toml"])
        .assert()
        .failure()
        .stdout(contains("error"));
}

#[test]
fn config_check_defaults_without_flag() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(contains("ok"));
}

// ===========================================================================
// 10. Receipt subcommand (verify & diff)
// ===========================================================================

#[test]
fn receipt_verify_valid_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
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
    let path = tmp.path().join("tampered.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(contains("INVALID"));
}

#[test]
fn receipt_diff_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    let p1 = tmp.path().join("r1.json");
    let p2 = tmp.path().join("r2.json");
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
fn receipt_diff_different_backends() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = abp_core::ReceiptBuilder::new("other")
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

// ===========================================================================
// 11. Config loading / backend registration via config file
// ===========================================================================

#[test]
fn config_file_registers_custom_mock_backend() {
    let tmp = tempfile::tempdir().unwrap();
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
    abp()
        .current_dir(tmp.path())
        .args([
            "run",
            "--backend",
            "custom-mock",
            "--task",
            "config test",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt.exists());
}

#[test]
fn config_flag_overrides_default_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("custom.toml");
    std::fs::write(
        &config_path,
        r#"
default_backend = "custom-mock"

[backends.custom-mock]
type = "mock"
"#,
    )
    .unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "run",
            "--task",
            "default backend test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt.exists());
}

#[test]
fn debug_flag_is_accepted_before_subcommand() {
    abp().args(["--debug", "backends"]).assert().success();
}

// ===========================================================================
// 12. Library API tests (abp_cli::commands)
// ===========================================================================

#[test]
fn lib_schema_work_order_valid() {
    let s = abp_cli::commands::schema_json(abp_cli::commands::SchemaKind::WorkOrder).unwrap();
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn lib_schema_receipt_valid() {
    let s = abp_cli::commands::schema_json(abp_cli::commands::SchemaKind::Receipt).unwrap();
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn lib_schema_config_valid() {
    let s = abp_cli::commands::schema_json(abp_cli::commands::SchemaKind::Config).unwrap();
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn lib_validate_file_detects_work_order() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_work_order(tmp.path());
    assert_eq!(
        abp_cli::commands::validate_file(&path).unwrap(),
        abp_cli::commands::ValidatedType::WorkOrder
    );
}

#[test]
fn lib_validate_file_detects_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    assert_eq!(
        abp_cli::commands::validate_file(&path).unwrap(),
        abp_cli::commands::ValidatedType::Receipt
    );
}

#[test]
fn lib_validate_file_rejects_bad_json() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    assert!(abp_cli::commands::validate_file(&path).is_err());
}

#[test]
fn lib_validate_file_rejects_unknown_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("random.json");
    std::fs::write(&path, r#"{"x": 1}"#).unwrap();
    assert!(abp_cli::commands::validate_file(&path).is_err());
}

#[test]
fn lib_validate_work_order_file_accepts_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_work_order(tmp.path());
    abp_cli::commands::validate_work_order_file(&path).unwrap();
}

#[test]
fn lib_validate_work_order_file_rejects_bad() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    assert!(abp_cli::commands::validate_work_order_file(&path).is_err());
}

#[test]
fn lib_inspect_receipt_valid_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    let (_, valid) = abp_cli::commands::inspect_receipt_file(&path).unwrap();
    assert!(valid);
}

#[test]
fn lib_inspect_receipt_no_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_no_hash(tmp.path());
    let (_, valid) = abp_cli::commands::inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn lib_inspect_receipt_tampered_hash() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("bad".into());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tampered.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (_, valid) = abp_cli::commands::inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn lib_verify_receipt_delegates() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_with_hash(tmp.path());
    let (_, valid) = abp_cli::commands::verify_receipt_file(&path).unwrap();
    assert!(valid);
}

#[test]
fn lib_config_check_defaults_ok() {
    let diags = abp_cli::commands::config_check(None).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

#[test]
fn lib_config_check_bad_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.toml");
    std::fs::write(&path, "invalid [toml =").unwrap();
    let diags = abp_cli::commands::config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.starts_with("error:")));
}

#[test]
fn lib_receipt_diff_no_differences() {
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
    let diff = abp_cli::commands::receipt_diff(&p1, &p2).unwrap();
    assert_eq!(diff, "no differences");
}

#[test]
fn lib_receipt_diff_shows_changes() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = abp_core::ReceiptBuilder::new("other")
        .outcome(abp_core::Outcome::Failed)
        .with_hash()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let p1 = tmp.path().join("r1.json");
    let p2 = tmp.path().join("r2.json");
    std::fs::write(&p1, serde_json::to_string_pretty(&r1).unwrap()).unwrap();
    std::fs::write(&p2, serde_json::to_string_pretty(&r2).unwrap()).unwrap();
    let diff = abp_cli::commands::receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("outcome"));
    assert!(diff.contains("backend"));
}

// ===========================================================================
// 13. Library API tests (abp_cli::format)
// ===========================================================================

#[test]
fn lib_output_format_display_roundtrip() {
    for fmt in &[
        abp_cli::format::OutputFormat::Json,
        abp_cli::format::OutputFormat::JsonPretty,
        abp_cli::format::OutputFormat::Text,
        abp_cli::format::OutputFormat::Table,
        abp_cli::format::OutputFormat::Compact,
    ] {
        let s = fmt.to_string();
        let parsed: abp_cli::format::OutputFormat = s.parse().unwrap();
        assert_eq!(&parsed, fmt);
    }
}

#[test]
fn lib_output_format_unknown_rejected() {
    assert!("bogus".parse::<abp_cli::format::OutputFormat>().is_err());
}

#[test]
fn lib_formatter_receipt_json() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Json);
    let out = f.format_receipt(&receipt);
    let _: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
}

#[test]
fn lib_formatter_receipt_json_pretty() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::JsonPretty);
    let out = f.format_receipt(&receipt);
    assert!(out.contains('\n'));
    let _: serde_json::Value = serde_json::from_str(&out).expect("valid pretty JSON");
}

#[test]
fn lib_formatter_receipt_text() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Text);
    let out = f.format_receipt(&receipt);
    assert!(out.contains("Outcome:"));
    assert!(out.contains("Backend:"));
}

#[test]
fn lib_formatter_receipt_table() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Table);
    let out = f.format_receipt(&receipt);
    assert!(out.contains("outcome"));
    assert!(out.contains("backend"));
}

#[test]
fn lib_formatter_receipt_compact() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Compact);
    let out = f.format_receipt(&receipt);
    assert!(out.contains("[complete]"));
    assert!(out.contains("backend=mock"));
}

#[test]
fn lib_formatter_work_order_json() {
    let wo = abp_core::WorkOrderBuilder::new("test").build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Json);
    let out = f.format_work_order(&wo);
    let _: serde_json::Value = serde_json::from_str(&out).unwrap();
}

#[test]
fn lib_formatter_work_order_text() {
    let wo = abp_core::WorkOrderBuilder::new("my task").build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Text);
    let out = f.format_work_order(&wo);
    assert!(out.contains("my task"));
}

#[test]
fn lib_formatter_work_order_table() {
    let wo = abp_core::WorkOrderBuilder::new("my task").build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Table);
    let out = f.format_work_order(&wo);
    assert!(out.contains("task"));
    assert!(out.contains("lane"));
}

#[test]
fn lib_formatter_work_order_compact() {
    let wo = abp_core::WorkOrderBuilder::new("my task").build();
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Compact);
    let out = f.format_work_order(&wo);
    assert!(out.contains("my task"));
}

#[test]
fn lib_formatter_error_json() {
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Json);
    let out = f.format_error("something broke");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.get("error").unwrap().as_str().unwrap(), "something broke");
}

#[test]
fn lib_formatter_error_text() {
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Text);
    let out = f.format_error("something broke");
    assert!(out.contains("Error:"));
    assert!(out.contains("something broke"));
}

#[test]
fn lib_formatter_error_compact() {
    let f = abp_cli::format::Formatter::new(abp_cli::format::OutputFormat::Compact);
    let out = f.format_error("oops");
    assert!(out.contains("[error]"));
}

// ===========================================================================
// 14. Library API tests (abp_cli::config)
// ===========================================================================

#[test]
fn lib_config_load_defaults() {
    let config = abp_cli::config::load_config(None).unwrap();
    assert!(config.backends.is_empty());
}

#[test]
fn lib_config_load_from_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"
log_level = "debug"

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();
    let config = abp_cli::config::load_config(Some(&path)).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("mock"));
    assert_eq!(config.log_level.as_deref(), Some("debug"));
    assert_eq!(config.backends.len(), 1);
}

#[test]
fn lib_config_validate_valid() {
    let config = abp_cli::config::BackplaneConfig {
        backends: std::collections::HashMap::from([(
            "mock".into(),
            abp_cli::config::BackendConfig::Mock {},
        )]),
        ..Default::default()
    };
    abp_cli::config::validate_config(&config).unwrap();
}

#[test]
fn lib_config_validate_empty_command() {
    let config = abp_cli::config::BackplaneConfig {
        backends: std::collections::HashMap::from([(
            "bad".into(),
            abp_cli::config::BackendConfig::Sidecar {
                command: "  ".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let errs = abp_cli::config::validate_config(&config).unwrap_err();
    assert!(!errs.is_empty());
}

#[test]
fn lib_config_validate_zero_timeout() {
    let config = abp_cli::config::BackplaneConfig {
        backends: std::collections::HashMap::from([(
            "s".into(),
            abp_cli::config::BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        )]),
        ..Default::default()
    };
    let errs = abp_cli::config::validate_config(&config).unwrap_err();
    assert!(!errs.is_empty());
}

#[test]
fn lib_config_merge() {
    let base = abp_cli::config::BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = abp_cli::config::BackplaneConfig {
        default_backend: Some("custom".into()),
        ..Default::default()
    };
    let merged = abp_cli::config::merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("custom"));
    assert_eq!(merged.log_level.as_deref(), Some("info"));
}

#[test]
fn lib_config_error_display() {
    let e = abp_cli::config::ConfigError::InvalidBackend {
        name: "x".into(),
        reason: "bad".into(),
    };
    assert!(e.to_string().contains("invalid backend"));

    let e = abp_cli::config::ConfigError::InvalidTimeout { value: 0 };
    assert!(e.to_string().contains("invalid timeout"));

    let e = abp_cli::config::ConfigError::MissingRequiredField {
        field: "name".into(),
    };
    assert!(e.to_string().contains("missing required field"));
}

#[test]
fn lib_config_sidecar_with_args() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[backends.my-sidecar]
type = "sidecar"
command = "node"
args = ["host.js", "--verbose"]
timeout_secs = 60
"#,
    )
    .unwrap();
    let config = abp_cli::config::load_config(Some(&path)).unwrap();
    assert!(config.backends.contains_key("my-sidecar"));
}

// ===========================================================================
// 15. SchemaKind equality / copy / debug
// ===========================================================================

#[test]
fn lib_schema_kind_eq() {
    assert_eq!(
        abp_cli::commands::SchemaKind::WorkOrder,
        abp_cli::commands::SchemaKind::WorkOrder
    );
    assert_ne!(
        abp_cli::commands::SchemaKind::WorkOrder,
        abp_cli::commands::SchemaKind::Receipt
    );
}

#[test]
fn lib_schema_kind_debug() {
    let s = format!("{:?}", abp_cli::commands::SchemaKind::Config);
    assert!(s.contains("Config"));
}

#[test]
fn lib_validated_type_eq() {
    assert_eq!(
        abp_cli::commands::ValidatedType::WorkOrder,
        abp_cli::commands::ValidatedType::WorkOrder
    );
    assert_ne!(
        abp_cli::commands::ValidatedType::WorkOrder,
        abp_cli::commands::ValidatedType::Receipt
    );
}

#[test]
fn lib_validated_type_debug() {
    let s = format!("{:?}", abp_cli::commands::ValidatedType::Receipt);
    assert!(s.contains("Receipt"));
}

// ===========================================================================
// 16. Edge cases
// ===========================================================================

#[test]
fn run_defaults_to_mock_without_backend_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    // No --backend flag and running from temp dir (no backplane.toml) => defaults to mock.
    abp()
        .current_dir(tmp.path())
        .args([
            "run",
            "--task",
            "no backend specified",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt.exists());
}

#[test]
fn run_with_multiple_params() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "multi param",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--param",
            "key1=val1",
            "--param",
            "key2=val2",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_with_multiple_env_vars() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "multi env",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--env",
            "A=1",
            "--env",
            "B=2",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn validate_receipt_without_hash_still_detects_type() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_receipt_no_hash(tmp.path());
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("valid receipt"));
}

#[test]
fn schema_work_order_output_is_not_empty() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    assert!(!output.stdout.is_empty());
}

#[test]
fn schema_receipt_output_is_not_empty() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    assert!(!output.stdout.is_empty());
}
