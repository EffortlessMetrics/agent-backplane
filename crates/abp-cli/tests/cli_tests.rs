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
//! Comprehensive unit tests for the `abp` CLI binary covering argument parsing,
//! backend registration, and configuration loading.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::io::Write;

fn abp() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("abp").expect("binary `abp` should be built")
}

// ═══════════════════════════════════════════════════════════════════════
// Module: arg_parsing — tests for CLI argument parsing via clap
// ═══════════════════════════════════════════════════════════════════════
mod arg_parsing {
    use super::*;

    #[test]
    fn parse_run_task_backend_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        abp()
            .args([
                "run",
                "--task",
                "hello",
                "--backend",
                "mock",
                "--root",
                tmp.path().to_str().unwrap(),
                "--workspace-mode",
                "pass-through",
                "--out",
                receipt.to_str().unwrap(),
            ])
            .assert()
            .success()
            .stderr(contains("backend: mock"));
    }

    #[test]
    fn parse_run_task_default_backend() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        // Omit --backend; should default to "mock".
        abp()
            .current_dir(tmp.path())
            .args([
                "run",
                "--task",
                "hello",
                "--workspace-mode",
                "pass-through",
                "--out",
                receipt.to_str().unwrap(),
            ])
            .assert()
            .success()
            .stderr(contains("backend: mock"));
    }

    #[test]
    fn parse_backends_subcommand() {
        abp()
            .arg("backends")
            .assert()
            .success()
            .stdout(contains("mock"));
    }

    #[test]
    fn parse_debug_flag_before_subcommand() {
        abp()
            .args(["--debug", "backends"])
            .assert()
            .success()
            .stdout(contains("mock"));
    }

    #[test]
    fn parse_unknown_backend_name_errors_at_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        abp()
            .args([
                "run",
                "--task",
                "hello",
                "--backend",
                "does_not_exist_xyz",
                "--root",
                tmp.path().to_str().unwrap(),
                "--workspace-mode",
                "pass-through",
            ])
            .assert()
            .failure();
    }

    #[test]
    fn parse_run_without_task_errors() {
        abp()
            .args(["run", "--backend", "mock"])
            .assert()
            .failure()
            .stderr(contains("--task"));
    }

    #[test]
    fn parse_config_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("custom.toml");
        std::fs::write(
            &config_path,
            r#"[backends.cfg-mock]
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
                "--backend",
                "cfg-mock",
                "--task",
                "config path test",
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
    fn parse_empty_task_string_accepted() {
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
    }

    #[test]
    fn parse_workspace_mode_values() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        // pass-through
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "t",
                "--workspace-mode",
                "pass-through",
                "--root",
                tmp.path().to_str().unwrap(),
                "--out",
                receipt.to_str().unwrap(),
            ])
            .assert()
            .success();
        // staged (default)
        let receipt2 = tmp.path().join("receipt2.json");
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "t",
                "--workspace-mode",
                "staged",
                "--root",
                tmp.path().to_str().unwrap(),
                "--out",
                receipt2.to_str().unwrap(),
            ])
            .assert()
            .success();
    }

    #[test]
    fn parse_invalid_workspace_mode_rejected() {
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "t",
                "--workspace-mode",
                "invalid-mode",
            ])
            .assert()
            .failure()
            .stderr(contains("invalid value"));
    }

    #[test]
    fn parse_lane_values_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "lane",
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
    fn parse_model_flag_accepted() {
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
    fn parse_multiple_params_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "multi param",
                "--param",
                "model=test",
                "--param",
                "stream=true",
                "--param",
                "abp.mode=passthrough",
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
    fn parse_include_exclude_globs() {
        let tmp = tempfile::tempdir().unwrap();
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
                "--include",
                "Cargo.toml",
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

    #[test]
    fn parse_max_budget_and_turns() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "limits",
                "--max-budget-usd",
                "5.50",
                "--max-turns",
                "20",
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
    fn parse_invalid_param_format_rejected() {
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
    fn parse_unknown_flag_rejected() {
        abp()
            .args(["run", "--nonexistent-flag", "value"])
            .assert()
            .failure()
            .stderr(contains("unexpected argument").or(contains("error")));
    }

    #[test]
    fn parse_unknown_subcommand_rejected() {
        abp()
            .arg("nonexistent_subcmd")
            .assert()
            .failure()
            .stderr(contains("unrecognized subcommand"));
    }

    #[test]
    fn parse_no_subcommand_shows_usage() {
        abp()
            .assert()
            .failure()
            .stderr(contains("Usage").or(contains("subcommand")));
    }

    #[test]
    fn parse_help_shows_all_subcommands() {
        abp()
            .arg("--help")
            .assert()
            .success()
            .stdout(contains("Agent Backplane CLI"))
            .stdout(contains("backends"))
            .stdout(contains("run"))
            .stdout(contains("validate"))
            .stdout(contains("schema"))
            .stdout(contains("inspect"))
            .stdout(contains("config"))
            .stdout(contains("receipt"));
    }

    #[test]
    fn parse_run_help_shows_all_flags() {
        abp()
            .args(["run", "--help"])
            .assert()
            .success()
            .stdout(contains("--task"))
            .stdout(contains("--backend"))
            .stdout(contains("--model"))
            .stdout(contains("--workspace-mode"))
            .stdout(contains("--lane"))
            .stdout(contains("--include"))
            .stdout(contains("--exclude"))
            .stdout(contains("--param"))
            .stdout(contains("--json"))
            .stdout(contains("--policy"))
            .stdout(contains("--output"))
            .stdout(contains("--events"))
            .stdout(contains("--max-budget-usd"))
            .stdout(contains("--max-turns"));
    }

    #[test]
    fn parse_json_flag_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        let output = abp()
            .args([
                "run",
                "--backend",
                "mock",
                "--task",
                "json",
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
        // --json suppresses run_id header on stderr
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("run_id:"));
    }

    #[test]
    fn parse_env_flag_accepted() {
        let tmp = tempfile::tempdir().unwrap();
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
    fn parse_invalid_env_format_rejected() {
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
}

// ═══════════════════════════════════════════════════════════════════════
// Module: backend_registration — tests for Runtime backend management
// ═══════════════════════════════════════════════════════════════════════
mod backend_registration {
    use super::*;

    #[test]
    fn default_backends_include_mock() {
        let rt = abp_runtime::Runtime::with_default_backends();
        let names = rt.backend_names();
        assert!(
            names.contains(&"mock".to_string()),
            "default backends must include 'mock'"
        );
    }

    #[test]
    fn backend_list_is_non_empty() {
        let rt = abp_runtime::Runtime::with_default_backends();
        let names = rt.backend_names();
        assert!(!names.is_empty(), "backend list should never be empty");
    }

    #[test]
    fn backend_lookup_by_name_succeeds() {
        let rt = abp_runtime::Runtime::with_default_backends();
        assert!(
            rt.backend("mock").is_some(),
            "should find 'mock' backend by name"
        );
    }

    #[test]
    fn unknown_backend_lookup_returns_none() {
        let rt = abp_runtime::Runtime::with_default_backends();
        assert!(
            rt.backend("does_not_exist_at_all").is_none(),
            "unknown backend lookup should return None"
        );
    }

    #[test]
    fn register_custom_backend_is_discoverable() {
        let mut rt = abp_runtime::Runtime::with_default_backends();
        rt.register_backend("custom-mock", abp_integrations::MockBackend);
        let names = rt.backend_names();
        assert!(names.contains(&"custom-mock".to_string()));
        assert!(rt.backend("custom-mock").is_some());
    }

    #[test]
    fn register_backend_replaces_existing() {
        let mut rt = abp_runtime::Runtime::with_default_backends();
        // Re-register "mock" — should not duplicate, just replace.
        rt.register_backend("mock", abp_integrations::MockBackend);
        let count = rt.backend_names().iter().filter(|n| *n == "mock").count();
        assert_eq!(count, 1, "re-registering should replace, not duplicate");
    }

    #[test]
    fn backend_names_are_sorted() {
        let mut rt = abp_runtime::Runtime::with_default_backends();
        rt.register_backend("zzz-last", abp_integrations::MockBackend);
        rt.register_backend("aaa-first", abp_integrations::MockBackend);
        let names = rt.backend_names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "backend_names() should return sorted list");
    }

    #[test]
    fn cli_backends_lists_sidecar_types() {
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
                "backends output should list '{expected}'"
            );
        }
    }

    #[test]
    fn cli_backends_lists_short_aliases() {
        let output = abp().arg("backends").output().expect("execute abp");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        for alias in [
            "node", "python", "claude", "copilot", "kimi", "gemini", "codex",
        ] {
            assert!(
                stdout.lines().any(|l| l.trim() == alias),
                "backends should include short alias '{alias}' as its own line"
            );
        }
    }

    #[test]
    fn unknown_backend_run_produces_meaningful_error() {
        let tmp = tempfile::tempdir().unwrap();
        let output = abp()
            .args([
                "run",
                "--task",
                "test",
                "--backend",
                "totally-nonexistent",
                "--root",
                tmp.path().to_str().unwrap(),
                "--workspace-mode",
                "pass-through",
            ])
            .output()
            .expect("execute abp");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The error message should mention the backend name or "error".
        assert!(
            stderr.contains("error") || stderr.contains("totally-nonexistent"),
            "error should be meaningful, got: {stderr}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: config_loading — tests for config parsing and validation
// ═══════════════════════════════════════════════════════════════════════
mod config_loading {
    use super::*;
    use abp_cli::config::{self, BackendConfig, BackplaneConfig};
    use std::collections::HashMap;

    #[test]
    fn default_config_when_no_file_exists() {
        let cfg = config::load_config(None).unwrap();
        assert!(cfg.backends.is_empty());
        assert!(cfg.default_backend.is_none());
        config::validate_config(&cfg).unwrap();
    }

    #[test]
    fn config_from_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backplane.toml");
        std::fs::write(
            &path,
            r#"
default_backend = "sidecar:claude"
log_level = "debug"

[backends.mock]
type = "mock"

[backends."sidecar:node"]
type = "sidecar"
command = "node"
args = ["hosts/node/host.js"]
"#,
        )
        .unwrap();
        let cfg = config::load_config(Some(&path)).unwrap();
        assert_eq!(cfg.default_backend.as_deref(), Some("sidecar:claude"));
        assert_eq!(cfg.log_level.as_deref(), Some("debug"));
        assert_eq!(cfg.backends.len(), 2);
        config::validate_config(&cfg).unwrap();
    }

    #[test]
    fn config_validation_detects_empty_command() {
        let cfg = BackplaneConfig {
            backends: HashMap::from([(
                "bad-sidecar".into(),
                BackendConfig::Sidecar {
                    command: "  ".into(),
                    args: vec![],
                    timeout_secs: None,
                },
            )]),
            ..Default::default()
        };
        let errs = config::validate_config(&cfg).unwrap_err();
        let has_invalid = errs
            .iter()
            .any(|e| matches!(e, config::ConfigError::InvalidBackend { .. }));
        assert!(has_invalid, "should detect empty sidecar command");
    }

    #[test]
    fn config_validation_detects_excessive_timeout() {
        let cfg = BackplaneConfig {
            backends: HashMap::from([(
                "sc".into(),
                BackendConfig::Sidecar {
                    command: "node".into(),
                    args: vec![],
                    timeout_secs: Some(100_000),
                },
            )]),
            ..Default::default()
        };
        let errs = config::validate_config(&cfg).unwrap_err();
        let has_timeout = errs
            .iter()
            .any(|e| matches!(e, config::ConfigError::InvalidTimeout { .. }));
        assert!(has_timeout, "should detect out-of-range timeout");
    }

    #[test]
    fn config_invalid_toml_gives_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "not valid [[[ toml =").unwrap();
        let err = config::load_config(Some(&path)).unwrap_err();
        assert!(
            err.to_string().contains("failed to parse"),
            "should mention parse failure: {}",
            err
        );
    }

    #[test]
    fn debug_flag_enables_verbose_logging() {
        let tmp = tempfile::tempdir().unwrap();
        let receipt = tmp.path().join("receipt.json");
        // --debug should be accepted and produce debug-level output
        abp()
            .args([
                "--debug",
                "run",
                "--backend",
                "mock",
                "--task",
                "debug logging test",
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
    fn config_check_subcommand_validates_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("good.toml");
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
            .stdout(contains("ok"));
    }

    #[test]
    fn config_check_invalid_toml_reports_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.toml");
        std::fs::write(&path, "not [valid toml =").unwrap();
        abp()
            .args(["config", "check", "--config", path.to_str().unwrap()])
            .assert()
            .failure()
            .stdout(contains("error"));
    }

    #[test]
    fn config_merge_overlay_wins() {
        let base = BackplaneConfig {
            default_backend: Some("mock".into()),
            log_level: Some("warn".into()),
            ..Default::default()
        };
        let overlay = BackplaneConfig {
            default_backend: Some("sidecar:claude".into()),
            ..Default::default()
        };
        let merged = config::merge_configs(base, overlay);
        assert_eq!(merged.default_backend.as_deref(), Some("sidecar:claude"));
        // Base value preserved when overlay is None.
        assert_eq!(merged.log_level.as_deref(), Some("warn"));
    }

    #[test]
    fn config_file_in_cwd_auto_discovered() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("backplane.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"[backends.auto-mock]
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
                "auto-mock",
                "--task",
                "auto-discover",
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
