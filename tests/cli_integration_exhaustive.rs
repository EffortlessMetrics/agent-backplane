#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive CLI integration tests for the `abp` binary.
//!
//! Tests exercise the binary end-to-end using `assert_cmd` and `predicates`,
//! covering argument parsing, subcommands, flags, output formats, exit codes,
//! config loading, and mock backend receipt validation.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use std::path::PathBuf;

// ─── helpers ────────────────────────────────────────────────────────────

#[allow(deprecated)]
fn abp() -> Command {
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

/// Run mock backend in pass-through mode and return the receipt path.
fn run_mock(tmp: &tempfile::TempDir, task: &str, extra: &[&str]) -> PathBuf {
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

/// Create a minimal TOML config file and return its path.
fn write_config(dir: &tempfile::TempDir, content: &str) -> PathBuf {
    let path = dir.path().join("backplane.toml");
    std::fs::write(&path, content).unwrap();
    path
}

// ═══════════════════════════════════════════════════════════════════════
// 1. --version flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn version_flag_succeeds() {
    abp().arg("--version").assert().success();
}

#[test]
fn version_flag_contains_crate_version() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_flag_contains_binary_name() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("abp"));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. --help flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn help_flag_succeeds() {
    abp().arg("--help").assert().success();
}

#[test]
fn help_flag_shows_subcommands() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("run"))
        .stdout(predicate::str::contains("backends"));
}

#[test]
fn help_flag_shows_global_options() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--debug"))
        .stdout(predicate::str::contains("--config"));
}

#[test]
fn help_short_flag_succeeds() {
    abp().arg("-h").assert().success();
}

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
fn run_help_shows_json_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn run_help_shows_include_exclude_flags() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--include"))
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
fn run_help_shows_budget_and_turns_flags() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--max-budget-usd"))
        .stdout(predicate::str::contains("--max-turns"));
}

#[test]
fn run_help_shows_out_flag() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--out"));
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
fn backends_help_succeeds() {
    abp().args(["backends", "--help"]).assert().success();
}

#[test]
fn validate_help_succeeds() {
    abp().args(["validate", "--help"]).assert().success();
}

#[test]
fn schema_help_succeeds() {
    abp().args(["schema", "--help"]).assert().success();
}

#[test]
fn inspect_help_succeeds() {
    abp().args(["inspect", "--help"]).assert().success();
}

#[test]
fn config_help_succeeds() {
    abp().args(["config", "--help"]).assert().success();
}

#[test]
fn receipt_help_succeeds() {
    abp().args(["receipt", "--help"]).assert().success();
}

// ═══════════════════════════════════════════════════════════════════════
// 3. backends subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backends_lists_mock() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn backends_lists_sidecar_node() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:node"));
}

#[test]
fn backends_lists_sidecar_python() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:python"));
}

#[test]
fn backends_lists_sidecar_claude() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:claude"));
}

#[test]
fn backends_lists_sidecar_copilot() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:copilot"));
}

#[test]
fn backends_lists_sidecar_kimi() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:kimi"));
}

#[test]
fn backends_lists_sidecar_gemini() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:gemini"));
}

#[test]
fn backends_lists_sidecar_codex() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("sidecar:codex"));
}

#[test]
fn backends_lists_shorthand_aliases() {
    let assert = abp().arg("backends").assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("node"));
    assert!(stdout.contains("python"));
    assert!(stdout.contains("claude"));
    assert!(stdout.contains("copilot"));
    assert!(stdout.contains("kimi"));
    assert!(stdout.contains("gemini"));
    assert!(stdout.contains("codex"));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. run subcommand – missing required arguments
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_without_task_fails() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
}

#[test]
fn run_without_any_args_fails() {
    abp()
        .arg("run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. run with mock backend – basic success
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_mock_backend_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
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
}

#[test]
fn run_mock_backend_exit_code_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .code(0);
}

#[test]
fn run_mock_receipt_is_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "json test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let _: serde_json::Value =
        serde_json::from_str(&content).expect("receipt should be valid JSON");
}

#[test]
fn run_mock_receipt_has_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "outcome test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(val.get("outcome").is_some(), "receipt should have outcome");
}

#[test]
fn run_mock_receipt_has_backend_id() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "backend test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    let backend = val.get("backend").and_then(|b| b.get("id"));
    assert!(backend.is_some(), "receipt should have backend.id");
}

#[test]
fn run_mock_receipt_has_meta_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "meta test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    let run_id = val.get("meta").and_then(|m| m.get("run_id"));
    assert!(run_id.is_some(), "receipt should have meta.run_id");
}

#[test]
fn run_mock_receipt_has_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "hash test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    let sha = val.get("receipt_sha256");
    assert!(sha.is_some(), "receipt should have receipt_sha256");
    assert!(
        sha.unwrap().is_string(),
        "receipt_sha256 should be a string"
    );
}

#[test]
fn run_mock_receipt_has_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "version test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    let cv = val
        .get("meta")
        .and_then(|m| m.get("contract_version"))
        .and_then(|v| v.as_str());
    assert_eq!(cv, Some("abp/v0.1"));
}

#[test]
fn run_mock_receipt_has_trace_array() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "trace test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(val.get("trace").unwrap().is_array());
}

#[test]
fn run_mock_receipt_has_artifacts_array() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "artifacts test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(val.get("artifacts").unwrap().is_array());
}

#[test]
fn run_mock_receipt_has_mode_field() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "mode test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(val.get("mode").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. --task flag variations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_task_with_spaces() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "hello world with spaces", &[]);
}

#[test]
fn run_task_with_special_chars() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "fix bug #123 & add feature", &[]);
}

#[test]
fn run_task_with_unicode() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "修复错误 и добавить功能", &[]);
}

#[test]
fn run_task_single_word() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "test", &[]);
}

#[test]
fn run_task_very_long_string() {
    let tmp = tempfile::tempdir().unwrap();
    let long_task = "a".repeat(1000);
    run_mock(&tmp, &long_task, &[]);
}

#[test]
fn run_task_with_newlines_in_quotes() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "line1\nline2", &[]);
}

#[test]
fn run_task_with_json_like_string() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, r#"{"action": "fix"}"#, &[]);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. --backend flag variations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_backend_mock_explicit() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "explicit mock", &[]);
    assert!(path.exists());
}

#[test]
fn run_backend_invalid_name_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "nonexistent_backend_xyz",
            "--task",
            "test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

#[test]
fn run_backend_defaults_to_mock_without_config() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    // Without --backend and without a backplane.toml, defaults to "mock"
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
        .current_dir(tmp.path())
        .assert()
        .success();
    assert!(receipt.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. --debug flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_flag_accepted_before_subcommand() {
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
}

#[test]
fn debug_flag_with_backends_subcommand() {
    abp().args(["--debug", "backends"]).assert().success();
}

// ═══════════════════════════════════════════════════════════════════════
// 9. --json flag (output format)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_json_flag_outputs_json_events() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let assert = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json output",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    // JSON mode prints events as JSON lines to stdout
    for line in stdout.lines() {
        if !line.trim().is_empty() {
            let _: serde_json::Value =
                serde_json::from_str(line).expect("each line should be valid JSON");
        }
    }
}

#[test]
fn run_without_json_flag_prints_stderr_info() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let assert = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "text output",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("run_id:") || stderr.contains("backend:") || stderr.contains("receipt:"),
        "non-json mode should show human-readable info on stderr"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 10. --out / --output flags for receipt destination
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_out_flag_writes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("custom_receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "out test",
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
fn run_output_flag_writes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("output_receipt.json");
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
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt.exists());
}

#[test]
fn run_output_flag_takes_precedence_over_out() {
    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("out_receipt.json");
    let output_path = tmp.path().join("output_receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "precedence test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            out_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(
        output_path.exists(),
        "--output should be the actual receipt destination"
    );
}

#[test]
fn run_creates_receipt_parent_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("nested").join("dir").join("receipt.json");
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
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. --workspace-mode flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_workspace_mode_pass_through() {
    let tmp = tempfile::tempdir().unwrap();
    // run_mock already sets --workspace-mode pass-through
    run_mock(&tmp, "pass-through mode", &[]);
}

#[test]
fn run_workspace_mode_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "staged mode",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "staged",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_workspace_mode_invalid_fails() {
    let _tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--workspace-mode",
            "invalid_mode",
        ])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 12. --lane flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_lane_patch_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "lane test", &["--lane", "patch-first"]);
}

#[test]
fn run_lane_workspace_first() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "lane test", &["--lane", "workspace-first"]);
}

#[test]
fn run_lane_invalid_fails() {
    let _tmp = tempfile::tempdir().unwrap();
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--lane",
            "invalid_lane",
        ])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 13. --include / --exclude flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_include_single_glob() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "include test", &["--include", "*.rs"]);
}

#[test]
fn run_include_multiple_globs() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "multi include test",
        &["--include", "*.rs", "--include", "*.toml"],
    );
}

#[test]
fn run_exclude_single_glob() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "exclude test", &["--exclude", "target/**"]);
}

#[test]
fn run_exclude_multiple_globs() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "multi exclude test",
        &["--exclude", "target/**", "--exclude", "node_modules/**"],
    );
}

#[test]
fn run_include_and_exclude_combined() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "combined test",
        &["--include", "src/**", "--exclude", "**/*.bak"],
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 14. --param flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_param_simple_string() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "param test", &["--param", "key=value"]);
}

#[test]
fn run_param_multiple() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "multi param test",
        &["--param", "a=1", "--param", "b=two"],
    );
}

#[test]
fn run_param_json_value() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "json param test", &["--param", "stream=true"]);
}

#[test]
fn run_param_model_override() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "model param test", &["--param", "model=gpt-4o"]);
}

#[test]
fn run_param_dotted_key() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "dotted param test",
        &["--param", "abp.mode=passthrough"],
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 15. --env flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_env_single() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "env test", &["--env", "API_KEY=test123"]);
}

#[test]
fn run_env_multiple() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "multi env test", &["--env", "A=1", "--env", "B=2"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 16. --max-budget-usd / --max-turns flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_max_budget_usd() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "budget test", &["--max-budget-usd", "5.0"]);
}

#[test]
fn run_max_turns() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "turns test", &["--max-turns", "10"]);
}

#[test]
fn run_both_budget_and_turns() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(
        &tmp,
        "budget+turns test",
        &["--max-budget-usd", "1.5", "--max-turns", "5"],
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 17. --model flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_model_flag() {
    let tmp = tempfile::tempdir().unwrap();
    run_mock(&tmp, "model test", &["--model", "gpt-4o-mini"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. --events flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_events_flag_writes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let events_path = tmp.path().join("events.jsonl");
    run_mock(
        &tmp,
        "events test",
        &["--events", events_path.to_str().unwrap()],
    );
    assert!(events_path.exists(), "events file should be created");
}

#[test]
fn run_events_file_contains_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let events_path = tmp.path().join("events.jsonl");
    run_mock(
        &tmp,
        "events jsonl test",
        &["--events", events_path.to_str().unwrap()],
    );
    let content = std::fs::read_to_string(&events_path).unwrap();
    for line in content.lines() {
        if !line.trim().is_empty() {
            let _: serde_json::Value =
                serde_json::from_str(line).expect("each event line should be valid JSON");
        }
    }
}

#[test]
fn run_events_creates_parent_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let events_path = tmp.path().join("sub").join("dir").join("events.jsonl");
    run_mock(
        &tmp,
        "events nested test",
        &["--events", events_path.to_str().unwrap()],
    );
    assert!(events_path.exists());
}

// ═══════════════════════════════════════════════════════════════════════
// 19. --config flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_with_config_file() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(
        &tmp,
        r#"
default_backend = "mock"
log_level = "debug"
"#,
    );
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--config",
            config.to_str().unwrap(),
            "run",
            "--task",
            "config test",
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
fn run_with_config_default_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(
        &tmp,
        r#"
default_backend = "mock"
"#,
    );
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--config",
            config.to_str().unwrap(),
            "run",
            "--task",
            "default backend from config",
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
fn run_config_flag_overrides_cwd_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(
        &tmp,
        r#"
default_backend = "mock"
log_level = "warn"
"#,
    );
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--config",
            config.to_str().unwrap(),
            "run",
            "--task",
            "override test",
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
fn run_config_nonexistent_file_still_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    // The CLI logs a warning but doesn't fail when config can't be loaded
    abp()
        .args([
            "--config",
            "/nonexistent/config.toml",
            "run",
            "--backend",
            "mock",
            "--task",
            "missing config test",
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
fn config_with_mock_backend_definition() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(
        &tmp,
        r#"
default_backend = "mymock"

[backends.mymock]
type = "mock"
"#,
    );
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "--config",
            config.to_str().unwrap(),
            "run",
            "--backend",
            "mymock",
            "--task",
            "custom mock test",
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
// 20. Invalid argument combinations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_subcommand_fails() {
    abp().arg("foobar").assert().failure();
}

#[test]
fn unknown_global_flag_fails() {
    abp().arg("--nonexistent-flag").assert().failure();
}

#[test]
fn run_unknown_flag_fails() {
    abp()
        .args(["run", "--task", "test", "--unknown-flag", "val"])
        .assert()
        .failure();
}

#[test]
fn validate_without_file_fails() {
    abp().arg("validate").assert().failure();
}

#[test]
fn schema_without_kind_fails() {
    abp().arg("schema").assert().failure();
}

#[test]
fn inspect_without_file_fails() {
    abp().arg("inspect").assert().failure();
}

#[test]
fn receipt_without_action_fails() {
    abp().arg("receipt").assert().failure();
}

#[test]
fn receipt_verify_without_file_fails() {
    abp().args(["receipt", "verify"]).assert().failure();
}

#[test]
fn receipt_diff_without_files_fails() {
    abp().args(["receipt", "diff"]).assert().failure();
}

#[test]
fn receipt_diff_with_one_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("r.json");
    std::fs::write(&f, "{}").unwrap();
    abp()
        .args(["receipt", "diff", f.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn config_without_action_fails() {
    abp().arg("config").assert().failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Exit codes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn exit_code_zero_on_success() {
    abp().arg("backends").assert().code(0);
}

#[test]
fn exit_code_nonzero_on_missing_args() {
    abp().arg("run").assert().code(2);
}

#[test]
fn exit_code_nonzero_on_unknown_subcommand() {
    abp().arg("doesnotexist").assert().failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 22. schema subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn schema_work_order_outputs_valid_json() {
    let assert = abp().args(["schema", "work-order"]).assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).expect("should be valid JSON");
    assert!(
        val.get("properties").is_some() || val.get("$defs").is_some(),
        "should be a JSON schema"
    );
}

#[test]
fn schema_receipt_outputs_valid_json() {
    let assert = abp().args(["schema", "receipt"]).assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("should be valid JSON");
}

#[test]
fn schema_config_outputs_valid_json() {
    let assert = abp().args(["schema", "config"]).assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("should be valid JSON");
}

#[test]
fn schema_invalid_kind_fails() {
    abp().args(["schema", "invalid"]).assert().failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 23. validate subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_valid_work_order() {
    let tmp = tempfile::tempdir().unwrap();
    let _path = tmp.path().join("wo.json");
    // Use the mock backend to produce a valid receipt, then validate it
    let receipt_path = run_mock(&tmp, "validate test", &[]);
    abp()
        .args(["validate", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));
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
fn validate_wrong_shape_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("wrong.json");
    std::fs::write(&path, r#"{"foo": "bar"}"#).unwrap();
    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn validate_nonexistent_file_fails() {
    abp()
        .args(["validate", "/nonexistent/file.json"])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 24. config check subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_check_default_succeeds() {
    abp()
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok").or(predicate::str::is_empty().not()));
}

#[test]
fn config_check_with_valid_file() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(
        &tmp,
        r#"
default_backend = "mock"
log_level = "info"
"#,
    );
    abp()
        .args(["config", "check", "--config", config.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn config_check_with_invalid_toml_shows_error() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(&tmp, "not [valid toml =");
    // The CLI exits 1 when config check finds errors; error text goes to stdout
    abp()
        .args(["config", "check", "--config", config.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Inspect subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn inspect_valid_receipt_shows_valid_hash() {
    let tmp = tempfile::tempdir().unwrap();
    // First, run mock to get a receipt
    let receipt_path = run_mock(&tmp, "inspect test", &[]);
    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn inspect_shows_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "inspect outcome", &[]);
    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("outcome"));
}

#[test]
fn inspect_shows_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "inspect backend", &[]);
    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend"));
}

#[test]
fn inspect_shows_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "inspect run_id", &[]);
    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("run_id"));
}

#[test]
fn inspect_shows_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "inspect sha", &[]);
    abp()
        .args(["inspect", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("sha256"));
}

#[test]
fn inspect_nonexistent_file_fails() {
    abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Receipt verify subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_verify_valid_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "verify test", &[]);
    abp()
        .args(["receipt", "verify", receipt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn receipt_verify_tampered_receipt_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt_path = run_mock(&tmp, "tamper test", &[]);
    // Tamper with the receipt
    let content = std::fs::read_to_string(&receipt_path).unwrap();
    let mut val: serde_json::Value = serde_json::from_str(&content).unwrap();
    val["receipt_sha256"] = serde_json::Value::String("0000000000".to_string());
    std::fs::write(&receipt_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();
    abp()
        .args(["receipt", "verify", receipt_path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID"));
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Receipt diff subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_diff_identical_files() {
    let tmp = tempfile::tempdir().unwrap();
    let r = run_mock(&tmp, "diff identical", &[]);
    let r2 = tmp.path().join("receipt2.json");
    std::fs::copy(&r, &r2).unwrap();
    abp()
        .args(["receipt", "diff", r.to_str().unwrap(), r2.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("no differences"));
}

#[test]
fn receipt_diff_different_files() {
    let tmp = tempfile::tempdir().unwrap();
    let r1 = run_mock(&tmp, "diff first", &[]);
    let _r2_path = tmp.path().join("receipt2.json");
    // Create a second receipt by running again
    let tmp2 = tempfile::tempdir().unwrap();
    let r2 = run_mock(&tmp2, "diff second", &[]);
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
// 28. Multiple flags combined
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_all_optional_flags_combined() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let events = tmp.path().join("events.jsonl");
    abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "all flags test",
            "--model",
            "test-model",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--lane",
            "patch-first",
            "--include",
            "*.rs",
            "--exclude",
            "target/**",
            "--param",
            "key=val",
            "--env",
            "MY_VAR=123",
            "--max-budget-usd",
            "10.0",
            "--max-turns",
            "20",
            "--out",
            receipt.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(receipt.exists());
    assert!(events.exists());
}

#[test]
fn run_json_with_events_combined() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let events = tmp.path().join("events.jsonl");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json+events test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Error message quality
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn missing_task_error_mentions_task() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("task").or(predicate::str::contains("--task")));
}

#[test]
fn invalid_backend_error_contains_backend_name() {
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    let assert = abp()
        .args([
            "run",
            "--backend",
            "totally_fake_backend",
            "--task",
            "test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("totally_fake_backend") || stderr.contains("error"),
        "error should mention the bad backend or 'error'"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 30. Receipt integrity verification via mock
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mock_receipt_sha256_verifies_successfully() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "sha verify test", &[]);
    // Use the inspect subcommand to verify
    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn mock_receipt_outcome_is_complete() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "outcome complete test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        val["outcome"].as_str(),
        Some("complete"),
        "mock backend should produce 'complete' outcome"
    );
}

#[test]
fn mock_receipt_backend_id_is_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "backend id test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(val["backend"]["id"].as_str(), Some("mock"));
}

#[test]
fn mock_receipt_duration_is_non_negative() {
    let tmp = tempfile::tempdir().unwrap();
    let path = run_mock(&tmp, "duration test", &[]);
    let content = std::fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    let dur = val["meta"]["duration_ms"].as_u64();
    assert!(dur.is_some(), "duration should be present and non-negative");
}

#[test]
fn mock_receipts_have_unique_run_ids() {
    let tmp1 = tempfile::tempdir().unwrap();
    let tmp2 = tempfile::tempdir().unwrap();
    let p1 = run_mock(&tmp1, "unique1", &[]);
    let p2 = run_mock(&tmp2, "unique2", &[]);
    let c1 = std::fs::read_to_string(&p1).unwrap();
    let c2 = std::fs::read_to_string(&p2).unwrap();
    let v1: serde_json::Value = serde_json::from_str(&c1).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&c2).unwrap();
    assert_ne!(
        v1["meta"]["run_id"], v2["meta"]["run_id"],
        "two runs should have different run_ids"
    );
}

#[test]
fn mock_receipts_have_unique_sha256() {
    let tmp1 = tempfile::tempdir().unwrap();
    let tmp2 = tempfile::tempdir().unwrap();
    let p1 = run_mock(&tmp1, "sha-unique1", &[]);
    let p2 = run_mock(&tmp2, "sha-unique2", &[]);
    let c1 = std::fs::read_to_string(&p1).unwrap();
    let c2 = std::fs::read_to_string(&p2).unwrap();
    let v1: serde_json::Value = serde_json::from_str(&c1).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&c2).unwrap();
    assert_ne!(
        v1["receipt_sha256"], v2["receipt_sha256"],
        "different runs should produce different hashes"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 31. Additional edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_args_shows_help_or_error() {
    // Running with no args should show usage/help (clap exits with code 2)
    abp().assert().failure();
}

#[test]
fn double_dash_version_with_subcommand_fails() {
    // --version only works without a subcommand
    abp().args(["backends", "--version"]).assert().failure();
}

#[test]
fn schema_work_order_contains_task_property() {
    let assert = abp().args(["schema", "work-order"]).assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("task"),
        "work-order schema should reference 'task'"
    );
}

#[test]
fn schema_receipt_contains_outcome_property() {
    let assert = abp().args(["schema", "receipt"]).assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("outcome"),
        "receipt schema should reference 'outcome'"
    );
}

#[test]
fn run_root_flag_accepts_absolute_path() {
    let tmp = tempfile::tempdir().unwrap();
    let abs = tmp.path().canonicalize().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "abs path test",
            "--root",
            abs.to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_root_flag_defaults_to_dot() {
    // --root defaults to "." so not specifying it should work
    let tmp = tempfile::tempdir().unwrap();
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "default root test",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();
}
