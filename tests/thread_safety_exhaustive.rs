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
use std::path::Path;
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

use abp_protocol::{Envelope, JsonlCodec, ProtocolError};

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
    EventFilter as StreamEventFilter, EventRecorder, EventStats, EventTransform, MetricsSummary,
    StreamAggregator, StreamBuffer, StreamMetrics, StreamSummary, TeeError, ToolCallAggregate,
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
        _assert::<CircuitBreakerError<String>>();
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
        _assert::<MetricsSummary>();
        _assert::<StreamMetrics>();
        _assert::<TeeError>();
        _assert::<StreamEventFilter>();
        _assert::<EventTransform>();
        _assert::<EventRecorder>();
        _assert::<EventStats>();
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
        _assert::<ValidateValidationError>();
        _assert::<ValidationErrorKind>();
        _assert::<ValidationErrors>();
    }
};

// ---------------------------------------------------------------------------
// Helper: build values for tests.
// ---------------------------------------------------------------------------

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("thread-safety test task").build()
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock").build()
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: chrono::Utc::now(),
        kind,
        ext: None,
    }
}

fn run_started() -> AgentEventKind {
    AgentEventKind::RunStarted {
        message: "started".into(),
    }
}

fn run_completed() -> AgentEventKind {
    AgentEventKind::RunCompleted {
        message: "completed".into(),
    }
}

fn assistant_delta() -> AgentEventKind {
    AgentEventKind::AssistantDelta {
        text: "hello".into(),
    }
}

fn assistant_message() -> AgentEventKind {
    AgentEventKind::AssistantMessage {
        text: "hello world".into(),
    }
}

fn tool_call_event() -> AgentEventKind {
    AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: Value::Null,
    }
}

fn tool_result_event() -> AgentEventKind {
    AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: Value::String("ok".into()),
        is_error: false,
    }
}

fn warning_event() -> AgentEventKind {
    AgentEventKind::Warning {
        message: "warn".into(),
    }
}

fn error_event() -> AgentEventKind {
    AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    }
}

fn make_envelope() -> Envelope {
    Envelope::Hello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
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
    let ev = Arc::new(Mutex::new(make_agent_event(run_started())));
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
                let ev = make_agent_event(run_started());
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
            tx.send(make_agent_event(run_started())).unwrap();
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
                    tx.send(make_agent_event(assistant_delta())).unwrap();
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
        run_started(),
        run_completed(),
        assistant_message(),
        warning_event(),
        error_event(),
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
    tx.send(make_agent_event(run_started())).unwrap();
    tx.send(make_agent_event(run_completed())).unwrap();
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
        tx.send(make_envelope()).unwrap();
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
                        .push(make_agent_event(run_started()));
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
}

#[test]
fn test_no_data_race_agent_event_clone_and_mutate() {
    let original = Arc::new(make_agent_event(run_started()));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let original = Arc::clone(&original);
            thread::spawn(move || {
                let mut cloned = (*original).clone();
                cloned.ext = Some(BTreeMap::from([(format!("thread-{i}"), Value::from(i))]));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert!(original.ext.is_none());
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
    let ev = make_agent_event(run_started());
    let handle = thread::spawn(move || ev);
    let returned = handle.join().unwrap();
    assert!(matches!(returned.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn test_envelope_send_to_thread_and_back() {
    let env = make_envelope();
    let handle = thread::spawn(move || env);
    let returned = handle.join().unwrap();
    assert!(matches!(returned, Envelope::Hello { .. }));
}

#[test]
fn test_policy_engine_send_to_thread() {
    let profile = PolicyProfile::default();
    let engine = PolicyEngine::new(&profile).unwrap();
    let handle = thread::spawn(move || {
        let _ = engine.can_use_tool("bash");
        engine
    });
    let returned = handle.join().unwrap();
    let _ = returned.can_use_tool("read");
}

#[test]
fn test_policy_decision_send() {
    let profile = PolicyProfile::default();
    let engine = PolicyEngine::new(&profile).unwrap();
    let decision = engine.can_use_tool("bash");
    let handle = thread::spawn(move || decision);
    let _ = handle.join().unwrap();
}

#[test]
fn test_error_types_send() {
    let error = AbpError {
        code: abp_error::ErrorCode::ProtocolInvalidEnvelope,
        message: "test".into(),
        source: None,
        context: BTreeMap::new(),
        location: None,
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
    let ev = Arc::new(make_agent_event(assistant_message()));
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
    let env = Arc::new(make_envelope());
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
                let r = ReceiptBuilder::new("mock").build();
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
                let ev = make_agent_event(run_started());
                agg.lock().unwrap().add(&ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let summary = agg.lock().unwrap().summary();
    assert!(summary.total_events > 0);
}

#[test]
fn test_event_filter_concurrent_use() {
    let filter = Arc::new(EventFilter::include_kinds(&["run_started"]));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let filter = Arc::clone(&filter);
            thread::spawn(move || {
                let ev = make_agent_event(run_started());
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
    let receipt = Arc::new(make_receipt());
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
    let engine = Arc::new(PolicyEngine::new(&profile).unwrap());
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
                let _ = engine.can_use_tool(tool);
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
    let engine = Arc::new(PolicyEngine::new(&profile).unwrap());
    let paths = vec!["src/main.rs", "tests/foo.rs", "Cargo.toml", "README.md"];
    let handles: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                let _ = engine.can_read_path(Path::new(path));
                let _ = engine.can_write_path(Path::new(path));
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
                let _ = globs.decide_str(path);
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
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(move || {
                let env = make_envelope();
                let _ = JsonlCodec::encode(&env);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_jsonl_codec_concurrent_decode() {
    let env = make_envelope();
    let encoded = JsonlCodec::encode(&env).unwrap();
    let encoded = Arc::new(encoded);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let encoded = Arc::clone(&encoded);
            thread::spawn(move || {
                let _: Envelope = JsonlCodec::decode(&encoded).unwrap();
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
                    code: abp_error::ErrorCode::ProtocolInvalidEnvelope,
                    message: format!("error-{i}"),
                    source: None,
                    context: BTreeMap::new(),
                    location: None,
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
        code: abp_error::ErrorCode::ProtocolInvalidEnvelope,
        message: "test error".into(),
        context: BTreeMap::new(),
        source_message: None,
        location: None,
        cause_chain: Vec::new(),
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
    let manifest: BTreeMap<Capability, SupportLevel> = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::Vision, SupportLevel::Emulated),
    ]);
    let manifest = Arc::new(manifest);
    let request = Arc::new(NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let manifest = Arc::clone(&manifest);
            let request = Arc::clone(&request);
            thread::spawn(move || {
                let _ = CapabilityNegotiator::negotiate(&request, &manifest);
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
// Concurrent receipt chain operations
// ---------------------------------------------------------------------------

#[test]
fn test_receipt_chain_concurrent_read() {
    let mut chain = ReceiptChain::new();
    for _ in 0..4 {
        let r = make_receipt();
        let _ = chain.push(r);
    }
    let chain = Arc::new(chain);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let chain = Arc::clone(&chain);
            thread::spawn(move || {
                let _ = chain.len();
                let _ = chain.chain_summary();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent config serde
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
// Concurrent projection matrix access
// ---------------------------------------------------------------------------

#[test]
fn test_projection_matrix_concurrent_access() {
    let matrix = Arc::new(ProjectionMatrix::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let matrix = Arc::clone(&matrix);
            thread::spawn(move || {
                let _ = &*matrix;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent retry policy access
// ---------------------------------------------------------------------------

#[test]
fn test_retry_policy_concurrent_access() {
    let policy = Arc::new(RetryPolicy::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let policy = Arc::clone(&policy);
            thread::spawn(move || {
                let _ = policy.max_retries;
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
                let _ = registry.len();
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
    let validator = Arc::new(EnvelopeValidator::default());
    let env = Arc::new(make_envelope());
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
                let ev = make_agent_event(run_started());
                agg.lock().unwrap().push(&ev);
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
                    events.lock().unwrap().push(make_agent_event(run_started()));
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
        run_started(),
        run_completed(),
        assistant_delta(),
        assistant_message(),
        tool_call_event(),
        tool_result_event(),
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "modified".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        warning_event(),
        error_event(),
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
        files: vec!["main.rs".into()],
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
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(5),
        request_units: None,
        estimated_cost_usd: None,
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let usage = Arc::clone(&usage);
            thread::spawn(move || {
                assert_eq!(usage.input_tokens, Some(100));
                assert_eq!(usage.output_tokens, Some(200));
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
        backend_version: Some("4.0".into()),
        adapter_version: Some("1.0".into()),
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
        code: ErrorCode::InvalidContractVersion,
        message: "test error".into(),
        context: BTreeMap::new(),
        source: None,
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
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(move || {
                let _ = ErrorCatalog::lookup("ABP-C001");
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
                let _ = MappingError::FidelityLoss {
                    field: "test".into(),
                    source_dialect: "openai".into(),
                    target_dialect: "claude".into(),
                    detail: "detail".into(),
                };
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
        tx.send(make_agent_event(run_started())).unwrap();
        for i in 0..5 {
            tx.send(make_agent_event(AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            }))
            .unwrap();
        }
        tx.send(make_agent_event(assistant_message())).unwrap();
        tx.send(make_agent_event(run_completed())).unwrap();
    });

    let consumer = thread::spawn(move || {
        let events: Vec<_> = rx.iter().collect();
        assert_eq!(events.len(), 8);
        events
    });

    producer.join().unwrap();
    let events = consumer.join().unwrap();
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[events.len() - 1].kind,
        AgentEventKind::RunCompleted { .. }
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
                let _ = auditor.audit_batch(&[(*receipt).clone()]);
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
                let _ = registry.len();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent Capability usage
// ---------------------------------------------------------------------------

#[test]
fn test_capability_concurrent_access() {
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
                let _ = format!("{:?}", caps[i % 4]);
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
    let globs =
        IncludeExcludeGlobs::new(&["src/**/*.rs".to_string()], &["target/**".to_string()]).unwrap();
    let globs = Arc::new(globs);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let globs = Arc::clone(&globs);
            thread::spawn(move || {
                let decision = globs.decide_str("src/main.rs");
                let _ = format!("{:?}", decision);
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
        input_tokens: 100,
        output_tokens: 200,
        total_tokens: 300,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
    });
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let usage = Arc::clone(&usage);
            thread::spawn(move || {
                assert_eq!(usage.input_tokens, 100);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent SupportLevel debug
// ---------------------------------------------------------------------------

#[test]
fn test_support_level_concurrent_debug() {
    let levels = Arc::new(vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
    ]);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let levels = Arc::clone(&levels);
            thread::spawn(move || {
                let _ = format!("{:?}", levels[i % 3]);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Sync-channel backpressure test
// ---------------------------------------------------------------------------

#[test]
fn test_sync_channel_backpressure_agent_events() {
    let (tx, rx) = std::sync::mpsc::sync_channel::<AgentEvent>(4);
    let producer = thread::spawn(move || {
        for _ in 0..16 {
            tx.send(make_agent_event(run_started())).unwrap();
        }
    });
    let consumer = thread::spawn(move || {
        let mut count = 0;
        for _ in rx.iter() {
            count += 1;
        }
        count
    });
    producer.join().unwrap();
    let total = consumer.join().unwrap();
    assert_eq!(total, 16);
}

// ---------------------------------------------------------------------------
// Concurrent CapabilityDiff creation
// ---------------------------------------------------------------------------

#[test]
fn test_capability_diff_send() {
    let diff = CapabilityDiff {
        added: vec![Capability::Streaming],
        removed: vec![],
        upgraded: vec![],
        downgraded: vec![],
    };
    let handle = thread::spawn(move || diff);
    let returned = handle.join().unwrap();
    assert_eq!(returned.added.len(), 1);
}

// ---------------------------------------------------------------------------
// Concurrent NegotiationResult access
// ---------------------------------------------------------------------------

#[test]
fn test_negotiation_result_concurrent() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Vision],
        minimum_support: SupportLevel::Native,
    };
    let manifest: BTreeMap<Capability, SupportLevel> = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::Vision, SupportLevel::Emulated),
    ]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    let result = Arc::new(result);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let result = Arc::clone(&result);
            thread::spawn(move || {
                let _ = format!("{:?}", &*result);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent DetectionResult access
// ---------------------------------------------------------------------------

#[test]
fn test_detection_result_send() {
    let detector = DialectDetector::new();
    let val: Value = serde_json::from_str(r#"{"model":"gpt-4","messages":[]}"#).unwrap();
    let result = detector.detect(&val);
    let handle = thread::spawn(move || result);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent DialectValidator usage
// ---------------------------------------------------------------------------

#[test]
fn test_dialect_validator_concurrent() {
    let validator = Arc::new(DialectValidator::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let validator = Arc::clone(&validator);
            thread::spawn(move || {
                let _ = &*validator;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent ProjectionScore access
// ---------------------------------------------------------------------------

#[test]
fn test_projection_score_send() {
    let score = ProjectionScore {
        total: 1.0,
        capability_coverage: 0.9,
        mapping_fidelity: 0.8,
        priority: 0.7,
    };
    let handle = thread::spawn(move || score);
    let returned = handle.join().unwrap();
    assert_eq!(returned.total, 1.0);
}

// ---------------------------------------------------------------------------
// Concurrent MappingMatrix access
// ---------------------------------------------------------------------------

#[test]
fn test_mapping_matrix_concurrent() {
    let matrix = Arc::new(MappingMatrix::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let matrix = Arc::clone(&matrix);
            thread::spawn(move || {
                let _ = &*matrix;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent CircuitBreaker access
// ---------------------------------------------------------------------------

#[test]
fn test_circuit_breaker_concurrent() {
    let breaker = Arc::new(Mutex::new(CircuitBreaker::new(
        3,
        std::time::Duration::from_secs(30),
    )));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let breaker = Arc::clone(&breaker);
            thread::spawn(move || {
                let guard = breaker.lock().unwrap();
                let _ = guard.state();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent StreamBuffer access
// ---------------------------------------------------------------------------

#[test]
fn test_stream_buffer_concurrent_push() {
    let buffer = Arc::new(Mutex::new(StreamBuffer::new(100)));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let buffer = Arc::clone(&buffer);
            thread::spawn(move || {
                let ev = make_agent_event(run_started());
                buffer.lock().unwrap().push(ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent EventRecorder access
// ---------------------------------------------------------------------------

#[test]
fn test_event_recorder_concurrent() {
    let recorder = Arc::new(Mutex::new(EventRecorder::new()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let recorder = Arc::clone(&recorder);
            thread::spawn(move || {
                let ev = make_agent_event(run_started());
                recorder.lock().unwrap().record(&ev);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent Fidelity usage
// ---------------------------------------------------------------------------

#[test]
fn test_fidelity_send() {
    let fidelity = Fidelity::Lossless;
    let handle = thread::spawn(move || fidelity);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent ValidationErrors access
// ---------------------------------------------------------------------------

#[test]
fn test_validation_errors_concurrent() {
    let errors = Arc::new(ValidationErrors::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let errors = Arc::clone(&errors);
            thread::spawn(move || {
                let _ = errors.is_empty();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent ConfigDefaults access
// ---------------------------------------------------------------------------

#[test]
fn test_config_defaults_concurrent() {
    let defaults = Arc::new(ConfigDefaults);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let defaults = Arc::clone(&defaults);
            thread::spawn(move || {
                let _ = &*defaults;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// Concurrent RunAnalytics access
// ---------------------------------------------------------------------------

#[test]
fn test_run_analytics_send() {
    let events = vec![make_agent_event(run_started())];
    let analytics = RunAnalytics::from_events(&events);
    let handle = thread::spawn(move || analytics);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent EmulationStrategy access
// ---------------------------------------------------------------------------

#[test]
fn test_emulation_strategy_send() {
    let strategy = EmulationStrategy::ClientSide;
    let handle = thread::spawn(move || strategy);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent CompatibilityReport access
// ---------------------------------------------------------------------------

#[test]
fn test_compatibility_report_send() {
    let report = CompatibilityReport {
        compatible: true,
        native_count: 1,
        emulated_count: 0,
        unsupported_count: 0,
        summary: "all good".into(),
        details: vec![],
    };
    let handle = thread::spawn(move || report);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent TamperKind and TamperEvidence
// ---------------------------------------------------------------------------

#[test]
fn test_tamper_kind_send() {
    let kind = TamperKind::HashMismatch {
        stored: "abc".into(),
        computed: "def".into(),
    };
    let handle = thread::spawn(move || kind);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent FieldDiff and ReceiptDiff
// ---------------------------------------------------------------------------

#[test]
fn test_receipt_diff_send() {
    let r1 = make_receipt();
    let r2 = make_receipt();
    let diff = abp_receipt::diff_receipts(&r1, &r2);
    let handle = thread::spawn(move || diff);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent ChainSummary
// ---------------------------------------------------------------------------

#[test]
fn test_chain_summary_send() {
    let mut chain = ReceiptChain::new();
    let _ = chain.push(make_receipt());
    let summary = chain.chain_summary();
    let handle = thread::spawn(move || summary);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent VerificationResult (receipt crate)
// ---------------------------------------------------------------------------

#[test]
fn test_verification_result_send() {
    let result = abp_receipt::verify_receipt(&make_receipt());
    let handle = thread::spawn(move || result);
    let _ = handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent AuditReport
// ---------------------------------------------------------------------------

#[test]
fn test_audit_report_send() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[make_receipt()]);
    let handle = thread::spawn(move || report);
    let _ = handle.join().unwrap();
}
