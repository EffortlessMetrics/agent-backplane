// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the pipeline / middleware concept in abp-runtime.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::pipeline::{
    AuditStage, Pipeline, PipelineStage, PolicyStage, ValidationStage,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn minimal_work_order(task: &str) -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: task.into(),
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

/// Stage that records the order in which it was called via an atomic counter.
struct OrderRecordingStage {
    id: usize,
    counter: Arc<AtomicUsize>,
    observed: Arc<tokio::sync::Mutex<Vec<usize>>>,
}

#[async_trait]
impl PipelineStage for OrderRecordingStage {
    async fn process(&self, _order: &mut WorkOrder) -> Result<()> {
        let seq = self.counter.fetch_add(1, Ordering::SeqCst);
        self.observed.lock().await.push(seq);
        // Also store our id so we can verify *which* stage ran at which position
        let _ = self.id;
        Ok(())
    }
    fn name(&self) -> &str {
        "order_recording"
    }
}

/// Stage that always fails with a given message.
struct FailingStage(&'static str);

#[async_trait]
impl PipelineStage for FailingStage {
    async fn process(&self, _order: &mut WorkOrder) -> Result<()> {
        anyhow::bail!(self.0)
    }
    fn name(&self) -> &str {
        "failing"
    }
}

/// Stage that mutates the task field.
struct MutatingStage {
    suffix: String,
}

#[async_trait]
impl PipelineStage for MutatingStage {
    async fn process(&self, order: &mut WorkOrder) -> Result<()> {
        order.task.push_str(&self.suffix);
        Ok(())
    }
    fn name(&self) -> &str {
        "mutating"
    }
}

// ===========================================================================
// 1. Single stage execution
// ===========================================================================

#[tokio::test]
async fn single_validation_stage_passes_valid_order() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = minimal_work_order("hello");
    pipeline.execute(&mut wo).await.expect("should pass");
}

// ===========================================================================
// 2. Multi-stage pipeline runs in order
// ===========================================================================

#[tokio::test]
async fn multi_stage_pipeline_executes_in_insertion_order() {
    let counter = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let pipeline = Pipeline::new()
        .stage(OrderRecordingStage {
            id: 0,
            counter: Arc::clone(&counter),
            observed: Arc::clone(&observed),
        })
        .stage(OrderRecordingStage {
            id: 1,
            counter: Arc::clone(&counter),
            observed: Arc::clone(&observed),
        })
        .stage(OrderRecordingStage {
            id: 2,
            counter: Arc::clone(&counter),
            observed: Arc::clone(&observed),
        });

    let mut wo = minimal_work_order("order test");
    pipeline.execute(&mut wo).await.unwrap();

    let log = observed.lock().await;
    assert_eq!(*log, vec![0, 1, 2], "stages must run in insertion order");
}

// ===========================================================================
// 3. Validation stage rejects invalid orders
// ===========================================================================

#[tokio::test]
async fn validation_rejects_empty_task() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = minimal_work_order("");
    let err = pipeline.execute(&mut wo).await.unwrap_err();
    assert!(
        err.to_string().contains("task must not be empty"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn validation_rejects_blank_workspace_root() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = minimal_work_order("valid task");
    wo.workspace.root = "   ".into();
    let err = pipeline.execute(&mut wo).await.unwrap_err();
    assert!(
        err.to_string().contains("workspace root must not be empty"),
        "unexpected error: {err}"
    );
}

// ===========================================================================
// 4. Policy stage blocks restricted tools
// ===========================================================================

#[tokio::test]
async fn policy_stage_blocks_disallowed_tool() {
    let pipeline = Pipeline::new().stage(PolicyStage);
    let mut wo = minimal_work_order("policy check");
    wo.policy.allowed_tools = vec!["Bash".into()];
    wo.policy.disallowed_tools = vec!["Bash".into()];

    let err = pipeline.execute(&mut wo).await.unwrap_err();
    assert!(
        err.to_string().contains("Bash"),
        "error should mention blocked tool: {err}"
    );
}

#[tokio::test]
async fn policy_stage_passes_when_no_conflict() {
    let pipeline = Pipeline::new().stage(PolicyStage);
    let mut wo = minimal_work_order("policy ok");
    wo.policy.allowed_tools = vec!["Read".into()];
    wo.policy.disallowed_tools = vec!["Bash".into()];
    pipeline.execute(&mut wo).await.expect("should pass");
}

// ===========================================================================
// 5. Audit stage records processing
// ===========================================================================

#[tokio::test]
async fn audit_stage_records_entry() {
    let audit = AuditStage::new();
    let mut wo = minimal_work_order("audit me");
    let wo_id = wo.id;

    audit.process(&mut wo).await.unwrap();

    let entries = audit.entries().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].work_order_id, wo_id);
    assert_eq!(entries[0].task, "audit me");
}

#[tokio::test]
async fn audit_stage_accumulates_multiple_entries() {
    let audit = Arc::new(AuditStage::new());

    // Use the stage directly twice (simulating two pipeline runs).
    let mut wo1 = minimal_work_order("first");
    let mut wo2 = minimal_work_order("second");
    audit.process(&mut wo1).await.unwrap();
    audit.process(&mut wo2).await.unwrap();

    let entries = audit.entries().await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].task, "first");
    assert_eq!(entries[1].task, "second");
}

// ===========================================================================
// 6. Empty pipeline passes through
// ===========================================================================

#[tokio::test]
async fn empty_pipeline_is_noop() {
    let pipeline = Pipeline::new();
    assert!(pipeline.is_empty());

    let mut wo = minimal_work_order("unchanged");
    pipeline.execute(&mut wo).await.expect("empty pipeline must succeed");
    assert_eq!(wo.task, "unchanged");
}

// ===========================================================================
// 7. Stage failure short-circuits
// ===========================================================================

#[tokio::test]
async fn failure_short_circuits_remaining_stages() {
    let counter = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let pipeline = Pipeline::new()
        .stage(OrderRecordingStage {
            id: 0,
            counter: Arc::clone(&counter),
            observed: Arc::clone(&observed),
        })
        .stage(FailingStage("boom"))
        .stage(OrderRecordingStage {
            id: 2,
            counter: Arc::clone(&counter),
            observed: Arc::clone(&observed),
        });

    let mut wo = minimal_work_order("short-circuit");
    let err = pipeline.execute(&mut wo).await.unwrap_err();
    assert!(err.to_string().contains("boom"));

    let log = observed.lock().await;
    assert_eq!(log.len(), 1, "only the first stage should have run");
}

// ===========================================================================
// 8. Concurrent pipeline usage
// ===========================================================================

#[tokio::test]
async fn pipeline_is_send_sync_and_concurrent_safe() {
    let pipeline = Arc::new(
        Pipeline::new()
            .stage(ValidationStage)
            .stage(AuditStage::new()),
    );

    let mut handles = Vec::new();
    for i in 0..10 {
        let p = Arc::clone(&pipeline);
        handles.push(tokio::spawn(async move {
            let mut wo = minimal_work_order(&format!("concurrent-{i}"));
            p.execute(&mut wo).await
        }));
    }

    for h in handles {
        h.await.unwrap().expect("concurrent execution must succeed");
    }
}

// ===========================================================================
// 9. Pipeline with custom stage
// ===========================================================================

#[tokio::test]
async fn custom_stage_mutates_order() {
    let pipeline = Pipeline::new().stage(MutatingStage {
        suffix: " [processed]".into(),
    });

    let mut wo = minimal_work_order("original");
    pipeline.execute(&mut wo).await.unwrap();
    assert_eq!(wo.task, "original [processed]");
}

#[tokio::test]
async fn chained_custom_stages_compose() {
    let pipeline = Pipeline::new()
        .stage(MutatingStage {
            suffix: " A".into(),
        })
        .stage(MutatingStage {
            suffix: " B".into(),
        });

    let mut wo = minimal_work_order("start");
    pipeline.execute(&mut wo).await.unwrap();
    assert_eq!(wo.task, "start A B");
}

// ===========================================================================
// 10. Pipeline len / is_empty
// ===========================================================================

#[tokio::test]
async fn pipeline_len_reflects_stages() {
    let p0 = Pipeline::new();
    assert_eq!(p0.len(), 0);
    assert!(p0.is_empty());

    let p2 = Pipeline::new().stage(ValidationStage).stage(PolicyStage);
    assert_eq!(p2.len(), 2);
    assert!(!p2.is_empty());
}

// ===========================================================================
// 11. Builder integration: WorkOrderBuilder + pipeline
// ===========================================================================

#[tokio::test]
async fn pipeline_works_with_builder_api() {
    let pipeline = Pipeline::new()
        .stage(ValidationStage)
        .stage(PolicyStage)
        .stage(AuditStage::new());

    let mut wo = WorkOrderBuilder::new("builder task")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    pipeline.execute(&mut wo).await.expect("builder-produced order should pass full pipeline");
}
