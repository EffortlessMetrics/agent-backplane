// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests exercising combinations of new modules across crates.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, ExecutionMode, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    CONTRACT_VERSION,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_receipt(work_order_id: Uuid) -> Receipt {
    ReceiptBuilder::new("mock")
        .work_order_id(work_order_id)
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build()
}

fn make_hashed_receipt(work_order_id: Uuid) -> Receipt {
    ReceiptBuilder::new("mock")
        .work_order_id(work_order_id)
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .expect("hash computation should succeed")
}

// ===========================================================================
// 1. Budget + Cancellation: Budget exceeded → triggers cancellation
// ===========================================================================

#[test]
fn budget_exceeded_triggers_cancellation() {
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};
    use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};

    let limit = BudgetLimit {
        max_tokens: Some(100),
        max_cost_usd: None,
        max_turns: Some(3),
        max_duration: None,
    };
    let tracker = BudgetTracker::new(limit);
    let token = CancellationToken::new();
    let run = CancellableRun::new(token);

    // Consume budget
    tracker.record_tokens(50);
    tracker.record_turn();
    assert!(matches!(tracker.check(), BudgetStatus::WithinLimits));
    assert!(!run.is_cancelled());

    // Exceed turns
    tracker.record_turn();
    tracker.record_turn();
    tracker.record_turn();
    match tracker.check() {
        BudgetStatus::Exceeded(_) => {
            run.cancel(CancellationReason::BudgetExhausted);
        }
        _ => panic!("expected budget exceeded"),
    }
    assert!(run.is_cancelled());
    assert!(matches!(
        run.reason(),
        Some(CancellationReason::BudgetExhausted)
    ));
}

// ===========================================================================
// 2. Event Bus + Aggregator: Publish → subscribe + aggregate → summary
// ===========================================================================

#[tokio::test]
async fn event_bus_publish_subscribe_aggregate() {
    use abp_core::aggregate::EventAggregator;
    use abp_runtime::bus::EventBus;

    let bus = EventBus::new();
    let mut sub = bus.subscribe();

    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs"}),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "Done reading".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "fin".into(),
        }),
    ];

    for ev in &events {
        bus.publish(ev.clone());
    }
    drop(bus);

    let mut aggregator = EventAggregator::new();
    while let Some(ev) = sub.recv().await {
        aggregator.add(&ev);
    }

    let summary = aggregator.summary();
    assert_eq!(summary.total_events, 4);
    assert_eq!(summary.tool_calls, 1);
    assert!(summary.total_text_chars > 0);
}

// ===========================================================================
// 3. Pool + Lifecycle: Pool entries follow lifecycle state machine
// ===========================================================================

#[test]
fn pool_entries_follow_lifecycle() {
    use abp_host::lifecycle::{LifecycleManager, LifecycleState};
    use abp_host::pool::{PoolConfig, SidecarPool};

    let config = PoolConfig {
        min_size: 1,
        max_size: 5,
        idle_timeout: Duration::from_secs(60),
        health_check_interval: Duration::from_secs(30),
    };
    let pool = SidecarPool::new(config);

    // Lifecycle: Uninitialized → Starting → Ready
    let mut lifecycle = LifecycleManager::new();
    assert!(matches!(lifecycle.state(), LifecycleState::Uninitialized));
    lifecycle
        .transition(LifecycleState::Starting, Some("booting".into()))
        .unwrap();
    lifecycle
        .transition(LifecycleState::Ready, Some("handshake done".into()))
        .unwrap();

    // Add to pool once ready
    assert!(pool.add("sidecar-1"));
    assert_eq!(pool.total_count(), 1);
    assert_eq!(pool.idle_count(), 1);

    // Acquire for use → lifecycle Running
    let entry = pool.acquire().expect("should acquire");
    lifecycle
        .transition(LifecycleState::Running, Some("processing".into()))
        .unwrap();
    assert_eq!(entry.id, "sidecar-1");

    // Release back → lifecycle transitions to Stopping → Stopped
    pool.release(&entry.id);
    lifecycle
        .transition(LifecycleState::Stopping, None)
        .unwrap();
    lifecycle
        .transition(LifecycleState::Stopped, Some("graceful".into()))
        .unwrap();
    assert!(matches!(lifecycle.state(), LifecycleState::Stopped));
    assert!(lifecycle.history().len() >= 4);
}

// ===========================================================================
// 4. Policy Rules + Compose: Rule engine + composed policies → combined
// ===========================================================================

#[test]
fn policy_rules_and_compose_combined_decisions() {
    use abp_policy::compose::{ComposedEngine, PolicyPrecedence};
    use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

    // Rule engine: deny Bash, allow Read
    let mut rule_engine = RuleEngine::new();
    rule_engine.add_rule(Rule {
        id: "deny-bash".into(),
        description: "Deny bash".into(),
        condition: RuleCondition::Pattern("Bash*".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    rule_engine.add_rule(Rule {
        id: "allow-read".into(),
        description: "Allow read".into(),
        condition: RuleCondition::Pattern("Read".into()),
        effect: RuleEffect::Allow,
        priority: 5,
    });

    assert!(matches!(
        rule_engine.evaluate("BashExec"),
        RuleEffect::Deny
    ));
    assert!(matches!(rule_engine.evaluate("Read"), RuleEffect::Allow));

    // Composed engine: two profiles, deny-overrides
    let p1 = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let p2 = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let composed = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(composed.check_tool("Read").is_allow());
    assert!(composed.check_tool("Bash").is_deny());
}

// ===========================================================================
// 5. Metrics + Health: Record metrics → derive health status
// ===========================================================================

#[test]
fn metrics_drive_health_status() {
    use abp_integrations::health::{HealthChecker, HealthStatus};
    use abp_integrations::metrics::BackendMetrics;

    let metrics = BackendMetrics::new();

    // Record several successful runs
    metrics.record_run(true, 10, 500);
    metrics.record_run(true, 15, 600);
    metrics.record_run(false, 5, 300);

    let snap = metrics.snapshot();
    let mut checker = HealthChecker::new();

    // Derive health from success rate
    if snap.success_rate >= 0.5 {
        checker.add_check("backend-a", HealthStatus::Healthy);
    } else {
        checker.add_check(
            "backend-a",
            HealthStatus::Unhealthy {
                reason: "low success rate".into(),
            },
        );
    }

    assert!(checker.is_healthy());
    assert_eq!(checker.check_count(), 1);

    // Add a degraded check
    checker.add_check(
        "backend-b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    assert!(!checker.is_healthy());
    assert!(matches!(
        checker.overall_status(),
        HealthStatus::Degraded { .. }
    ));
}

// ===========================================================================
// 6. Config Validation + Defaults: Validate → defaults → validate again
// ===========================================================================

#[test]
fn config_validate_then_defaults_then_validate() {
    use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};

    let mut wo = make_work_order("");
    let validator = ConfigValidator::new();

    // Validate bare work order — should have warnings (empty task, no model, etc.)
    let warnings_before = validator.validate_work_order(&wo);
    let error_count_before = warnings_before
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .count();
    assert!(error_count_before > 0, "empty task should produce errors");

    // Apply defaults
    ConfigDefaults::apply_defaults(&mut wo);
    wo.task = "Fix the bug".into();

    // Validate again — fewer or no errors
    let warnings_after = validator.validate_work_order(&wo);
    let error_count_after = warnings_after
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .count();
    assert!(error_count_after < error_count_before);
}

// ===========================================================================
// 7. Envelope Validation + Routing: Validate → route to destinations
// ===========================================================================

#[test]
fn envelope_validate_then_route() {
    use abp_protocol::router::{MessageRoute, MessageRouter};
    use abp_protocol::validate::EnvelopeValidator;
    use abp_protocol::Envelope;

    let wo = make_work_order("test task");
    let receipt = make_receipt(wo.id);

    let envelopes = vec![
        Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: BackendIdentity {
                id: "mock".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: BTreeMap::new(),
            mode: ExecutionMode::Mapped,
        },
        Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "run-1".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        },
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt,
        },
    ];

    // Validate sequence
    let validator = EnvelopeValidator::new();
    let seq_errors = validator.validate_sequence(&envelopes);
    assert!(seq_errors.is_empty(), "sequence should be valid: {seq_errors:?}");

    // Route each envelope
    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "hello".into(),
        destination: "handshake-handler".into(),
        priority: 1,
    });
    router.add_route(MessageRoute {
        pattern: "run".into(),
        destination: "executor".into(),
        priority: 1,
    });
    router.add_route(MessageRoute {
        pattern: "event".into(),
        destination: "event-log".into(),
        priority: 1,
    });
    router.add_route(MessageRoute {
        pattern: "final".into(),
        destination: "receipt-store".into(),
        priority: 1,
    });

    let matches = router.route_all(&envelopes);
    assert_eq!(matches.len(), 4);
}

// ===========================================================================
// 8. Change Tracker + File Ops: Record ops → verify tracker matches
// ===========================================================================

#[test]
fn change_tracker_matches_file_ops() {
    use abp_workspace::ops::{FileOperation, OperationLog};
    use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};

    let mut ops_log = OperationLog::new();
    let mut tracker = ChangeTracker::new();

    // Simulate file operations
    ops_log.record(FileOperation::Write {
        path: "src/new.rs".into(),
        size: 1024,
    });
    tracker.record(FileChange {
        path: "src/new.rs".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(1024),
        content_hash: None,
    });

    ops_log.record(FileOperation::Write {
        path: "src/main.rs".into(),
        size: 2048,
    });
    tracker.record(FileChange {
        path: "src/main.rs".into(),
        kind: ChangeKind::Modified,
        size_before: Some(1500),
        size_after: Some(2048),
        content_hash: None,
    });

    ops_log.record(FileOperation::Delete {
        path: "src/old.rs".into(),
    });
    tracker.record(FileChange {
        path: "src/old.rs".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(500),
        size_after: None,
        content_hash: None,
    });

    // Verify ops log matches tracker
    let ops_paths = ops_log.affected_paths();
    let tracker_paths: std::collections::BTreeSet<String> = tracker
        .affected_paths()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(ops_paths, tracker_paths);

    let ops_summary = ops_log.summary();
    let change_summary = tracker.summary();
    assert_eq!(ops_summary.writes, change_summary.created + change_summary.modified);
    assert_eq!(ops_summary.deletes, change_summary.deleted);
}

// ===========================================================================
// 9. Version Negotiation + API Versioning compatibility
// ===========================================================================

#[test]
fn version_negotiation_and_api_compatibility() {
    use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};

    let local = ProtocolVersion::current();
    let remote = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();

    // Same version should negotiate successfully
    let result = negotiate_version(&local, &remote);
    assert!(result.is_ok());

    // Version range should contain current
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 0 },
        max: ProtocolVersion { major: 0, minor: 9 },
    };
    assert!(range.contains(&local));
    assert!(local.is_compatible(&remote));
}

// ===========================================================================
// 10. Receipt Verification + Chain: Verify individual → verify chain
// ===========================================================================

#[test]
fn receipt_verify_individual_then_chain() {
    use abp_core::chain::ReceiptChain;
    use abp_core::verify::{ChainVerifier, ReceiptVerifier};

    let r1 = make_hashed_receipt(Uuid::new_v4());
    let r2 = make_hashed_receipt(Uuid::new_v4());

    // Verify individually
    let verifier = ReceiptVerifier::new();
    let report1 = verifier.verify(&r1);
    let report2 = verifier.verify(&r2);
    assert!(report1.passed, "r1 should pass: {:?}", report1.checks);
    assert!(report2.passed, "r2 should pass: {:?}", report2.checks);

    // Build chain and verify
    let mut chain = ReceiptChain::new();
    chain.push(r1.clone()).unwrap();
    chain.push(r2.clone()).unwrap();
    chain.verify().unwrap();

    // Chain-level verification
    let chain_report = ChainVerifier::verify_chain(&[r1, r2]);
    assert!(chain_report.all_valid, "chain should be valid: {:?}", chain_report.chain_checks);
}

// ===========================================================================
// 11. Batch Processing + Compression: Batch → compress → decompress → verify
// ===========================================================================

#[test]
fn batch_compress_decompress_roundtrip() {
    use abp_protocol::batch::{BatchProcessor, BatchRequest};
    use abp_protocol::compress::{CompressionAlgorithm, CompressionStats, MessageCompressor};
    use abp_protocol::Envelope;

    let wo = make_work_order("batch test");
    let envelopes = vec![
        Envelope::Run {
            id: "run-batch".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "run-batch".into(),
            event: make_event(AgentEventKind::AssistantMessage {
                text: "batch message".into(),
            }),
        },
    ];

    // Process batch
    let processor = BatchProcessor::new();
    let request = BatchRequest {
        id: "batch-1".into(),
        envelopes: envelopes.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    let response = processor.process(request);
    assert_eq!(response.results.len(), 2);

    // Compress the batch as JSON
    let batch_json = serde_json::to_vec(&envelopes).unwrap();
    let compressor = MessageCompressor::new(CompressionAlgorithm::None);
    let compressed = compressor.compress_message(&batch_json).unwrap();
    let decompressed = compressor.decompress_message(&compressed).unwrap();
    assert_eq!(batch_json, decompressed);

    // Track compression stats
    let mut stats = CompressionStats::new();
    stats.record(batch_json.len(), compressed.compressed_size);
    assert!(stats.compression_ratio() > 0.0);
}

// ===========================================================================
// 12. Selection Strategy + Capability: Select backend by capability
// ===========================================================================

#[test]
fn select_backend_by_capability_and_strategy() {
    use abp_integrations::capability::CapabilityMatrix;
    use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};

    // Build capability matrix
    let mut matrix = CapabilityMatrix::new();
    matrix.register("gpt-4", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register(
        "claude",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    matrix.register("local", vec![Capability::ToolRead]);

    // Best backend for streaming + tool_write
    let best = matrix.best_backend(&[Capability::Streaming, Capability::ToolWrite]);
    assert_eq!(best.as_deref(), Some("claude"));

    // Selector with BestFit strategy
    let mut selector = BackendSelector::new(SelectionStrategy::BestFit);
    selector.add_candidate(BackendCandidate {
        name: "gpt-4".into(),
        capabilities: vec![Capability::Streaming, Capability::ToolRead],
        priority: 1,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    selector.add_candidate(BackendCandidate {
        name: "claude".into(),
        capabilities: vec![Capability::Streaming, Capability::ToolRead, Capability::ToolWrite],
        priority: 2,
        enabled: true,
        metadata: BTreeMap::new(),
    });

    let result = selector.select_with_result(&[Capability::Streaming, Capability::ToolWrite]);
    assert_eq!(result.selected, "claude");
}

// ===========================================================================
// 13. Observability + Stages: Pipeline stages → traces + metrics
// ===========================================================================

#[test]
fn observability_with_pipeline_stages() {
    use abp_runtime::observe::{RuntimeObserver, SpanStatus};

    let mut observer = RuntimeObserver::new();

    // Record metrics for pipeline stages
    observer.record_metric("validation_ms", 5.0);
    observer.record_metric("policy_ms", 3.0);
    observer.record_metric("execution_ms", 150.0);

    // Create trace spans for stages
    let tc = observer.trace_collector();
    let root = tc.start_span("pipeline");
    let validate_span = tc.start_child_span("validate", &root);
    tc.end_span(&validate_span);
    let policy_span = tc.start_child_span("policy", &root);
    tc.end_span(&policy_span);
    let exec_span = tc.start_child_span("execute", &root);
    tc.set_status(&exec_span, SpanStatus::Ok);
    tc.end_span(&exec_span);
    tc.end_span(&root);

    // Verify observer summary
    let summary = observer.summary();
    assert_eq!(summary.metrics_count, 3);
    assert_eq!(summary.total_spans, 4);
    assert_eq!(summary.active_spans, 0);
}

// ===========================================================================
// 14. Transform Chain + Diagnostics: Transform events → collect diagnostics
// ===========================================================================

#[test]
fn transform_chain_with_diagnostics() {
    use sidecar_kit::diagnostics::DiagnosticCollector;
    use sidecar_kit::transform::{RedactTransformer, TimestampTransformer, TransformerChain};

    let chain = TransformerChain::new()
        .with(Box::new(TimestampTransformer::new()))
        .with(Box::new(RedactTransformer::new(vec!["secret".into()])));

    let mut diag = DiagnosticCollector::new();

    // Transform an event containing a secret
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "The secret password is 1234".into(),
    });

    let result = chain.process(event);
    if let Some(transformed) = result {
        if let AgentEventKind::AssistantMessage { ref text } = transformed.kind {
            if text.contains("[REDACTED]") {
                diag.add_info("REDACT001", "Redaction applied");
            }
        }
    }

    diag.add_info("TRANSFORM001", "Transform chain complete");
    let summary = diag.summary();
    assert!(summary.total > 0);
    assert_eq!(summary.error_count, 0);
}

// ===========================================================================
// 15. Event Stream + Filter + Aggregate: Full event processing pipeline
// ===========================================================================

#[test]
fn event_stream_filter_aggregate_pipeline() {
    use abp_core::aggregate::EventAggregator;
    use abp_core::filter::EventFilter;
    use abp_core::stream::EventStream;

    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "response".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "oops".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    // Filter: exclude errors
    let filter = EventFilter::exclude_kinds(&["error"]);
    let stream = EventStream::new(events);
    let filtered: Vec<AgentEvent> = stream.into_iter().filter(|e| filter.matches(e)).collect();
    assert_eq!(filtered.len(), 4);

    // Aggregate filtered events
    let mut aggregator = EventAggregator::new();
    for ev in &filtered {
        aggregator.add(ev);
    }
    assert_eq!(aggregator.event_count(), 4);
    assert!(!aggregator.has_errors());
    assert_eq!(aggregator.unique_tool_count(), 1);
}

// ===========================================================================
// 16. Budget Tracker + Metrics + Health: Budget state drives health
// ===========================================================================

#[test]
fn budget_metrics_health_pipeline() {
    use abp_integrations::health::{HealthChecker, HealthStatus};
    use abp_integrations::metrics::BackendMetrics;
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};

    let limit = BudgetLimit {
        max_tokens: Some(1000),
        max_cost_usd: Some(5.0),
        max_turns: None,
        max_duration: None,
    };
    let tracker = BudgetTracker::new(limit);

    // Record some usage
    tracker.record_tokens(200);
    tracker.record_cost(1.0);

    let metrics = BackendMetrics::new();
    metrics.record_run(true, 10, 500);

    let mut health = HealthChecker::new();

    match tracker.check() {
        BudgetStatus::WithinLimits => {
            health.add_check("budget", HealthStatus::Healthy);
        }
        BudgetStatus::Warning { .. } => {
            health.add_check(
                "budget",
                HealthStatus::Degraded {
                    reason: "budget warning".into(),
                },
            );
        }
        BudgetStatus::Exceeded(_) => {
            health.add_check(
                "budget",
                HealthStatus::Unhealthy {
                    reason: "budget exceeded".into(),
                },
            );
        }
    }

    // Add metrics-based health
    let snap = metrics.snapshot();
    if snap.success_rate >= 0.9 {
        health.add_check("backend", HealthStatus::Healthy);
    }

    assert!(health.is_healthy());
    assert_eq!(health.check_count(), 2);
}

// ===========================================================================
// 17. Queue Priority + Selection: Queue by priority → backend selection
// ===========================================================================

#[test]
fn queue_priority_drives_backend_selection() {
    use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};

    let mut selector = BackendSelector::new(SelectionStrategy::Priority);
    selector.add_candidate(BackendCandidate {
        name: "low-priority".into(),
        capabilities: vec![Capability::Streaming],
        priority: 10,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    selector.add_candidate(BackendCandidate {
        name: "high-priority".into(),
        capabilities: vec![Capability::Streaming],
        priority: 1, // lower value = higher priority
        enabled: true,
        metadata: BTreeMap::new(),
    });
    selector.add_candidate(BackendCandidate {
        name: "mid-priority".into(),
        capabilities: vec![Capability::Streaming],
        priority: 5,
        enabled: true,
        metadata: BTreeMap::new(),
    });

    // Priority selection should pick lowest priority value (highest priority)
    let result = selector.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "high-priority");
    assert!(!result.alternatives.is_empty());
}

// ===========================================================================
// 18. Workspace Ops + Template + Snapshot: Full workspace lifecycle
// ===========================================================================

#[test]
fn workspace_ops_template_snapshot_lifecycle() {
    use abp_workspace::ops::{FileOperation, OperationLog};
    use abp_workspace::snapshot;
    use abp_workspace::template::WorkspaceTemplate;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Create workspace from template
    let mut template = WorkspaceTemplate::new("test-project", "A test project");
    template.add_file("src/main.rs", "fn main() {}");
    template.add_file("Cargo.toml", "[package]\nname = \"test\"");
    template.add_file("README.md", "# Test");

    let warnings = template.validate();
    assert!(warnings.is_empty());
    let files_written = template.apply(root).unwrap();
    assert_eq!(files_written, 3);

    // Take snapshot
    let snap1 = snapshot::capture(root).unwrap();
    assert_eq!(snap1.file_count(), 3);
    assert!(snap1.has_file("src/main.rs"));

    // Record an operation and modify workspace
    let mut ops = OperationLog::new();
    std::fs::write(root.join("src/lib.rs"), "pub mod utils;").unwrap();
    ops.record(FileOperation::Write {
        path: "src/lib.rs".into(),
        size: 15,
    });

    // Take another snapshot and compare
    let snap2 = snapshot::capture(root).unwrap();
    let diff = snapshot::compare(&snap1, &snap2);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.unchanged.len(), 3);
}

// ===========================================================================
// 19. Lifecycle + Health + Pool: State machine drives pool management
// ===========================================================================

#[test]
fn lifecycle_health_pool_integration() {
    use abp_host::health::{HealthMonitor, HealthStatus as SidecarHealth};
    use abp_host::lifecycle::{LifecycleManager, LifecycleState};
    use abp_host::pool::{PoolConfig, SidecarPool};

    let pool = SidecarPool::new(PoolConfig {
        min_size: 0,
        max_size: 3,
        idle_timeout: Duration::from_secs(60),
        health_check_interval: Duration::from_secs(10),
    });

    // Start two sidecars with lifecycle tracking
    let mut lc1 = LifecycleManager::new();
    let mut lc2 = LifecycleManager::new();

    lc1.transition(LifecycleState::Starting, None).unwrap();
    lc1.transition(LifecycleState::Ready, None).unwrap();
    pool.add("sc-1");

    lc2.transition(LifecycleState::Starting, None).unwrap();
    lc2.transition(LifecycleState::Ready, None).unwrap();
    pool.add("sc-2");

    // Health monitor
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", SidecarHealth::Healthy, None);
    monitor.record_check("sc-2", SidecarHealth::Healthy, None);
    assert!(monitor.all_healthy());

    // One sidecar fails
    pool.mark_failed("sc-2");
    lc2.transition(LifecycleState::Failed, Some("crash".into()))
        .unwrap();
    monitor.record_check(
        "sc-2",
        SidecarHealth::Unhealthy {
            reason: "sc-2 crashed".into(),
        },
        None,
    );

    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.idle, 1);
    assert!(!monitor.all_healthy());
}

// ===========================================================================
// 20. Full Pipeline: WorkOrder → validate → budget → select → receipt → chain
// ===========================================================================

#[test]
fn full_pipeline_end_to_end() {
    use abp_core::chain::ReceiptChain;
    use abp_core::config::{ConfigDefaults, ConfigValidator};
    use abp_core::verify::ReceiptVerifier;
    use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};

    // 1. Create and validate work order
    let mut wo = WorkOrderBuilder::new("Full pipeline test")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    ConfigDefaults::apply_defaults(&mut wo);
    let validator = ConfigValidator::new();
    let warnings = validator.validate_work_order(&wo);
    assert!(
        warnings.iter().all(|w| !matches!(
            w.severity,
            abp_core::config::WarningSeverity::Error
        )),
        "no errors expected"
    );

    // 2. Setup budget
    let limit = BudgetLimit {
        max_tokens: Some(10_000),
        max_cost_usd: Some(5.0),
        max_turns: Some(10),
        max_duration: None,
    };
    let budget = BudgetTracker::new(limit);

    // 3. Select backend
    let mut selector = BackendSelector::new(SelectionStrategy::BestFit);
    selector.add_candidate(BackendCandidate {
        name: "mock".into(),
        capabilities: vec![Capability::Streaming, Capability::ToolRead],
        priority: 1,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let selected = selector.select(&[Capability::Streaming]);
    assert!(selected.is_some());

    // 4. Simulate execution
    budget.record_tokens(500);
    budget.record_turn();
    assert!(matches!(budget.check(), BudgetStatus::WithinLimits));

    // 5. Build and verify receipt
    let receipt = make_hashed_receipt(wo.id);
    let verifier = ReceiptVerifier::new();
    let report = verifier.verify(&receipt);
    assert!(report.passed);

    // 6. Add to chain
    let mut chain = ReceiptChain::new();
    chain.push(receipt).unwrap();
    chain.verify().unwrap();
    assert_eq!(chain.len(), 1);
}

// ===========================================================================
// 21. Error catalog + diagnostics: Errors → diagnostics → summary
// ===========================================================================

#[test]
fn error_catalog_to_diagnostics() {
    use abp_core::error::{ErrorCatalog, ErrorInfo};
    use sidecar_kit::diagnostics::DiagnosticCollector;

    let mut diag = DiagnosticCollector::new();

    // Look up errors from catalog and record as diagnostics
    let contract_errors = ErrorCatalog::by_category("contract");
    assert!(!contract_errors.is_empty());

    for code in &contract_errors[..2.min(contract_errors.len())] {
        let info = ErrorInfo::new(code.clone(), format!("Test error: {}", code.description()))
            .with_context("test", "true");
        diag.add_error(code.code(), &info.message);
    }

    // Add some runtime warnings
    let runtime_errors = ErrorCatalog::by_category("runtime");
    assert!(!runtime_errors.is_empty());
    diag.add_warning(runtime_errors[0].code(), "Potential runtime issue");

    let summary = diag.summary();
    assert!(summary.error_count >= 1);
    assert!(summary.warning_count >= 1);
    assert!(summary.total >= 3);
}

// ===========================================================================
// 22. Protocol validate + batch + compress: Full protocol pipeline
// ===========================================================================

#[test]
fn protocol_validate_batch_compress_pipeline() {
    use abp_protocol::batch::{BatchProcessor, BatchRequest};
    use abp_protocol::compress::{CompressionAlgorithm, MessageCompressor};
    use abp_protocol::validate::EnvelopeValidator;
    use abp_protocol::Envelope;

    let wo = make_work_order("protocol pipeline");
    let receipt = make_receipt(wo.id);

    let envelopes = vec![
        Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: BackendIdentity {
                id: "mock".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: BTreeMap::new(),
            mode: ExecutionMode::Mapped,
        },
        Envelope::Run {
            id: "r1".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        },
    ];

    // Validate
    let validator = EnvelopeValidator::new();
    let seq_errors = validator.validate_sequence(&envelopes);
    assert!(seq_errors.is_empty());

    // Batch
    let processor = BatchProcessor::new();
    let request = BatchRequest {
        id: "proto-batch".into(),
        envelopes: envelopes.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    let response = processor.process(request);
    assert_eq!(response.results.len(), 4);

    // Compress batch result as JSON
    let json_data = serde_json::to_vec(&envelopes).unwrap();
    let compressor = MessageCompressor::new(CompressionAlgorithm::None);
    let compressed = compressor.compress(&json_data).unwrap();
    let decompressed = compressor.decompress(&compressed).unwrap();
    assert_eq!(json_data, decompressed);
}

// ===========================================================================
// 23. Policy audit + rules + compose: Complete policy pipeline
// ===========================================================================

#[test]
fn policy_audit_rules_compose_pipeline() {
    use abp_policy::audit::PolicyAuditor;
    use abp_policy::compose::{ComposedEngine, PolicyPrecedence, PolicyValidator};
    use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

    // Step 1: Rule engine evaluation
    let mut rule_engine = RuleEngine::new();
    rule_engine.add_rule(Rule {
        id: "deny-secrets".into(),
        description: "Deny reading secrets".into(),
        condition: RuleCondition::Pattern("secret*".into()),
        effect: RuleEffect::Deny,
        priority: 100,
    });
    rule_engine.add_rule(Rule {
        id: "log-config".into(),
        description: "Log config access".into(),
        condition: RuleCondition::Pattern("config*".into()),
        effect: RuleEffect::Log,
        priority: 50,
    });

    assert!(matches!(rule_engine.evaluate("secret.key"), RuleEffect::Deny));
    assert!(matches!(rule_engine.evaluate("config.toml"), RuleEffect::Log));
    assert!(matches!(rule_engine.evaluate("readme.md"), RuleEffect::Allow));

    // Step 2: Compose policies
    let p1 = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        deny_read: vec!["secret*".into()],
        ..PolicyProfile::default()
    };
    let p2 = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };

    // Validate policies
    let w1 = PolicyValidator::validate(&p1);
    let w2 = PolicyValidator::validate(&p2);
    assert!(w1.is_empty(), "p1 should have no warnings");
    assert!(w2.is_empty(), "p2 should have no warnings");

    let composed = ComposedEngine::new(vec![p1.clone(), p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(composed.check_tool("Read").is_allow());
    assert!(composed.check_tool("Bash").is_deny());
    assert!(composed.check_read("secret.key").is_deny());
    assert!(composed.check_write(".git/config").is_deny());

    // Step 3: Audit trail
    let engine = abp_policy::PolicyEngine::new(&p1).unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_read("secret.key");

    let summary = auditor.summary();
    assert!(summary.allowed >= 1);
    assert!(summary.denied >= 1);
    assert_eq!(auditor.entries().len(), 3);
}

// ===========================================================================
// 24. Receipt verify + hash + chain: Complete receipt integrity pipeline
// ===========================================================================

#[test]
fn receipt_verify_hash_chain_integrity() {
    use abp_core::chain::ReceiptChain;
    use abp_core::validate::validate_receipt;
    use abp_core::verify::{ChainVerifier, ReceiptVerifier};

    // Create multiple hashed receipts
    let receipts: Vec<Receipt> = (0..5)
        .map(|_| make_hashed_receipt(Uuid::new_v4()))
        .collect();

    // Validate each receipt
    for (i, r) in receipts.iter().enumerate() {
        validate_receipt(r).unwrap_or_else(|e| panic!("receipt {i} validation errors: {e:?}"));
    }

    // Verify each receipt
    let verifier = ReceiptVerifier::new();
    for (i, r) in receipts.iter().enumerate() {
        let report = verifier.verify(r);
        assert!(report.passed, "receipt {i} verification failed: {:?}", report.checks);
    }

    // Build and verify chain
    let mut chain = ReceiptChain::new();
    for r in &receipts {
        chain.push(r.clone()).unwrap();
    }
    chain.verify().unwrap();
    assert_eq!(chain.len(), 5);
    assert!(chain.success_rate() > 0.0);

    // Chain-level verification
    let chain_report = ChainVerifier::verify_chain(&receipts);
    assert!(chain_report.all_valid);
    assert_eq!(chain_report.receipt_count, 5);
    assert_eq!(chain_report.individual_reports.len(), 5);
}

// ===========================================================================
// 25. Multi-crate roundtrip: Touch every major crate
// ===========================================================================

#[test]
fn multi_crate_roundtrip() {
    use abp_core::aggregate::EventAggregator;
    use abp_core::chain::ReceiptChain;
    use abp_core::config::{ConfigDefaults, ConfigValidator};
    use abp_core::error::ErrorCatalog;
    use abp_core::filter::EventFilter;
    use abp_core::verify::ReceiptVerifier;
    use abp_glob::IncludeExcludeGlobs;
    use abp_integrations::capability::CapabilityMatrix;
    use abp_integrations::health::{HealthChecker, HealthStatus};
    use abp_integrations::metrics::MetricsRegistry;
    use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
    use abp_host::lifecycle::{LifecycleManager, LifecycleState};
    use abp_host::pool::{PoolConfig, SidecarPool};
    use abp_policy::audit::PolicyAuditor;
    use abp_policy::compose::{ComposedEngine, PolicyPrecedence};
    use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
    use abp_protocol::batch::{BatchProcessor, BatchRequest};
    use abp_protocol::compress::{CompressionAlgorithm, MessageCompressor};
    use abp_protocol::router::{MessageRoute, MessageRouter};
    use abp_protocol::validate::EnvelopeValidator;
    use abp_protocol::version::ProtocolVersion;
    use abp_protocol::{Envelope, JsonlCodec};
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};
    use abp_runtime::bus::EventBus;
    use abp_runtime::cancel::{CancellationToken, CancellableRun};
    use abp_runtime::observe::RuntimeObserver;
    use abp_workspace::ops::{FileOperation, OperationLog};
    use abp_workspace::template::WorkspaceTemplate;
    use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};

    // --- abp-core: Build work order + receipt ---
    let mut wo = WorkOrderBuilder::new("multi-crate roundtrip")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .policy(PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["secret*".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .build();
    ConfigDefaults::apply_defaults(&mut wo);
    let validator = ConfigValidator::new();
    assert!(!validator.validate_work_order(&wo).iter().any(|w| {
        matches!(w.severity, abp_core::config::WarningSeverity::Error)
    }));

    // --- abp-core: error catalog ---
    let all_errors = ErrorCatalog::all();
    assert!(!all_errors.is_empty());

    // --- abp-glob: pattern matching ---
    let globs = IncludeExcludeGlobs::new(
        &["src/**".to_string()],
        &["src/test/**".to_string()],
    )
    .unwrap();
    assert!(globs.decide_str("src/main.rs").is_allowed());

    // --- abp-policy: engine + audit + rules + compose ---
    let engine = abp_policy::PolicyEngine::new(&wo.policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);

    let mut auditor = PolicyAuditor::new(
        abp_policy::PolicyEngine::new(&wo.policy).unwrap(),
    );
    auditor.check_tool("Read");

    let mut rule_engine = RuleEngine::new();
    rule_engine.add_rule(Rule {
        id: "r1".into(),
        description: "test".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    assert!(matches!(rule_engine.evaluate("anything"), RuleEffect::Allow));

    let composed = ComposedEngine::new(
        vec![wo.policy.clone()],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();
    assert!(composed.check_tool("Read").is_allow());

    // --- abp-protocol: encode/decode + validate + batch + compress + route + version ---
    let envelope = Envelope::Run {
        id: "roundtrip".into(),
        work_order: wo.clone(),
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));

    let proto_validator = EnvelopeValidator::new();
    let vr = proto_validator.validate(&decoded);
    assert!(vr.errors.is_empty());

    let processor = BatchProcessor::new();
    let batch_resp = processor.process(BatchRequest {
        id: "b1".into(),
        envelopes: vec![decoded],
        created_at: Utc::now().to_rfc3339(),
    });
    assert_eq!(batch_resp.results.len(), 1);

    let compressor = MessageCompressor::new(CompressionAlgorithm::None);
    let compressed = compressor.compress(b"hello").unwrap();
    assert_eq!(compressor.decompress(&compressed).unwrap(), b"hello");

    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "run".into(),
        destination: "exec".into(),
        priority: 1,
    });
    assert_eq!(router.route_count(), 1);

    let version = ProtocolVersion::current();
    assert!(version.is_compatible(&ProtocolVersion::current()));

    // --- abp-integrations: capability + health + metrics + selector ---
    let mut matrix = CapabilityMatrix::new();
    matrix.register("mock", vec![Capability::Streaming]);
    assert!(matrix.supports("mock", &Capability::Streaming));

    let mut health = HealthChecker::new();
    health.add_check("all", HealthStatus::Healthy);
    assert!(health.is_healthy());

    let registry = MetricsRegistry::new();
    let m = registry.get_or_create("mock");
    m.record_run(true, 5, 100);
    assert_eq!(m.total_runs(), 1);

    let mut selector = BackendSelector::new(SelectionStrategy::FirstMatch);
    selector.add_candidate(BackendCandidate {
        name: "mock".into(),
        capabilities: vec![Capability::Streaming],
        priority: 1,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    assert!(selector.select(&[Capability::Streaming]).is_some());

    // --- abp-host: lifecycle + pool ---
    let mut lifecycle = LifecycleManager::new();
    lifecycle.transition(LifecycleState::Starting, None).unwrap();
    lifecycle.transition(LifecycleState::Ready, None).unwrap();

    let pool = SidecarPool::new(PoolConfig {
        min_size: 0,
        max_size: 2,
        idle_timeout: Duration::from_secs(60),
        health_check_interval: Duration::from_secs(10),
    });
    pool.add("roundtrip-sc");
    assert_eq!(pool.total_count(), 1);

    // --- abp-runtime: budget + cancel + bus + observe ---
    let budget = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(10_000),
        max_cost_usd: None,
        max_turns: None,
        max_duration: None,
    });
    budget.record_tokens(100);
    assert!(matches!(budget.check(), BudgetStatus::WithinLimits));

    let token = CancellationToken::new();
    let run = CancellableRun::new(token);
    assert!(!run.is_cancelled());

    let bus = EventBus::new();
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "roundtrip".into(),
    }));
    assert_eq!(bus.stats().total_published, 1);

    let mut observer = RuntimeObserver::new();
    observer.record_metric("test", 42.0);
    assert_eq!(observer.metrics()["test"], 42.0);

    // --- abp-workspace: ops + template + tracker ---
    let mut ops = OperationLog::new();
    ops.record(FileOperation::Write {
        path: "test.rs".into(),
        size: 100,
    });
    assert_eq!(ops.summary().writes, 1);

    let tmpl = WorkspaceTemplate::new("roundtrip", "test");
    assert_eq!(tmpl.file_count(), 0);

    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "test.rs".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    assert!(tracker.has_changes());

    // --- abp-core: events + aggregate + filter + verify + chain ---
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "go".into() }),
        make_event(AgentEventKind::AssistantMessage { text: "result".into() }),
        make_event(AgentEventKind::RunCompleted { message: "done".into() }),
    ];

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    assert_eq!(agg.event_count(), 3);

    let filter = EventFilter::include_kinds(&["assistant_message"]);
    let filtered: Vec<_> = events.iter().filter(|e| filter.matches(e)).collect();
    assert_eq!(filtered.len(), 1);

    let receipt = make_hashed_receipt(wo.id);
    let verifier = ReceiptVerifier::new();
    assert!(verifier.verify(&receipt).passed);

    let mut chain = ReceiptChain::new();
    chain.push(receipt).unwrap();
    chain.verify().unwrap();
}

// ===========================================================================
// 26. Budget warning threshold drives partial health degradation
// ===========================================================================

#[test]
fn budget_warning_causes_degraded_health() {
    use abp_integrations::health::{HealthChecker, HealthStatus};
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};

    let limit = BudgetLimit {
        max_tokens: Some(100),
        max_cost_usd: None,
        max_turns: None,
        max_duration: None,
    };
    let tracker = BudgetTracker::new(limit);
    tracker.record_tokens(85); // > 80% threshold

    let mut health = HealthChecker::new();
    match tracker.check() {
        BudgetStatus::Warning { usage_pct } => {
            assert!(usage_pct >= 80.0);
            health.add_check(
                "budget",
                HealthStatus::Degraded {
                    reason: format!("budget at {usage_pct:.0}%"),
                },
            );
        }
        other => panic!("expected warning, got {other:?}"),
    }

    assert!(!health.is_healthy());
    assert!(matches!(
        health.overall_status(),
        HealthStatus::Degraded { .. }
    ));
}

// ===========================================================================
// 27. Filter + Aggregate analytics: Success vs failure analysis
// ===========================================================================

#[test]
fn filter_aggregate_analytics() {
    use abp_core::aggregate::RunAnalytics;
    use abp_core::filter::EventFilter;

    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "go".into() }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            output: json!("content"),
            is_error: false,
        }),
        make_event(AgentEventKind::AssistantMessage { text: "analysis complete".into() }),
        make_event(AgentEventKind::RunCompleted { message: "done".into() }),
    ];

    // Filter to only tool-related events
    let tool_filter = EventFilter::include_kinds(&["tool_call", "tool_result"]);
    let tool_events: Vec<_> = events.iter().filter(|e| tool_filter.matches(e)).cloned().collect();
    assert_eq!(tool_events.len(), 2);

    // Analytics on full stream
    let analytics = RunAnalytics::from_events(&events);
    assert!(analytics.is_successful());
    assert!(analytics.tool_usage_ratio() > 0.0);
}

// ===========================================================================
// 28. Composed policy validator catches overlapping rules
// ===========================================================================

#[test]
fn policy_validator_detects_overlap() {
    use abp_policy::compose::{PolicySet, PolicyValidator, WarningKind};

    let profile = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Read".into()], // overlap with allowed
        ..PolicyProfile::default()
    };

    let warnings = PolicyValidator::validate(&profile);
    assert!(
        warnings.iter().any(|w| w.kind == WarningKind::OverlappingAllowDeny),
        "should detect overlapping allow/deny: {warnings:?}"
    );

    // PolicySet merges both profiles
    let mut set = PolicySet::new("test-set");
    set.add(profile);
    set.add(PolicyProfile {
        deny_read: vec!["secret*".into()],
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&"Read".into()));
    assert!(merged.deny_read.contains(&"secret*".into()));
}
