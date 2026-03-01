// SPDX-License-Identifier: MIT OR Apache-2.0
use serde_json::Value;
use serde_json::json;
use sidecar_kit::pipeline::{
    EventPipeline, PipelineError, PipelineStage, RedactStage, TimestampStage, ValidateStage,
};

// ── helpers ──────────────────────────────────────────────────────────

fn sample_event() -> Value {
    json!({
        "type": "message",
        "content": "hello",
        "secret": "s3cret"
    })
}

/// A stage that unconditionally drops events.
struct DropStage;

impl PipelineStage for DropStage {
    fn name(&self) -> &str {
        "drop"
    }
    fn process(&self, _event: Value) -> Result<Option<Value>, PipelineError> {
        Ok(None)
    }
}

/// A stage that always errors.
struct FailStage;

impl PipelineStage for FailStage {
    fn name(&self) -> &str {
        "fail"
    }
    fn process(&self, _event: Value) -> Result<Option<Value>, PipelineError> {
        Err(PipelineError::StageError {
            stage: "fail".into(),
            message: "boom".into(),
        })
    }
}

// ── EventPipeline basics ─────────────────────────────────────────────

#[test]
fn empty_pipeline_passes_through() {
    let pipeline = EventPipeline::new();
    assert_eq!(pipeline.stage_count(), 0);
    let event = sample_event();
    let result = pipeline.process(event.clone()).unwrap();
    assert_eq!(result, Some(event));
}

#[test]
fn stage_count_tracks_additions() {
    let mut pipeline = EventPipeline::new();
    assert_eq!(pipeline.stage_count(), 0);
    pipeline.add_stage(Box::new(TimestampStage::new()));
    assert_eq!(pipeline.stage_count(), 1);
    pipeline.add_stage(Box::new(RedactStage::new(vec![])));
    assert_eq!(pipeline.stage_count(), 2);
}

#[test]
fn pipeline_short_circuits_on_none() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(DropStage));
    // This stage should never run:
    pipeline.add_stage(Box::new(FailStage));
    let result = pipeline.process(sample_event()).unwrap();
    assert_eq!(result, None);
}

#[test]
fn pipeline_propagates_stage_error() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(FailStage));
    let err = pipeline.process(sample_event()).unwrap_err();
    match err {
        PipelineError::StageError { stage, message } => {
            assert_eq!(stage, "fail");
            assert_eq!(message, "boom");
        }
        _ => panic!("expected StageError"),
    }
}

// ── TimestampStage ───────────────────────────────────────────────────

#[test]
fn timestamp_stage_adds_processed_at() {
    let stage = TimestampStage::new();
    assert_eq!(stage.name(), "timestamp");
    let result = stage.process(sample_event()).unwrap().unwrap();
    assert!(result.get("processed_at").is_some());
    assert!(result["processed_at"].is_u64());
}

#[test]
fn timestamp_stage_overwrites_existing() {
    let stage = TimestampStage::new();
    let mut event = sample_event();
    event["processed_at"] = json!(0);
    let result = stage.process(event).unwrap().unwrap();
    assert!(result["processed_at"].as_u64().unwrap() > 0);
}

#[test]
fn timestamp_stage_rejects_non_object() {
    let stage = TimestampStage::new();
    let err = stage.process(json!("not an object")).unwrap_err();
    assert!(matches!(err, PipelineError::InvalidEvent));
}

// ── RedactStage ──────────────────────────────────────────────────────

#[test]
fn redact_stage_removes_fields() {
    let stage = RedactStage::new(vec!["secret".into()]);
    assert_eq!(stage.name(), "redact");
    let result = stage.process(sample_event()).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert_eq!(result.get("content").unwrap(), "hello");
}

#[test]
fn redact_stage_ignores_missing_fields() {
    let stage = RedactStage::new(vec!["nonexistent".into()]);
    let event = sample_event();
    let result = stage.process(event.clone()).unwrap().unwrap();
    assert_eq!(result, event);
}

#[test]
fn redact_stage_rejects_non_object() {
    let stage = RedactStage::new(vec!["x".into()]);
    let err = stage.process(json!(42)).unwrap_err();
    assert!(matches!(err, PipelineError::InvalidEvent));
}

// ── ValidateStage ────────────────────────────────────────────────────

#[test]
fn validate_stage_passes_when_fields_present() {
    let stage = ValidateStage::new(vec!["type".into(), "content".into()]);
    assert_eq!(stage.name(), "validate");
    let result = stage.process(sample_event()).unwrap();
    assert!(result.is_some());
}

#[test]
fn validate_stage_errors_on_missing_field() {
    let stage = ValidateStage::new(vec!["type".into(), "missing_field".into()]);
    let err = stage.process(sample_event()).unwrap_err();
    match err {
        PipelineError::StageError { stage, message } => {
            assert_eq!(stage, "validate");
            assert!(message.contains("missing_field"));
        }
        _ => panic!("expected StageError"),
    }
}

// ── multi-stage integration ──────────────────────────────────────────

#[test]
fn multi_stage_validate_redact_timestamp() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
    pipeline.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
    pipeline.add_stage(Box::new(TimestampStage::new()));

    let result = pipeline.process(sample_event()).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("processed_at").is_some());
    assert_eq!(result.get("type").unwrap(), "message");
}

// ── PipelineError display ────────────────────────────────────────────

#[test]
fn pipeline_error_display() {
    let err = PipelineError::StageError {
        stage: "s".into(),
        message: "m".into(),
    };
    assert_eq!(format!("{err}"), "stage 's' failed: m");

    let err2 = PipelineError::InvalidEvent;
    assert_eq!(format!("{err2}"), "event is not a valid JSON object");
}
