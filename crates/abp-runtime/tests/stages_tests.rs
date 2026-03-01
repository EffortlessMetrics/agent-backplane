// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the additional pipeline stages in `abp_runtime::stages`.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::pipeline::{PipelineStage, ValidationStage};
use abp_runtime::stages::{
    DeduplicationStage, LoggingStage, MetricsStage, PipelineBuilder, RateLimitStage,
};
use std::time::Duration;

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

// ===========================================================================
// RateLimitStage
// ===========================================================================

#[tokio::test]
async fn rate_limit_allows_under_limit() {
    let stage = RateLimitStage::new(5);
    let mut wo = minimal_work_order("rate ok");
    stage.process(&mut wo).await.expect("should be under limit");
}

#[tokio::test]
async fn rate_limit_allows_exactly_at_limit() {
    let stage = RateLimitStage::new(3);
    for i in 0..3 {
        let mut wo = minimal_work_order(&format!("rate {i}"));
        stage
            .process(&mut wo)
            .await
            .unwrap_or_else(|e| panic!("run {i} should pass: {e}"));
    }
}

#[tokio::test]
async fn rate_limit_rejects_over_limit() {
    let stage = RateLimitStage::new(2);
    for i in 0..2 {
        let mut wo = minimal_work_order(&format!("rate {i}"));
        stage.process(&mut wo).await.unwrap();
    }
    let mut wo = minimal_work_order("rate excess");
    let err = stage.process(&mut wo).await.unwrap_err();
    assert!(
        err.to_string().contains("rate limit exceeded"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn rate_limit_name() {
    let stage = RateLimitStage::new(1);
    assert_eq!(stage.name(), "rate_limit");
}

// ===========================================================================
// DeduplicationStage
// ===========================================================================

#[tokio::test]
async fn dedup_allows_first_order() {
    let stage = DeduplicationStage::new(Duration::from_secs(60));
    let mut wo = minimal_work_order("first");
    stage.process(&mut wo).await.expect("first order must pass");
}

#[tokio::test]
async fn dedup_rejects_duplicate_within_window() {
    let stage = DeduplicationStage::new(Duration::from_secs(60));
    let mut wo1 = minimal_work_order("dup");
    stage.process(&mut wo1).await.unwrap();

    let mut wo2 = minimal_work_order("dup");
    let err = stage.process(&mut wo2).await.unwrap_err();
    assert!(
        err.to_string().contains("duplicate"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn dedup_allows_different_tasks() {
    let stage = DeduplicationStage::new(Duration::from_secs(60));
    let mut wo1 = minimal_work_order("task_a");
    let mut wo2 = minimal_work_order("task_b");
    stage.process(&mut wo1).await.unwrap();
    stage
        .process(&mut wo2)
        .await
        .expect("different task must pass");
}

#[tokio::test]
async fn dedup_allows_after_window_expires() {
    let stage = DeduplicationStage::new(Duration::from_millis(50));
    let mut wo1 = minimal_work_order("expire");
    stage.process(&mut wo1).await.unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;

    let mut wo2 = minimal_work_order("expire");
    stage
        .process(&mut wo2)
        .await
        .expect("should pass after window expires");
}

#[tokio::test]
async fn dedup_name() {
    let stage = DeduplicationStage::new(Duration::from_secs(1));
    assert_eq!(stage.name(), "deduplication");
}

// ===========================================================================
// LoggingStage
// ===========================================================================

#[tokio::test]
async fn logging_stage_passes_through() {
    let stage = LoggingStage::new("test");
    let mut wo = minimal_work_order("log me");
    stage.process(&mut wo).await.expect("logging must not fail");
}

#[tokio::test]
async fn logging_stage_does_not_mutate_order() {
    let stage = LoggingStage::new("prefix");
    let mut wo = minimal_work_order("unchanged");
    let task_before = wo.task.clone();
    stage.process(&mut wo).await.unwrap();
    assert_eq!(wo.task, task_before);
}

#[tokio::test]
async fn logging_stage_name() {
    let stage = LoggingStage::new("x");
    assert_eq!(stage.name(), "logging");
}

// ===========================================================================
// MetricsStage
// ===========================================================================

#[tokio::test]
async fn metrics_stage_initially_zero() {
    let stage = MetricsStage::new();
    let stats = stage.stats().await;
    assert_eq!(stats.invocations, 0);
    assert_eq!(stats.successes, 0);
    assert_eq!(stats.failures, 0);
    assert_eq!(stats.total_duration_ms, 0);
}

#[tokio::test]
async fn metrics_stage_records_success() {
    let stage = MetricsStage::new();
    let mut wo = minimal_work_order("metric");
    stage.process(&mut wo).await.unwrap();
    let stats = stage.stats().await;
    assert_eq!(stats.invocations, 1);
    assert_eq!(stats.successes, 1);
    assert_eq!(stats.failures, 0);
}

#[tokio::test]
async fn metrics_stage_accumulates() {
    let stage = MetricsStage::new();
    for _ in 0..5 {
        let mut wo = minimal_work_order("m");
        stage.process(&mut wo).await.unwrap();
    }
    let stats = stage.stats().await;
    assert_eq!(stats.invocations, 5);
    assert_eq!(stats.successes, 5);
}

#[tokio::test]
async fn metrics_stage_name() {
    let stage = MetricsStage::new();
    assert_eq!(stage.name(), "metrics");
}

#[tokio::test]
async fn metrics_default_impl() {
    let stage = MetricsStage::default();
    let stats = stage.stats().await;
    assert_eq!(stats.invocations, 0);
}

// ===========================================================================
// PipelineBuilder
// ===========================================================================

#[tokio::test]
async fn builder_empty_pipeline() {
    let pipeline = PipelineBuilder::new().build();
    assert!(pipeline.stage_names().is_empty());
}

#[tokio::test]
async fn builder_stage_count() {
    let builder = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .add_stage(Box::new(LoggingStage::new("b")));
    assert_eq!(builder.stage_count(), 2);
}

#[tokio::test]
async fn builder_produces_working_pipeline() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .add_stage(Box::new(LoggingStage::new("test")))
        .build();

    let mut wo = minimal_work_order("builder test");
    let results = pipeline.execute(&mut wo).await;
    assert_eq!(results.len(), 2);
    assert!(results[0].passed);
    assert!(results[1].passed);
}

#[tokio::test]
async fn builder_default() {
    let builder = PipelineBuilder::default();
    assert_eq!(builder.stage_count(), 0);
}

// ===========================================================================
// StagePipeline (via PipelineBuilder)
// ===========================================================================

#[tokio::test]
async fn stage_pipeline_reports_stage_names() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .add_stage(Box::new(MetricsStage::new()))
        .build();
    let names = pipeline.stage_names();
    assert_eq!(names, vec!["validation", "metrics"]);
}

#[tokio::test]
async fn stage_pipeline_records_per_stage_results() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(LoggingStage::new("a")))
        .add_stage(Box::new(LoggingStage::new("b")))
        .build();

    let mut wo = minimal_work_order("results");
    let results = pipeline.execute(&mut wo).await;
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.passed);
        assert!(r.message.is_none());
        assert_eq!(r.stage_name, "logging");
    }
}

#[tokio::test]
async fn stage_pipeline_continues_after_failure() {
    // Build a pipeline where the first stage will fail (empty task → validation error)
    // but the second stage (logging) should still run since StagePipeline
    // does NOT short-circuit.
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .add_stage(Box::new(LoggingStage::new("after")))
        .build();

    let mut wo = minimal_work_order(""); // empty task fails validation
    let results = pipeline.execute(&mut wo).await;
    assert_eq!(results.len(), 2);
    assert!(!results[0].passed, "validation should fail");
    assert!(results[0].message.is_some());
    // Second stage still ran
    assert!(results[1].passed, "logging should still run");
}

#[tokio::test]
async fn stage_pipeline_empty_execute() {
    let pipeline = PipelineBuilder::new().build();
    let mut wo = minimal_work_order("empty");
    let results = pipeline.execute(&mut wo).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn stage_result_duration_is_non_negative() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(MetricsStage::new()))
        .build();
    let mut wo = minimal_work_order("timing");
    let results = pipeline.execute(&mut wo).await;
    // duration_ms is u64 so always >= 0, but verify it exists
    assert_eq!(results.len(), 1);
    // Just ensure it doesn't panic or overflow
    let _ = results[0].duration_ms;
}

// ===========================================================================
// Integration: combining new stages
// ===========================================================================

#[tokio::test]
async fn combined_rate_limit_and_dedup() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(RateLimitStage::new(10)))
        .add_stage(Box::new(DeduplicationStage::new(Duration::from_secs(60))))
        .add_stage(Box::new(ValidationStage))
        .build();

    let mut wo = minimal_work_order("combo");
    let results = pipeline.execute(&mut wo).await;
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.passed));
}

#[tokio::test]
async fn full_pipeline_with_all_stages() {
    let pipeline = PipelineBuilder::new()
        .add_stage(Box::new(RateLimitStage::new(100)))
        .add_stage(Box::new(DeduplicationStage::new(Duration::from_secs(60))))
        .add_stage(Box::new(LoggingStage::new("full")))
        .add_stage(Box::new(MetricsStage::new()))
        .add_stage(Box::new(ValidationStage))
        .build();

    let names = pipeline.stage_names();
    assert_eq!(
        names,
        vec![
            "rate_limit",
            "deduplication",
            "logging",
            "metrics",
            "validation"
        ]
    );

    let mut wo = minimal_work_order("full pipeline");
    let results = pipeline.execute(&mut wo).await;
    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|r| r.passed));
}
