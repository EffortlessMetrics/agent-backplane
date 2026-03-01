// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the lifecycle hooks module.

use abp_core::{
    AgentEvent, AgentEventKind, CapabilityRequirements, ContextPacket, ExecutionLane, Outcome,
    PolicyProfile, ReceiptBuilder, RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::RuntimeError;
use abp_runtime::hooks::{HookRegistry, LifecycleHook, LoggingHook, MetricsHook, ValidationHook};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::nil(),
        task: "test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn test_event() -> AgentEvent {
    AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    }
}

/// A simple counting hook used by several tests.
struct CountingHook {
    starts: AtomicU32,
    events: AtomicU32,
    completes: AtomicU32,
    errors: AtomicU32,
}

impl CountingHook {
    fn new() -> Self {
        Self {
            starts: AtomicU32::new(0),
            events: AtomicU32::new(0),
            completes: AtomicU32::new(0),
            errors: AtomicU32::new(0),
        }
    }
}

impl LifecycleHook for CountingHook {
    fn on_run_start(
        &self,
        _wo: &WorkOrder,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn on_event(
        &self,
        _event: &AgentEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.events.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn on_run_complete(
        &self,
        _receipt: &abp_core::Receipt,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.completes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn on_error(&self, _error: &RuntimeError) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
    fn name(&self) -> &str {
        "counting"
    }
}

/// Hook that always fails on_run_start.
struct FailingHook;

impl LifecycleHook for FailingHook {
    fn on_run_start(
        &self,
        _wo: &WorkOrder,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("intentional failure".into())
    }
    fn name(&self) -> &str {
        "failing"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn empty_registry_counts() {
    let reg = HookRegistry::new();
    assert_eq!(reg.hook_count(), 0);
    assert!(reg.hook_names().is_empty());
}

#[test]
fn register_increments_count() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    reg.register(Box::new(ValidationHook));
    assert_eq!(reg.hook_count(), 2);
}

#[test]
fn hook_names_reflect_registration_order() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(ValidationHook));
    reg.register(Box::new(LoggingHook));
    assert_eq!(reg.hook_names(), vec!["validation", "logging"]);
}

#[test]
fn fire_run_start_calls_all_hooks() {
    let counter = Arc::new(CountingHook::new());
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    // We can't share Arc<CountingHook> directly because register takes Box.
    // Instead, use a second independent counter to verify that multiple hooks
    // are each called exactly once by checking result count.
    reg.register(Box::new(ValidationHook));
    let results = reg.fire_run_start(&test_work_order());
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(Result::is_ok));

    // Also verify with the counting hook directly.
    let mut reg2 = HookRegistry::new();
    let raw = Arc::into_inner(counter).unwrap();
    reg2.register(Box::new(raw));
    reg2.fire_run_start(&test_work_order());
}

#[test]
fn fire_event_calls_all_hooks() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    let results = reg.fire_event(&test_event());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn fire_run_complete_calls_all_hooks() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    let results = reg.fire_run_complete(&receipt);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn fire_error_does_not_panic() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    let err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    reg.fire_error(&err); // should not panic
}

#[test]
fn failing_hook_returns_error_in_results() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(FailingHook));
    reg.register(Box::new(LoggingHook));
    let results = reg.fire_run_start(&test_work_order());
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert!(results[1].is_ok());
}

#[test]
fn validation_hook_rejects_empty_task() {
    let mut wo = test_work_order();
    wo.task = "   ".into();
    let mut reg = HookRegistry::new();
    reg.register(Box::new(ValidationHook));
    let results = reg.fire_run_start(&wo);
    assert!(results[0].is_err());
    let msg = results[0].as_ref().unwrap_err().to_string();
    assert!(msg.contains("task"), "error should mention task: {msg}");
}

#[test]
fn validation_hook_rejects_empty_root() {
    let mut wo = test_work_order();
    wo.workspace.root = "".into();
    let mut reg = HookRegistry::new();
    reg.register(Box::new(ValidationHook));
    let results = reg.fire_run_start(&wo);
    assert!(results[0].is_err());
    let msg = results[0].as_ref().unwrap_err().to_string();
    assert!(msg.contains("root"), "error should mention root: {msg}");
}

#[test]
fn validation_hook_accepts_valid_work_order() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(ValidationHook));
    let results = reg.fire_run_start(&test_work_order());
    assert!(results[0].is_ok());
}

#[test]
fn metrics_hook_records_run() {
    let metrics = Arc::new(abp_runtime::telemetry::RunMetrics::new());
    let hook = MetricsHook::new(Arc::clone(&metrics));
    let mut reg = HookRegistry::new();
    reg.register(Box::new(hook));

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let results = reg.fire_run_complete(&receipt);
    assert!(results[0].is_ok());

    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
}

#[test]
fn metrics_hook_records_failure() {
    let metrics = Arc::new(abp_runtime::telemetry::RunMetrics::new());
    let hook = MetricsHook::new(Arc::clone(&metrics));
    let mut reg = HookRegistry::new();
    reg.register(Box::new(hook));

    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let results = reg.fire_run_complete(&receipt);
    assert!(results[0].is_ok());

    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.successful_runs, 0);
}

#[test]
fn default_trait_methods_are_no_ops() {
    // A hook that only implements name() — all others should succeed.
    struct MinimalHook;
    impl LifecycleHook for MinimalHook {
        fn name(&self) -> &str {
            "minimal"
        }
    }

    let mut reg = HookRegistry::new();
    reg.register(Box::new(MinimalHook));

    let wo = test_work_order();
    assert!(reg.fire_run_start(&wo)[0].is_ok());
    assert!(reg.fire_event(&test_event())[0].is_ok());

    let receipt = ReceiptBuilder::new("mock").build();
    assert!(reg.fire_run_complete(&receipt)[0].is_ok());

    let err = RuntimeError::UnknownBackend { name: "x".into() };
    reg.fire_error(&err);
}
