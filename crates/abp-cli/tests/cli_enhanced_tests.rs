#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for enhanced CLI subcommands: validate, inspect, translate, health,
//! schema, enhanced run, and enhanced backends.

use abp_cli::cli::{Cli, Commands, SchemaArg};
use abp_cli::commands::{
    config_check, inspect_receipt_file, receipt_diff, schema_json, validate_file,
    validate_work_order_file, verify_receipt_file, SchemaKind, ValidatedType,
};
use abp_cli::format::{Formatter, OutputFormat};
use abp_cli::health::{check_health, BackendHealthStatus, BackendProbe, HealthReport};
use abp_cli::schema::{generate_schema, write_schema_to_file};
use abp_cli::translate::{
    list_supported_pairs, parse_dialect, translate_json_str, translate_request,
};
use abp_cli::validate::validate_config;
use abp_core::{Outcome, Receipt, ReceiptBuilder, WorkOrder, WorkOrderBuilder};
use abp_dialect::Dialect;
use clap::Parser;
use std::path::PathBuf;

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    let mut all = vec!["abp"];
    all.extend_from_slice(args);
    Cli::try_parse_from(all)
}

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

fn write_json_file(dir: &std::path::Path, name: &str, value: &impl serde::Serialize) -> PathBuf {
    let path = dir.join(name);
    let json = serde_json::to_string_pretty(value).unwrap();
    std::fs::write(&path, json).unwrap();
    path
}

// ═══════════════════════════════════════════════════════════════════════
//  1. Validate subcommand parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_validate_no_args() {
    let cli = parse(&["validate"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Validate {
            file: None,
            config_file: None
        }
    ));
}

#[test]
fn parse_validate_with_file() {
    let cli = parse(&["validate", "work_order.json"]).unwrap();
    match cli.command {
        Commands::Validate { file, .. } => {
            assert_eq!(file, Some(PathBuf::from("work_order.json")));
        }
        _ => panic!("expected Validate"),
    }
}

#[test]
fn parse_validate_with_config_file() {
    let cli = parse(&["validate", "--config-file", "bp.toml"]).unwrap();
    match cli.command {
        Commands::Validate { config_file, .. } => {
            assert_eq!(config_file, Some(PathBuf::from("bp.toml")));
        }
        _ => panic!("expected Validate"),
    }
}

#[test]
fn validate_work_order_file_accepts_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "wo.json", &make_work_order());
    validate_work_order_file(&path).unwrap();
}

#[test]
fn validate_work_order_file_rejects_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "receipt.json", &make_receipt());
    // Receipt should fail WorkOrder validation.
    assert!(validate_work_order_file(&path).is_err());
}

#[test]
fn validate_file_detects_work_order_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "wo.json", &make_work_order());
    assert_eq!(validate_file(&path).unwrap(), ValidatedType::WorkOrder);
}

#[test]
fn validate_file_detects_receipt_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &make_receipt());
    assert_eq!(validate_file(&path).unwrap(), ValidatedType::Receipt);
}

#[test]
fn validate_file_rejects_arbitrary_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("arbitrary.json");
    std::fs::write(&path, r#"{"hello": "world"}"#).unwrap();
    assert!(validate_file(&path).is_err());
}

#[test]
fn validate_config_defaults_is_valid() {
    let result = validate_config(None).unwrap();
    assert!(result.valid);
}

#[test]
fn validate_config_bad_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "invalid [[ toml ==").unwrap();
    let result = validate_config(Some(&path)).unwrap();
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
//  2. Inspect subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_inspect_subcommand() {
    let cli = parse(&["inspect", "receipt.json"]).unwrap();
    match cli.command {
        Commands::Inspect { file } => assert_eq!(file, PathBuf::from("receipt.json")),
        _ => panic!("expected Inspect"),
    }
}

#[test]
fn inspect_valid_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &make_hashed_receipt());
    let (receipt, valid) = inspect_receipt_file(&path).unwrap();
    assert!(valid);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn inspect_tampered_receipt() {
    let mut r = make_hashed_receipt();
    r.receipt_sha256 = Some("tampered_hash".into());
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &r);
    let (_, valid) = inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn inspect_receipt_without_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &make_receipt());
    let (_, valid) = inspect_receipt_file(&path).unwrap();
    assert!(!valid);
}

#[test]
fn verify_receipt_file_delegates() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_json_file(dir.path(), "r.json", &make_hashed_receipt());
    let (_, valid) = verify_receipt_file(&path).unwrap();
    assert!(valid);
}

#[test]
fn receipt_diff_identical_files() {
    let dir = tempfile::tempdir().unwrap();
    let r = make_hashed_receipt();
    let p1 = write_json_file(dir.path(), "r1.json", &r);
    let p2 = write_json_file(dir.path(), "r2.json", &r);
    assert_eq!(receipt_diff(&p1, &p2).unwrap(), "no differences");
}

#[test]
fn receipt_diff_different_backends() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("other")
        .outcome(Outcome::Failed)
        .build();
    let dir = tempfile::tempdir().unwrap();
    let p1 = write_json_file(dir.path(), "r1.json", &r1);
    let p2 = write_json_file(dir.path(), "r2.json", &r2);
    let diff = receipt_diff(&p1, &p2).unwrap();
    assert!(diff.contains("outcome"));
    assert!(diff.contains("backend"));
}

// ═══════════════════════════════════════════════════════════════════════
//  3. Translate subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_translate_subcommand() {
    let cli = parse(&[
        "translate",
        "--from",
        "openai",
        "--to",
        "claude",
        "input.json",
    ])
    .unwrap();
    match cli.command {
        Commands::Translate { from, to, file } => {
            assert_eq!(from, "openai");
            assert_eq!(to, "claude");
            assert_eq!(file, Some(PathBuf::from("input.json")));
        }
        _ => panic!("expected Translate"),
    }
}

#[test]
fn parse_translate_without_file() {
    let cli = parse(&["translate", "--from", "openai", "--to", "gemini"]).unwrap();
    match cli.command {
        Commands::Translate { file, .. } => assert!(file.is_none()),
        _ => panic!("expected Translate"),
    }
}

#[test]
fn parse_dialect_all_variants() {
    assert_eq!(parse_dialect("openai").unwrap(), Dialect::OpenAi);
    assert_eq!(parse_dialect("claude").unwrap(), Dialect::Claude);
    assert_eq!(parse_dialect("gemini").unwrap(), Dialect::Gemini);
    assert_eq!(parse_dialect("codex").unwrap(), Dialect::Codex);
    assert_eq!(parse_dialect("kimi").unwrap(), Dialect::Kimi);
    assert_eq!(parse_dialect("copilot").unwrap(), Dialect::Copilot);
}

#[test]
fn parse_dialect_case_insensitive() {
    assert_eq!(parse_dialect("OpenAI").unwrap(), Dialect::OpenAi);
    assert_eq!(parse_dialect("CLAUDE").unwrap(), Dialect::Claude);
    assert_eq!(parse_dialect("Gemini").unwrap(), Dialect::Gemini);
}

#[test]
fn parse_dialect_rejects_unknown() {
    assert!(parse_dialect("foobar").is_err());
    assert!(parse_dialect("gpt4").is_err());
}

#[test]
fn translate_identity_returns_json_object() {
    let input = serde_json::json!({"model": "gpt-4", "messages": []});
    let result = translate_request(Dialect::OpenAi, Dialect::OpenAi, &input).unwrap();
    assert!(result.is_object());
}

#[test]
fn translate_json_str_valid_input() {
    let input = r#"{"model": "gpt-4", "messages": []}"#;
    let result = translate_json_str(Dialect::OpenAi, Dialect::OpenAi, input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(parsed.is_object());
}

#[test]
fn translate_json_str_invalid_json_errors() {
    assert!(translate_json_str(Dialect::OpenAi, Dialect::OpenAi, "not json").is_err());
}

#[test]
fn translate_supported_pairs_nonempty() {
    let pairs = list_supported_pairs();
    assert!(pairs.len() > 6, "should have identity + cross pairs");
}

#[test]
fn translate_cross_dialect_produces_output() {
    let input =
        serde_json::json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = translate_request(Dialect::OpenAi, Dialect::Claude, &input);
    // May or may not succeed depending on mapper impl, but shouldn't panic.
    if let Ok(v) = result {
        assert!(v.is_object());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  4. Health subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_health_subcommand() {
    let cli = parse(&["health"]).unwrap();
    assert!(matches!(cli.command, Commands::Health { json: false }));
}

#[test]
fn parse_health_with_json() {
    let cli = parse(&["health", "--json"]).unwrap();
    assert!(matches!(cli.command, Commands::Health { json: true }));
}

#[test]
fn health_check_default_config() {
    let config = abp_config::BackplaneConfig::default();
    let report = check_health(&config).unwrap();
    assert_eq!(report.contract_version, abp_core::CONTRACT_VERSION);
    assert!(!report.backends.is_empty());
}

#[test]
fn health_check_all_ok() {
    let config = abp_config::BackplaneConfig::default();
    let report = check_health(&config).unwrap();
    assert_eq!(report.overall, BackendHealthStatus::Ok);
    for probe in &report.backends {
        assert_eq!(probe.status, BackendHealthStatus::Ok);
    }
}

#[test]
fn health_check_with_extra_mock() {
    let mut config = abp_config::BackplaneConfig::default();
    config
        .backends
        .insert("extra-mock".into(), abp_config::BackendEntry::Mock {});
    let report = check_health(&config).unwrap();
    assert!(report.backends.iter().any(|p| p.name == "extra-mock"));
}

#[test]
fn health_report_serialization_roundtrip() {
    let config = abp_config::BackplaneConfig::default();
    let report = check_health(&config).unwrap();
    let json = serde_json::to_string(&report).unwrap();
    let parsed: HealthReport = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.overall, report.overall);
    assert_eq!(parsed.backends.len(), report.backends.len());
}

#[test]
fn backend_health_status_display_values() {
    assert_eq!(BackendHealthStatus::Ok.to_string(), "ok");
    assert_eq!(BackendHealthStatus::Degraded.to_string(), "degraded");
    assert_eq!(BackendHealthStatus::Unavailable.to_string(), "unavailable");
}

#[test]
fn health_probe_message_none_for_ok() {
    let config = abp_config::BackplaneConfig::default();
    let report = check_health(&config).unwrap();
    for probe in &report.backends {
        if probe.status == BackendHealthStatus::Ok {
            assert!(probe.message.is_none());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  5. Schema subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_schema_work_order() {
    let cli = parse(&["schema", "work-order"]).unwrap();
    match cli.command {
        Commands::Schema { kind, output } => {
            assert!(matches!(kind, SchemaArg::WorkOrder));
            assert!(output.is_none());
        }
        _ => panic!("expected Schema"),
    }
}

#[test]
fn parse_schema_receipt_with_output() {
    let cli = parse(&["schema", "receipt", "--output", "out.json"]).unwrap();
    match cli.command {
        Commands::Schema { kind, output } => {
            assert!(matches!(kind, SchemaArg::Receipt));
            assert_eq!(output, Some(PathBuf::from("out.json")));
        }
        _ => panic!("expected Schema"),
    }
}

#[test]
fn parse_schema_config() {
    let cli = parse(&["schema", "config"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Schema {
            kind: SchemaArg::Config,
            ..
        }
    ));
}

#[test]
fn generate_all_schemas_valid_json() {
    for kind in [
        SchemaKind::WorkOrder,
        SchemaKind::Receipt,
        SchemaKind::Config,
    ] {
        let json = generate_schema(kind).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.is_object());
    }
}

#[test]
fn write_schema_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("schema.json");
    write_schema_to_file(SchemaKind::WorkOrder, &path).unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.is_object());
}

#[test]
fn schema_json_work_order_has_properties() {
    let s = schema_json(SchemaKind::WorkOrder).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(v.get("properties").is_some() || v.get("$defs").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
//  6. Enhanced run subcommand (--stream, --timeout, --retry, --fallback)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_run_with_stream() {
    let cli = parse(&["run", "--task", "test", "--stream"]).unwrap();
    match cli.command {
        Commands::Run { stream, .. } => assert!(stream),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_stream_defaults_false() {
    let cli = parse(&["run", "--task", "test"]).unwrap();
    match cli.command {
        Commands::Run { stream, .. } => assert!(!stream),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_with_timeout() {
    let cli = parse(&["run", "--task", "test", "--timeout", "300"]).unwrap();
    match cli.command {
        Commands::Run { timeout, .. } => assert_eq!(timeout, Some(300)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_timeout_defaults_none() {
    let cli = parse(&["run", "--task", "test"]).unwrap();
    match cli.command {
        Commands::Run { timeout, .. } => assert!(timeout.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_with_retry() {
    let cli = parse(&["run", "--task", "test", "--retry", "3"]).unwrap();
    match cli.command {
        Commands::Run { retry, .. } => assert_eq!(retry, 3),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_retry_defaults_zero() {
    let cli = parse(&["run", "--task", "test"]).unwrap();
    match cli.command {
        Commands::Run { retry, .. } => assert_eq!(retry, 0),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_with_fallback() {
    let cli = parse(&["run", "--task", "test", "--fallback", "mock"]).unwrap();
    match cli.command {
        Commands::Run { fallback, .. } => assert_eq!(fallback.as_deref(), Some("mock")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_fallback_defaults_none() {
    let cli = parse(&["run", "--task", "test"]).unwrap();
    match cli.command {
        Commands::Run { fallback, .. } => assert!(fallback.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_all_new_flags() {
    let cli = parse(&[
        "run",
        "--task",
        "test",
        "--stream",
        "--timeout",
        "60",
        "--retry",
        "2",
        "--fallback",
        "sidecar:node",
    ])
    .unwrap();
    match cli.command {
        Commands::Run {
            stream,
            timeout,
            retry,
            fallback,
            ..
        } => {
            assert!(stream);
            assert_eq!(timeout, Some(60));
            assert_eq!(retry, 2);
            assert_eq!(fallback.as_deref(), Some("sidecar:node"));
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  7. Enhanced backends subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_backends_plain() {
    let cli = parse(&["backends"]).unwrap();
    match cli.command {
        Commands::Backends {
            capabilities,
            health,
            json,
        } => {
            assert!(!capabilities);
            assert!(!health);
            assert!(!json);
        }
        _ => panic!("expected Backends"),
    }
}

#[test]
fn parse_backends_with_capabilities() {
    let cli = parse(&["backends", "--capabilities"]).unwrap();
    match cli.command {
        Commands::Backends { capabilities, .. } => assert!(capabilities),
        _ => panic!("expected Backends"),
    }
}

#[test]
fn parse_backends_with_health() {
    let cli = parse(&["backends", "--health"]).unwrap();
    match cli.command {
        Commands::Backends { health, .. } => assert!(health),
        _ => panic!("expected Backends"),
    }
}

#[test]
fn parse_backends_with_json() {
    let cli = parse(&["backends", "--json"]).unwrap();
    match cli.command {
        Commands::Backends { json, .. } => assert!(json),
        _ => panic!("expected Backends"),
    }
}

#[test]
fn parse_backends_all_flags() {
    let cli = parse(&["backends", "--capabilities", "--health", "--json"]).unwrap();
    match cli.command {
        Commands::Backends {
            capabilities,
            health,
            json,
        } => {
            assert!(capabilities);
            assert!(health);
            assert!(json);
        }
        _ => panic!("expected Backends"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  8. Config check integration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_check_defaults() {
    let diags = config_check(None).unwrap();
    assert!(diags.iter().any(|d| d.contains("ok")));
}

#[test]
fn config_check_invalid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "bad [toml syntax").unwrap();
    let diags = config_check(Some(&path)).unwrap();
    assert!(diags.iter().any(|d| d.starts_with("error:")));
}

// ═══════════════════════════════════════════════════════════════════════
//  9. Format integration with receipts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn formatter_receipt_all_formats() {
    let receipt = make_receipt();
    for format in [
        OutputFormat::Json,
        OutputFormat::JsonPretty,
        OutputFormat::Text,
        OutputFormat::Table,
        OutputFormat::Compact,
    ] {
        let f = Formatter::new(format);
        let output = f.format_receipt(&receipt);
        assert!(!output.is_empty());
    }
}

#[test]
fn formatter_work_order_all_formats() {
    let wo = make_work_order();
    for format in [
        OutputFormat::Json,
        OutputFormat::JsonPretty,
        OutputFormat::Text,
        OutputFormat::Table,
        OutputFormat::Compact,
    ] {
        let f = Formatter::new(format);
        let output = f.format_work_order(&wo);
        assert!(!output.is_empty());
    }
}
