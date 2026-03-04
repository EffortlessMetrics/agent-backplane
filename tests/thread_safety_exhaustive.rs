#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive thread-safety tests for all ABP types that should be Send + Sync.
//!
//! These tests verify:
//! - Compile-time Send + Sync bounds for all core, protocol, policy, config, error,
//!   capability, receipt, IR, and integration types.
//! - Concurrent access patterns with Arc<Mutex<T>>.
//! - Cross-thread mpsc channel usage with AgentEvent streams.
//! - Shared BTreeMap state under concurrent mutation.
//! - Data-race freedom under contention.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ContractError, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};

use abp_core::aggregate::{AggregationSummary, EventAggregator, RunAnalytics};
use abp_core::config::{ConfigDefaults, ConfigValidator, ConfigWarning, WarningSeverity};
use abp_core::error::{ErrorCatalog, ErrorCode, ErrorInfo, MappingError, MappingErrorKind};
use abp_core::filter::EventFilter;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport, CapabilityReportEntry,
    DialectSupportLevel, NegotiationRequest, NegotiationResult,
};
use abp_core::stream::EventStream;
use abp_core::validate::ValidationError as CoreValidationError;
use abp_core::verify::{
    ChainBuilder as CoreChainBuilder, ChainEntry, ChainError as CoreChainError, ChainVerification,
    ChainVerificationReport, ReceiptChain as CoreReceiptChain, ReceiptVerifier, VerificationCheck,
};

use abp_protocol::{Envelope, JsonlCodec, ProtocolError, RawCodec, RawFrame};

use abp_policy::{Decision, PolicyEngine};

use abp_glob::{IncludeExcludeGlobs, MatchDecision};

use abp_error::{AbpError, AbpErrorDto, ErrorCategory};
use abp_error::{ErrorCode as AbpErrorCode, ErrorInfo as AbpErrorInfo};

use abp_config::{BackendEntry, BackplaneConfig, ConfigError, ConfigWarning as CfgConfigWarning};

use abp_capability::{
    CapabilityRegistry, CompatibilityReport, EmulationStrategy, NegotiationResult as CapNegResult,
    SupportLevel as CapSupportLevel,
};

use abp_receipt::{
    AuditIssue, AuditReport, ChainBuilder, ChainError, ChainGap, ChainSummary, FieldDiff,
    ReceiptAuditor, ReceiptChain, ReceiptDiff, ReceiptValidator as ReceiptCrateValidator,
    TamperEvidence, TamperKind, ValidationError as ReceiptValidationError, VerificationResult,
};

use abp_dialect::{
    DetectionResult, Dialect, DialectDetector, DialectValidator,
    ValidationError as DialectValidationError, ValidationResult as DialectValidationResult,
};

use abp_projection::{
    CompatibilityScore, DialectPair, FallbackEntry, ProjectionEntry, ProjectionError,
    ProjectionMatrix, ProjectionMode, ProjectionResult, ProjectionScore, RequiredEmulation,
    RoutingHop, RoutingPath,
};

use abp_mapping::{
    Fidelity, MappingError as MappingCrateError, MappingMatrix, MappingRegistry, MappingRule,
    MappingValidation,
};

use abp_retry::{CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy};

use abp_stream::{
    EventCollector, EventFilter as StreamEventFilter, EventMultiplexer, EventRecorder, EventStats,
    EventStream as StreamEventStream, EventTransform, MergedStream, MetricsSummary,
    StreamAggregator, StreamBuffer, StreamMetrics, StreamPipeline, StreamPipelineBuilder,
    StreamSummary, StreamTee, StreamTimeout, TeeError, TimeoutItem, ToolCallAggregate,
};

use abp_validate::{
    EnvelopeValidator, EventValidator, JsonType, ReceiptValidator as ValidateReceiptValidator,
    SchemaValidator, ValidationError as ValidateValidationError, ValidationErrorKind,
    ValidationErrors, Validator, WorkOrderValidator,
};

use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Compile-time assertions: every type listed must be Send + Sync.
// ---------------------------------------------------------------------------

// --- abp-core contract types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<WorkOrder>();
        _assert::<ExecutionLane>();
        _assert::<WorkspaceSpec>();
        _assert::<WorkspaceMode>();
        _assert::<ContextPacket>();
        _assert::<ContextSnippet>();
        _assert::<RuntimeConfig>();
        _assert::<PolicyProfile>();
        _assert::<CapabilityRequirements>();
        _assert::<CapabilityRequirement>();
        _assert::<MinSupport>();
        _assert::<Capability>();
        _assert::<SupportLevel>();
        _assert::<ExecutionMode>();
        _assert::<BackendIdentity>();
        _assert::<Receipt>();
        _assert::<RunMetadata>();
        _assert::<UsageNormalized>();
        _assert::<Outcome>();
        _assert::<ArtifactRef>();
        _assert::<VerificationReport>();
        _assert::<AgentEvent>();
        _assert::<AgentEventKind>();
        _assert::<ContractError>();
        _assert::<WorkOrderBuilder>();
        _assert::<ReceiptBuilder>();
    }
};

// --- abp-core error types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<ErrorCode>();
        _assert::<ErrorInfo>();
        _assert::<ErrorCatalog>();
        _assert::<MappingErrorKind>();
        _assert::<MappingError>();
    }
};

// --- abp-core config types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<WarningSeverity>();
        _assert::<ConfigWarning>();
        _assert::<ConfigValidator>();
        _assert::<ConfigDefaults>();
    }
};

// --- abp-core aggregate types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<EventAggregator>();
        _assert::<AggregationSummary>();
        _assert::<RunAnalytics>();
    }
};

// --- abp-core filter ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<EventFilter>();
    }
};

// --- abp-core stream ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<EventStream>();
    }
};

// --- abp-core IR types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<IrRole>();
        _assert::<IrContentBlock>();
        _assert::<IrMessage>();
        _assert::<IrToolDefinition>();
        _assert::<IrConversation>();
        _assert::<IrUsage>();
    }
};

// --- abp-core negotiate types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<NegotiationRequest>();
        _assert::<NegotiationResult>();
        _assert::<CapabilityNegotiator>();
        _assert::<CapabilityDiff>();
        _assert::<DialectSupportLevel>();
        _assert::<CapabilityReportEntry>();
        _assert::<CapabilityReport>();
    }
};

// --- abp-core validate ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<CoreValidationError>();
    }
};

// --- abp-core verify types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<VerificationCheck>();
        _assert::<ChainVerificationReport>();
        _assert::<ReceiptVerifier>();
        _assert::<CoreChainError>();
        _assert::<ChainEntry>();
        _assert::<CoreReceiptChain>();
        _assert::<CoreChainBuilder>();
        _assert::<ChainVerification>();
    }
};

// --- abp-core CapabilityManifest (BTreeMap alias) ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<BTreeMap<Capability, SupportLevel>>();
    }
};

// --- abp-protocol types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<Envelope>();
        _assert::<ProtocolError>();
        _assert::<JsonlCodec>();
        _assert::<RawFrame>();
        _assert::<RawCodec>();
    }
};

// --- abp-policy types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<Decision>();
        _assert::<PolicyEngine>();
    }
};

// --- abp-glob types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<MatchDecision>();
        _assert::<IncludeExcludeGlobs>();
    }
};

// --- abp-error types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<ErrorCategory>();
        _assert::<AbpErrorCode>();
        _assert::<AbpErrorInfo>();
        _assert::<AbpError>();
        _assert::<AbpErrorDto>();
    }
};

// --- abp-config types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<ConfigError>();
        _assert::<CfgConfigWarning>();
        _assert::<BackplaneConfig>();
        _assert::<BackendEntry>();
    }
};

// --- abp-capability types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<EmulationStrategy>();
        _assert::<CapSupportLevel>();
        _assert::<CapNegResult>();
        _assert::<CompatibilityReport>();
        _assert::<CapabilityRegistry>();
    }
};

// --- abp-receipt types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<VerificationResult>();
        _assert::<AuditIssue>();
        _assert::<AuditReport>();
        _assert::<ReceiptAuditor>();
        _assert::<ReceiptCrateValidator>();
        _assert::<ReceiptValidationError>();
        _assert::<ChainBuilder>();
        _assert::<ChainError>();
        _assert::<ChainGap>();
        _assert::<ChainSummary>();
        _assert::<ReceiptChain>();
        _assert::<TamperEvidence>();
        _assert::<TamperKind>();
        _assert::<FieldDiff>();
        _assert::<ReceiptDiff>();
    }
};

// --- abp-dialect types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<Dialect>();
        _assert::<DetectionResult>();
        _assert::<DialectDetector>();
        _assert::<DialectValidationError>();
        _assert::<DialectValidationResult>();
        _assert::<DialectValidator>();
    }
};

// --- abp-projection types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<ProjectionError>();
        _assert::<ProjectionScore>();
        _assert::<RequiredEmulation>();
        _assert::<RoutingHop>();
        _assert::<RoutingPath>();
        _assert::<CompatibilityScore>();
        _assert::<ProjectionMode>();
        _assert::<DialectPair>();
        _assert::<ProjectionEntry>();
        _assert::<FallbackEntry>();
        _assert::<ProjectionResult>();
        _assert::<ProjectionMatrix>();
    }
};

// --- abp-mapping types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<MappingCrateError>();
        _assert::<Fidelity>();
        _assert::<MappingRule>();
        _assert::<MappingValidation>();
        _assert::<MappingRegistry>();
        _assert::<MappingMatrix>();
    }
};

// --- abp-retry types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<RetryPolicy>();
        _assert::<CircuitState>();
        _assert::<CircuitBreakerError>();
        _assert::<CircuitBreaker>();
    }
};

// --- abp-stream types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<StreamAggregator>();
        _assert::<StreamSummary>();
        _assert::<ToolCallAggregate>();
        _assert::<StreamBuffer>();
        _assert::<EventCollector>();
        _assert::<MetricsSummary>();
        _assert::<StreamMetrics>();
        _assert::<TeeError>();
        _assert::<StreamEventFilter>();
        _assert::<EventTransform>();
        _assert::<EventRecorder>();
        _assert::<EventStats>();
        _assert::<StreamPipelineBuilder>();
    }
};

// --- abp-validate types ---
const _: () = {
    fn _assert<T: Send + Sync>() {}
    fn _check() {
        _assert::<EnvelopeValidator>();
        _assert::<EventValidator>();
        _assert::<ValidateReceiptValidator>();
        _assert::<JsonType>();
        _assert::<SchemaValidator>();
        _assert::<WorkOrderValidator>();
        _assert::<Validator>();
        _assert::<ValidateValidationError>();
        _assert::<ValidationErrorKind>();
        _assert::<ValidationErrors>();
    }
};

// ---------------------------------------------------------------------------
// Helper: build a minimal WorkOrder for tests.
// ---------------------------------------------------------------------------

fn make_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "thread-safety test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: None,
        context: None,
        policy: None,
        requirements: None,
        config: RuntimeConfig::default(),
    }
}

fn make_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: "abp/v0.1".into(),
            started_at: chrono::Utc::now(),
            finished_at: chrono::Utc::now(),
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: "1.0".into(),
            adapter_version: "1.0".into(),
        },
        execution_mode: ExecutionMode::Mapped,
        capabilities_used: BTreeMap::new(),
        usage: None,
        trace: vec![],
        artifacts: vec![],
        verification: None,
        outcome: Outcome::Complete,
        receipt_sha256: None,
        ext: BTreeMap::new(),
    }
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: chrono::Utc::now(),
        kind,
        ext: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Runtime tests: concurrent access via Arc<Mutex<T>>
// ---------------------------------------------------------------------------

#[test]
fn test_work_order_arc_mutex_concurrent_read() {
    let wo = Arc::new(Mutex::new(make_work_order()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let wo = Arc::clone(&wo);
            thread::spawn(move || {
                let guard = wo.lock().unwrap();
                let _ = guard.task.clone();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_receipt_arc_mutex_concurrent_read() {
    let r = Arc::new(Mutex::new(make_receipt()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let r = Arc::clone(&r);
            thread::spawn(move || {
                let guard = r.lock().unwrap();
                let _ = guard.meta.run_id;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_agent_event_arc_mutex_concurrent_access() {
    let ev = Arc::new(Mutex::new(make_agent_event(AgentEventKind::RunStarted)));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let ev = Arc::clone(&ev);
            thread::spawn(move || {
                let guard = ev.lock().unwrap();
                let _ = &guard.kind;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_work_order_arc_mutex_concurrent_write() {
    let wo = Arc::new(Mutex::new(make_work_order()));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let wo = Arc::clone(&wo);
            thread::spawn(move || {
                let mut guard = wo.lock().unwrap();
                guard.task = format!("task-{i}");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let guard = wo.lock().unwrap();
    assert!(guard.task.starts_with("task-"));
}

#[test]
fn test_receipt_arc_mutex_concurrent_write() {
    let r = Arc::new(Mutex::new(make_receipt()));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let r = Arc::clone(&r);
            thread::spawn(move || {
                let mut guard = r.lock().unwrap();
                guard.meta.duration_ms = i;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_agent_event_vec_concurrent_push() {
    let events = Arc::new(Mutex::new(Vec::<AgentEvent>::new()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let events = Arc::clone(&events);
            thread::spawn(move || {
                let ev = make_agent_event(AgentEventKind::RunStarted);
                events.lock().unwrap().push(ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(events.lock().unwrap().len(), 8);
}

// ---------------------------------------------------------------------------
// mpsc channel tests: AgentEvent streams across threads
// ---------------------------------------------------------------------------

#[test]
fn test_agent_event_mpsc_send() {
    let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
    let producer = thread::spawn(move || {
        for _ in 0..10 {
            tx.send(make_agent_event(AgentEventKind::RunStarted))
                .unwrap();
        }
    });
    producer.join().unwrap();
    let collected: Vec<_> = rx.iter().collect();
    assert_eq!(collected.len(), 10);
}

#[test]
fn test_agent_event_mpsc_multi_producer() {
    let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let tx = tx.clone();
            thread::spawn(move || {
                for _ in 0..5 {
                    tx.send(make_agent_event(AgentEventKind::AssistantDelta {
                        content: "hello".into(),
                    }))
                    .unwrap();
                }
            })
        })
        .collect();
    drop(tx);
    for h in handles {
        h.join().unwrap();
    }
    let collected: Vec<_> = rx.iter().collect();
    assert_eq!(collected.len(), 20);
}

#[test]
fn test_agent_event_mpsc_mixed_kinds() {
    let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
    let kinds = vec![
        AgentEventKind::RunStarted,
        AgentEventKind::RunCompleted,
        AgentEventKind::AssistantMessage {
            content: "msg".into(),
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
        },
    ];
    let kinds_len = kinds.len();
    let producer = thread::spawn(move || {
        for k in kinds {
            tx.send(make_agent_event(k)).unwrap();
        }
    });
    producer.join().unwrap();
    let collected: Vec<_> = rx.iter().collect();
    assert_eq!(collected.len(), kinds_len);
}

#[test]
fn test_agent_event_receiver_across_thread() {
    let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
    tx.send(make_agent_event(AgentEventKind::RunStarted))
        .unwrap();
    tx.send(make_agent_event(AgentEventKind::RunCompleted))
        .unwrap();
    drop(tx);
    let consumer = thread::spawn(move || {
        let collected: Vec<_> = rx.iter().collect();
        assert_eq!(collected.len(), 2);
        collected
    });
    let result = consumer.join().unwrap();
    assert_eq!(result.len(), 2);
}

#[test]
fn test_work_order_mpsc_channel() {
    let (tx, rx) = std::sync::mpsc::channel::<WorkOrder>();
    let producer = thread::spawn(move || {
        for _ in 0..5 {
            tx.send(make_work_order()).unwrap();
        }
    });
    producer.join().unwrap();
    let collected: Vec<_> = rx.iter().collect();
    assert_eq!(collected.len(), 5);
}

#[test]
fn test_receipt_mpsc_channel() {
    let (tx, rx) = std::sync::mpsc::channel::<Receipt>();
    let producer = thread::spawn(move || {
        for _ in 0..5 {
            tx.send(make_receipt()).unwrap();
        }
    });
    producer.join().unwrap();
    let collected: Vec<_> = rx.iter().collect();
    assert_eq!(collected.len(), 5);
}

#[test]
fn test_envelope_mpsc_channel() {
    let (tx, rx) = std::sync::mpsc::channel::<Envelope>();
    let producer = thread::spawn(move || {
        let env = Envelope::Hello {
            ref_id: Uuid::new_v4(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: "1.0".into(),
                adapter_version: "1.0".into(),
            },
            capabilities: BTreeMap::new(),
        };
        tx.send(env).unwrap();
    });
    producer.join().unwrap();
    let collected: Vec<_> = rx.iter().collect();
    assert_eq!(collected.len(), 1);
}

// ---------------------------------------------------------------------------
// RwLock patterns: concurrent readers, exclusive writers
// ---------------------------------------------------------------------------

#[test]
fn test_work_order_rwlock_concurrent_readers() {
    let wo = Arc::new(RwLock::new(make_work_order()));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let wo = Arc::clone(&wo);
            thread::spawn(move || {
                let guard = wo.read().unwrap();
                let _ = guard.task.len();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_receipt_rwlock_write_then_read() {
    let r = Arc::new(RwLock::new(make_receipt()));
    {
        let mut guard = r.write().unwrap();
        guard.meta.duration_ms = 999;
    }
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let r = Arc::clone(&r);
            thread::spawn(move || {
                let guard = r.read().unwrap();
                assert_eq!(guard.meta.duration_ms, 999);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_agent_event_rwlock_mixed_rw() {
    let events = Arc::new(RwLock::new(Vec::<AgentEvent>::new()));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let events = Arc::clone(&events);
            thread::spawn(move || {
                if i % 2 == 0 {
                    events
                        .write()
                        .unwrap()
                        .push(make_agent_event(AgentEventKind::RunStarted));
                } else {
                    let _ = events.read().unwrap().len();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// BTreeMap concurrent access (deterministic map types)
// ---------------------------------------------------------------------------

#[test]
fn test_capability_manifest_concurrent_insert() {
    let manifest = Arc::new(Mutex::new(BTreeMap::<Capability, SupportLevel>::new()));
    let capabilities = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Vision,
        Capability::ExtendedThinking,
    ];
    let handles: Vec<_> = capabilities
        .into_iter()
        .map(|cap| {
            let manifest = Arc::clone(&manifest);
            thread::spawn(move || {
                manifest.lock().unwrap().insert(cap, SupportLevel::Native);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert!(manifest.lock().unwrap().len() >= 1);
}

#[test]
fn test_btreemap_ext_concurrent_insert() {
    let ext = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let ext = Arc::clone(&ext);
            thread::spawn(move || {
                ext.lock()
                    .unwrap()
                    .insert(format!("key-{i}"), Value::from(i));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(ext.lock().unwrap().len(), 8);
}

#[test]
fn test_btreemap_ext_concurrent_read_write() {
    let ext = Arc::new(RwLock::new(BTreeMap::<String, Value>::new()));
    // Pre-populate
    for i in 0..4 {
        ext.write()
            .unwrap()
            .insert(format!("key-{i}"), Value::from(i));
    }
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let ext = Arc::clone(&ext);
            thread::spawn(move || {
                if i < 4 {
                    let guard = ext.read().unwrap();
                    let _ = guard.get(&format!("key-{}", i % 4));
                } else {
                    ext.write()
                        .unwrap()
                        .insert(format!("new-key-{i}"), Value::from(i));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Shared state: data race freedom tests
// ---------------------------------------------------------------------------

#[test]
fn test_no_data_race_work_order_clone_and_mutate() {
    let original = Arc::new(make_work_order());
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let original = Arc::clone(&original);
            thread::spawn(move || {
                let mut cloned = (*original).clone();
                cloned.task = format!("cloned-task-{i}");
                assert!(cloned.task.starts_with("cloned-task-"));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(original.task, "thread-safety test task");
}

#[test]
fn test_no_data_race_receipt_clone_and_mutate() {
    let original = Arc::new(make_receipt());
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let original = Arc::clone(&original);
            thread::spawn(move || {
                let mut cloned = (*original).clone();
                cloned.meta.duration_ms = i * 100;
                assert_eq!(cloned.meta.duration_ms, i * 100);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(original.meta.duration_ms, 42);
}

#[test]
fn test_no_data_race_agent_event_clone_and_mutate() {
    let original = Arc::new(make_agent_event(AgentEventKind::RunStarted));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let original = Arc::clone(&original);
            thread::spawn(move || {
                let mut cloned = (*original).clone();
                cloned.ext.insert(format!("thread-{i}"), Value::from(i));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert!(original.ext.is_empty());
}

// ---------------------------------------------------------------------------
// Cross-thread ownership transfer tests (Send)
// ---------------------------------------------------------------------------

#[test]
fn test_work_order_send_to_thread_and_back() {
    let wo = make_work_order();
    let handle = thread::spawn(move || {
        assert!(!wo.task.is_empty());
        wo
    });
    let returned = handle.join().unwrap();
    assert_eq!(returned.task, "thread-safety test task");
}

#[test]
fn test_receipt_send_to_thread_and_back() {
    let r = make_receipt();
    let id = r.meta.run_id;
    let handle = thread::spawn(move || {
        assert_eq!(r.meta.contract_version, "abp/v0.1");
        r
    });
    let returned = handle.join().unwrap();
    assert_eq!(returned.meta.run_id, id);
}

#[test]
fn test_agent_event_send_to_thread_and_back() {
    let ev = make_agent_event(AgentEventKind::RunStarted);
    let handle = thread::spawn(move || ev);
    let returned = handle.join().unwrap();
    assert!(matches!(returned.kind, AgentEventKind::RunStarted));
}

#[test]
fn test_envelope_send_to_thread_and_back() {
    let env = Envelope::Hello {
        ref_id: Uuid::new_v4(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: "1.0".into(),
            adapter_version: "1.0".into(),
        },
        capabilities: BTreeMap::new(),
    };
    let handle = thread::spawn(move || env);
    let returned = handle.join().unwrap();
    assert!(matches!(returned, Envelope::Hello { .. }));
}

#[test]
fn test_policy_engine_send_to_thread() {
    let profile = PolicyProfile::default();
    let engine = PolicyEngine::new(&profile);
    let handle = thread::spawn(move || {
        let _ = engine.check_tool("bash");
        engine
    });
    let returned = handle.join().unwrap();
    let _ = returned.check_tool("read");
}

#[test]
fn test_policy_decision_send() {
    let profile = PolicyProfile::default();
    let engine = PolicyEngine::new(&profile);
    let decision = engine.check_tool("bash");
    let handle = thread::spawn(move || decision);
    let _ = handle.join().unwrap();
}

#[test]
fn test_error_types_send() {
    let error = AbpError {
        code: abp_error::ErrorCode::ContractSerializationFailed,
        message: "test".into(),
        source: None,
        context: BTreeMap::new(),
    };
    let handle = thread::spawn(move || error);
    let _ = handle.join().unwrap();
}

#[test]
fn test_config_types_send() {
    let config = BackplaneConfig::default();
    let handle = thread::spawn(move || config);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent serialization / deserialization
// ---------------------------------------------------------------------------

#[test]
fn test_work_order_concurrent_serde() {
    let wo = Arc::new(make_work_order());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let wo = Arc::clone(&wo);
            thread::spawn(move || {
                let json = serde_json::to_string(&*wo).unwrap();
                let _: WorkOrder = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_receipt_concurrent_serde() {
    let r = Arc::new(make_receipt());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let r = Arc::clone(&r);
            thread::spawn(move || {
                let json = serde_json::to_string(&*r).unwrap();
                let _: Receipt = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_agent_event_concurrent_serde() {
    let ev = Arc::new(make_agent_event(AgentEventKind::AssistantMessage {
        content: "hello".into(),
    }));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let ev = Arc::clone(&ev);
            thread::spawn(move || {
                let json = serde_json::to_string(&*ev).unwrap();
                let _: AgentEvent = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_envelope_concurrent_serde() {
    let env = Arc::new(Envelope::Hello {
        ref_id: Uuid::new_v4(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: "1.0".into(),
            adapter_version: "1.0".into(),
        },
        capabilities: BTreeMap::new(),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let env = Arc::clone(&env);
            thread::spawn(move || {
                let json = serde_json::to_string(&*env).unwrap();
                let _: Envelope = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent builder usage
// ---------------------------------------------------------------------------

#[test]
fn test_work_order_builder_concurrent() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let wo = WorkOrderBuilder::new(format!("task-{i}")).build();
                assert!(wo.task.starts_with("task-"));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_receipt_builder_concurrent() {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(move || {
                let wo = make_work_order();
                let backend = BackendIdentity {
                    id: "mock".into(),
                    backend_version: "1.0".into(),
                    adapter_version: "1.0".into(),
                };
                let r = ReceiptBuilder::new(&wo, backend).build();
                assert_eq!(r.meta.contract_version, "abp/v0.1");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent event aggregation
// ---------------------------------------------------------------------------

#[test]
fn test_event_aggregator_concurrent_feed() {
    let agg = Arc::new(Mutex::new(EventAggregator::new()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let agg = Arc::clone(&agg);
            thread::spawn(move || {
                let ev = make_agent_event(AgentEventKind::RunStarted);
                agg.lock().unwrap().push(&ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let summary = agg.lock().unwrap().summary();
    assert!(summary.total > 0);
}

#[test]
fn test_event_filter_concurrent_use() {
    let filter = Arc::new(EventFilter::kind("run_started"));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let filter = Arc::clone(&filter);
            thread::spawn(move || {
                let ev = make_agent_event(AgentEventKind::RunStarted);
                let _ = filter.matches(&ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent verification
// ---------------------------------------------------------------------------

#[test]
fn test_receipt_verifier_concurrent() {
    let verifier = Arc::new(ReceiptVerifier::new());
    let receipt = make_receipt();
    let receipt = Arc::new(receipt);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let verifier = Arc::clone(&verifier);
            let receipt = Arc::clone(&receipt);
            thread::spawn(move || {
                let _ = verifier.verify(&receipt);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent policy checks
// ---------------------------------------------------------------------------

#[test]
fn test_policy_engine_concurrent_tool_check() {
    let profile = PolicyProfile::default();
    let engine = Arc::new(PolicyEngine::new(&profile));
    let tools = vec![
        "bash",
        "read",
        "write",
        "glob",
        "grep",
        "edit",
        "web_search",
        "ask_user",
    ];
    let handles: Vec<_> = tools
        .into_iter()
        .map(|tool| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                let _ = engine.check_tool(tool);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_policy_engine_concurrent_path_check() {
    let profile = PolicyProfile::default();
    let engine = Arc::new(PolicyEngine::new(&profile));
    let paths = vec!["src/main.rs", "tests/foo.rs", "Cargo.toml", "README.md"];
    let handles: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                let _ = engine.check_read(path);
                let _ = engine.check_write(path);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent glob matching
// ---------------------------------------------------------------------------

#[test]
fn test_include_exclude_globs_concurrent() {
    let globs =
        IncludeExcludeGlobs::new(&["src/**/*.rs".to_string()], &["target/**".to_string()]).unwrap();
    let globs = Arc::new(globs);
    let paths = vec![
        "src/main.rs",
        "src/lib.rs",
        "target/debug/foo",
        "tests/test.rs",
    ];
    let handles: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let globs = Arc::clone(&globs);
            thread::spawn(move || {
                let _ = globs.decide(path);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent JSONL codec usage
// ---------------------------------------------------------------------------

#[test]
fn test_jsonl_codec_concurrent_encode() {
    let codec = Arc::new(JsonlCodec::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let codec = Arc::clone(&codec);
            thread::spawn(move || {
                let env = Envelope::Hello {
                    ref_id: Uuid::new_v4(),
                    backend: BackendIdentity {
                        id: "test".into(),
                        backend_version: "1.0".into(),
                        adapter_version: "1.0".into(),
                    },
                    capabilities: BTreeMap::new(),
                };
                let _ = codec.encode(&env);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_jsonl_codec_concurrent_decode() {
    let codec = Arc::new(JsonlCodec::new());
    let env = Envelope::Hello {
        ref_id: Uuid::new_v4(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: "1.0".into(),
            adapter_version: "1.0".into(),
        },
        capabilities: BTreeMap::new(),
    };
    let encoded = codec.encode(&env).unwrap();
    let encoded = Arc::new(encoded);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let codec = Arc::clone(&codec);
            let encoded = Arc::clone(&encoded);
            thread::spawn(move || {
                let _: Envelope = codec.decode(&encoded).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent error creation and access
// ---------------------------------------------------------------------------

#[test]
fn test_abp_error_concurrent_creation() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let error = AbpError {
                    code: abp_error::ErrorCode::ContractSerializationFailed,
                    message: format!("error-{i}"),
                    source: None,
                    context: BTreeMap::new(),
                };
                assert!(error.message.starts_with("error-"));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_abp_error_dto_concurrent_serde() {
    let dto = Arc::new(AbpErrorDto {
        code: abp_error::ErrorCode::ContractSerializationFailed,
        message: "test error".into(),
        context: BTreeMap::new(),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let dto = Arc::clone(&dto);
            thread::spawn(move || {
                let json = serde_json::to_string(&*dto).unwrap();
                let _: AbpErrorDto = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent IR type access
// ---------------------------------------------------------------------------

#[test]
fn test_ir_message_concurrent_create() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let msg = IrMessage {
                    role: IrRole::User,
                    content: vec![IrContentBlock::Text {
                        text: format!("msg-{i}"),
                    }],
                    metadata: BTreeMap::new(),
                };
                assert_eq!(msg.role, IrRole::User);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_ir_conversation_concurrent_serde() {
    let conv = Arc::new(IrConversation {
        messages: vec![IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentBlock::Text {
                text: "hello".into(),
            }],
            metadata: BTreeMap::new(),
        }],
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let conv = Arc::clone(&conv);
            thread::spawn(move || {
                let json = serde_json::to_string(&*conv).unwrap();
                let _: IrConversation = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_ir_tool_definition_concurrent_access() {
    let tool = Arc::new(IrToolDefinition {
        name: "bash".into(),
        description: "run shell commands".into(),
        parameters: serde_json::json!({"type": "object"}),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let tool = Arc::clone(&tool);
            thread::spawn(move || {
                let _ = tool.name.clone();
                let _ = tool.description.clone();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent capability negotiation
// ---------------------------------------------------------------------------

#[test]
fn test_capability_negotiator_concurrent() {
    let negotiator = Arc::new(CapabilityNegotiator);
    let manifest: BTreeMap<Capability, SupportLevel> = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::Vision, SupportLevel::Emulated),
    ]);
    let manifest = Arc::new(manifest);
    let request = Arc::new(NegotiationRequest {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
        preferred: vec![],
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let negotiator = Arc::clone(&negotiator);
            let manifest = Arc::clone(&manifest);
            let request = Arc::clone(&request);
            thread::spawn(move || {
                let _ = negotiator.negotiate(&request, &manifest);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent dialect detection
// ---------------------------------------------------------------------------

#[test]
fn test_dialect_detector_concurrent() {
    let detector = Arc::new(DialectDetector::new());
    let samples = vec![
        r#"{"model":"gpt-4","messages":[]}"#,
        r#"{"model":"claude-3","messages":[]}"#,
        r#"{"model":"gemini-pro","contents":[]}"#,
    ];
    let handles: Vec<_> = samples
        .into_iter()
        .map(|sample| {
            let detector = Arc::clone(&detector);
            thread::spawn(move || {
                let val: Value = serde_json::from_str(sample).unwrap();
                let _ = detector.detect(&val);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent receipt chain building
// ---------------------------------------------------------------------------

#[test]
fn test_receipt_chain_concurrent_read() {
    let mut chain = ReceiptChain::new();
    for _ in 0..4 {
        let r = make_receipt();
        let _ = chain.append(r);
    }
    let chain = Arc::new(chain);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let chain = Arc::clone(&chain);
            thread::spawn(move || {
                let _ = chain.len();
                let _ = chain.summary();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent config validation
// ---------------------------------------------------------------------------

#[test]
fn test_config_concurrent_serde() {
    let config = Arc::new(BackplaneConfig::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let config = Arc::clone(&config);
            thread::spawn(move || {
                let json = serde_json::to_string(&*config).unwrap();
                let _: BackplaneConfig = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent projection matrix
// ---------------------------------------------------------------------------

#[test]
fn test_projection_matrix_concurrent_access() {
    let matrix = Arc::new(ProjectionMatrix::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let matrix = Arc::clone(&matrix);
            thread::spawn(move || {
                let _ = matrix.entries();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent retry policy
// ---------------------------------------------------------------------------

#[test]
fn test_retry_policy_concurrent_access() {
    let policy = Arc::new(RetryPolicy::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let policy = Arc::clone(&policy);
            thread::spawn(move || {
                let _ = policy.max_retries();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent mapping types
// ---------------------------------------------------------------------------

#[test]
fn test_mapping_registry_concurrent_access() {
    let registry = Arc::new(MappingRegistry::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let registry = Arc::clone(&registry);
            thread::spawn(move || {
                let _ = registry.rules();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent validate types
// ---------------------------------------------------------------------------

#[test]
fn test_envelope_validator_concurrent() {
    let validator = Arc::new(EnvelopeValidator::new());
    let env = Envelope::Hello {
        ref_id: Uuid::new_v4(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: "1.0".into(),
            adapter_version: "1.0".into(),
        },
        capabilities: BTreeMap::new(),
    };
    let env = Arc::new(env);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let validator = Arc::clone(&validator);
            let env = Arc::clone(&env);
            thread::spawn(move || {
                let _ = validator.validate(&env);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent stream aggregator
// ---------------------------------------------------------------------------

#[test]
fn test_stream_aggregator_concurrent_feed() {
    let agg = Arc::new(Mutex::new(StreamAggregator::new()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let agg = Arc::clone(&agg);
            thread::spawn(move || {
                let ev = make_agent_event(AgentEventKind::RunStarted);
                agg.lock().unwrap().push(ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Stress test: high-contention shared WorkOrder
// ---------------------------------------------------------------------------

#[test]
fn test_high_contention_work_order() {
    let wo = Arc::new(Mutex::new(make_work_order()));
    let handles: Vec<_> = (0..32)
        .map(|i| {
            let wo = Arc::clone(&wo);
            thread::spawn(move || {
                for j in 0..10 {
                    let mut guard = wo.lock().unwrap();
                    guard.task = format!("t-{i}-{j}");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Stress test: high-contention shared Receipt
// ---------------------------------------------------------------------------

#[test]
fn test_high_contention_receipt() {
    let r = Arc::new(Mutex::new(make_receipt()));
    let handles: Vec<_> = (0..32)
        .map(|i| {
            let r = Arc::clone(&r);
            thread::spawn(move || {
                for j in 0..10 {
                    let mut guard = r.lock().unwrap();
                    guard.meta.duration_ms = (i * 10 + j) as u64;
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Stress test: high-contention event vector
// ---------------------------------------------------------------------------

#[test]
fn test_high_contention_event_vec() {
    let events = Arc::new(Mutex::new(Vec::<AgentEvent>::new()));
    let handles: Vec<_> = (0..32)
        .map(|_| {
            let events = Arc::clone(&events);
            thread::spawn(move || {
                for _ in 0..10 {
                    events
                        .lock()
                        .unwrap()
                        .push(make_agent_event(AgentEventKind::RunStarted));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(events.lock().unwrap().len(), 320);
}

// ---------------------------------------------------------------------------
// Concurrent event kind construction
// ---------------------------------------------------------------------------

#[test]
fn test_agent_event_kind_variants_send() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted,
        AgentEventKind::RunCompleted,
        AgentEventKind::AssistantDelta {
            content: "delta".into(),
        },
        AgentEventKind::AssistantMessage {
            content: "message".into(),
        },
        AgentEventKind::ToolCall {
            tool: "bash".into(),
            input: Value::Null,
        },
        AgentEventKind::ToolResult {
            tool: "bash".into(),
            output: Value::String("ok".into()),
        },
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
        },
    ];
    let handles: Vec<_> = kinds
        .into_iter()
        .map(|k| {
            thread::spawn(move || {
                let ev = make_agent_event(k);
                let _ = serde_json::to_string(&ev).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent outcome enum access
// ---------------------------------------------------------------------------

#[test]
fn test_outcome_concurrent_match() {
    let outcomes = vec![Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let outcomes = Arc::new(outcomes);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let outcomes = Arc::clone(&outcomes);
            thread::spawn(move || {
                let o = &outcomes[i % 3];
                match o {
                    Outcome::Complete => "complete",
                    Outcome::Partial => "partial",
                    Outcome::Failed => "failed",
                };
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent access to WorkspaceSpec
// ---------------------------------------------------------------------------

#[test]
fn test_workspace_spec_concurrent_clone() {
    let spec = Arc::new(WorkspaceSpec {
        root: "src".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["**/*.rs".into()],
        exclude: vec!["target/**".into()],
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let spec = Arc::clone(&spec);
            thread::spawn(move || {
                let cloned = (*spec).clone();
                assert_eq!(cloned.root, "src");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent RuntimeConfig access
// ---------------------------------------------------------------------------

#[test]
fn test_runtime_config_concurrent_serde() {
    let config = Arc::new(RuntimeConfig::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let config = Arc::clone(&config);
            thread::spawn(move || {
                let json = serde_json::to_string(&*config).unwrap();
                let _: RuntimeConfig = serde_json::from_str(&json).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent ContextPacket access
// ---------------------------------------------------------------------------

#[test]
fn test_context_packet_concurrent_access() {
    let ctx = Arc::new(ContextPacket {
        files: BTreeMap::from([("main.rs".into(), "fn main() {}".into())]),
        snippets: vec![ContextSnippet {
            name: "example".into(),
            content: "snippet content".into(),
        }],
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let ctx = Arc::clone(&ctx);
            thread::spawn(move || {
                let _ = ctx.files.len();
                let _ = ctx.snippets.len();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent UsageNormalized access
// ---------------------------------------------------------------------------

#[test]
fn test_usage_normalized_concurrent() {
    let usage = Arc::new(UsageNormalized {
        input_tokens: 100,
        output_tokens: 200,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
        request_units: None,
        estimated_cost_usd: None,
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let usage = Arc::clone(&usage);
            thread::spawn(move || {
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 200);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent BackendIdentity access
// ---------------------------------------------------------------------------

#[test]
fn test_backend_identity_concurrent() {
    let id = Arc::new(BackendIdentity {
        id: "openai".into(),
        backend_version: "4.0".into(),
        adapter_version: "1.0".into(),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let id = Arc::clone(&id);
            thread::spawn(move || {
                assert_eq!(id.id, "openai");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent ErrorInfo access
// ---------------------------------------------------------------------------

#[test]
fn test_error_info_concurrent() {
    let info = Arc::new(ErrorInfo {
        code: ErrorCode::ContractSerializationFailed,
        message: "test error".into(),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let info = Arc::clone(&info);
            thread::spawn(move || {
                let _ = info.message.clone();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent ErrorCatalog access
// ---------------------------------------------------------------------------

#[test]
fn test_error_catalog_concurrent() {
    let catalog = Arc::new(ErrorCatalog);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let catalog = Arc::clone(&catalog);
            thread::spawn(move || {
                let _ = catalog.lookup(ErrorCode::ContractSerializationFailed);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent MappingError creation
// ---------------------------------------------------------------------------

#[test]
fn test_mapping_error_send() {
    let handles: Vec<_> = (0..4)
        .map(|_| {
            thread::spawn(|| {
                let _ = MappingError::Unsupported(MappingErrorKind::UnsupportedCapability);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Producer-consumer pattern with multiple event types
// ---------------------------------------------------------------------------

#[test]
fn test_producer_consumer_event_pipeline() {
    let (tx, rx) = std::sync::mpsc::sync_channel::<AgentEvent>(64);

    let producer = thread::spawn(move || {
        tx.send(make_agent_event(AgentEventKind::RunStarted))
            .unwrap();
        for i in 0..5 {
            tx.send(make_agent_event(AgentEventKind::AssistantDelta {
                content: format!("chunk-{i}"),
            }))
            .unwrap();
        }
        tx.send(make_agent_event(AgentEventKind::AssistantMessage {
            content: "final message".into(),
        }))
        .unwrap();
        tx.send(make_agent_event(AgentEventKind::RunCompleted))
            .unwrap();
    });

    let consumer = thread::spawn(move || {
        let events: Vec<_> = rx.iter().collect();
        assert_eq!(events.len(), 8);
        events
    });

    producer.join().unwrap();
    let events = consumer.join().unwrap();
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted));
    assert!(matches!(
        events[events.len() - 1].kind,
        AgentEventKind::RunCompleted
    ));
}

// ---------------------------------------------------------------------------
// Fan-out / fan-in pattern
// ---------------------------------------------------------------------------

#[test]
fn test_fan_out_fan_in_receipts() {
    let (tx, rx) = std::sync::mpsc::channel::<Receipt>();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let tx = tx.clone();
            thread::spawn(move || {
                let r = make_receipt();
                tx.send(r).unwrap();
            })
        })
        .collect();
    drop(tx);
    for h in handles {
        h.join().unwrap();
    }
    let receipts: Vec<_> = rx.iter().collect();
    assert_eq!(receipts.len(), 8);
}

// ---------------------------------------------------------------------------
// Concurrent receipt auditing
// ---------------------------------------------------------------------------

#[test]
fn test_receipt_auditor_concurrent() {
    let auditor = Arc::new(ReceiptAuditor::new());
    let receipt = Arc::new(make_receipt());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let auditor = Arc::clone(&auditor);
            let receipt = Arc::clone(&receipt);
            thread::spawn(move || {
                let _ = auditor.audit(&receipt);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent capability registry
// ---------------------------------------------------------------------------

#[test]
fn test_capability_registry_concurrent_read() {
    let registry = Arc::new(CapabilityRegistry::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let registry = Arc::clone(&registry);
            thread::spawn(move || {
                let _ = registry.all_capabilities();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent SupportLevel comparisons
// ---------------------------------------------------------------------------

#[test]
fn test_support_level_concurrent_compare() {
    let levels = Arc::new(vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
    ]);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let levels = Arc::clone(&levels);
            thread::spawn(move || {
                let a = &levels[i % 3];
                let b = &levels[(i + 1) % 3];
                let _ = a == b;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent Capability comparisons
// ---------------------------------------------------------------------------

#[test]
fn test_capability_concurrent_compare() {
    let caps = Arc::new(vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Vision,
        Capability::ExtendedThinking,
    ]);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let caps = Arc::clone(&caps);
            thread::spawn(move || {
                let a = &caps[i % 4];
                let b = &caps[(i + 1) % 4];
                let _ = a == b;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent MatchDecision usage
// ---------------------------------------------------------------------------

#[test]
fn test_match_decision_concurrent() {
    let decisions = Arc::new(vec![
        MatchDecision::Include,
        MatchDecision::Exclude,
        MatchDecision::NoMatch,
    ]);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let decisions = Arc::clone(&decisions);
            thread::spawn(move || {
                let d = &decisions[i % 3];
                let _ = format!("{:?}", d);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent ArtifactRef access
// ---------------------------------------------------------------------------

#[test]
fn test_artifact_ref_concurrent() {
    let artifact = Arc::new(ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let artifact = Arc::clone(&artifact);
            thread::spawn(move || {
                assert_eq!(artifact.kind, "patch");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent IrUsage access
// ---------------------------------------------------------------------------

#[test]
fn test_ir_usage_concurrent() {
    let usage = Arc::new(IrUsage {
        input_tokens: Some(100),
        output_tokens: Some(200),
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let usage = Arc::clone(&usage);
            thread::spawn(move || {
                assert_eq!(usage.input_tokens, Some(100));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}
