// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for the `abp` CLI binary.
//!
//! Tests exercise the binary end-to-end using `assert_cmd` and `predicates`.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;

// ─── helpers ────────────────────────────────────────────────────────────

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

/// Run mock backend in pass-through mode and return the receipt path.
fn run_mock(tmp: &tempfile::TempDir, task: &str, extra: &[&str]) -> std::path::PathBuf {
    let receipt = tmp.path().join("receipt.json");
    let mut cmd = abp();
    cmd.args([
        "run",
        "--backend",
        "mock",
        "--task",
        task,
        "--root",
        tmp.path().to_str().unwrap(),
        "--workspace-mode",
        "pass-through",
        "--out",
        receipt.to_str().unwrap(),
    ]);
    cmd.args(extra);
    cmd.assert().success();
    receipt
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Version output
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn version_contains_crate_version() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_contains_binary_name() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("abp "));
}

#[test]
fn version_short_flag_works() {
    abp()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_exit_code_is_zero() {
    let output = abp().arg("--version").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Help text shows all subcommands
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn help_shows_about_text() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"));
}

#[test]
fn help_lists_backends_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("backends"));
}

#[test]
fn help_lists_run_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("run"));
}

#[test]
fn help_lists_validate_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("validate"));
}

#[test]
fn help_lists_schema_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("schema"));
}

#[test]
fn help_lists_inspect_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("inspect"));
}

#[test]
fn help_lists_config_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("config"));
}

#[test]
fn help_lists_receipt_subcommand() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("receipt"));
}

#[test]
fn help_shows_debug_flag() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--debug"));
}

#[test]
fn help_shows_config_flag() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--config"));
}

#[test]
fn short_help_flag_works() {
    abp()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. `abp run --help` shows all run flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_help_shows_task_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--task"));
}

#[test]
fn run_help_shows_backend_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--backend"));
}

#[test]
fn run_help_shows_json_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn run_help_shows_model_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--model"));
}

#[test]
fn run_help_shows_workspace_mode_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--workspace-mode"));
}

#[test]
fn run_help_shows_lane_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--lane"));
}

#[test]
fn run_help_shows_include_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--include"));
}

#[test]
fn run_help_shows_exclude_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--exclude"));
}

#[test]
fn run_help_shows_param_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--param"));
}

#[test]
fn run_help_shows_env_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--env"));
}

#[test]
fn run_help_shows_max_budget_usd_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--max-budget-usd"));
}

#[test]
fn run_help_shows_max_turns_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--max-turns"));
}

#[test]
fn run_help_shows_output_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--output"));
}

#[test]
fn run_help_shows_events_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--events"));
}

#[test]
fn run_help_shows_policy_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--policy"));
}

#[test]
fn run_help_shows_root_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--root"));
}

#[test]
fn run_help_shows_out_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--out"));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. `abp backends` lists registered backends
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backends_includes_mock() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn backends_includes_sidecar_node() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:node"));
}

#[test]
fn backends_includes_sidecar_python() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:python"));
}

#[test]
fn backends_includes_sidecar_claude() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:claude"));
}

#[test]
fn backends_includes_sidecar_copilot() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:copilot"));
}

#[test]
fn backends_includes_sidecar_kimi() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:kimi"));
}

#[test]
fn backends_includes_sidecar_gemini() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:gemini"));
}

#[test]
fn backends_includes_sidecar_codex() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:codex"));
}

#[test]
fn backends_includes_alias_node() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|l| l.trim() == "node"));
}

#[test]
fn backends_includes_alias_python() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|l| l.trim() == "python"));
}

#[test]
fn backends_includes_alias_claude() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|l| l.trim() == "claude"));
}

#[test]
fn backends_includes_alias_gemini() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|l| l.trim() == "gemini"));
}

#[test]
fn backends_exit_code_is_zero() {
    let output = abp().arg("backends").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. `abp run --task "hello" --backend mock` succeeds
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_mock_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "hello", &[]);
}

#[test]
fn run_mock_produces_receipt_file() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = run_mock(&tmp, "hello", &[]);
    assert!(receipt.exists());
}

#[test]
fn run_mock_receipt_is_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = run_mock(&tmp, "hello", &[]);
    let content = std::fs::read_to_string(&receipt).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json.is_object());
}

#[test]
fn run_mock_receipt_contains_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = run_mock(&tmp, "hello", &[]);
    let content = std::fs::read_to_string(&receipt).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json.get("outcome").is_some());
}

#[test]
fn run_mock_receipt_contains_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = run_mock(&tmp, "hello", &[]);
    let content = std::fs::read_to_string(&receipt).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json.get("receipt_sha256").is_some());
}

#[test]
fn run_mock_receipt_contains_meta() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = run_mock(&tmp, "hello", &[]);
    let content = std::fs::read_to_string(&receipt).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json.get("meta").is_some());
    assert!(json["meta"].get("run_id").is_some());
    assert!(json["meta"].get("contract_version").is_some());
}

#[test]
fn run_mock_receipt_contains_backend_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = run_mock(&tmp, "hello", &[]);
    let content = std::fs::read_to_string(&receipt).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["backend"]["id"].as_str(), Some("mock"));
}

#[test]
fn run_mock_stderr_shows_run_info() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "hello",
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

// ═══════════════════════════════════════════════════════════════════════
// 6. Unknown backend fails with error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_nonexistent_backend_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--task",
            "test",
            "--backend",
            "nonexistent",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

#[test]
fn run_nonexistent_backend_prints_error() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--task",
            "test",
            "--backend",
            "nonexistent-xyz-999",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn run_nonexistent_backend_exit_code_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = abp()
        .args([
            "run",
            "--task",
            "test",
            "--backend",
            "nonexistent-xyz-999",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. `abp run` without --task fails
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_missing_task_fails() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
}

#[test]
fn run_missing_task_exit_code_2() {
    let output = abp().args(["run", "--backend", "mock"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. `abp --debug run --task "test" --backend mock` enables debug
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_flag_with_run_succeeds() {
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
fn debug_flag_with_backends_succeeds() {
    abp()
        .args(["--debug", "backends"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn debug_flag_emits_tracing_output() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "debug trace",
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
    // Debug mode enables tracing — expect some DEBUG or tracing output
    assert!(
        stderr.len() > 10,
        "debug mode should produce stderr output, got: {stderr}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Unknown subcommand shows error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_subcommand_exits_with_failure() {
    abp().arg("foobar").assert().failure();
}

#[test]
fn unknown_subcommand_shows_unrecognized_message() {
    abp()
        .arg("nonexistent-cmd")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn unknown_subcommand_exit_code_2() {
    let output = abp().arg("foobar").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn no_subcommand_fails() {
    abp().assert().failure();
}

#[test]
fn no_subcommand_exit_code_2() {
    let output = abp().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. JSON output format when requested
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_flag_emits_json_to_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json output",
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
            .unwrap_or_else(|e| panic!("line should be valid JSON: {e}\n{line}"));
        assert!(parsed.is_object());
    }
}

#[test]
fn json_flag_suppresses_run_id_on_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json suppress",
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
        "json mode should suppress pretty headers on stderr"
    );
}

#[test]
fn json_flag_suppresses_backend_on_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json suppress backend",
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
        !stderr.contains("backend:"),
        "json mode should suppress backend header"
    );
}

#[test]
fn json_and_debug_flags_together_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
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
        .assert()
        .success();
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Exit codes for success/failure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn success_run_exit_code_0() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "exit code test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn backends_exit_code_0() {
    let output = abp().arg("backends").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn runtime_error_exit_code_1() {
    let output = abp()
        .args(["inspect", "/nonexistent/file.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn usage_error_missing_subcommand_exit_code_2() {
    let output = abp().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn usage_error_bad_flag_exit_code_2() {
    let output = abp().arg("--no-such-flag").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn invalid_workspace_mode_exit_code_2() {
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "t",
            "--workspace-mode",
            "bogus",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn invalid_lane_exit_code_2() {
    let output = abp()
        .args(["run", "--backend", "mock", "--task", "t", "--lane", "bogus"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Multiple sequential runs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn two_sequential_runs_produce_different_run_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let r1_path = tmp.path().join("r1.json");
    let r2_path = tmp.path().join("r2.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "run1",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r1_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "run2",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let r1: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&r1_path).unwrap()).unwrap();
    let r2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&r2_path).unwrap()).unwrap();
    assert_ne!(
        r1["meta"]["run_id"], r2["meta"]["run_id"],
        "sequential runs must have different run_ids"
    );
}

#[test]
fn three_sequential_runs_all_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..3 {
        let receipt = tmp.path().join(format!("r{i}.json"));
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                &format!("run {i}"),
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
}

#[test]
fn sequential_runs_with_different_options() {
    let tmp = tempfile::tempdir().unwrap();

    let r1 = tmp.path().join("r1.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "first",
            "--lane",
            "patch-first",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r1.to_str().unwrap(),
        ])
        .assert()
        .success();

    let r2 = tmp.path().join("r2.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "second",
            "--lane",
            "workspace-first",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(r1.exists());
    assert!(r2.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Very long task strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn long_task_string_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let long_task = "a".repeat(1000);
    run_mock(&tmp, &long_task, &[]);
}

#[test]
fn very_long_task_string_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let long_task = "word ".repeat(500);
    run_mock(&tmp, &long_task, &[]);
}

#[test]
fn task_with_special_chars_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "hello! @#$%^&*() [brackets] {braces}", &[]);
}

#[test]
fn task_with_newlines_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "line1\nline2\nline3", &[]);
}

#[test]
fn task_with_quotes_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, r#"task with "double quotes" and 'single'"#, &[]);
}

#[test]
fn empty_task_string_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "", &[]);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Unicode in task strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unicode_task_chinese() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "你好世界", &[]);
}

#[test]
fn unicode_task_emoji() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "🚀 deploy code 🎉", &[]);
}

#[test]
fn unicode_task_japanese() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "テストタスク", &[]);
}

#[test]
fn unicode_task_arabic() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "مهمة الاختبار", &[]);
}

#[test]
fn unicode_task_mixed_scripts() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "Hello мир 世界 🌍", &[]);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Environment variable overrides
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rust_log_env_var_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    // The CLI initializes its own tracing subscriber, so RUST_LOG may not
    // override the filter, but the process should not crash.
    abp()
        .env("RUST_LOG", "debug")
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
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn custom_env_var_does_not_crash() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .env("ABP_TEST_VAR", "some-value")
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "custom env",
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
fn env_flag_passes_env_to_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "env flag", &["--env", "MY_KEY=my_value"]);
}

#[test]
fn multiple_env_flags_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "multi env",
        &["--env", "KEY1=val1", "--env", "KEY2=val2"],
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Additional subcommand and flag tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn schema_work_order_outputs_json() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v.is_object());
}

#[test]
fn schema_receipt_outputs_json() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
}

#[test]
fn schema_config_outputs_json() {
    let output = abp().args(["schema", "config"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
}

#[test]
fn schema_invalid_kind_fails() {
    abp()
        .args(["schema", "nonsense"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn validate_work_order_json() {
    let wo = abp_core::WorkOrderBuilder::new("integration test").build();
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
fn validate_receipt_json() {
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
fn validate_bad_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn inspect_valid_receipt_shows_valid() {
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
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn inspect_tampered_receipt_fails() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("tampered".into());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    let output = abp()
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("INVALID"));
}

#[test]
fn config_check_default_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn config_check_with_valid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("test.toml");
    std::fs::write(
        &config_path,
        r#"
default_backend = "mock"
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
fn config_check_with_bad_toml_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.toml");
    std::fs::write(&path, "[invalid toml =").unwrap();

    abp()
        .args(["config", "check", "--config", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

#[test]
fn global_config_flag_applies_to_config_check() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("custom.toml");
    std::fs::write(
        &config_path,
        r#"
default_backend = "mock"
[backends.local]
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
fn config_file_registers_custom_backend_for_run() {
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
            "custom backend",
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
// 17. Run flag combinations
// ═══════════════════════════════════════════════════════════════════════

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
fn run_model_flag() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "model test", &["--model", "gpt-4o"]);
}

#[test]
fn run_param_key_value() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "param test", &["--param", "key=value"]);
}

#[test]
fn run_multiple_params() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "multi param",
        &["--param", "a=1", "--param", "b=2", "--param", "c=true"],
    );
}

#[test]
fn run_param_without_equals_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "bad param",
            "--param",
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
fn run_env_without_equals_fails() {
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
fn run_include_exclude_globs() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "globs",
        &["--include", "**/*.rs", "--exclude", "target/**"],
    );
}

#[test]
fn run_max_budget_and_turns() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "budget and turns",
        &["--max-budget-usd", "10.0", "--max-turns", "5"],
    );
}

#[test]
fn run_lane_patch_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "patch first", &["--lane", "patch-first"]);
}

#[test]
fn run_lane_workspace_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "workspace first", &["--lane", "workspace-first"]);
}

#[test]
fn run_output_writes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let output_path = tmp.path().join("out-receipt.json");
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
            output_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(output_path.exists());
    let content = std::fs::read_to_string(&output_path).unwrap();
    let _: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
}

#[test]
fn run_events_flag_writes_jsonl_file() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let events = tmp.path().join("events.jsonl");
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
            receipt.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(events.exists());
    let content = std::fs::read_to_string(&events).unwrap();
    for line in content.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("each event line should be valid JSON: {e}"));
    }
}

#[test]
fn run_policy_file_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("policy.json");
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
    run_mock(
        &tmp,
        "policy test",
        &["--policy", policy_path.to_str().unwrap()],
    );
}

#[test]
fn run_invalid_policy_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("bad.json");
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
            tmp.path().join("ghost.json").to_str().unwrap(),
        ])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Receipt subcommand tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_verify_valid() {
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
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn receipt_verify_tampered_fails() {
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("bad-hash".into());
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
fn receipt_diff_identical_shows_no_differences() {
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
fn receipt_diff_different_shows_changes() {
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
        .stdout(predicate::str::contains("outcome"));
}

// ═══════════════════════════════════════════════════════════════════════
// 19. E2E pipeline tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn e2e_run_then_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "e2e verify", &[]);

    abp()
        .args(["receipt", "verify", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn e2e_run_then_inspect() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "e2e inspect", &[]);

    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"))
        .stdout(predicate::str::contains("backend: mock"));
}

#[test]
fn e2e_run_then_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "e2e validate", &[]);

    abp()
        .args(["validate", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid receipt"));
}

#[test]
fn e2e_two_runs_then_diff_shows_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let r1 = tmp.path().join("r1.json");
    let r2 = tmp.path().join("r2.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "first",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r1.to_str().unwrap(),
        ])
        .assert()
        .success();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "second",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2.to_str().unwrap(),
        ])
        .assert()
        .success();

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
