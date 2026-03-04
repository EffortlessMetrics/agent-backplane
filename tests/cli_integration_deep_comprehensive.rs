#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive CLI integration tests for the `abp` binary.
//!
//! Exercises the binary end-to-end via `assert_cmd` and `predicates`, covering
//! version, help, backends, run, debug, error formatting, exit codes, and
//! various WorkOrder configuration combinations.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;

// ─── helpers ────────────────────────────────────────────────────────────

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

/// Run mock backend in pass-through mode, returning the receipt path.
fn run_mock_pt(tmp: &tempfile::TempDir, task: &str, extra: &[&str]) -> std::path::PathBuf {
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

/// Parse a receipt file into a JSON value.
fn read_receipt(path: &std::path::Path) -> serde_json::Value {
    let content = std::fs::read_to_string(path).expect("read receipt");
    serde_json::from_str(&content).expect("parse receipt JSON")
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Version output (`--version`)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_version_long_flag_contains_version() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn dc_version_short_flag_works() {
    abp()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn dc_version_starts_with_binary_name() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("abp "));
}

#[test]
fn dc_version_exit_code_zero() {
    let output = abp().arg("--version").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn dc_version_no_stderr() {
    let output = abp().arg("--version").output().unwrap();
    assert!(output.stderr.is_empty(), "no stderr expected for --version");
}

#[test]
fn dc_version_single_line() {
    let output = abp().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 1, "version should be a single line");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Help output (`--help`)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_help_long_flag_shows_about() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"));
}

#[test]
fn dc_help_short_flag_works() {
    abp()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"));
}

#[test]
fn dc_help_lists_all_subcommands() {
    let output = abp().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for subcmd in [
        "backends", "run", "validate", "schema", "inspect", "config", "receipt",
    ] {
        assert!(
            stdout.contains(subcmd),
            "help output should list subcommand '{subcmd}'"
        );
    }
}

#[test]
fn dc_help_shows_debug_flag() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--debug"));
}

#[test]
fn dc_help_shows_config_flag() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--config"));
}

#[test]
fn dc_help_exit_code_zero() {
    let output = abp().arg("--help").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. `abp run --help` shows all run flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_run_help_shows_all_flags() {
    let output = abp().args(["run", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in [
        "--task",
        "--backend",
        "--model",
        "--root",
        "--workspace-mode",
        "--lane",
        "--include",
        "--exclude",
        "--param",
        "--env",
        "--max-budget-usd",
        "--max-turns",
        "--out",
        "--output",
        "--events",
        "--policy",
        "--json",
    ] {
        assert!(stdout.contains(flag), "run --help should mention '{flag}'");
    }
}

#[test]
fn dc_run_help_shows_workspace_mode_values() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pass-through"))
        .stdout(predicate::str::contains("staged"));
}

#[test]
fn dc_run_help_shows_lane_values() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("patch-first"))
        .stdout(predicate::str::contains("workspace-first"));
}

#[test]
fn dc_run_help_exit_code_zero() {
    let output = abp().args(["run", "--help"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. `abp backends` lists available backends
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_backends_lists_mock() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn dc_backends_lists_all_sidecar_variants() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in [
        "sidecar:node",
        "sidecar:python",
        "sidecar:claude",
        "sidecar:copilot",
        "sidecar:kimi",
        "sidecar:gemini",
        "sidecar:codex",
    ] {
        assert!(stdout.contains(name), "backends should list '{name}'");
    }
}

#[test]
fn dc_backends_lists_all_aliases() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for alias in [
        "node", "python", "claude", "copilot", "kimi", "gemini", "codex",
    ] {
        assert!(
            stdout.lines().any(|l| l.trim() == alias),
            "backends should list alias '{alias}' as a standalone line"
        );
    }
}

#[test]
fn dc_backends_exit_code_zero() {
    let output = abp().arg("backends").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn dc_backends_no_stderr_output() {
    let output = abp().arg("backends").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // There might be tracing output, but no "error" lines.
    assert!(
        !stderr.contains("error"),
        "backends should not produce errors on stderr"
    );
}

#[test]
fn dc_backends_mock_appears_first_line() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().expect("at least one line");
    assert_eq!(
        first.trim(),
        "mock",
        "mock should be the first listed backend"
    );
}

#[test]
fn dc_backends_at_least_14_entries() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(count >= 14, "expected ≥14 backend entries, got {count}");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Backend registration — mock is always available
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_mock_always_registered_via_backends() {
    let output = abp().arg("backends").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|l| l.trim() == "mock"),
        "mock should always be registered"
    );
}

#[test]
fn dc_mock_always_registered_via_run() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "registration check", &[]);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. `abp run` with `--backend mock` executes successfully
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_run_mock_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "basic run", &[]);
}

#[test]
fn dc_run_mock_creates_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "receipt created", &[]);
    assert!(receipt_path.exists());
}

#[test]
fn dc_run_mock_receipt_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "valid json", &[]);
    let v = read_receipt(&receipt_path);
    assert!(v.is_object());
}

#[test]
fn dc_run_mock_receipt_has_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "has outcome", &[]);
    let v = read_receipt(&receipt_path);
    assert!(v.get("outcome").is_some(), "receipt must have outcome");
}

#[test]
fn dc_run_mock_receipt_has_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "has sha256", &[]);
    let v = read_receipt(&receipt_path);
    assert!(v.get("receipt_sha256").is_some());
}

#[test]
fn dc_run_mock_receipt_has_meta() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "has meta", &[]);
    let v = read_receipt(&receipt_path);
    assert!(v.get("meta").is_some());
    assert!(v["meta"].get("run_id").is_some());
    assert!(v["meta"].get("contract_version").is_some());
}

#[test]
fn dc_run_mock_receipt_backend_id_is_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "backend id", &[]);
    let v = read_receipt(&receipt_path);
    assert_eq!(v["backend"]["id"].as_str(), Some("mock"));
}

#[test]
fn dc_run_mock_receipt_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "contract version", &[]);
    let v = read_receipt(&receipt_path);
    assert_eq!(
        v["meta"]["contract_version"].as_str(),
        Some("abp/v0.1"),
        "contract version must be abp/v0.1"
    );
}

#[test]
fn dc_run_mock_stderr_shows_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "stderr check",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("run_id:"));
}

#[test]
fn dc_run_mock_stderr_shows_backend_name() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "backend name stderr",
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
fn dc_run_mock_stderr_shows_receipt_path() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "receipt path",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("receipt:"));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. `--task` parameter
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_run_task_empty_string() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "", &[]);
}

#[test]
fn dc_run_task_long_string() {
    let tmp = tempfile::tempdir().unwrap();
    let task = "z".repeat(2000);
    run_mock_pt(&tmp, &task, &[]);
}

#[test]
fn dc_run_task_special_chars() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "!@#$%^&*()_+-=[]{}|;':\",./<>?", &[]);
}

#[test]
fn dc_run_task_unicode_emoji() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "🚀🎉🔥 rocket party fire", &[]);
}

#[test]
fn dc_run_task_unicode_cjk() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "你好世界 テスト 테스트", &[]);
}

#[test]
fn dc_run_task_multiline() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "line one\nline two\nline three", &[]);
}

#[test]
fn dc_run_task_whitespace_only() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "   ", &[]);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. `--debug` flag enables debug logging
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_debug_flag_run_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "debug",
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
fn dc_debug_flag_produces_extra_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "debug trace test",
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
        stderr.len() > 20,
        "debug flag should produce substantial stderr output, got {len} bytes",
        len = stderr.len()
    );
}

#[test]
fn dc_debug_flag_backends_succeeds() {
    abp()
        .args(["--debug", "backends"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn dc_debug_flag_combined_with_json() {
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
// 9. Unknown backend returns error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_unknown_backend_fails() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--task",
            "x",
            "--backend",
            "totally-made-up-backend",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

#[test]
fn dc_unknown_backend_exit_code_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = abp()
        .args([
            "run",
            "--task",
            "x",
            "--backend",
            "does-not-exist-42",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn dc_unknown_backend_error_on_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--task",
            "x",
            "--backend",
            "phantom-backend",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Missing required arguments produce help text
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_no_subcommand_fails() {
    abp().assert().failure();
}

#[test]
fn dc_no_subcommand_exit_code_2() {
    let output = abp().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_run_missing_task_fails() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
}

#[test]
fn dc_run_missing_task_exit_code_2() {
    let output = abp().args(["run", "--backend", "mock"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_unknown_subcommand_fails() {
    abp().arg("nonexistent-subcommand").assert().failure();
}

#[test]
fn dc_unknown_subcommand_shows_message() {
    abp()
        .arg("not-a-command")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn dc_unknown_subcommand_exit_code_2() {
    let output = abp().arg("bogus").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_unknown_global_flag_fails() {
    let output = abp().arg("--nonexistent-flag").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Exit codes for success/failure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_exit_code_0_on_successful_run() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "exit 0",
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
fn dc_exit_code_0_on_backends() {
    let output = abp().arg("backends").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn dc_exit_code_1_on_runtime_error() {
    let output = abp()
        .args(["inspect", "/nonexistent/file.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn dc_exit_code_2_on_missing_subcommand() {
    let output = abp().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_exit_code_2_on_bad_flag() {
    let output = abp().arg("--no-such-flag-at-all").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_exit_code_2_invalid_workspace_mode() {
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "t",
            "--workspace-mode",
            "invalid-mode",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_exit_code_2_invalid_lane() {
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "t",
            "--lane",
            "invalid-lane",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn dc_exit_code_2_schema_invalid_kind() {
    let output = abp().args(["schema", "invalid-kind"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Error output formatting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_runtime_error_prefixed_with_error() {
    abp()
        .args(["inspect", "/no/such/file.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error:"));
}

#[test]
fn dc_param_without_equals_reports_key_value() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "bad",
            "--param",
            "no-equals-sign",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("KEY=VALUE"));
}

#[test]
fn dc_env_without_equals_reports_key_value() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "bad",
            "--env",
            "no-equals-sign",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("KEY=VALUE"));
}

#[test]
fn dc_invalid_policy_reports_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let bad = tmp.path().join("bad.json");
    std::fs::write(&bad, "{not valid json!}").unwrap();
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
            bad.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error:"));
}

#[test]
fn dc_missing_policy_file_reports_error() {
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
        .failure()
        .stderr(predicate::str::contains("error:"));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Run with various WorkOrder configurations via CLI flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_run_without_backend_defaults_to_mock() {
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
fn dc_run_model_flag() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "model flag", &["--model", "gpt-4o"]);
}

#[test]
fn dc_run_single_param() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "single param", &["--param", "temperature=0.7"]);
}

#[test]
fn dc_run_multiple_params() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(
        &tmp,
        "multi params",
        &[
            "--param",
            "temperature=0.5",
            "--param",
            "top_p=0.9",
            "--param",
            "stream=true",
        ],
    );
}

#[test]
fn dc_run_dotted_param() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "dotted param", &["--param", "abp.mode=passthrough"]);
}

#[test]
fn dc_run_single_env() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "env flag", &["--env", "API_KEY=secret"]);
}

#[test]
fn dc_run_multiple_env() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(
        &tmp,
        "multi env",
        &["--env", "KEY1=val1", "--env", "KEY2=val2"],
    );
}

#[test]
fn dc_run_include_glob() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "include glob", &["--include", "**/*.rs"]);
}

#[test]
fn dc_run_exclude_glob() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "exclude glob", &["--exclude", "target/**"]);
}

#[test]
fn dc_run_include_and_exclude_globs() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(
        &tmp,
        "include+exclude",
        &[
            "--include",
            "src/**",
            "--include",
            "tests/**",
            "--exclude",
            "*.lock",
        ],
    );
}

#[test]
fn dc_run_lane_patch_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "patch first", &["--lane", "patch-first"]);
}

#[test]
fn dc_run_lane_workspace_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "workspace first", &["--lane", "workspace-first"]);
}

#[test]
fn dc_run_max_budget() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "budget", &["--max-budget-usd", "5.50"]);
}

#[test]
fn dc_run_max_turns() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(&tmp, "turns", &["--max-turns", "10"]);
}

#[test]
fn dc_run_max_budget_and_turns_together() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock_pt(
        &tmp,
        "budget+turns",
        &["--max-budget-usd", "1.0", "--max-turns", "3"],
    );
}

#[test]
fn dc_run_output_flag_writes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("output-receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "output flag",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(out.exists());
    let v = read_receipt(&out);
    assert!(v.is_object());
}

#[test]
fn dc_run_events_flag_writes_jsonl() {
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
            .unwrap_or_else(|e| panic!("event line must be valid JSON: {e}"));
    }
}

#[test]
fn dc_run_policy_file_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("policy.json");
    let policy = serde_json::json!({
        "allowed_tools": ["Read"],
        "disallowed_tools": [],
        "deny_read": [],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    });
    std::fs::write(&policy_path, serde_json::to_string(&policy).unwrap()).unwrap();
    run_mock_pt(&tmp, "policy", &["--policy", policy_path.to_str().unwrap()]);
}

#[test]
fn dc_run_all_flags_combined() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let events = tmp.path().join("events.jsonl");
    let policy_path = tmp.path().join("policy.json");
    let policy = serde_json::json!({
        "allowed_tools": [],
        "disallowed_tools": [],
        "deny_read": [],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    });
    std::fs::write(&policy_path, serde_json::to_string(&policy).unwrap()).unwrap();

    abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "all flags",
            "--model",
            "gpt-4",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--lane",
            "workspace-first",
            "--include",
            "**/*.rs",
            "--exclude",
            "target/**",
            "--param",
            "key1=val1",
            "--param",
            "key2=true",
            "--env",
            "API_KEY=test",
            "--max-budget-usd",
            "10.0",
            "--max-turns",
            "5",
            "--out",
            receipt.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--policy",
            policy_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt.exists());
    assert!(events.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// 14. JSON output mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_json_flag_stdout_is_json_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json mode",
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
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("bad JSON: {e}\n{line}"));
        assert!(v.is_object());
    }
}

#[test]
fn dc_json_flag_suppresses_run_id_header() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "suppress",
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
}

#[test]
fn dc_json_flag_suppresses_backend_header() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "suppress be",
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

// ═══════════════════════════════════════════════════════════════════════
// 15. Schema subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_schema_work_order_is_valid_json() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v.is_object());
}

#[test]
fn dc_schema_receipt_is_valid_json() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
}

#[test]
fn dc_schema_config_is_valid_json() {
    let output = abp().args(["schema", "config"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
}

#[test]
fn dc_schema_work_order_has_task_field() {
    let output = abp().args(["schema", "work-order"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("task"),
        "work-order schema should mention task field"
    );
}

#[test]
fn dc_schema_receipt_has_outcome_field() {
    let output = abp().args(["schema", "receipt"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("outcome"),
        "receipt schema should mention outcome field"
    );
}

#[test]
fn dc_schema_invalid_kind_exit_code_2() {
    let output = abp().args(["schema", "bogus"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Validate subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_validate_work_order() {
    let wo = abp_core::WorkOrderBuilder::new("validate test").build();
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
fn dc_validate_receipt() {
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
fn dc_validate_bad_json_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn dc_validate_nonexistent_file_fails() {
    abp()
        .args(["validate", "/nonexistent/path.json"])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Inspect subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_inspect_valid_receipt_shows_valid() {
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
        .stdout(predicate::str::contains("backend: mock"));
}

#[test]
fn dc_inspect_tampered_receipt_shows_invalid() {
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
fn dc_inspect_nonexistent_file_fails() {
    let output = abp()
        .args(["inspect", "/no/such/file.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Config subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_config_check_default_ok() {
    let tmp = tempfile::tempdir().unwrap();
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn dc_config_check_valid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"
[backends.mock]
type = "mock"
"#,
    )
    .unwrap();
    abp()
        .args(["config", "check", "--config", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn dc_config_check_bad_toml_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.toml");
    std::fs::write(&path, "[broken toml =").unwrap();
    abp()
        .args(["config", "check", "--config", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

#[test]
fn dc_global_config_flag_applies() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("custom.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"
[backends.local]
type = "mock"
"#,
    )
    .unwrap();
    abp()
        .args(["--config", path.to_str().unwrap(), "config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn dc_config_file_registers_custom_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("backplane.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        r#"[backends.my-custom-mock]
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
            "my-custom-mock",
            "--task",
            "custom",
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
// 19. Receipt subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_receipt_verify_valid() {
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
fn dc_receipt_verify_tampered_fails() {
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
fn dc_receipt_diff_identical() {
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
fn dc_receipt_diff_different() {
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
// 20. Sequential runs produce unique IDs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_sequential_runs_different_run_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let r1_path = tmp.path().join("r1.json");
    let r2_path = tmp.path().join("r2.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "seq-1",
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
            "seq-2",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let r1 = read_receipt(&r1_path);
    let r2 = read_receipt(&r2_path);
    assert_ne!(
        r1["meta"]["run_id"], r2["meta"]["run_id"],
        "sequential runs must have different run_ids"
    );
}

#[test]
fn dc_sequential_runs_different_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let r1_path = tmp.path().join("r1.json");
    let r2_path = tmp.path().join("r2.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "sha-1",
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
            "sha-2",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let r1 = read_receipt(&r1_path);
    let r2 = read_receipt(&r2_path);
    assert_ne!(
        r1["receipt_sha256"], r2["receipt_sha256"],
        "sequential runs should produce different sha256 hashes"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 21. E2E pipeline: run → verify/inspect/validate
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_e2e_run_then_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "e2e verify", &[]);
    abp()
        .args(["receipt", "verify", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn dc_e2e_run_then_inspect() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "e2e inspect", &[]);
    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"))
        .stdout(predicate::str::contains("backend: mock"));
}

#[test]
fn dc_e2e_run_then_validate_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock_pt(&tmp, "e2e validate", &[]);
    abp()
        .args(["validate", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid receipt"));
}

#[test]
fn dc_e2e_run_twice_then_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let r1 = tmp.path().join("r1.json");
    let r2 = tmp.path().join("r2.json");

    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "diff-1",
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
            "diff-2",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            r2.to_str().unwrap(),
        ])
        .assert()
        .success();

    // They should differ (different run_ids at minimum)
    abp()
        .args([
            "receipt",
            "diff",
            r1.to_str().unwrap(),
            r2.to_str().unwrap(),
        ])
        .assert()
        .success();
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Environment variable does not crash
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dc_rust_log_env_var_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .env("RUST_LOG", "debug")
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "env override",
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
fn dc_custom_env_var_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .env("ABP_CUSTOM_VAR", "test_value")
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "custom env var",
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
