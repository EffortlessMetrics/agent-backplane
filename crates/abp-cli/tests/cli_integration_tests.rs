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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! CLI integration tests for the `abp` binary.
//!
//! Covers help output, backend listing, run subcommand with mock backend,
//! configuration handling, and error handling for invalid inputs.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

// ── 1. Help output ──────────────────────────────────────────────────

#[test]
fn help_shows_available_commands() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"))
        .stdout(predicate::str::contains("backends"))
        .stdout(predicate::str::contains("run"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("schema"))
        .stdout(predicate::str::contains("inspect"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("receipt"));
}

#[test]
fn run_help_shows_run_options() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--task"))
        .stdout(predicate::str::contains("--backend"))
        .stdout(predicate::str::contains("--model"))
        .stdout(predicate::str::contains("--root"))
        .stdout(predicate::str::contains("--workspace-mode"))
        .stdout(predicate::str::contains("--lane"))
        .stdout(predicate::str::contains("--include"))
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--param"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--policy"))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--events"))
        .stdout(predicate::str::contains("--max-budget-usd"))
        .stdout(predicate::str::contains("--max-turns"));
}

#[test]
fn backends_help_shows_backend_description() {
    abp()
        .args(["backends", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend"));
}

// ── 2. Backend listing ──────────────────────────────────────────────

#[test]
fn backends_lists_mock_backend() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn backends_output_includes_sidecar_names() {
    let assert = abp().arg("backends").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for name in [
        "sidecar:node",
        "sidecar:python",
        "sidecar:claude",
        "sidecar:copilot",
        "sidecar:kimi",
        "sidecar:gemini",
        "sidecar:codex",
    ] {
        assert!(
            stdout.contains(name),
            "backends output should contain '{name}'"
        );
    }
}

#[test]
fn backends_output_includes_short_aliases() {
    let assert = abp().arg("backends").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for alias in [
        "node", "python", "claude", "copilot", "kimi", "gemini", "codex",
    ] {
        assert!(
            stdout.lines().any(|l| l.trim() == alias),
            "backends output should include alias '{alias}' as its own line"
        );
    }
}

#[test]
fn backends_output_has_one_entry_per_line() {
    let assert = abp().arg("backends").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    // mock + 7 sidecar: variants + 7 short aliases = 15 minimum
    assert!(
        lines.len() >= 15,
        "expected at least 15 backend lines, got {}",
        lines.len()
    );
}

// ── 3. Run subcommand with mock backend ─────────────────────────────

#[test]
fn run_mock_succeeds_and_writes_receipt() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
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
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt_path.exists(), "receipt file should be written");
}

#[test]
fn run_mock_receipt_contains_sha256() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
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
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&receipt_path).expect("read receipt");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse receipt");
    assert!(
        json.get("receipt_sha256").is_some(),
        "receipt should contain receipt_sha256"
    );
}

#[test]
fn run_mock_receipt_has_expected_structure() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "structure test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&receipt_path).expect("read receipt");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse receipt");
    assert!(json.get("outcome").is_some(), "receipt needs outcome");
    assert!(json.get("backend").is_some(), "receipt needs backend");
    assert!(json.get("meta").is_some(), "receipt needs meta");
    assert!(json.get("trace").is_some(), "receipt needs trace");
    assert!(json["meta"].get("run_id").is_some(), "meta needs run_id");
    assert!(
        json["meta"].get("contract_version").is_some(),
        "meta needs contract_version"
    );
}

#[test]
fn run_mock_stderr_shows_run_metadata() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "metadata test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("run_id:"))
        .stderr(predicate::str::contains("backend: mock"))
        .stderr(predicate::str::contains("receipt:"));
}

#[test]
fn debug_flag_enables_verbose_logging() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
    // --debug should be accepted and produce more output (at minimum, no error)
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
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn run_missing_task_fails_with_error() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
}

#[test]
fn run_unknown_backend_fails_with_error() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    abp()
        .args([
            "run",
            "--task",
            "test",
            "--backend",
            "does_not_exist_backend",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

// ── 4. Configuration handling ───────────────────────────────────────

#[test]
fn default_config_works_without_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    // Run from a directory with no backplane.toml; defaults should work.
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn config_file_parsed_and_backend_registered() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("backplane.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(
        f,
        r#"default_backend = "custom-mock"

[backends.custom-mock]
type = "mock"
"#
    )
    .unwrap();

    let receipt_path = tmp.path().join("receipt.json");
    abp()
        .current_dir(tmp.path())
        .args([
            "run",
            "--backend",
            "custom-mock",
            "--task",
            "config backend test",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt_path.exists());
}

#[test]
fn invalid_config_file_rejected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("bad.toml");
    std::fs::write(&config_path, "not [valid toml =").unwrap();

    abp()
        .args(["config", "check", "--config", config_path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

#[test]
fn config_check_valid_toml_succeeds() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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
fn config_flag_overrides_cwd_lookup() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("custom-config.toml");
    std::fs::write(
        &config_path,
        r#"
[backends.from-flag]
type = "mock"
"#,
    )
    .unwrap();

    let receipt_path = tmp.path().join("receipt.json");
    abp()
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "run",
            "--backend",
            "from-flag",
            "--task",
            "flag config test",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();
}

// ── 5. Error handling — invalid inputs ──────────────────────────────

#[test]
fn empty_task_string_is_accepted_by_mock() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
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
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn nonexistent_backend_fails() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    abp()
        .args([
            "run",
            "--task",
            "test",
            "--backend",
            "no-such-backend-xyz",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

#[test]
fn invalid_argument_rejected() {
    abp()
        .args(["run", "--nonexistent-flag", "value"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unexpected argument").or(predicate::str::contains("error")),
        );
}

#[test]
fn unknown_subcommand_fails() {
    abp()
        .arg("nonexistent_subcommand")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn no_subcommand_shows_usage() {
    abp()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage").or(predicate::str::contains("subcommand")));
}

#[test]
fn invalid_workspace_mode_rejected() {
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--workspace-mode",
            "bogus_mode",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn invalid_param_format_rejected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "param test",
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
fn invalid_env_format_rejected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "env test",
            "--env",
            "missing-equals",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .failure();
}

// ── 6. Additional coverage ──────────────────────────────────────────

#[test]
fn json_flag_emits_valid_jsonl_on_stdout() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json output test",
            "--json",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("each stdout line should be valid JSON: {e}\n  line: {line}")
        });
        assert!(parsed.is_object());
    }
}

#[test]
fn json_flag_suppresses_pretty_headers_on_stderr() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
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
            receipt_path.to_str().unwrap(),
        ])
        .output()
        .expect("execute abp");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("run_id:"),
        "--json should suppress run_id header"
    );
}

#[test]
fn run_default_backend_is_mock_when_no_flag() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
    abp()
        .current_dir(tmp.path())
        .args([
            "run",
            "--task",
            "default backend",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("backend: mock"));
}

#[test]
fn version_flag_shows_version() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("abp"));
}

#[test]
fn runtime_error_exits_code_1() {
    let output = abp()
        .args(["inspect", "/nonexistent/path/receipt.json"])
        .output()
        .expect("execute abp");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn usage_error_exits_code_2() {
    let output = abp().output().expect("execute abp");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}
