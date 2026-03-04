#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
//! Comprehensive integration tests for the ABP CLI crate.
//!
//! Tests CLI argument parsing, commands, config loading, formatting,
//! and error handling — all without spawning the binary.

use abp_cli::cli::{
    Cli, Commands, ConfigAction, LaneArg, ReceiptAction, SchemaArg, WorkspaceModeArg,
};
use abp_cli::commands::{self, SchemaKind, ValidatedType};
use abp_cli::config::{BackendConfig, BackplaneConfig};
use abp_cli::format::{Formatter, OutputFormat};
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, ExecutionLane, Outcome, ReceiptBuilder,
    WorkOrderBuilder, WorkspaceMode,
};
use std::collections::HashMap;
use std::path::PathBuf;

// Re-import Parser trait so try_parse_from is in scope.
use clap::Parser as _;

// ═══════════════════════════════════════════════════════════════════════
// Helper: parse CLI args
// ═══════════════════════════════════════════════════════════════════════

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    let mut full = vec!["abp"];
    full.extend_from_slice(args);
    Cli::try_parse_from(full)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Run subcommand argument parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_run_minimal() {
    let cli = parse(&["run", "--task", "hello"]).unwrap();
    match cli.command {
        Commands::Run { task, backend, .. } => {
            assert_eq!(task, "hello");
            assert!(backend.is_none());
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_with_backend_mock() {
    let cli = parse(&["run", "--task", "fix bugs", "--backend", "mock"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("mock")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_with_sidecar_node() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:node"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:node")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_with_model() {
    let cli = parse(&["run", "--task", "t", "--model", "gpt-4"]).unwrap();
    match cli.command {
        Commands::Run { model, .. } => assert_eq!(model.as_deref(), Some("gpt-4")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_root_default() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "."),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_custom_root() {
    let cli = parse(&["run", "--task", "t", "--root", "/some/path"]).unwrap();
    match cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "/some/path"),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_workspace_mode_pass_through() {
    let cli = parse(&["run", "--task", "t", "--workspace-mode", "pass-through"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_workspace_mode_staged() {
    let cli = parse(&["run", "--task", "t", "--workspace-mode", "staged"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::Staged));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_workspace_mode_default_is_staged() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::Staged));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_lane_patch_first() {
    let cli = parse(&["run", "--task", "t", "--lane", "patch-first"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::PatchFirst)),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_lane_workspace_first() {
    let cli = parse(&["run", "--task", "t", "--lane", "workspace-first"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::WorkspaceFirst)),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_lane_default_is_patch_first() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::PatchFirst)),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_include_exclude_globs() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--include",
        "src/**",
        "--include",
        "lib/**",
        "--exclude",
        "*.log",
    ])
    .unwrap();
    match cli.command {
        Commands::Run {
            include, exclude, ..
        } => {
            assert_eq!(include, vec!["src/**", "lib/**"]);
            assert_eq!(exclude, vec!["*.log"]);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_params() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--param",
        "model=gemini-2.5-flash",
        "--param",
        "stream=true",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { params, .. } => {
            assert_eq!(params.len(), 2);
            assert!(params.contains(&"model=gemini-2.5-flash".to_string()));
            assert!(params.contains(&"stream=true".to_string()));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_env_vars() {
    let cli = parse(&["run", "--task", "t", "--env", "FOO=bar", "--env", "BAZ=42"]).unwrap();
    match cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars.len(), 2);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_max_budget() {
    let cli = parse(&["run", "--task", "t", "--max-budget-usd", "10.5"]).unwrap();
    match cli.command {
        Commands::Run { max_budget_usd, .. } => {
            assert!((max_budget_usd.unwrap() - 10.5).abs() < f64::EPSILON);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_max_turns() {
    let cli = parse(&["run", "--task", "t", "--max-turns", "5"]).unwrap();
    match cli.command {
        Commands::Run { max_turns, .. } => assert_eq!(max_turns, Some(5)),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_out_path() {
    let cli = parse(&["run", "--task", "t", "--out", "/tmp/receipt.json"]).unwrap();
    match cli.command {
        Commands::Run { out, .. } => assert_eq!(out, Some(PathBuf::from("/tmp/receipt.json"))),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_json_flag() {
    let cli = parse(&["run", "--task", "t", "--json"]).unwrap();
    match cli.command {
        Commands::Run { json, .. } => assert!(json),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_json_flag_absent() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { json, .. } => assert!(!json),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_policy_path() {
    let cli = parse(&["run", "--task", "t", "--policy", "policy.json"]).unwrap();
    match cli.command {
        Commands::Run { policy, .. } => assert_eq!(policy, Some(PathBuf::from("policy.json"))),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_output_path() {
    let cli = parse(&["run", "--task", "t", "--output", "out.json"]).unwrap();
    match cli.command {
        Commands::Run { output, .. } => assert_eq!(output, Some(PathBuf::from("out.json"))),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_events_path() {
    let cli = parse(&["run", "--task", "t", "--events", "events.jsonl"]).unwrap();
    match cli.command {
        Commands::Run { events, .. } => {
            assert_eq!(events, Some(PathBuf::from("events.jsonl")));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_all_options_combined() {
    let cli = parse(&[
        "run",
        "--task",
        "do the thing",
        "--backend",
        "sidecar:claude",
        "--model",
        "claude-3",
        "--root",
        "/repo",
        "--workspace-mode",
        "pass-through",
        "--lane",
        "workspace-first",
        "--include",
        "*.rs",
        "--exclude",
        "target/**",
        "--param",
        "x=1",
        "--env",
        "A=B",
        "--max-budget-usd",
        "5.0",
        "--max-turns",
        "10",
        "--json",
        "--policy",
        "p.json",
        "--output",
        "receipt.json",
        "--events",
        "events.jsonl",
    ])
    .unwrap();
    match cli.command {
        Commands::Run {
            task,
            backend,
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
            json,
            policy,
            output,
            events,
            ..
        } => {
            assert_eq!(task, "do the thing");
            assert_eq!(backend.as_deref(), Some("sidecar:claude"));
            assert_eq!(model.as_deref(), Some("claude-3"));
            assert_eq!(root, "/repo");
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
            assert!(matches!(lane, LaneArg::WorkspaceFirst));
            assert_eq!(include, vec!["*.rs"]);
            assert_eq!(exclude, vec!["target/**"]);
            assert_eq!(params, vec!["x=1"]);
            assert_eq!(env_vars, vec!["A=B"]);
            assert!((max_budget_usd.unwrap() - 5.0).abs() < f64::EPSILON);
            assert_eq!(max_turns, Some(10));
            assert!(json);
            assert_eq!(policy, Some(PathBuf::from("p.json")));
            assert_eq!(output, Some(PathBuf::from("receipt.json")));
            assert_eq!(events, Some(PathBuf::from("events.jsonl")));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Backends subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_backends_subcommand() {
    let cli = parse(&["backends"]).unwrap();
    assert!(matches!(cli.command, Commands::Backends));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Validate subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_validate_subcommand() {
    let cli = parse(&["validate", "some/file.json"]).unwrap();
    match cli.command {
        Commands::Validate { file } => assert_eq!(file, PathBuf::from("some/file.json")),
        other => panic!("expected Validate, got {other:?}"),
    }
}

#[test]
fn parse_validate_requires_file_arg() {
    assert!(parse(&["validate"]).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Schema subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_schema_work_order() {
    let cli = parse(&["schema", "work-order"]).unwrap();
    match cli.command {
        Commands::Schema { kind } => assert!(matches!(kind, SchemaArg::WorkOrder)),
        other => panic!("expected Schema, got {other:?}"),
    }
}

#[test]
fn parse_schema_receipt() {
    let cli = parse(&["schema", "receipt"]).unwrap();
    match cli.command {
        Commands::Schema { kind } => assert!(matches!(kind, SchemaArg::Receipt)),
        other => panic!("expected Schema, got {other:?}"),
    }
}

#[test]
fn parse_schema_config() {
    let cli = parse(&["schema", "config"]).unwrap();
    match cli.command {
        Commands::Schema { kind } => assert!(matches!(kind, SchemaArg::Config)),
        other => panic!("expected Schema, got {other:?}"),
    }
}

#[test]
fn parse_schema_requires_kind_arg() {
    assert!(parse(&["schema"]).is_err());
}

#[test]
fn parse_schema_rejects_invalid_kind() {
    assert!(parse(&["schema", "invalid"]).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Inspect subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_inspect_subcommand() {
    let cli = parse(&["inspect", "receipt.json"]).unwrap();
    match cli.command {
        Commands::Inspect { file } => assert_eq!(file, PathBuf::from("receipt.json")),
        other => panic!("expected Inspect, got {other:?}"),
    }
}

#[test]
fn parse_inspect_requires_file() {
    assert!(parse(&["inspect"]).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Config subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_config_check() {
    let cli = parse(&["config", "check"]).unwrap();
    match cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert!(config.is_none()),
        other => panic!("expected ConfigCmd Check, got {other:?}"),
    }
}

#[test]
fn parse_config_check_with_path() {
    let cli = parse(&["config", "check", "--config", "my.toml"]).unwrap();
    match cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert_eq!(config, Some(PathBuf::from("my.toml"))),
        other => panic!("expected ConfigCmd Check, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Receipt subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_receipt_verify() {
    let cli = parse(&["receipt", "verify", "r.json"]).unwrap();
    match cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Verify { file },
        } => assert_eq!(file, PathBuf::from("r.json")),
        other => panic!("expected ReceiptCmd Verify, got {other:?}"),
    }
}

#[test]
fn parse_receipt_diff() {
    let cli = parse(&["receipt", "diff", "r1.json", "r2.json"]).unwrap();
    match cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Diff { file1, file2 },
        } => {
            assert_eq!(file1, PathBuf::from("r1.json"));
            assert_eq!(file2, PathBuf::from("r2.json"));
        }
        other => panic!("expected ReceiptCmd Diff, got {other:?}"),
    }
}

#[test]
fn parse_receipt_diff_requires_two_files() {
    assert!(parse(&["receipt", "diff", "r1.json"]).is_err());
}

#[test]
fn parse_receipt_verify_requires_file() {
    assert!(parse(&["receipt", "verify"]).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Debug flag and global config
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_flag_false_by_default() {
    let cli = parse(&["backends"]).unwrap();
    assert!(!cli.debug);
}

#[test]
fn debug_flag_set() {
    let cli = parse(&["--debug", "backends"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn global_config_path() {
    let cli = parse(&["--config", "myconfig.toml", "backends"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("myconfig.toml")));
}

#[test]
fn global_config_absent_by_default() {
    let cli = parse(&["backends"]).unwrap();
    assert!(cli.config.is_none());
}

#[test]
fn global_config_flag_works_with_run() {
    let cli = parse(&["--config", "c.toml", "run", "--task", "t"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("c.toml")));
}

#[test]
fn config_flag_after_subcommand() {
    let cli = parse(&["run", "--config", "c.toml", "--task", "t"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("c.toml")));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Error handling: invalid arguments
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn missing_subcommand_is_error() {
    assert!(parse(&[]).is_err());
}

#[test]
fn unknown_subcommand_is_error() {
    assert!(parse(&["foobar"]).is_err());
}

#[test]
fn run_missing_task_is_error() {
    assert!(parse(&["run"]).is_err());
}

#[test]
fn run_invalid_workspace_mode_is_error() {
    assert!(parse(&["run", "--task", "t", "--workspace-mode", "invalid"]).is_err());
}

#[test]
fn run_invalid_lane_is_error() {
    assert!(parse(&["run", "--task", "t", "--lane", "garbage"]).is_err());
}

#[test]
fn run_invalid_max_turns_not_a_number() {
    assert!(parse(&["run", "--task", "t", "--max-turns", "abc"]).is_err());
}

#[test]
fn run_invalid_max_budget_not_a_number() {
    assert!(parse(&["run", "--task", "t", "--max-budget-usd", "abc"]).is_err());
}

#[test]
fn unknown_flag_is_error() {
    assert!(parse(&["run", "--task", "t", "--nonexistent"]).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. WorkspaceModeArg → WorkspaceMode conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_mode_arg_pass_through_converts() {
    let mode: WorkspaceMode = WorkspaceModeArg::PassThrough.into();
    assert!(matches!(mode, WorkspaceMode::PassThrough));
}

#[test]
fn workspace_mode_arg_staged_converts() {
    let mode: WorkspaceMode = WorkspaceModeArg::Staged.into();
    assert!(matches!(mode, WorkspaceMode::Staged));
}

// ═══════════════════════════════════════════════════════════════════════
// 11. LaneArg → ExecutionLane conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lane_arg_patch_first_converts() {
    let lane: ExecutionLane = LaneArg::PatchFirst.into();
    assert!(matches!(lane, ExecutionLane::PatchFirst));
}

#[test]
fn lane_arg_workspace_first_converts() {
    let lane: ExecutionLane = LaneArg::WorkspaceFirst.into();
    assert!(matches!(lane, ExecutionLane::WorkspaceFirst));
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Schema generation (commands module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn schema_work_order_valid_json() {
    let s = commands::schema_json(SchemaKind::WorkOrder).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.is_object());
}

#[test]
fn schema_receipt_valid_json() {
    let s = commands::schema_json(SchemaKind::Receipt).unwrap();
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn schema_config_valid_json() {
    let s = commands::schema_json(SchemaKind::Config).unwrap();
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn schema_work_order_has_properties_or_defs() {
    let s = commands::schema_json(SchemaKind::WorkOrder).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.get("properties").is_some() || v.get("$defs").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Validate file (commands module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_file_detects_work_order() {
    let wo = WorkOrderBuilder::new("test task").build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();
    assert_eq!(
        commands::validate_file(&path).unwrap(),
        ValidatedType::WorkOrder
    );
}

#[test]
fn validate_file_detects_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    assert_eq!(
        commands::validate_file(&path).unwrap(),
        ValidatedType::Receipt
    );
}

#[test]
fn validate_file_rejects_unknown_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unknown.json");
    std::fs::write(&path, r#"{"foo":"bar"}"#).unwrap();
    assert!(commands::validate_file(&path).is_err());
}

#[test]
fn validate_file_rejects_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not json at all").unwrap();
    assert!(commands::validate_file(&path).is_err());
}

#[test]
fn validate_file_rejects_missing_file() {
    let path = std::path::Path::new("nonexistent_file_12345.json");
    assert!(commands::validate_file(path).is_err());
}

#[test]
fn validate_work_order_file_accepts_valid() {
    let wo = WorkOrderBuilder::new("do thing").build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("valid_wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();
    commands::validate_work_order_file(&path).unwrap();
}

#[test]
fn validate_work_order_file_rejects_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt_as_wo.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    assert!(commands::validate_work_order_file(&path).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Inspect receipt (commands module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn inspect_receipt_with_valid_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (r, valid) = commands::inspect_receipt_file(&path).unwrap();
    assert!(valid);
    assert_eq!(r.receipt_sha256, receipt.receipt_sha256);
}

#[test]
fn inspect_receipt_with_tampered_hash() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("0000000000000000".into());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tampered.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (_, valid) = commands::inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn inspect_receipt_with_no_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no_hash.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (_, valid) = commands::inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn inspect_receipt_missing_file() {
    assert!(commands::inspect_receipt_file(std::path::Path::new("nope.json")).is_err());
}

#[test]
fn verify_receipt_delegates_to_inspect() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    let (_, valid) = commands::verify_receipt_file(&path).unwrap();
    assert!(valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Receipt diff (commands module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_diff_identical_is_no_differences() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("r1.json");
    let p2 = dir.path().join("r2.json");
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    std::fs::write(&p1, &json).unwrap();
    std::fs::write(&p2, &json).unwrap();
    let diff = commands::receipt_diff(&p1, &p2).unwrap();
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
    let p1 = dir.path().join("r1.json");
    let p2 = dir.path().join("r2.json");
    std::fs::write(&p1, serde_json::to_string_pretty(&r1).unwrap()).unwrap();
    std::fs::write(&p2, serde_json::to_string_pretty(&r2).unwrap()).unwrap();
    let diff = commands::receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("outcome"));
}

#[test]
fn receipt_diff_different_backend() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("other")
        .outcome(Outcome::Complete)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("r1.json");
    let p2 = dir.path().join("r2.json");
    std::fs::write(&p1, serde_json::to_string_pretty(&r1).unwrap()).unwrap();
    std::fs::write(&p2, serde_json::to_string_pretty(&r2).unwrap()).unwrap();
    let diff = commands::receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("backend"));
}

#[test]
fn receipt_diff_missing_file_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("exists.json");
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    std::fs::write(&p1, serde_json::to_string_pretty(&r).unwrap()).unwrap();
    assert!(commands::receipt_diff(&p1, std::path::Path::new("nope.json")).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Config check (commands module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_check_defaults_contains_ok() {
    let diags = commands::config_check(None).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

#[test]
fn config_check_bad_file_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "not valid [toml =").unwrap();
    let diags = commands::config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.starts_with("error:")));
}

#[test]
fn config_check_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("valid.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();
    let diags = commands::config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. TOML config (abp_cli::config module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_default_has_no_backends() {
    let config = BackplaneConfig::default();
    assert!(config.backends.is_empty());
}

#[test]
fn config_default_default_backend_is_none() {
    let config = BackplaneConfig::default();
    assert!(config.default_backend.is_none());
}

#[test]
fn config_parse_example_toml() {
    let content = include_str!("../backplane.example.toml");
    let config: BackplaneConfig = toml::from_str(content).expect("parse example config");
    assert!(!config.backends.is_empty());
}

#[test]
fn config_serde_roundtrip_mock_backend() {
    let mut config = BackplaneConfig::default();
    config
        .backends
        .insert("mock".into(), BackendConfig::Mock {});
    let toml_str = toml::to_string(&config).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&toml_str).unwrap();
    assert!(deserialized.backends.contains_key("mock"));
}

#[test]
fn config_serde_roundtrip_sidecar_backend() {
    let mut config = BackplaneConfig::default();
    config.backends.insert(
        "node".into(),
        BackendConfig::Sidecar {
            command: "node".into(),
            args: vec!["host.js".into()],
            timeout_secs: Some(120),
        },
    );
    let toml_str = toml::to_string(&config).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&toml_str).unwrap();
    match &deserialized.backends["node"] {
        BackendConfig::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn config_validate_empty_command_invalid() {
    use abp_cli::config::ConfigError;
    let config = BackplaneConfig {
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
    let errs = abp_cli::config::validate_config(&config).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::InvalidBackend { .. }))
    );
}

#[test]
fn config_validate_zero_timeout_invalid() {
    use abp_cli::config::ConfigError;
    let config = BackplaneConfig {
        backends: HashMap::from([(
            "s".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        )]),
        ..Default::default()
    };
    let errs = abp_cli::config::validate_config(&config).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::InvalidTimeout { value: 0 }))
    );
}

#[test]
fn config_validate_valid_passes() {
    let config = BackplaneConfig {
        backends: HashMap::from([
            ("mock".into(), BackendConfig::Mock {}),
            (
                "sc".into(),
                BackendConfig::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(300),
                },
            ),
        ]),
        ..Default::default()
    };
    abp_cli::config::validate_config(&config).unwrap();
}

#[test]
fn config_merge_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let merged = abp_cli::config::merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
}

#[test]
fn config_merge_preserves_base_when_overlay_empty() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig::default();
    let merged = abp_cli::config::merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
}

#[test]
fn config_merge_combines_backends() {
    let mut base = BackplaneConfig::default();
    base.backends.insert("a".into(), BackendConfig::Mock {});
    let mut overlay = BackplaneConfig::default();
    overlay.backends.insert("b".into(), BackendConfig::Mock {});
    let merged = abp_cli::config::merge_configs(base, overlay);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

#[test]
fn config_load_from_toml_string() {
    let toml_str = r#"
default_backend = "mock"
[backends.mock]
type = "mock"
"#;
    let config: BackplaneConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("mock"));
    assert!(config.backends.contains_key("mock"));
}

#[test]
fn config_invalid_toml_is_error() {
    let result = toml::from_str::<BackplaneConfig>("not valid [toml =");
    assert!(result.is_err());
}

#[test]
fn config_error_display_invalid_backend() {
    use abp_cli::config::ConfigError;
    let e = ConfigError::InvalidBackend {
        name: "x".into(),
        reason: "bad".into(),
    };
    assert_eq!(e.to_string(), "invalid backend 'x': bad");
}

#[test]
fn config_error_display_invalid_timeout() {
    use abp_cli::config::ConfigError;
    let e = ConfigError::InvalidTimeout { value: 0 };
    assert!(e.to_string().contains("invalid timeout"));
}

#[test]
fn config_error_display_missing_field() {
    use abp_cli::config::ConfigError;
    let e = ConfigError::MissingRequiredField {
        field: "name".into(),
    };
    assert!(e.to_string().contains("missing required field"));
}

#[test]
fn config_load_file_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "claude"
[backends.mock]
type = "mock"
"#,
    )
    .unwrap();
    let config = abp_cli::config::load_config(Some(&path)).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("claude"));
}

#[test]
fn config_load_none_returns_default() {
    let config = abp_cli::config::load_config(None).unwrap();
    assert!(config.default_backend.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Output format (format module)
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
    assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
}

#[test]
fn output_format_from_str_json_pretty_variants() {
    assert_eq!(
        "json-pretty".parse::<OutputFormat>().unwrap(),
        OutputFormat::JsonPretty
    );
    assert_eq!(
        "json_pretty".parse::<OutputFormat>().unwrap(),
        OutputFormat::JsonPretty
    );
    assert_eq!(
        "jsonpretty".parse::<OutputFormat>().unwrap(),
        OutputFormat::JsonPretty
    );
}

#[test]
fn output_format_from_str_text() {
    assert_eq!("text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
}

#[test]
fn output_format_from_str_table() {
    assert_eq!(
        "table".parse::<OutputFormat>().unwrap(),
        OutputFormat::Table
    );
}

#[test]
fn output_format_from_str_compact() {
    assert_eq!(
        "compact".parse::<OutputFormat>().unwrap(),
        OutputFormat::Compact
    );
}

#[test]
fn output_format_from_str_case_insensitive() {
    assert_eq!("JSON".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
    assert_eq!("Text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
}

#[test]
fn output_format_from_str_unknown_is_error() {
    assert!("nope".parse::<OutputFormat>().is_err());
}

#[test]
fn output_format_display_roundtrips() {
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

// ═══════════════════════════════════════════════════════════════════════
// 19. Formatter - receipt formatting
// ═══════════════════════════════════════════════════════════════════════

fn sample_receipt() -> abp_core::Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

#[test]
fn format_receipt_json_is_valid() {
    let f = Formatter::new(OutputFormat::Json);
    let s = f.format_receipt(&sample_receipt());
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn format_receipt_json_pretty_is_valid() {
    let f = Formatter::new(OutputFormat::JsonPretty);
    let s = f.format_receipt(&sample_receipt());
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn format_receipt_text_contains_outcome() {
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_receipt(&sample_receipt());
    assert!(s.contains("complete"));
    assert!(s.contains("mock"));
}

#[test]
fn format_receipt_table_contains_backend() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_receipt(&sample_receipt());
    assert!(s.contains("mock"));
    assert!(s.contains("outcome"));
}

#[test]
fn format_receipt_compact_is_single_line() {
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_receipt(&sample_receipt());
    assert!(!s.contains('\n'));
    assert!(s.contains("complete"));
    assert!(s.contains("mock"));
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Formatter - event formatting
// ═══════════════════════════════════════════════════════════════════════

fn sample_event() -> AgentEvent {
    AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "run started".into(),
        },
        ext: None,
    }
}

#[test]
fn format_event_json_is_valid() {
    let f = Formatter::new(OutputFormat::Json);
    let s = f.format_event(&sample_event());
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn format_event_text_contains_tag() {
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_event(&sample_event());
    assert!(s.contains("run_started"));
}

#[test]
fn format_event_compact_contains_tag() {
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_event(&sample_event());
    assert!(s.contains("run_started"));
}

#[test]
fn format_event_table_contains_tag() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_event(&sample_event());
    assert!(s.contains("run_started"));
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Formatter - work order formatting
// ═══════════════════════════════════════════════════════════════════════

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

#[test]
fn format_work_order_json_is_valid() {
    let f = Formatter::new(OutputFormat::Json);
    let s = f.format_work_order(&sample_work_order());
    let _: serde_json::Value = serde_json::from_str(&s).unwrap();
}

#[test]
fn format_work_order_text_contains_task() {
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_work_order(&sample_work_order());
    assert!(s.contains("test task"));
}

#[test]
fn format_work_order_table_contains_lane() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_work_order(&sample_work_order());
    assert!(s.contains("patch_first"));
}

#[test]
fn format_work_order_compact_contains_task() {
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_work_order(&sample_work_order());
    assert!(s.contains("test task"));
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Formatter - error formatting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn format_error_json_contains_error_key() {
    let f = Formatter::new(OutputFormat::Json);
    let s = f.format_error("boom");
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["error"], "boom");
}

#[test]
fn format_error_text_contains_message() {
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_error("boom");
    assert!(s.contains("Error:"));
    assert!(s.contains("boom"));
}

#[test]
fn format_error_table_contains_message() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_error("boom");
    assert!(s.contains("error"));
    assert!(s.contains("boom"));
}

#[test]
fn format_error_compact_contains_message() {
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_error("boom");
    assert!(s.contains("[error]"));
    assert!(s.contains("boom"));
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Event kind formatting for various event types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn format_event_assistant_delta() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "hello world".into(),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_event(&ev);
    assert!(s.contains("assistant_delta"));
}

#[test]
fn format_event_assistant_message() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "message text".into(),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_event(&ev);
    assert!(s.contains("assistant_message"));
}

#[test]
fn format_event_tool_call() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("id1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls"}),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_event(&ev);
    assert!(s.contains("tool_call"));
    assert!(s.contains("bash"));
}

#[test]
fn format_event_tool_result() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("id1".into()),
            output: "ok".into(),
            is_error: false,
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_event(&ev);
    assert!(s.contains("tool_result"));
}

#[test]
fn format_event_tool_result_error() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: "fail".into(),
            is_error: true,
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_event(&ev);
    assert!(s.contains("(error)"));
}

#[test]
fn format_event_file_changed() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_event(&ev);
    assert!(s.contains("file_changed"));
    assert!(s.contains("src/main.rs"));
}

#[test]
fn format_event_command_executed() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Text);
    let s = f.format_event(&ev);
    assert!(s.contains("command_executed"));
}

#[test]
fn format_event_warning() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Warning {
            message: "something sus".into(),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_event(&ev);
    assert!(s.contains("warning"));
}

#[test]
fn format_event_error() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Error {
            message: "bad thing".into(),
            error_code: None,
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Compact);
    let s = f.format_event(&ev);
    assert!(s.contains("[error]"));
}

#[test]
fn format_event_run_completed() {
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_event(&ev);
    assert!(s.contains("run_completed"));
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Contract version used by builders
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn receipt_builder_uses_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn work_order_builder_with_model() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_with_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(5.0).build();
    assert!((wo.config.max_budget_usd.unwrap() - 5.0).abs() < f64::EPSILON);
}

#[test]
fn work_order_builder_with_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn work_order_builder_with_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_builder_with_root() {
    let wo = WorkOrderBuilder::new("t").root("/my/project").build();
    assert_eq!(wo.workspace.root, "/my/project");
}

#[test]
fn work_order_builder_with_workspace_mode() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn work_order_builder_with_include_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Receipt hashing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_hash_produces_sha256() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
}

#[test]
fn receipt_hash_is_deterministic() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_nulls_sha256_before_hashing() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    receipt.receipt_sha256 = Some("garbage".into());
    let hash = abp_core::receipt_hash(&receipt).unwrap();

    receipt.receipt_sha256 = None;
    let hash2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash, hash2);
}

// ═══════════════════════════════════════════════════════════════════════
// 26. ErrorCode serde is snake_case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_code_serializes_snake_case() {
    use abp_core::error::ErrorCode;
    let code = ErrorCode::InternalError;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""internal_error""#);
}

#[test]
fn error_code_serializes_backend_unavailable_snake() {
    use abp_core::error::ErrorCode;
    let code = ErrorCode::BackendUnavailable;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""backend_unavailable""#);
}

#[test]
fn error_code_deserializes_from_snake_case() {
    use abp_core::error::ErrorCode;
    let code: ErrorCode = serde_json::from_str(r#""internal_error""#).unwrap();
    assert_eq!(code, ErrorCode::InternalError);
}

#[test]
fn error_code_display_shows_abp_code() {
    use abp_core::error::ErrorCode;
    assert_eq!(ErrorCode::InternalError.to_string(), "ABP-S003");
    assert_eq!(ErrorCode::BackendUnavailable.to_string(), "ABP-R001");
}

#[test]
fn error_code_code_method() {
    use abp_core::error::ErrorCode;
    assert_eq!(ErrorCode::InvalidContractVersion.code(), "ABP-C001");
    assert_eq!(ErrorCode::InvalidEnvelope.code(), "ABP-P001");
    assert_eq!(ErrorCode::ToolDenied.code(), "ABP-L001");
    assert_eq!(ErrorCode::BackendUnavailable.code(), "ABP-R001");
    assert_eq!(ErrorCode::IoError.code(), "ABP-S001");
}

#[test]
fn error_code_category() {
    use abp_core::error::ErrorCode;
    assert_eq!(ErrorCode::InvalidContractVersion.category(), "contract");
    assert_eq!(ErrorCode::InvalidEnvelope.category(), "protocol");
    assert_eq!(ErrorCode::ToolDenied.category(), "policy");
    assert_eq!(ErrorCode::BackendUnavailable.category(), "runtime");
    assert_eq!(ErrorCode::IoError.category(), "system");
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Outcome serde
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn outcome_serde_complete() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert!(json.contains("complete"));
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Complete);
}

#[test]
fn outcome_serde_partial() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    assert!(json.contains("partial"));
}

#[test]
fn outcome_serde_failed() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    assert!(json.contains("failed"));
}

// ═══════════════════════════════════════════════════════════════════════
// 28. Backend selection / runtime basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_with_default_backends_has_mock() {
    let rt = abp_runtime::Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[test]
fn runtime_register_custom_backend() {
    let mut rt = abp_runtime::Runtime::new();
    rt.register_backend("custom_mock", abp_integrations::MockBackend);
    let names = rt.backend_names();
    assert!(names.contains(&"custom_mock".to_string()));
}

#[test]
fn runtime_backend_names_empty_for_new() {
    let rt = abp_runtime::Runtime::new();
    assert!(rt.backend_names().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Sidecar registration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn register_sidecar_backend() {
    let mut rt = abp_runtime::Runtime::new();
    let spec = abp_host::SidecarSpec::new("node");
    rt.register_backend("sidecar:test", abp_integrations::SidecarBackend::new(spec));
    assert!(rt.backend_names().contains(&"sidecar:test".to_string()));
}

#[test]
fn register_multiple_sidecar_backends() {
    let mut rt = abp_runtime::Runtime::new();
    let spec1 = abp_host::SidecarSpec::new("node");
    let spec2 = abp_host::SidecarSpec::new("python3");
    rt.register_backend("sidecar:node", abp_integrations::SidecarBackend::new(spec1));
    rt.register_backend(
        "sidecar:python",
        abp_integrations::SidecarBackend::new(spec2),
    );
    let names = rt.backend_names();
    assert!(names.contains(&"sidecar:node".to_string()));
    assert!(names.contains(&"sidecar:python".to_string()));
}

#[test]
fn sidecar_spec_new() {
    let spec = abp_host::SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
}

#[test]
fn sidecar_spec_with_args() {
    let mut spec = abp_host::SidecarSpec::new("node");
    spec.args = vec!["host.js".into(), "--verbose".into()];
    assert_eq!(spec.args.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 30. Edge cases and misc
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_run_empty_task_string_succeeds() {
    let cli = parse(&["run", "--task", ""]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert_eq!(task, ""),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_task_with_special_chars() {
    let cli = parse(&[
        "run",
        "--task",
        "fix bug #123: handle \"quotes\" & <brackets>",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert!(task.contains("#123")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_multiple_include_globs() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--include",
        "a",
        "--include",
        "b",
        "--include",
        "c",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { include, .. } => assert_eq!(include.len(), 3),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_multiple_exclude_globs() {
    let cli = parse(&["run", "--task", "t", "--exclude", "x", "--exclude", "y"]).unwrap();
    match cli.command {
        Commands::Run { exclude, .. } => assert_eq!(exclude.len(), 2),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_empty_include_exclude() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run {
            include, exclude, ..
        } => {
            assert!(include.is_empty());
            assert!(exclude.is_empty());
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_params() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { params, .. } => assert!(params.is_empty()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_env_vars() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { env_vars, .. } => assert!(env_vars.is_empty()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_max_budget() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { max_budget_usd, .. } => assert!(max_budget_usd.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_max_turns() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { max_turns, .. } => assert!(max_turns.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_policy() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { policy, .. } => assert!(policy.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_output() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { output, .. } => assert!(output.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_events() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { events, .. } => assert!(events.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_out() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { out, .. } => assert!(out.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_no_model() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { model, .. } => assert!(model.is_none()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_backend_alias_copilot() {
    let cli = parse(&["run", "--task", "t", "--backend", "copilot"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("copilot")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_backend_alias_gemini() {
    let cli = parse(&["run", "--task", "t", "--backend", "gemini"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("gemini")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn parse_run_backend_codex() {
    let cli = parse(&["run", "--task", "t", "--backend", "codex"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("codex")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn format_receipt_json_pretty_has_newlines() {
    let f = Formatter::new(OutputFormat::JsonPretty);
    let s = f.format_receipt(&sample_receipt());
    assert!(s.contains('\n'));
}

#[test]
fn format_receipt_table_has_run_id() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_receipt(&sample_receipt());
    assert!(s.contains("run_id"));
}

#[test]
fn format_work_order_json_pretty_has_newlines() {
    let f = Formatter::new(OutputFormat::JsonPretty);
    let s = f.format_work_order(&sample_work_order());
    assert!(s.contains('\n'));
}

#[test]
fn format_work_order_table_has_root() {
    let f = Formatter::new(OutputFormat::Table);
    let s = f.format_work_order(&sample_work_order());
    assert!(s.contains("root"));
}

#[test]
fn config_receipts_dir_roundtrip() {
    let mut config = BackplaneConfig::default();
    config.receipts_dir = Some("/tmp/receipts".into());
    let s = toml::to_string(&config).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(back.receipts_dir.as_deref(), Some("/tmp/receipts"));
}

#[test]
fn config_log_level_roundtrip() {
    let mut config = BackplaneConfig::default();
    config.log_level = Some("debug".into());
    let s = toml::to_string(&config).unwrap();
    let back: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(back.log_level.as_deref(), Some("debug"));
}

// ═══════════════════════════════════════════════════════════════════════
// 31. Receipt builder outcomes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_builder_complete() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.backend.id, "mock");
}

#[test]
fn receipt_builder_partial() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_builder_failed() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_default_outcome_is_complete() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.outcome, Outcome::Complete);
}
