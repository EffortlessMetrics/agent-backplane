// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end CLI tests that exercise the `abp` binary through its CLI interface.
//!
//! These tests spawn the actual binary and verify its behaviour from the
//! outside, covering every subcommand, flag combination, and error path.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;

/// Helper: build a [`Command`] for the `abp` binary.
fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

/// Helper: run the mock backend and return the receipt path.
fn run_mock_to_receipt(tmp: &tempfile::TempDir, extra_args: &[&str]) -> std::path::PathBuf {
    let receipt = tmp.path().join("receipt.json");
    let mut cmd = abp();
    cmd.args([
        "run",
        "--backend",
        "mock",
        "--task",
        "e2e test",
        "--root",
        tmp.path().to_str().unwrap(),
        "--workspace-mode",
        "pass-through",
        "--out",
        receipt.to_str().unwrap(),
    ]);
    cmd.args(extra_args);
    cmd.assert().success();
    receipt
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Help text
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn help_flag_shows_all_subcommands() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"))
        .stdout(predicate::str::contains("Usage"))
        .stdout(predicate::str::contains("backends"))
        .stdout(predicate::str::contains("run"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("schema"))
        .stdout(predicate::str::contains("inspect"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("receipt"));
}

#[test]
fn help_short_flag() {
    abp()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"));
}

#[test]
fn run_help_shows_all_options() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--task"))
        .stdout(predicate::str::contains("--backend"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--param"))
        .stdout(predicate::str::contains("--policy"))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--events"))
        .stdout(predicate::str::contains("--model"))
        .stdout(predicate::str::contains("--max-budget-usd"))
        .stdout(predicate::str::contains("--max-turns"))
        .stdout(predicate::str::contains("--workspace-mode"))
        .stdout(predicate::str::contains("--lane"))
        .stdout(predicate::str::contains("--include"))
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--env"));
}

#[test]
fn backends_help() {
    abp()
        .args(["backends", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend"));
}

#[test]
fn validate_help() {
    abp()
        .args(["validate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("JSON file"));
}

#[test]
fn schema_help() {
    abp()
        .args(["schema", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema"));
}

#[test]
fn inspect_help() {
    abp()
        .args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("receipt"));
}

#[test]
fn config_check_help() {
    abp()
        .args(["config", "check", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config"));
}

#[test]
fn receipt_verify_help() {
    abp()
        .args(["receipt", "verify", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("receipt"));
}

#[test]
fn receipt_diff_help() {
    abp().args(["receipt", "diff", "--help"]).assert().success();
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Version
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn version_flag() {
    abp().arg("--version").assert().success().stdout(
        predicate::str::contains("abp").and(predicate::str::contains(env!("CARGO_PKG_VERSION"))),
    );
}

#[test]
fn version_short_flag() {
    abp()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("abp"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Backends subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backends_lists_mock_and_sidecars() {
    let output = abp().arg("backends").output().expect("execute abp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    for expected in [
        "mock",
        "sidecar:node",
        "sidecar:python",
        "sidecar:claude",
        "sidecar:copilot",
        "sidecar:kimi",
        "sidecar:gemini",
        "sidecar:codex",
    ] {
        assert!(
            stdout.contains(expected),
            "backends output should contain '{expected}'"
        );
    }
}

#[test]
fn backends_lists_short_aliases() {
    let output = abp().arg("backends").output().expect("execute abp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    for alias in [
        "node", "python", "claude", "copilot", "kimi", "gemini", "codex",
    ] {
        assert!(
            stdout.lines().any(|l| l.trim() == alias),
            "backends should list alias '{alias}' on its own line"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Run subcommand with mock backend
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_mock_produces_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_to_receipt(&tmp, &[]);

    assert!(receipt_path.exists(), "receipt file should be written");
    let content = std::fs::read_to_string(&receipt_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert!(json.get("receipt_sha256").is_some());
    assert!(json.get("outcome").is_some());
    assert!(json.get("backend").is_some());
    assert!(json.get("meta").is_some());
    assert!(json.get("trace").is_some());
    assert!(json["meta"].get("run_id").is_some());
    assert!(json["meta"].get("contract_version").is_some());
}

#[test]
fn run_mock_stderr_shows_run_id_and_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "stderr test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("run_id:"))
        .stderr(predicate::str::contains("backend: mock"))
        .stderr(predicate::str::contains("receipt:"));
}

#[test]
fn run_without_backend_defaults_to_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--task",
            "default backend",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("backend: mock"));
}

#[test]
fn run_unknown_backend_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--task",
            "test",
            "--backend",
            "nonexistent-backend-xyz",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

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

    assert!(events_path.exists(), "--events should create events file");
    let content = std::fs::read_to_string(&events_path).unwrap();
    for line in content.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("each line should be valid JSON: {e}\n{line}"));
    }
}

#[test]
fn run_model_flag_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(&tmp, &["--model", "gpt-4o"]);
}

#[test]
fn run_param_flag_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(
        &tmp,
        &["--param", "model=test-model", "--param", "stream=true"],
    );
}

#[test]
fn run_env_flag_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(&tmp, &["--env", "MY_KEY=my_value"]);
}

#[test]
fn run_include_exclude_globs_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(
        &tmp,
        &["--include", "src/**/*.rs", "--exclude", "target/**"],
    );
}

#[test]
fn run_max_budget_and_turns_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(&tmp, &["--max-budget-usd", "5.0", "--max-turns", "10"]);
}

#[test]
fn run_workspace_mode_pass_through() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "ws mode",
            "--workspace-mode",
            "pass-through",
            "--root",
            tmp.path().to_str().unwrap(),
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_lane_workspace_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(&tmp, &["--lane", "workspace-first"]);
}

#[test]
fn run_lane_patch_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_to_receipt(&tmp, &["--lane", "patch-first"]);
}

#[test]
fn run_policy_flag_loads_policy() {
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
fn run_empty_task_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "",
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

// ═══════════════════════════════════════════════════════════════════════
// 5. Error cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn no_subcommand_fails() {
    abp()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage").or(predicate::str::contains("subcommand")));
}

#[test]
fn unknown_subcommand_fails() {
    abp()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn run_missing_task_fails() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
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
        .stderr(predicate::str::contains("invalid value"));
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
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn run_invalid_param_format_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "bad param",
            "--param",
            "no-equals-sign",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

#[test]
fn run_invalid_env_format_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "bad env",
            "--env",
            "no-equals",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
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

#[test]
fn run_nonexistent_policy_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "missing policy",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--policy",
            tmp.path().join("does-not-exist.json").to_str().unwrap(),
        ])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 6. --debug flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_flag_on_backends() {
    abp()
        .args(["--debug", "backends"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn debug_flag_on_run() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "debug test",
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
fn debug_flag_on_config_check() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .current_dir(tmp.path())
        .args(["--debug", "config", "check"])
        .assert()
        .success();
}

// ═══════════════════════════════════════════════════════════════════════
// 7. --format json / --json output
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_flag_emits_valid_json_lines() {
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
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("each stdout line should be valid JSON: {e}\nline: {line}"));
        assert!(parsed.is_object());
    }
}

#[test]
fn json_flag_suppresses_pretty_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json stderr",
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("run_id:"),
        "json mode should suppress pretty headers"
    );
    assert!(
        !stderr.contains("backend:"),
        "json mode should suppress pretty headers"
    );
}

#[test]
fn debug_and_json_flags_together() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "debug+json",
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
    // Only validate lines that look like JSON objects.
    for line in stdout.lines().filter(|l| l.starts_with('{')) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("JSON line invalid: {e}\nline: {line}"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Validate subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_work_order_file() {
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid work_order"));
}

#[test]
fn validate_receipt_file() {
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
        .stdout(predicate::str::contains("valid receipt"));
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

#[test]
fn validate_missing_arg_fails() {
    abp().args(["validate"]).assert().failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Config check subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_check_valid_toml() {
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
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn config_check_invalid_toml_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("bad.toml");
    std::fs::write(&config_path, "not [valid toml =").unwrap();

    abp()
        .args(["config", "check", "--config", config_path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

#[test]
fn config_check_missing_file_fails() {
    abp()
        .args(["config", "check", "--config", "/nonexistent/backplane.toml"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

#[test]
fn config_check_defaults_when_no_flag() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn config_check_global_config_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("custom.toml");
    std::fs::write(
        &config_path,
        r#"
default_backend = "mock"

[backends.test-mock]
type = "mock"
"#,
    )
    .unwrap();

    abp()
        .args(["--config", config_path.to_str().unwrap(), "config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn config_file_in_cwd_registers_custom_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("backplane.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(
        f,
        r#"[backends.my-e2e-mock]
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
            "my-e2e-mock",
            "--task",
            "custom backend test",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Receipt verify subcommand
// ═══════════════════════════════════════════════════════════════════════

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
        .stdout(predicate::str::contains("VALID"))
        .stdout(predicate::str::contains("sha256:"));
}

#[test]
fn receipt_verify_tampered_hash_fails() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("tampered-hash".into());

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID"));
}

#[test]
fn receipt_verify_no_hash_fails() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID"));
}

#[test]
fn receipt_verify_missing_file_fails() {
    abp()
        .args(["receipt", "verify", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

#[test]
fn receipt_verify_missing_arg_fails() {
    abp().args(["receipt", "verify"]).assert().failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Receipt diff subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_diff_identical() {
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
        .stdout(predicate::str::contains("no differences"));
}

#[test]
fn receipt_diff_different() {
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
        .stdout(predicate::str::contains("outcome"))
        .stdout(predicate::str::contains("backend"));
}

#[test]
fn receipt_diff_missing_file_fails() {
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

#[test]
fn receipt_diff_missing_both_args_fails() {
    abp().args(["receipt", "diff"]).assert().failure();
}

#[test]
fn receipt_diff_missing_second_arg_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let p1 = tmp.path().join("r1.json");
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    std::fs::write(&p1, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "diff", p1.to_str().unwrap()])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Inspect subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn inspect_valid_receipt() {
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
        .stdout(predicate::str::contains("VALID"))
        .stdout(predicate::str::contains("outcome"))
        .stdout(predicate::str::contains("backend: mock"))
        .stdout(predicate::str::contains("run_id:"))
        .stdout(predicate::str::contains("sha256:"));
}

#[test]
fn inspect_tampered_receipt_exit_code_1() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("0000dead".into());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    let output = abp()
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("INVALID"));
}

#[test]
fn inspect_missing_file_fails() {
    abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Schema subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn schema_work_order_valid_json() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        v.get("properties").is_some() || v.get("$defs").is_some(),
        "schema should have properties or $defs"
    );
    assert!(
        stdout.contains("task"),
        "work_order schema should have 'task'"
    );
}

#[test]
fn schema_receipt_valid_json() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        stdout.contains("outcome"),
        "receipt schema should have 'outcome'"
    );
}

#[test]
fn schema_config_valid_json() {
    let output = abp().args(["schema", "config"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        stdout.contains("backends"),
        "config schema should have 'backends'"
    );
}

#[test]
fn schema_invalid_kind_fails() {
    abp()
        .args(["schema", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Exit codes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn usage_error_exits_with_code_2() {
    let output = abp().output().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn runtime_error_exits_with_code_1() {
    let output = abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. End-to-end pipeline: run → receipt verify → receipt diff
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn e2e_run_then_verify_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_to_receipt(&tmp, &[]);

    // Verify the receipt produced by mock backend.
    abp()
        .args(["receipt", "verify", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn e2e_run_then_inspect_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_to_receipt(&tmp, &[]);

    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"))
        .stdout(predicate::str::contains("backend: mock"));
}

#[test]
fn e2e_run_then_validate_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_to_receipt(&tmp, &[]);

    abp()
        .args(["validate", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid receipt"));
}

#[test]
fn e2e_two_runs_then_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let r1 = tmp.path().join("r1.json");
    let r2 = tmp.path().join("r2.json");

    // First run.
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "first run",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r1.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Second run.
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "second run",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Diff should show differences (different run_ids at minimum).
    abp()
        .args([
            "receipt",
            "diff",
            r1.to_str().unwrap(),
            r2.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("run_id"));
}

#[test]
fn double_dash_before_help_fails() {
    abp().args(["--", "--help"]).assert().failure();
}
