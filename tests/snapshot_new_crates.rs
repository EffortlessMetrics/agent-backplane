// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot tests for newer ABP crates: emulation, telemetry, dialect, error, daemon.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport,
};
use abp_daemon::api::{
    ApiError, ApiRequest, ApiResponse, BackendDetail, HealthResponse, RunInfo,
    RunStatus as ApiRunStatus,
};
use abp_daemon::{BackendInfo, RunMetrics as DaemonRunMetrics, RunStatus};
use abp_dialect::Dialect;
use abp_emulation::{EmulationConfig, EmulationEntry, EmulationReport, EmulationStrategy};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_telemetry::{MetricsCollector, MetricsSummary, RunMetrics, TelemetrySpan};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn sample_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps
}

fn sample_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "sidecar:test".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 100, "output_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.001),
        },
        trace: vec![AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "file".into(),
            path: "output.txt".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file.txt".into()),
            git_status: Some("M file.txt".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. abp-emulation snapshots
// ===========================================================================

#[test]
fn emulation_strategy_system_prompt_injection() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "Think step by step before answering.".into(),
    };
    insta::assert_json_snapshot!(s);
}

#[test]
fn emulation_strategy_post_processing() {
    let s = EmulationStrategy::PostProcessing {
        detail: "Parse and validate JSON from text response".into(),
    };
    insta::assert_json_snapshot!(s);
}

#[test]
fn emulation_strategy_disabled() {
    let s = EmulationStrategy::Disabled {
        reason: "Cannot safely emulate sandboxed code execution".into(),
    };
    insta::assert_json_snapshot!(s);
}

#[test]
fn emulation_config_empty() {
    let config = EmulationConfig::new();
    insta::assert_json_snapshot!(config);
}

#[test]
fn emulation_config_with_multiple_strategies() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );
    config.set(
        Capability::StructuredOutputJsonSchema,
        EmulationStrategy::PostProcessing {
            detail: "Validate JSON output".into(),
        },
    );
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    let json_str = serde_json::to_string_pretty(&config).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn emulation_entry_snapshot() {
    let entry = EmulationEntry {
        capability: Capability::ExtendedThinking,
        strategy: EmulationStrategy::SystemPromptInjection {
            prompt: "Think carefully.".into(),
        },
    };
    insta::assert_json_snapshot!(entry);
}

#[test]
fn emulation_report_empty() {
    let report = EmulationReport::default();
    insta::assert_json_snapshot!(report);
}

#[test]
fn emulation_report_with_applied_and_warnings() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think step by step.".into(),
                },
            },
            EmulationEntry {
                capability: Capability::StructuredOutputJsonSchema,
                strategy: EmulationStrategy::PostProcessing {
                    detail: "Validate JSON".into(),
                },
            },
        ],
        warnings: vec![
            "Capability CodeExecution not emulated: unsafe".into(),
            "Capability Streaming not emulated: no emulation available".into(),
        ],
    };
    insta::assert_json_snapshot!(report);
}

// ===========================================================================
// 2. abp-telemetry snapshots
// ===========================================================================

#[test]
fn telemetry_run_metrics_default() {
    let m = RunMetrics::default();
    insta::assert_json_snapshot!(m);
}

#[test]
fn telemetry_run_metrics_populated() {
    let m = RunMetrics {
        backend_name: "sidecar:claude".into(),
        dialect: "claude".into(),
        duration_ms: 4500,
        events_count: 12,
        tokens_in: 1500,
        tokens_out: 3200,
        tool_calls_count: 5,
        errors_count: 1,
        emulations_applied: 2,
    };
    insta::assert_json_snapshot!(m);
}

#[test]
fn telemetry_metrics_summary_default() {
    let s = MetricsSummary::default();
    insta::assert_json_snapshot!(s);
}

#[test]
fn telemetry_metrics_summary_aggregated() {
    let c = MetricsCollector::new();
    c.record(RunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 100,
        events_count: 5,
        tokens_in: 200,
        tokens_out: 400,
        tool_calls_count: 2,
        errors_count: 0,
        emulations_applied: 0,
    });
    c.record(RunMetrics {
        backend_name: "sidecar:claude".into(),
        dialect: "claude".into(),
        duration_ms: 300,
        events_count: 8,
        tokens_in: 500,
        tokens_out: 1000,
        tool_calls_count: 3,
        errors_count: 1,
        emulations_applied: 1,
    });
    c.record(RunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 200,
        events_count: 3,
        tokens_in: 150,
        tokens_out: 300,
        tool_calls_count: 1,
        errors_count: 0,
        emulations_applied: 0,
    });
    let s = c.summary();
    insta::assert_json_snapshot!(s);
}

#[test]
fn telemetry_span_minimal() {
    let span = TelemetrySpan::new("agent.run");
    insta::assert_json_snapshot!(span);
}

#[test]
fn telemetry_span_with_attributes() {
    let span = TelemetrySpan::new("agent.run")
        .with_attribute("backend", "sidecar:claude")
        .with_attribute("dialect", "claude")
        .with_attribute("run_id", "00000000-0000-4000-8000-000000000001");
    insta::assert_json_snapshot!(span);
}

// ===========================================================================
// 3. abp-dialect snapshots
// ===========================================================================

#[test]
fn dialect_all_variants() {
    let dialects: Vec<Dialect> = Dialect::all().to_vec();
    insta::assert_json_snapshot!(dialects);
}

#[test]
fn dialect_openai() {
    insta::assert_json_snapshot!(Dialect::OpenAi);
}

#[test]
fn dialect_claude() {
    insta::assert_json_snapshot!(Dialect::Claude);
}

#[test]
fn dialect_gemini() {
    insta::assert_json_snapshot!(Dialect::Gemini);
}

#[test]
fn dialect_codex() {
    insta::assert_json_snapshot!(Dialect::Codex);
}

#[test]
fn dialect_kimi() {
    insta::assert_json_snapshot!(Dialect::Kimi);
}

#[test]
fn dialect_copilot() {
    insta::assert_json_snapshot!(Dialect::Copilot);
}

// ===========================================================================
// 4. abp-error snapshots
// ===========================================================================

#[test]
fn error_code_protocol_invalid_envelope() {
    insta::assert_json_snapshot!(ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn error_code_backend_timeout() {
    insta::assert_json_snapshot!(ErrorCode::BackendTimeout);
}

#[test]
fn error_code_all_variants() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    insta::assert_json_snapshot!(codes);
}

#[test]
fn error_category_all_variants() {
    let cats = vec![
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Internal,
    ];
    insta::assert_json_snapshot!(cats);
}

#[test]
fn abp_error_dto_simple() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend: openai");
    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn abp_error_dto_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_context("backend", "sidecar:claude")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn abp_error_dto_with_source() {
    let src = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "sidecar crashed").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn abp_error_dto_nested_context() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml")
        .with_context("details", json!({"line": 42, "column": 5}));
    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

// ===========================================================================
// 5. abp-daemon (api.rs) snapshots
// ===========================================================================

#[test]
fn daemon_api_run_status_queued() {
    insta::assert_json_snapshot!(ApiRunStatus::Queued);
}

#[test]
fn daemon_api_run_status_running() {
    insta::assert_json_snapshot!(ApiRunStatus::Running);
}

#[test]
fn daemon_api_run_status_completed() {
    insta::assert_json_snapshot!(ApiRunStatus::Completed);
}

#[test]
fn daemon_api_run_status_failed() {
    insta::assert_json_snapshot!(ApiRunStatus::Failed);
}

#[test]
fn daemon_api_run_status_cancelled() {
    insta::assert_json_snapshot!(ApiRunStatus::Cancelled);
}

#[test]
fn daemon_api_run_info() {
    let info = RunInfo {
        id: fixed_uuid(),
        status: ApiRunStatus::Running,
        backend: "sidecar:claude".into(),
        created_at: fixed_ts(),
        events_count: 42,
    };
    insta::assert_json_snapshot!(info);
}

#[test]
fn daemon_api_health_response() {
    let resp = HealthResponse {
        status: "ok".into(),
        version: "abp/v0.1".into(),
        uptime_seconds: 3600,
        backends_count: 3,
    };
    insta::assert_json_snapshot!(resp);
}

#[test]
fn daemon_api_backend_detail() {
    let detail = BackendDetail {
        id: "sidecar:claude".into(),
        capabilities: sample_capabilities(),
    };
    let json_str = serde_json::to_string_pretty(&detail).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn daemon_api_error_not_found() {
    let err = ApiError::not_found("run abc-123 not found");
    insta::assert_json_snapshot!(err);
}

#[test]
fn daemon_api_error_invalid_request() {
    let err = ApiError::invalid_request("missing required field: backend");
    insta::assert_json_snapshot!(err);
}

#[test]
fn daemon_api_error_with_details() {
    let err = ApiError::invalid_request("validation failed")
        .with_details(json!({"field": "work_order.task", "reason": "must not be empty"}));
    insta::assert_json_snapshot!(err);
}

#[test]
fn daemon_api_error_conflict() {
    let err = ApiError::conflict("run is already in a terminal state");
    insta::assert_json_snapshot!(err);
}

#[test]
fn daemon_api_error_internal() {
    let err = ApiError::internal("unexpected database error");
    insta::assert_json_snapshot!(err);
}

#[test]
fn daemon_api_response_run_created() {
    let resp = ApiResponse::RunCreated {
        run_id: fixed_uuid(),
    };
    insta::assert_json_snapshot!(resp);
}

#[test]
fn daemon_api_response_health() {
    let resp = ApiResponse::Health(HealthResponse {
        status: "ok".into(),
        version: "abp/v0.1".into(),
        uptime_seconds: 120,
        backends_count: 2,
    });
    insta::assert_json_snapshot!(resp);
}

#[test]
fn daemon_api_response_backend_list() {
    let resp = ApiResponse::BackendList {
        backends: vec![BackendDetail {
            id: "mock".into(),
            capabilities: BTreeMap::new(),
        }],
    };
    insta::assert_json_snapshot!(resp);
}

#[test]
fn daemon_api_response_run_cancelled() {
    let resp = ApiResponse::RunCancelled {
        run_id: fixed_uuid(),
    };
    insta::assert_json_snapshot!(resp);
}

#[test]
fn daemon_api_request_cancel_run() {
    let req = ApiRequest::CancelRun {
        run_id: fixed_uuid(),
    };
    insta::assert_json_snapshot!(req);
}

// ===========================================================================
// 6. abp-daemon (lib.rs) snapshots
// ===========================================================================

#[test]
fn daemon_run_status_pending() {
    insta::assert_json_snapshot!(RunStatus::Pending);
}

#[test]
fn daemon_run_status_completed_with_receipt() {
    let status = RunStatus::Completed {
        receipt: Box::new(sample_receipt()),
    };
    let json_str = serde_json::to_string_pretty(&status).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn daemon_run_status_failed() {
    let status = RunStatus::Failed {
        error: "sidecar crashed with exit code 1".into(),
    };
    insta::assert_json_snapshot!(status);
}

#[test]
fn daemon_run_metrics() {
    let m = DaemonRunMetrics {
        total_runs: 100,
        running: 3,
        completed: 90,
        failed: 7,
    };
    insta::assert_json_snapshot!(m);
}

#[test]
fn daemon_backend_info() {
    let info = BackendInfo {
        id: "mock".into(),
        capabilities: sample_capabilities(),
    };
    let json_str = serde_json::to_string_pretty(&info).unwrap();
    insta::assert_snapshot!(json_str);
}
