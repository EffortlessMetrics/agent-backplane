#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use abp_cli::cli::{
    Cli, Commands, ConfigAction, LaneArg, ReceiptAction, SchemaArg, WorkspaceModeArg,
};
use abp_cli::commands::{
    SchemaKind, ValidatedType, config_check, inspect_receipt_file, receipt_diff, schema_json,
    validate_file, validate_work_order_file, verify_receipt_file,
};
use abp_cli::config::{
    BackendConfig, BackplaneConfig, ConfigError, apply_env_overrides, load_config, merge_configs,
    validate_config,
};
use abp_cli::format::{Formatter, OutputFormat};
use abp_core::{
    AgentEvent, AgentEventKind, ExecutionLane, Outcome, Receipt, ReceiptBuilder, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use chrono::Utc;
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

// ═══════════════════════════════════════════════════════════════════════
//  Helper functions
// ═══════════════════════════════════════════════════════════════════════

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn make_hashed_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn write_json_file(dir: &std::path::Path, name: &str, value: &impl serde::Serialize) -> PathBuf {
    let path = dir.join(name);
    let json = serde_json::to_string_pretty(value).unwrap();
    std::fs::write(&path, json).unwrap();
    path
}

// ═══════════════════════════════════════════════════════════════════════
//  CLI argument parsing tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_backends_subcommand() {
    let cli = Cli::try_parse_from(["abp", "backends"]).unwrap();
    assert!(matches!(cli.command, Commands::Backends));
    assert!(!cli.debug);
}

#[test]
fn parse_run_minimal() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "hello"]).unwrap();
    match cli.command {
        Commands::Run { task, backend, .. } => {
            assert_eq!(task, "hello");
            assert!(backend.is_none());
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_with_backend() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--backend", "mock"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("mock")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_with_model() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--model", "gpt-4"]).unwrap();
    match cli.command {
        Commands::Run { model, .. } => assert_eq!(model.as_deref(), Some("gpt-4")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_with_root() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--root", "/tmp/ws"]).unwrap();
    match cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "/tmp/ws"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_default_root() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x"]).unwrap();
    match cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "."),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_workspace_mode_pass_through() {
    let cli = Cli::try_parse_from([
        "abp",
        "run",
        "--task",
        "x",
        "--workspace-mode",
        "pass-through",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_workspace_mode_staged_default() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::Staged));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_lane_patch_first_default() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => {
            assert!(matches!(lane, LaneArg::PatchFirst));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_lane_workspace_first() {
    let cli =
        Cli::try_parse_from(["abp", "run", "--task", "x", "--lane", "workspace-first"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => {
            assert!(matches!(lane, LaneArg::WorkspaceFirst));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_include_patterns() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--include", "src/**"]).unwrap();
    match cli.command {
        Commands::Run { include, .. } => {
            assert_eq!(include, vec!["src/**"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_exclude_patterns() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--exclude", "target/**"]).unwrap();
    match cli.command {
        Commands::Run { exclude, .. } => {
            assert_eq!(exclude, vec!["target/**"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_multiple_includes() {
    let cli = Cli::try_parse_from([
        "abp",
        "run",
        "--task",
        "x",
        "--include",
        "src/**",
        "--include",
        "lib/**",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { include, .. } => {
            assert_eq!(include.len(), 2);
            assert_eq!(include[0], "src/**");
            assert_eq!(include[1], "lib/**");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_params() {
    let cli = Cli::try_parse_from([
        "abp",
        "run",
        "--task",
        "x",
        "--param",
        "model=gpt-4",
        "--param",
        "stream=true",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { params, .. } => {
            assert_eq!(params.len(), 2);
            assert_eq!(params[0], "model=gpt-4");
            assert_eq!(params[1], "stream=true");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_env_vars() {
    let cli =
        Cli::try_parse_from(["abp", "run", "--task", "x", "--env", "API_KEY=secret"]).unwrap();
    match cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars, vec!["API_KEY=secret"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_max_budget() {
    let cli =
        Cli::try_parse_from(["abp", "run", "--task", "x", "--max-budget-usd", "1.50"]).unwrap();
    match cli.command {
        Commands::Run { max_budget_usd, .. } => {
            assert_eq!(max_budget_usd, Some(1.50));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_max_turns() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--max-turns", "10"]).unwrap();
    match cli.command {
        Commands::Run { max_turns, .. } => assert_eq!(max_turns, Some(10)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_out_option() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--out", "receipt.json"]).unwrap();
    match cli.command {
        Commands::Run { out, .. } => assert_eq!(out, Some(PathBuf::from("receipt.json"))),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_output_option() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--output", "out.json"]).unwrap();
    match cli.command {
        Commands::Run { output, .. } => assert_eq!(output, Some(PathBuf::from("out.json"))),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_events_option() {
    let cli =
        Cli::try_parse_from(["abp", "run", "--task", "x", "--events", "events.jsonl"]).unwrap();
    match cli.command {
        Commands::Run { events, .. } => {
            assert_eq!(events, Some(PathBuf::from("events.jsonl")));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_json_flag() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x", "--json"]).unwrap();
    match cli.command {
        Commands::Run { json, .. } => assert!(json),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_json_not_set_by_default() {
    let cli = Cli::try_parse_from(["abp", "run", "--task", "x"]).unwrap();
    match cli.command {
        Commands::Run { json, .. } => assert!(!json),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_policy_option() {
    let cli =
        Cli::try_parse_from(["abp", "run", "--task", "x", "--policy", "policy.json"]).unwrap();
    match cli.command {
        Commands::Run { policy, .. } => {
            assert_eq!(policy, Some(PathBuf::from("policy.json")));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_validate_subcommand() {
    let cli = Cli::try_parse_from(["abp", "validate", "wo.json"]).unwrap();
    match cli.command {
        Commands::Validate { file, .. } => assert_eq!(file, Some(PathBuf::from("wo.json"))),
        _ => panic!("expected Validate"),
    }
}

#[test]
fn parse_schema_work_order() {
    let cli = Cli::try_parse_from(["abp", "schema", "work-order"]).unwrap();
    match cli.command {
        Commands::Schema { kind, .. } => assert!(matches!(kind, SchemaArg::WorkOrder)),
        _ => panic!("expected Schema"),
    }
}

#[test]
fn parse_schema_receipt() {
    let cli = Cli::try_parse_from(["abp", "schema", "receipt"]).unwrap();
    match cli.command {
        Commands::Schema { kind, .. } => assert!(matches!(kind, SchemaArg::Receipt)),
        _ => panic!("expected Schema"),
    }
}

#[test]
fn parse_schema_config() {
    let cli = Cli::try_parse_from(["abp", "schema", "config"]).unwrap();
    match cli.command {
        Commands::Schema { kind, .. } => assert!(matches!(kind, SchemaArg::Config)),
        _ => panic!("expected Schema"),
    }
}

#[test]
fn parse_inspect_subcommand() {
    let cli = Cli::try_parse_from(["abp", "inspect", "receipt.json"]).unwrap();
    match cli.command {
        Commands::Inspect { file } => assert_eq!(file, PathBuf::from("receipt.json")),
        _ => panic!("expected Inspect"),
    }
}

#[test]
fn parse_config_check() {
    let cli = Cli::try_parse_from(["abp", "config", "check"]).unwrap();
    match cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert!(config.is_none()),
        _ => panic!("expected ConfigCmd"),
    }
}

#[test]
fn parse_config_check_with_path() {
    let cli = Cli::try_parse_from(["abp", "config", "check", "--config", "my.toml"]).unwrap();
    match cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert_eq!(config, Some(PathBuf::from("my.toml"))),
        _ => panic!("expected ConfigCmd"),
    }
}

#[test]
fn parse_receipt_verify() {
    let cli = Cli::try_parse_from(["abp", "receipt", "verify", "r.json"]).unwrap();
    match cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Verify { file },
        } => assert_eq!(file, PathBuf::from("r.json")),
        _ => panic!("expected ReceiptCmd::Verify"),
    }
}

#[test]
fn parse_receipt_diff() {
    let cli = Cli::try_parse_from(["abp", "receipt", "diff", "a.json", "b.json"]).unwrap();
    match cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Diff { file1, file2 },
        } => {
            assert_eq!(file1, PathBuf::from("a.json"));
            assert_eq!(file2, PathBuf::from("b.json"));
        }
        _ => panic!("expected ReceiptCmd::Diff"),
    }
}

#[test]
fn parse_debug_flag() {
    let cli = Cli::try_parse_from(["abp", "--debug", "backends"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn parse_debug_flag_not_set_default() {
    let cli = Cli::try_parse_from(["abp", "backends"]).unwrap();
    assert!(!cli.debug);
}

#[test]
fn parse_global_config_option() {
    let cli = Cli::try_parse_from(["abp", "--config", "my.toml", "backends"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("my.toml")));
}

#[test]
fn parse_global_config_after_subcommand() {
    let cli = Cli::try_parse_from(["abp", "backends", "--config", "my.toml"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("my.toml")));
}

#[test]
fn parse_missing_subcommand_fails() {
    assert!(Cli::try_parse_from(["abp"]).is_err());
}

#[test]
fn parse_run_missing_task_fails() {
    assert!(Cli::try_parse_from(["abp", "run"]).is_err());
}

#[test]
fn parse_unknown_subcommand_fails() {
    assert!(Cli::try_parse_from(["abp", "nonexistent"]).is_err());
}

#[test]
fn parse_run_all_options_combined() {
    let cli = Cli::try_parse_from([
        "abp",
        "--debug",
        "--config",
        "bp.toml",
        "run",
        "--backend",
        "sidecar:node",
        "--task",
        "build everything",
        "--model",
        "gpt-4o",
        "--root",
        "/workspace",
        "--workspace-mode",
        "pass-through",
        "--lane",
        "workspace-first",
        "--include",
        "src/**",
        "--exclude",
        "target/**",
        "--param",
        "stream=true",
        "--env",
        "TOKEN=abc",
        "--max-budget-usd",
        "5.0",
        "--max-turns",
        "20",
        "--out",
        "out.json",
        "--json",
        "--policy",
        "pol.json",
        "--output",
        "final.json",
        "--events",
        "ev.jsonl",
    ])
    .unwrap();
    assert!(cli.debug);
    assert_eq!(cli.config, Some(PathBuf::from("bp.toml")));
    match cli.command {
        Commands::Run {
            backend,
            task,
            model,
            root,
            workspace_mode,
            lane,
            include,
            exclude,
            params,
            env_vars,
            max_budget_usd,
            max_turns,
            out,
            json,
            policy,
            output,
            events,
        } => {
            assert_eq!(backend.as_deref(), Some("sidecar:node"));
            assert_eq!(task, "build everything");
            assert_eq!(model.as_deref(), Some("gpt-4o"));
            assert_eq!(root, "/workspace");
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
            assert!(matches!(lane, LaneArg::WorkspaceFirst));
            assert_eq!(include, vec!["src/**"]);
            assert_eq!(exclude, vec!["target/**"]);
            assert_eq!(params, vec!["stream=true"]);
            assert_eq!(env_vars, vec!["TOKEN=abc"]);
            assert_eq!(max_budget_usd, Some(5.0));
            assert_eq!(max_turns, Some(20));
            assert_eq!(out, Some(PathBuf::from("out.json")));
            assert!(json);
            assert_eq!(policy, Some(PathBuf::from("pol.json")));
            assert_eq!(output, Some(PathBuf::from("final.json")));
            assert_eq!(events, Some(PathBuf::from("ev.jsonl")));
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Commands module tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn schema_json_work_order_valid() {
    let s = schema_json(SchemaKind::WorkOrder).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn schema_json_receipt_valid() {
    let s = schema_json(SchemaKind::Receipt).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn schema_json_config_valid() {
    let s = schema_json(SchemaKind::Config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn schema_json_work_order_has_properties() {
    let s = schema_json(SchemaKind::WorkOrder).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.get("properties").is_some() || v.get("$defs").is_some());
}

#[test]
fn schema_kind_equality() {
    assert_eq!(SchemaKind::WorkOrder, SchemaKind::WorkOrder);
    assert_eq!(SchemaKind::Receipt, SchemaKind::Receipt);
    assert_eq!(SchemaKind::Config, SchemaKind::Config);
    assert_ne!(SchemaKind::WorkOrder, SchemaKind::Receipt);
}

#[test]
fn schema_kind_is_copy() {
    let k = SchemaKind::WorkOrder;
    let k2 = k; // Copy
    assert_eq!(k, k2);
}

#[test]
fn validated_type_equality() {
    assert_eq!(ValidatedType::WorkOrder, ValidatedType::WorkOrder);
    assert_eq!(ValidatedType::Receipt, ValidatedType::Receipt);
    assert_ne!(ValidatedType::WorkOrder, ValidatedType::Receipt);
}

#[test]
fn validated_type_is_copy() {
    let v = ValidatedType::WorkOrder;
    let v2 = v; // Copy
    assert_eq!(v, v2);
}

#[test]
fn validate_file_detects_work_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "wo.json", &make_work_order());
    assert_eq!(validate_file(&path).unwrap(), ValidatedType::WorkOrder);
}

#[test]
fn validate_file_detects_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "receipt.json", &make_receipt());
    assert_eq!(validate_file(&path).unwrap(), ValidatedType::Receipt);
}

#[test]
fn validate_file_rejects_bad_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    assert!(validate_file(&path).is_err());
}

#[test]
fn validate_file_rejects_unknown_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unknown.json");
    std::fs::write(&path, r#"{"foo": 42}"#).unwrap();
    assert!(validate_file(&path).is_err());
}

#[test]
fn validate_file_nonexistent_errors() {
    let result = validate_file(std::path::Path::new("/nonexistent/file.json"));
    assert!(result.is_err());
}

#[test]
fn validate_work_order_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "wo.json", &make_work_order());
    validate_work_order_file(&path).unwrap();
}

#[test]
fn validate_work_order_bad_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "nope").unwrap();
    assert!(validate_work_order_file(&path).is_err());
}

#[test]
fn validate_work_order_wrong_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wrong.json");
    std::fs::write(&path, r#"{"a": 1}"#).unwrap();
    assert!(validate_work_order_file(&path).is_err());
}

#[test]
fn inspect_receipt_valid_hash() {
    let receipt = make_hashed_receipt();
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &receipt);
    let (r, valid) = inspect_receipt_file(&path).unwrap();
    assert!(valid);
    assert_eq!(r.receipt_sha256, receipt.receipt_sha256);
}

#[test]
fn inspect_receipt_tampered_hash() {
    let mut receipt = make_hashed_receipt();
    receipt.receipt_sha256 = Some("deadbeef".into());
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &receipt);
    let (_, valid) = inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn inspect_receipt_missing_hash() {
    let receipt = make_receipt();
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &receipt);
    let (_, valid) = inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn verify_receipt_delegates_to_inspect() {
    let receipt = make_hashed_receipt();
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &receipt);
    let (_, valid) = verify_receipt_file(&path).unwrap();
    assert!(valid);
}

#[test]
fn verify_receipt_invalid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "nope").unwrap();
    assert!(verify_receipt_file(&path).is_err());
}

#[test]
fn config_check_default_ok() {
    let diags = config_check(None).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

#[test]
fn config_check_bad_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "not valid [toml =").unwrap();
    let diags = config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.starts_with("error:")));
}

#[test]
fn config_check_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ok.toml");
    std::fs::write(&path, "").unwrap();
    let diags = config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

#[test]
fn receipt_diff_identical() {
    let receipt = make_hashed_receipt();
    let dir = tempfile::tempdir().unwrap();
    let p1 = write_json_file(dir.path(), "r1.json", &receipt);
    let p2 = write_json_file(dir.path(), "r2.json", &receipt);
    let diff = receipt_diff(&p1, &p2).unwrap();
    assert_eq!(diff, "no differences");
}

#[test]
fn receipt_diff_different_outcome() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p1 = write_json_file(dir.path(), "r1.json", &r1);
    let p2 = write_json_file(dir.path(), "r2.json", &r2);
    let diff = receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("outcome"));
}

#[test]
fn receipt_diff_different_backend() {
    let r1 = ReceiptBuilder::new("alpha")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("beta")
        .outcome(Outcome::Complete)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let p1 = write_json_file(dir.path(), "r1.json", &r1);
    let p2 = write_json_file(dir.path(), "r2.json", &r2);
    let diff = receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("backend"));
}

#[test]
fn receipt_diff_bad_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("bad.json");
    let p2 = dir.path().join("bad2.json");
    std::fs::write(&p1, "nope").unwrap();
    std::fs::write(&p2, "nah").unwrap();
    assert!(receipt_diff(&p1, &p2).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
//  Config module tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backplane_config_default() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.backends.is_empty());
    assert!(cfg.default_backend.is_none());
    assert!(cfg.log_level.is_none());
    assert!(cfg.receipts_dir.is_none());
}

#[test]
fn backplane_config_serde_json_roundtrip() {
    let mut cfg = BackplaneConfig::default();
    cfg.default_backend = Some("mock".into());
    cfg.log_level = Some("debug".into());
    cfg.backends.insert("mock".into(), BackendConfig::Mock {});

    let json = serde_json::to_string(&cfg).unwrap();
    let cfg2: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg2.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg2.log_level.as_deref(), Some("debug"));
    assert!(cfg2.backends.contains_key("mock"));
}

#[test]
fn backplane_config_toml_parse() {
    let toml_str = r#"
default_backend = "mock"
log_level = "info"

[backends.my_mock]
type = "mock"

[backends.my_sidecar]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 300
"#;
    let cfg: BackplaneConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.backends.len(), 2);
    assert!(cfg.backends.contains_key("my_mock"));
    assert!(cfg.backends.contains_key("my_sidecar"));
}

#[test]
fn backend_config_mock_serde_json() {
    let mock = BackendConfig::Mock {};
    let json = serde_json::to_string(&mock).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v.get("type").unwrap().as_str().unwrap(), "mock");
}

#[test]
fn backend_config_sidecar_serde_json() {
    let sc = BackendConfig::Sidecar {
        command: "node".into(),
        args: vec!["host.js".into()],
        timeout_secs: Some(60),
    };
    let json = serde_json::to_string(&sc).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v.get("type").unwrap().as_str().unwrap(), "sidecar");
    assert_eq!(v.get("command").unwrap().as_str().unwrap(), "node");
}

#[test]
fn backend_config_sidecar_roundtrip() {
    let sc = BackendConfig::Sidecar {
        command: "python".into(),
        args: vec!["-m".into(), "host".into()],
        timeout_secs: Some(120),
    };
    let json = serde_json::to_string(&sc).unwrap();
    let sc2: BackendConfig = serde_json::from_str(&json).unwrap();
    match sc2 {
        BackendConfig::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python");
            assert_eq!(args, vec!["-m", "host"]);
            assert_eq!(timeout_secs, Some(120));
        }
        _ => panic!("expected Sidecar"),
    }
}

#[test]
fn validate_empty_config_ok() {
    let cfg = BackplaneConfig::default();
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_empty_command_fails() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "bad".into(),
            BackendConfig::Sidecar {
                command: "  ".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let errs = validate_config(&cfg).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::InvalidBackend { .. }))
    );
}

#[test]
fn validate_zero_timeout_fails() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        )]),
        ..Default::default()
    };
    let errs = validate_config(&cfg).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::InvalidTimeout { value: 0 }))
    );
}

#[test]
fn validate_huge_timeout_fails() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(86_401),
            },
        )]),
        ..Default::default()
    };
    let errs = validate_config(&cfg).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::InvalidTimeout { .. }))
    );
}

#[test]
fn validate_valid_sidecar_ok() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: Some(300),
            },
        )]),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_mock_ok() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([("m".into(), BackendConfig::Mock {})]),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_multiple_errors() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([
            (
                "bad_cmd".into(),
                BackendConfig::Sidecar {
                    command: "".into(),
                    args: vec![],
                    timeout_secs: None,
                },
            ),
            (
                "bad_timeout".into(),
                BackendConfig::Sidecar {
                    command: "ok".into(),
                    args: vec![],
                    timeout_secs: Some(0),
                },
            ),
        ]),
        ..Default::default()
    };
    let errs = validate_config(&cfg).unwrap_err();
    assert!(errs.len() >= 2);
}

#[test]
fn validate_valid_timeout_boundary() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(86_400),
            },
        )]),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn validate_timeout_one_is_valid() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(1),
            },
        )]),
        ..Default::default()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn merge_configs_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("sidecar:node".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("sidecar:node"));
}

#[test]
fn merge_configs_backends_combined() {
    let mut base = BackplaneConfig::default();
    base.backends.insert("mock".into(), BackendConfig::Mock {});
    let mut overlay = BackplaneConfig::default();
    overlay.backends.insert(
        "sc".into(),
        BackendConfig::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let merged = merge_configs(base, overlay);
    assert!(merged.backends.contains_key("mock"));
    assert!(merged.backends.contains_key("sc"));
}

#[test]
fn merge_configs_overlay_backend_replaces_base() {
    let mut base = BackplaneConfig::default();
    base.backends.insert("x".into(), BackendConfig::Mock {});
    let mut overlay = BackplaneConfig::default();
    overlay.backends.insert(
        "x".into(),
        BackendConfig::Sidecar {
            command: "new".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let merged = merge_configs(base, overlay);
    match merged.backends.get("x").unwrap() {
        BackendConfig::Sidecar { command, .. } => assert_eq!(command, "new"),
        _ => panic!("expected Sidecar after merge"),
    }
}

#[test]
fn merge_configs_base_preserved_when_overlay_empty() {
    let base = BackplaneConfig {
        log_level: Some("debug".into()),
        receipts_dir: Some("/receipts".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig::default();
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/receipts"));
}

#[test]
fn load_config_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn load_config_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"

[backends.m]
type = "mock"
"#,
    )
    .unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert!(cfg.backends.contains_key("m"));
}

#[test]
fn load_config_invalid_toml_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[broken =").unwrap();
    assert!(load_config(Some(&path)).is_err());
}

#[test]
fn load_config_nonexistent_file_errors() {
    assert!(load_config(Some(std::path::Path::new("/no/such/file.toml"))).is_err());
}

#[test]
fn config_error_display_invalid_backend() {
    let e = ConfigError::InvalidBackend {
        name: "x".into(),
        reason: "bad".into(),
    };
    let s = e.to_string();
    assert!(s.contains("invalid backend"));
    assert!(s.contains("x"));
    assert!(s.contains("bad"));
}

#[test]
fn config_error_display_invalid_timeout() {
    let e = ConfigError::InvalidTimeout { value: 0 };
    let s = e.to_string();
    assert!(s.contains("invalid timeout"));
}

#[test]
fn config_error_display_missing_field() {
    let e = ConfigError::MissingRequiredField {
        field: "name".into(),
    };
    let s = e.to_string();
    assert!(s.contains("missing required field"));
}

#[test]
fn config_error_implements_error_trait() {
    let e: Box<dyn std::error::Error> = Box::new(ConfigError::InvalidTimeout { value: 99 });
    let _ = e.to_string();
}

// ═══════════════════════════════════════════════════════════════════════
//  Format module tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn output_format_display_json() {
    assert_eq!(OutputFormat::Json.to_string(), "json");
}

#[test]
fn output_format_display_json_pretty() {
    assert_eq!(OutputFormat::JsonPretty.to_string(), "json-pretty");
}

#[test]
fn output_format_display_text() {
    assert_eq!(OutputFormat::Text.to_string(), "text");
}

#[test]
fn output_format_display_table() {
    assert_eq!(OutputFormat::Table.to_string(), "table");
}

#[test]
fn output_format_display_compact() {
    assert_eq!(OutputFormat::Compact.to_string(), "compact");
}

#[test]
fn output_format_from_str_json() {
    assert_eq!(OutputFormat::from_str("json").unwrap(), OutputFormat::Json);
}

#[test]
fn output_format_from_str_json_pretty() {
    assert_eq!(
        OutputFormat::from_str("json-pretty").unwrap(),
        OutputFormat::JsonPretty
    );
}

#[test]
fn output_format_from_str_json_pretty_underscore() {
    assert_eq!(
        OutputFormat::from_str("json_pretty").unwrap(),
        OutputFormat::JsonPretty
    );
}

#[test]
fn output_format_from_str_json_pretty_no_sep() {
    assert_eq!(
        OutputFormat::from_str("jsonpretty").unwrap(),
        OutputFormat::JsonPretty
    );
}

#[test]
fn output_format_from_str_text() {
    assert_eq!(OutputFormat::from_str("text").unwrap(), OutputFormat::Text);
}

#[test]
fn output_format_from_str_table() {
    assert_eq!(
        OutputFormat::from_str("table").unwrap(),
        OutputFormat::Table
    );
}

#[test]
fn output_format_from_str_compact() {
    assert_eq!(
        OutputFormat::from_str("compact").unwrap(),
        OutputFormat::Compact
    );
}

#[test]
fn output_format_from_str_case_insensitive() {
    assert_eq!(OutputFormat::from_str("JSON").unwrap(), OutputFormat::Json);
    assert_eq!(OutputFormat::from_str("Text").unwrap(), OutputFormat::Text);
    assert_eq!(
        OutputFormat::from_str("TABLE").unwrap(),
        OutputFormat::Table
    );
}

#[test]
fn output_format_from_str_unknown_fails() {
    assert!(OutputFormat::from_str("yaml").is_err());
    assert!(OutputFormat::from_str("").is_err());
    assert!(OutputFormat::from_str("xml").is_err());
}

#[test]
fn output_format_serde_roundtrip() {
    for fmt in &[
        OutputFormat::Json,
        OutputFormat::JsonPretty,
        OutputFormat::Text,
        OutputFormat::Table,
        OutputFormat::Compact,
    ] {
        let json = serde_json::to_string(fmt).unwrap();
        let parsed: OutputFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, fmt);
    }
}

#[test]
fn output_format_display_roundtrip() {
    for fmt in &[
        OutputFormat::Json,
        OutputFormat::JsonPretty,
        OutputFormat::Text,
        OutputFormat::Table,
        OutputFormat::Compact,
    ] {
        let s = fmt.to_string();
        let parsed: OutputFormat = s.parse().unwrap();
        assert_eq!(&parsed, fmt);
    }
}

#[test]
fn formatter_receipt_json() {
    let f = Formatter::new(OutputFormat::Json);
    let r = make_receipt();
    let s = f.format_receipt(&r);
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn formatter_receipt_json_pretty() {
    let f = Formatter::new(OutputFormat::JsonPretty);
    let r = make_receipt();
    let s = f.format_receipt(&r);
    assert!(s.contains('\n'));
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn formatter_receipt_text() {
    let f = Formatter::new(OutputFormat::Text);
    let r = make_receipt();
    let s = f.format_receipt(&r);
    assert!(s.contains("Outcome:"));
    assert!(s.contains("Backend:"));
}

#[test]
fn formatter_receipt_table() {
    let f = Formatter::new(OutputFormat::Table);
    let r = make_receipt();
    let s = f.format_receipt(&r);
    assert!(s.contains("outcome"));
    assert!(s.contains("backend"));
    assert!(s.contains("run_id"));
}

#[test]
fn formatter_receipt_compact() {
    let f = Formatter::new(OutputFormat::Compact);
    let r = make_receipt();
    let s = f.format_receipt(&r);
    assert!(s.starts_with('['));
    assert!(s.contains("backend="));
}

#[test]
fn formatter_event_json() {
    let f = Formatter::new(OutputFormat::Json);
    let ev = make_agent_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let s = f.format_event(&ev);
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn formatter_event_text() {
    let f = Formatter::new(OutputFormat::Text);
    let ev = make_agent_event(AgentEventKind::Warning {
        message: "caution".into(),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("warning"));
    assert!(s.contains("caution"));
}

#[test]
fn formatter_event_compact() {
    let f = Formatter::new(OutputFormat::Compact);
    let ev = make_agent_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("[run_started]"));
}

#[test]
fn formatter_event_table() {
    let f = Formatter::new(OutputFormat::Table);
    let ev = make_agent_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "created".into(),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("file_changed"));
}

#[test]
fn formatter_work_order_json() {
    let f = Formatter::new(OutputFormat::Json);
    let wo = make_work_order();
    let s = f.format_work_order(&wo);
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn formatter_work_order_text() {
    let f = Formatter::new(OutputFormat::Text);
    let wo = make_work_order();
    let s = f.format_work_order(&wo);
    assert!(s.contains("ID:"));
    assert!(s.contains("Task:"));
}

#[test]
fn formatter_work_order_table() {
    let f = Formatter::new(OutputFormat::Table);
    let wo = make_work_order();
    let s = f.format_work_order(&wo);
    assert!(s.contains("id"));
    assert!(s.contains("task"));
    assert!(s.contains("lane"));
}

#[test]
fn formatter_work_order_compact() {
    let f = Formatter::new(OutputFormat::Compact);
    let wo = make_work_order();
    let s = f.format_work_order(&wo);
    assert!(s.contains("lane="));
}

#[test]
fn formatter_error_json() {
    let f = Formatter::new(OutputFormat::Json);
    let s = f.format_error("something broke");
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v.get("error").unwrap().as_str().unwrap(), "something broke");
}

#[test]
fn formatter_error_text() {
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_error("oops");
    assert!(s.starts_with("Error:"));
    assert!(s.contains("oops"));
}

#[test]
fn formatter_error_compact() {
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_error("fail");
    assert!(s.starts_with("[error]"));
    assert!(s.contains("fail"));
}

#[test]
fn formatter_error_table() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_error("bad");
    assert!(s.contains("error"));
    assert!(s.contains("bad"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Conversion tests (From impls)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_mode_arg_to_pass_through() {
    let mode: WorkspaceMode = WorkspaceModeArg::PassThrough.into();
    assert!(matches!(mode, WorkspaceMode::PassThrough));
}

#[test]
fn workspace_mode_arg_to_staged() {
    let mode: WorkspaceMode = WorkspaceModeArg::Staged.into();
    assert!(matches!(mode, WorkspaceMode::Staged));
}

#[test]
fn lane_arg_to_patch_first() {
    let lane: ExecutionLane = LaneArg::PatchFirst.into();
    assert!(matches!(lane, ExecutionLane::PatchFirst));
}

#[test]
fn lane_arg_to_workspace_first() {
    let lane: ExecutionLane = LaneArg::WorkspaceFirst.into();
    assert!(matches!(lane, ExecutionLane::WorkspaceFirst));
}

// ═══════════════════════════════════════════════════════════════════════
//  Additional edge-case tests for event formatting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn formatter_event_tool_call() {
    let f = Formatter::new(OutputFormat::Text);
    let ev = make_agent_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "main.rs"}),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("tool_call"));
    assert!(s.contains("read_file"));
}

#[test]
fn formatter_event_tool_result() {
    let f = Formatter::new(OutputFormat::Text);
    let ev = make_agent_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        output: serde_json::json!("file contents"),
        is_error: false,
    });
    let s = f.format_event(&ev);
    assert!(s.contains("tool_result"));
    assert!(s.contains("read_file"));
}

#[test]
fn formatter_event_tool_result_error() {
    let f = Formatter::new(OutputFormat::Compact);
    let ev = make_agent_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: serde_json::json!("not found"),
        is_error: true,
    });
    let s = f.format_event(&ev);
    assert!(s.contains("error"));
}

#[test]
fn formatter_event_command_executed() {
    let f = Formatter::new(OutputFormat::Text);
    let ev = make_agent_event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: Some(0),
        output_preview: Some("Compiling...".into()),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("command_executed"));
}

#[test]
fn formatter_event_error_kind() {
    let f = Formatter::new(OutputFormat::Text);
    let ev = make_agent_event(AgentEventKind::Error {
        message: "fatal crash".into(),
        error_code: None,
    });
    let s = f.format_event(&ev);
    assert!(s.contains("error"));
    assert!(s.contains("fatal crash"));
}

#[test]
fn formatter_event_assistant_delta() {
    let f = Formatter::new(OutputFormat::Compact);
    let ev = make_agent_event(AgentEventKind::AssistantDelta {
        text: "streaming chunk".into(),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("assistant_delta"));
}

#[test]
fn formatter_event_run_completed() {
    let f = Formatter::new(OutputFormat::Text);
    let ev = make_agent_event(AgentEventKind::RunCompleted {
        message: "done!".into(),
    });
    let s = f.format_event(&ev);
    assert!(s.contains("run_completed"));
    assert!(s.contains("done!"));
}
