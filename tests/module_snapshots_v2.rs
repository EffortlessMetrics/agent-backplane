// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive insta snapshot tests for types added in recent module waves.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{TimeZone, Utc};

use abp_core::aggregate::{EventAggregator, RunAnalytics};
use abp_core::config::{ConfigWarning, WarningSeverity};
use abp_core::{AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, WorkOrderBuilder};
use abp_host::lifecycle::{LifecycleState, LifecycleTransition};
use abp_host::pool::{PoolConfig, SidecarPool};
use abp_integrations::health::{HealthCheck, HealthStatus, SystemHealth};
use abp_integrations::metrics::{BackendMetrics, MetricsRegistry};
use abp_integrations::selector::SelectionStrategy;
use abp_policy::compose::{PolicyDecision, PolicySet};
use abp_protocol::Envelope;
use abp_protocol::batch::BatchRequest;
use abp_protocol::compress::{CompressionAlgorithm, MessageCompressor};
use abp_protocol::router::MessageRoute;
use abp_runtime::budget::{BudgetLimit, BudgetTracker, BudgetViolation};
use abp_runtime::cancel::CancellationReason;
use abp_runtime::observe::{ObservabilitySummary, Span, SpanStatus};
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};

// ── Helpers ──────────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 5).unwrap()
}

fn sample_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("t-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "main.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("t-1".into()),
                output: serde_json::json!("fn main() {}"),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Warning {
                message: "rate limit".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts2(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 1. BudgetLimit — default and configured
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn budget_limit_default_snapshot() {
    let limit = BudgetLimit::default();
    insta::assert_json_snapshot!("budget_limit_default", limit);
}

#[test]
fn budget_limit_configured_snapshot() {
    let limit = BudgetLimit {
        max_tokens: Some(100_000),
        max_cost_usd: Some(5.0),
        max_turns: Some(25),
        max_duration: Some(Duration::from_secs(600)),
    };
    insta::assert_json_snapshot!("budget_limit_configured", limit);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. BudgetStatus::Warning — at 85%
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn budget_status_warning_snapshot() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        ..BudgetLimit::default()
    });
    tracker.record_tokens(850);
    let status = tracker.check();
    let display = format!("{status:?}");
    insta::assert_snapshot!("budget_status_warning_85pct", display);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. BudgetViolation::TokensExceeded
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn budget_violation_tokens_exceeded_snapshot() {
    let violation = BudgetViolation::TokensExceeded {
        used: 105_000,
        limit: 100_000,
    };
    insta::assert_snapshot!("budget_violation_tokens_exceeded", violation.to_string());
}

#[test]
fn budget_violation_cost_exceeded_snapshot() {
    let violation = BudgetViolation::CostExceeded {
        used: 5.1234,
        limit: 5.0,
    };
    insta::assert_snapshot!("budget_violation_cost_exceeded", violation.to_string());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. CancellationReason — all 5 variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancellation_reason_all_variants_snapshot() {
    let reasons = vec![
        CancellationReason::UserRequested,
        CancellationReason::Timeout,
        CancellationReason::BudgetExhausted,
        CancellationReason::PolicyViolation,
        CancellationReason::SystemShutdown,
    ];
    insta::assert_json_snapshot!("cancellation_reason_all_variants", reasons);
}

#[test]
fn cancellation_reason_descriptions_snapshot() {
    let descs: Vec<&str> = [
        CancellationReason::UserRequested,
        CancellationReason::Timeout,
        CancellationReason::BudgetExhausted,
        CancellationReason::PolicyViolation,
        CancellationReason::SystemShutdown,
    ]
    .iter()
    .map(|r| r.description())
    .collect();
    insta::assert_json_snapshot!("cancellation_reason_descriptions", descs);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. CancellableRun — with reason
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancellable_run_with_reason_snapshot() {
    use abp_runtime::cancel::{CancellableRun, CancellationToken};
    let run = CancellableRun::new(CancellationToken::new());
    run.cancel(CancellationReason::BudgetExhausted);
    let reason = run.reason().unwrap();
    insta::assert_json_snapshot!("cancellable_run_reason", reason);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. BackendMetrics snapshot — after 3 runs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_metrics_after_3_runs_snapshot() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    m.record_run(true, 8, 300);
    m.record_run(false, 3, 1200);
    let snap = m.snapshot();
    insta::assert_json_snapshot!("backend_metrics_after_3_runs", snap);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. MetricsRegistry snapshot — multiple backends
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn metrics_registry_multiple_backends_snapshot() {
    let reg = MetricsRegistry::new();
    let openai = reg.get_or_create("openai");
    openai.record_run(true, 12, 800);
    openai.record_run(true, 15, 600);
    let anthropic = reg.get_or_create("anthropic");
    anthropic.record_run(true, 20, 450);
    anthropic.record_run(false, 2, 100);
    let snap = reg.snapshot_all();
    insta::assert_json_snapshot!("metrics_registry_multiple_backends", snap);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. EventAggregator summary — mixed events
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_aggregator_summary_snapshot() {
    let mut agg = EventAggregator::new();
    for e in &sample_events() {
        agg.add(e);
    }
    let summary = agg.summary();
    insta::assert_json_snapshot!("event_aggregator_summary", summary);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. RunAnalytics — complete run data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_analytics_complete_snapshot() {
    let analytics = RunAnalytics::from_events(&sample_events());
    let summary = analytics.summary();
    let extra = serde_json::json!({
        "summary": summary,
        "is_successful": analytics.is_successful(),
        "tool_usage_ratio": analytics.tool_usage_ratio(),
        "average_text_per_event": analytics.average_text_per_event(),
    });
    insta::assert_json_snapshot!("run_analytics_complete", extra);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. SidecarPool stats — various states
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_pool_stats_snapshot() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire(); // s1 → Busy
    pool.mark_failed("s3");
    let stats = pool.stats();
    insta::assert_json_snapshot!("sidecar_pool_stats_various", stats);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. PoolConfig — default values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pool_config_default_snapshot() {
    let cfg = PoolConfig::default();
    insta::assert_json_snapshot!("pool_config_default", cfg);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. PolicyDecision::Allow and Deny
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_decision_allow_snapshot() {
    let decision = PolicyDecision::Allow {
        reason: "tool 'read_file' permitted by allowlist".into(),
    };
    insta::assert_json_snapshot!("policy_decision_allow", decision);
}

#[test]
fn policy_decision_deny_snapshot() {
    let decision = PolicyDecision::Deny {
        reason: "tool 'execute_command' is disallowed".into(),
    };
    insta::assert_json_snapshot!("policy_decision_deny", decision);
}

#[test]
fn policy_decision_abstain_snapshot() {
    let decision = PolicyDecision::Abstain;
    insta::assert_json_snapshot!("policy_decision_abstain", decision);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. PolicySet merged — from 2 profiles
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_set_merged_snapshot() {
    use abp_core::PolicyProfile;
    let mut set = PolicySet::new("combined");
    let p1 = PolicyProfile {
        allowed_tools: vec!["read_file".into(), "write_file".into()],
        deny_read: vec!["*.secret".into()],
        ..PolicyProfile::default()
    };
    let p2 = PolicyProfile {
        allowed_tools: vec!["write_file".into(), "execute".into()],
        deny_write: vec!["/etc/**".into()],
        ..PolicyProfile::default()
    };
    set.add(p1);
    set.add(p2);
    let merged = set.merge();
    insta::assert_json_snapshot!("policy_set_merged", merged);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. ChangeTracker summary — mixed changes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn change_tracker_summary_snapshot() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "src/main.rs".into(),
        kind: ChangeKind::Modified,
        size_before: Some(1024),
        size_after: Some(2048),
        content_hash: Some("abcd1234".into()),
    });
    tracker.record(FileChange {
        path: "src/new.rs".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(512),
        content_hash: Some("ef567890".into()),
    });
    tracker.record(FileChange {
        path: "src/old.rs".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(256),
        size_after: None,
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "src/moved.rs".into(),
        kind: ChangeKind::Renamed {
            from: "src/legacy.rs".into(),
        },
        size_before: Some(128),
        size_after: Some(128),
        content_hash: Some("face0000".into()),
    });
    let summary = tracker.summary();
    insta::assert_json_snapshot!("change_tracker_summary_mixed", summary);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. LifecycleState — all 7 variants serialized
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_state_all_variants_snapshot() {
    let states = vec![
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
        LifecycleState::Failed,
    ];
    insta::assert_json_snapshot!("lifecycle_state_all_variants", states);
}

// ═══════════════════════════════════════════════════════════════════════
// 16. LifecycleTransition — example transition
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_transition_snapshot() {
    let transition = LifecycleTransition {
        from: LifecycleState::Starting,
        to: LifecycleState::Ready,
        timestamp: "2025-01-15T12:00:00+00:00".into(),
        reason: Some("handshake complete".into()),
    };
    insta::assert_json_snapshot!("lifecycle_transition_example", transition);
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Span — complete with attributes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn span_complete_snapshot() {
    let mut attrs = BTreeMap::new();
    attrs.insert("backend".into(), "openai".into());
    attrs.insert("model".into(), "gpt-4o".into());
    let span = Span {
        id: "span-001".into(),
        name: "run_work_order".into(),
        parent_id: None,
        start_time: "2025-01-15T12:00:00+00:00".into(),
        end_time: Some("2025-01-15T12:00:05+00:00".into()),
        attributes: attrs,
        status: SpanStatus::Ok,
    };
    insta::assert_json_snapshot!("span_complete", span);
}

#[test]
fn span_with_error_snapshot() {
    let span = Span {
        id: "span-002".into(),
        name: "sidecar_call".into(),
        parent_id: Some("span-001".into()),
        start_time: "2025-01-15T12:00:01+00:00".into(),
        end_time: Some("2025-01-15T12:00:02+00:00".into()),
        attributes: BTreeMap::new(),
        status: SpanStatus::Error {
            message: "sidecar timed out".into(),
        },
    };
    insta::assert_json_snapshot!("span_with_error", span);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. ObservabilitySummary — with metrics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn observability_summary_snapshot() {
    let summary = ObservabilitySummary {
        total_spans: 12,
        active_spans: 2,
        error_spans: 1,
        metrics_count: 5,
    };
    insta::assert_json_snapshot!("observability_summary", summary);
}

// ═══════════════════════════════════════════════════════════════════════
// 19. HealthStatus — all 4 variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_status_all_variants_snapshot() {
    let statuses = vec![
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "high latency on openai backend".into(),
        },
        HealthStatus::Unhealthy {
            reason: "connection refused".into(),
        },
        HealthStatus::Unknown,
    ];
    insta::assert_json_snapshot!("health_status_all_variants", statuses);
}

// ═══════════════════════════════════════════════════════════════════════
// 20. SystemHealth — complete system report
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn system_health_complete_snapshot() {
    let health = SystemHealth {
        backends: vec![
            HealthCheck {
                name: "openai".into(),
                status: HealthStatus::Healthy,
                checked_at: "2025-01-15T12:00:00+00:00".into(),
                response_time_ms: Some(45),
                details: BTreeMap::new(),
            },
            HealthCheck {
                name: "anthropic".into(),
                status: HealthStatus::Degraded {
                    reason: "slow responses".into(),
                },
                checked_at: "2025-01-15T12:00:00+00:00".into(),
                response_time_ms: Some(2500),
                details: {
                    let mut d = BTreeMap::new();
                    d.insert("region".into(), "us-east-1".into());
                    d
                },
            },
        ],
        overall: HealthStatus::Degraded {
            reason: "slow responses".into(),
        },
        uptime_seconds: 3600,
        version: "0.1.0".into(),
    };
    insta::assert_json_snapshot!("system_health_complete", health);
}

// ═══════════════════════════════════════════════════════════════════════
// 21. SelectionStrategy — all 5 variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn selection_strategy_all_variants_snapshot() {
    let strategies = vec![
        SelectionStrategy::FirstMatch,
        SelectionStrategy::BestFit,
        SelectionStrategy::LeastLoaded,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::Priority,
    ];
    insta::assert_json_snapshot!("selection_strategy_all_variants", strategies);
}

// ═══════════════════════════════════════════════════════════════════════
// 22. BatchRequest — with 3 envelopes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn batch_request_with_3_envelopes_snapshot() {
    let backend = BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    };
    let wo = WorkOrderBuilder::new("test task").model("gpt-4o").build();
    let req = BatchRequest {
        id: "batch-001".into(),
        envelopes: vec![
            Envelope::hello(backend, CapabilityManifest::new()),
            Envelope::Run {
                id: "run-001".into(),
                work_order: wo,
            },
            Envelope::Fatal {
                ref_id: Some("run-001".into()),
                error: "out of memory".into(),
            },
        ],
        created_at: "2025-01-15T12:00:00+00:00".into(),
    };
    insta::assert_json_snapshot!("batch_request_3_envelopes", req, {
        ".envelopes[1].work_order.id" => "[UUID]",
    });
}

// ═══════════════════════════════════════════════════════════════════════
// 23. ConfigWarning — all severity levels
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_warning_all_severities_snapshot() {
    let warnings = [
        ConfigWarning {
            field: "config.model".into(),
            message: "Model name is unusual".into(),
            severity: WarningSeverity::Info,
        },
        ConfigWarning {
            field: "policy.allowed_tools".into(),
            message: "Duplicate tool in allowlist: read_file".into(),
            severity: WarningSeverity::Warning,
        },
        ConfigWarning {
            field: "task".into(),
            message: "Task description must not be empty".into(),
            severity: WarningSeverity::Error,
        },
    ];
    let display: Vec<String> = warnings
        .iter()
        .map(|w| format!("[{:?}] {}: {}", w.severity, w.field, w.message))
        .collect();
    insta::assert_json_snapshot!("config_warning_all_severities", display);
}

// ═══════════════════════════════════════════════════════════════════════
// 24. MessageRoute — serde format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn message_route_serde_snapshot() {
    let route = MessageRoute {
        pattern: "event".into(),
        destination: "event_handler".into(),
        priority: 10,
    };
    insta::assert_json_snapshot!("message_route_serde", route);
}

#[test]
fn message_route_ref_id_pattern_snapshot() {
    let route = MessageRoute {
        pattern: "run-".into(),
        destination: "run_dispatcher".into(),
        priority: 5,
    };
    insta::assert_json_snapshot!("message_route_ref_id_pattern", route);
}

// ═══════════════════════════════════════════════════════════════════════
// 25. CompressedMessage — gzip stub
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compressed_message_gzip_stub_snapshot() {
    let compressor = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let msg = compressor
        .compress_message(b"hello world")
        .expect("compress should succeed");
    insta::assert_json_snapshot!("compressed_message_gzip_stub", msg);
}

#[test]
fn compressed_message_none_snapshot() {
    let compressor = MessageCompressor::new(CompressionAlgorithm::None);
    let msg = compressor
        .compress_message(b"hello world")
        .expect("compress should succeed");
    insta::assert_json_snapshot!("compressed_message_none", msg);
}
