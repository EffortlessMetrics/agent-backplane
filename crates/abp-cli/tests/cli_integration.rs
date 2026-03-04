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
        .stdout(predicate::str::contains("run"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("schema"))
        .stdout(predicate::str::contains("inspect"));
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
    for name in [
        "sidecar:node",
        "sidecar:python",
        "sidecar:claude",
        "sidecar:copilot",
    ] {
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

    assert!(
        receipt.exists(),
        "receipt should be written even with empty task"
    );
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
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("each stdout line should be valid JSON: {e}\n  line: {line}")
        });
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
        .stderr(predicate::str::contains("Usage").or(predicate::str::contains("subcommand")));
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
    let status = abp().arg("bogus").assert().failure().get_output().status;
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
        .stdout(predicate::str::contains("--param"))
        .stdout(predicate::str::contains("--policy"))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--events"));
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

// ── 16. Combined debug + json flags ─────────────────────────────────

#[test]
fn debug_and_json_flags_together() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    let output = abp()
        .args([
            "--debug",
            "run",
            "--backend",
            "mock",
            "--task",
            "combined flags test",
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
    // With --debug, tracing output may appear on stdout; only validate
    // lines that look like JSON objects (start with '{').
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_lines: Vec<&str> = stdout.lines().filter(|l| l.starts_with('{')).collect();
    for line in &json_lines {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("each JSON line should be valid: {e}\n  line: {line}"));
    }
}

// ── 17. Model flag is accepted──────────────────────────────────────

#[test]
fn model_flag_is_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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

    assert!(receipt.exists());
}

// ── 18. Include/exclude globs are accepted ──────────────────────────

#[test]
fn include_exclude_globs_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "glob test",
            "--include",
            "src/**/*.rs",
            "--exclude",
            "target/**",
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

// ── 19. Receipt JSON contains expected structure ────────────────────

#[test]
fn receipt_contains_expected_fields() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "receipt fields test",
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
    assert!(json.get("outcome").is_some(), "receipt should have outcome");
    assert!(json.get("backend").is_some(), "receipt should have backend");
    assert!(json.get("meta").is_some(), "receipt should have meta");
    assert!(json.get("trace").is_some(), "receipt should have trace");
    assert!(
        json["meta"].get("run_id").is_some(),
        "meta should have run_id"
    );
}

// ── 20. Max budget and max turns flags are accepted ─────────────────

#[test]
fn max_budget_and_turns_flags_accepted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = tmp.path().join("receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "budget test",
            "--max-budget-usd",
            "5.0",
            "--max-turns",
            "10",
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

// ── 21. Invalid env flag format fails ───────────────────────────────

#[test]
fn invalid_env_format_fails() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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

// ── 22. Schema subcommand ───────────────────────────────────────────

#[test]
fn schema_work_order_outputs_valid_json_schema() {
    let output = abp()
        .args(["schema", "work-order"])
        .output()
        .expect("execute abp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("schema output should be valid JSON");
    assert!(parsed.is_object(), "schema should be a JSON object");
}

#[test]
fn schema_receipt_outputs_valid_json_schema() {
    let output = abp()
        .args(["schema", "receipt"])
        .output()
        .expect("execute abp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value =
        serde_json::from_str(&stdout).expect("receipt schema should be valid JSON");
}

#[test]
fn schema_config_outputs_valid_json_schema() {
    let output = abp()
        .args(["schema", "config"])
        .output()
        .expect("execute abp");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value =
        serde_json::from_str(&stdout).expect("config schema should be valid JSON");
}

#[test]
fn schema_invalid_kind_rejected() {
    abp()
        .args(["schema", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

// ── 23. Validate subcommand ─────────────────────────────────────────

#[test]
fn validate_valid_work_order_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let wo_path = tmp.path().join("wo.json");
    std::fs::write(&wo_path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();

    abp()
        .args(["validate", wo_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid work_order"));
}

#[test]
fn validate_valid_receipt_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid receipt"));
}

#[test]
fn validate_invalid_json_file_fails() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let path = tmp.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();

    abp()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn validate_nonexistent_file_fails() {
    abp()
        .args(["validate", "/nonexistent/path/file.json"])
        .assert()
        .failure();
}

// ── 24. Inspect subcommand ──────────────────────────────────────────

#[test]
fn inspect_valid_receipt_shows_hash_valid() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
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
fn inspect_tampered_receipt_shows_hash_invalid() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("0000000000000000".into());
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID"));
}

#[test]
fn inspect_nonexistent_file_fails() {
    abp()
        .args(["inspect", "/nonexistent/receipt.json"])
        .assert()
        .failure();
}

// ── 25. Receipt verify subcommand ───────────────────────────────────

#[test]
fn receipt_verify_valid_hash() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn receipt_verify_tampered_hash_fails() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let mut receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("tampered".into());
    let path = tmp.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

    abp()
        .args(["receipt", "verify", path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INVALID"));
}

// ── 26. Receipt diff subcommand ─────────────────────────────────────

#[test]
fn receipt_diff_identical_shows_no_differences() {
    let tmp = tempfile::tempdir().expect("create temp dir");
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
        .stdout(predicate::str::contains("no differences"));
}

#[test]
fn receipt_diff_different_shows_changes() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = abp_core::ReceiptBuilder::new("other")
        .outcome(abp_core::Outcome::Failed)
        .with_hash()
        .unwrap();
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

// ── 27. Events file output ──────────────────────────────────────────

#[test]
fn events_flag_writes_events_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("receipt.json");
    let events_path = tmp.path().join("events.jsonl");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "events file test",
            "--events",
            events_path.to_str().unwrap(),
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
            "--out",
            receipt_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(events_path.exists(), "events file should be created");
    let content = std::fs::read_to_string(&events_path).expect("read events file");
    for line in content.lines() {
        let _: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("each event line should be valid JSON: {e}\n  line: {line}")
        });
    }
}

// ── 28. Output flag for receipt destination ─────────────────────────

#[test]
fn output_flag_writes_receipt_to_specified_path() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let receipt_path = tmp.path().join("custom_receipt.json");
    abp()
        .args([
            "run",
            "--backend",
            "mock",
            "--task",
            "output flag test",
            "--output",
            receipt_path.to_str().unwrap(),
            "--root",
            tmp.path().to_str().unwrap(),
            "--workspace-mode",
            "pass-through",
        ])
        .assert()
        .success();

    assert!(receipt_path.exists(), "receipt should be at --output path");
    let content = std::fs::read_to_string(&receipt_path).expect("read receipt");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse receipt JSON");
    assert!(json.get("outcome").is_some());
}

// ── 29. Config check subcommand from CLI ────────────────────────────

#[test]
fn config_check_no_file_defaults_ok() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    abp()
        .current_dir(tmp.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

#[test]
fn config_check_with_explicit_config_flag() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("custom.toml");
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

// ── 30. Subcommand help texts ───────────────────────────────────────

#[test]
fn schema_help_shows_kind_options() {
    abp()
        .args(["schema", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work-order"))
        .stdout(predicate::str::contains("receipt"))
        .stdout(predicate::str::contains("config"));
}

#[test]
fn receipt_help_shows_subcommands() {
    abp()
        .args(["receipt", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("verify"))
        .stdout(predicate::str::contains("diff"));
}

#[test]
fn config_help_shows_check_subcommand() {
    abp()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("check"));
}
