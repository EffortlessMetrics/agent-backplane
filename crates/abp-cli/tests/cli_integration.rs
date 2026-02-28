// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive CLI integration tests for the `abp` binary.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

// ── 1. Help text ────────────────────────────────────────────────────

#[test]
fn help_exits_zero_and_contains_expected_text() {
    abp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"))
        .stdout(predicate::str::contains("Usage"))
        .stdout(predicate::str::contains("backends"))
        .stdout(predicate::str::contains("run"));
}

#[test]
fn help_short_flag_works() {
    abp()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Backplane CLI"));
}

// ── 2. Version ──────────────────────────────────────────────────────

#[test]
fn version_shows_version_string() {
    abp()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_short_flag_works() {
    abp()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("abp"));
}

// ── 3. Backends subcommand ──────────────────────────────────────────

#[test]
fn backends_lists_mock() {
    abp()
        .arg("backends")
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn backends_lists_sidecar_variants() {
    let assert = abp().arg("backends").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for name in ["sidecar:node", "sidecar:python", "sidecar:claude", "sidecar:copilot"] {
        assert!(
            stdout.contains(name),
            "expected backends output to contain '{name}'"
        );
    }
}

#[test]
fn backends_lists_aliases() {
    let assert = abp().arg("backends").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for alias in ["node", "python", "claude", "copilot", "gemini", "codex"] {
        assert!(
            stdout.lines().any(|l| l.trim() == alias),
            "expected backends output to contain alias '{alias}' as its own line"
        );
    }
}

// ── 4. Run with mock ────────────────────────────────────────────────

#[test]
fn run_mock_produces_receipt_with_hash() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "integration test task",
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&receipt).expect("read receipt");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse receipt JSON");
    assert!(
        json.get("receipt_sha256").is_some(),
        "receipt should contain receipt_sha256"
    );
    assert!(
        json.get("meta")
            .and_then(|m| m.get("contract_version"))
            .is_some(),
        "receipt should contain meta.contract_version"
    );
}

#[test]
fn run_mock_prints_run_id_and_backend_to_stderr() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "check stderr",
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
        .stderr(predicate::str::contains("backend: mock"));
}

// ── 5. Missing task ─────────────────────────────────────────────────

#[test]
fn run_missing_task_fails_with_error() {
    abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--task"));
}

// ── 6. Default backend is mock ──────────────────────────────────────

#[test]
fn run_without_backend_flag_defaults_to_mock() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
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
        .success()
        .stderr(predicate::str::contains("backend: mock"));
}

// ── 7. Unknown backend ─────────────────────────────────────────────

#[test]
fn run_unknown_backend_fails_gracefully() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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

// ── 8. Debug flag ───────────────────────────────────────────────────

#[test]
fn debug_flag_on_run_is_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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
fn debug_flag_on_backends_is_accepted() {
    abp()
        .args(["--debug", "backends"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock"));
}

// ── 9. Config file via CWD ─────────────────────────────────────────

#[test]
fn config_file_in_cwd_registers_custom_backend() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("backplane.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(
        f,
        r#"[backends.my-test-mock]
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
            "my-test-mock",
            "--task",
            "config test",
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(receipt.exists(), "receipt should be written");
}

// ── 10. Empty task ──────────────────────────────────────────────────

#[test]
fn run_empty_task_string_is_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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

    assert!(receipt.exists(), "receipt should be written even with empty task");
}

// ── 11. Long task ───────────────────────────────────────────────────

#[test]
fn run_long_task_string_works() {
    let long_task = "a]".repeat(2000);
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            &long_task,
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

// ── 12. JSON output ─────────────────────────────────────────────────

#[test]
fn json_flag_emits_valid_json_lines() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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
        .expect("execute abp");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("each stdout line should be valid JSON: {e}\n  line: {line}"));
        assert!(parsed.is_object(), "each JSON line should be an object");
    }
}

#[test]
fn json_flag_suppresses_pretty_stderr_headers() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "json stderr test",
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    // In --json mode, the pretty "run_id:" / "backend:" headers are suppressed
    assert!(
        !stderr.contains("run_id:"),
        "json mode should suppress run_id header on stderr"
    );
}

// ── 13. Unknown subcommand ──────────────────────────────────────────

#[test]
fn unknown_subcommand_fails_with_helpful_error() {
    abp()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn no_subcommand_shows_usage_hint() {
    abp()
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Usage")
                .or(predicate::str::contains("subcommand")),
        );
}

// ── 14. Double dash ─────────────────────────────────────────────────

#[test]
fn double_dash_before_help_fails() {
    // `abp -- --help` should not be treated as the --help flag;
    // clap does not recognize it as a subcommand.
    abp().args(["--", "--help"]).assert().failure();
}

// ── 15. Exit codes ──────────────────────────────────────────────────

#[test]
fn error_cases_use_nonzero_exit_code() {
    // Missing required subcommand
    let status = abp().assert().failure().get_output().status;
    assert!(!status.success());

    // Unknown subcommand
    let status = abp()
        .arg("bogus")
        .assert()
        .failure()
        .get_output()
        .status;
    assert!(!status.success());

    // Missing required flag
    let status = abp()
        .args(["run", "--backend", "mock"])
        .assert()
        .failure()
        .get_output()
        .status;
    assert!(!status.success());
}

// ── Additional: param and env flags ─────────────────────────────────

#[test]
fn param_flag_is_forwarded() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "param test",
            "--param",
            "model=test-model",
            "--param",
            "stream=true",
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
fn env_flag_is_forwarded() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "env test",
            "--env",
            "MY_KEY=my_value",
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
fn invalid_param_format_fails() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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

// ── Additional: subcommand help ─────────────────────────────────────

#[test]
fn run_help_shows_run_options() {
    abp()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--task"))
        .stdout(predicate::str::contains("--backend"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--param"));
}

#[test]
fn backends_help_shows_description() {
    abp()
        .args(["backends", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend"));
}

// ── Additional: workspace-mode and lane flags ───────────────────────

#[test]
fn workspace_mode_pass_through_is_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "ws mode test",
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
fn lane_workspace_first_is_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "lane test",
            "--lane",
            "workspace-first",
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
fn invalid_workspace_mode_is_rejected() {
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "test",
            "--workspace-mode",
            "invalid",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}
