// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the batch processing module.

use abp_core::{BackendIdentity, CapabilityManifest};
use abp_protocol::Envelope;
use abp_protocol::batch::{
    BatchItemStatus, BatchProcessor, BatchRequest, BatchResponse, BatchResult,
    BatchValidationError, MAX_BATCH_SIZE,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fatal_envelope(msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
    }
}

fn hello_envelope(id: &str) -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: id.into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_request(id: &str, envelopes: Vec<Envelope>) -> BatchRequest {
    BatchRequest {
        id: id.into(),
        envelopes,
        created_at: "2025-01-01T00:00:00Z".into(),
    }
}

// ---------------------------------------------------------------------------
// BatchRequest serde
// ---------------------------------------------------------------------------

#[test]
fn batch_request_roundtrip_serde() {
    let req = make_request("b1", vec![fatal_envelope("e1")]);
    let json = serde_json::to_string(&req).unwrap();
    let decoded: BatchRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, "b1");
    assert_eq!(decoded.envelopes.len(), 1);
}

#[test]
fn batch_response_roundtrip_serde() {
    let resp = BatchResponse {
        request_id: "b1".into(),
        results: vec![BatchResult {
            index: 0,
            status: BatchItemStatus::Success,
            envelope: Some(fatal_envelope("ok")),
        }],
        total_duration_ms: 42,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let decoded: BatchResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.request_id, "b1");
    assert_eq!(decoded.total_duration_ms, 42);
    assert_eq!(decoded.results.len(), 1);
}

// ---------------------------------------------------------------------------
// BatchItemStatus serde
// ---------------------------------------------------------------------------

#[test]
fn batch_item_status_success_serde() {
    let s = BatchItemStatus::Success;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("success"));
    let decoded: BatchItemStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, BatchItemStatus::Success);
}

#[test]
fn batch_item_status_failed_serde() {
    let s = BatchItemStatus::Failed {
        error: "boom".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("failed"));
    assert!(json.contains("boom"));
    let decoded: BatchItemStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn batch_item_status_skipped_serde() {
    let s = BatchItemStatus::Skipped {
        reason: "not needed".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("skipped"));
    let decoded: BatchItemStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

// ---------------------------------------------------------------------------
// BatchProcessor::process
// ---------------------------------------------------------------------------

#[test]
fn process_single_envelope() {
    let processor = BatchProcessor::new();
    let req = make_request("b1", vec![fatal_envelope("err1")]);
    let resp = processor.process(req);

    assert_eq!(resp.request_id, "b1");
    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.results[0].index, 0);
    assert_eq!(resp.results[0].status, BatchItemStatus::Success);
    assert!(resp.results[0].envelope.is_some());
}

#[test]
fn process_multiple_envelopes() {
    let processor = BatchProcessor::new();
    let req = make_request(
        "b2",
        vec![
            fatal_envelope("e1"),
            hello_envelope("sidecar-1"),
            fatal_envelope("e2"),
        ],
    );
    let resp = processor.process(req);

    assert_eq!(resp.request_id, "b2");
    assert_eq!(resp.results.len(), 3);
    for (i, result) in resp.results.iter().enumerate() {
        assert_eq!(result.index, i);
        assert_eq!(result.status, BatchItemStatus::Success);
    }
}

#[test]
fn process_empty_batch_returns_empty_results() {
    let processor = BatchProcessor::new();
    let req = make_request("b-empty", vec![]);
    let resp = processor.process(req);

    assert_eq!(resp.request_id, "b-empty");
    assert!(resp.results.is_empty());
}

#[test]
fn process_preserves_request_id() {
    let processor = BatchProcessor::new();
    let req = make_request("unique-id-123", vec![fatal_envelope("x")]);
    let resp = processor.process(req);
    assert_eq!(resp.request_id, "unique-id-123");
}

#[test]
fn process_records_duration() {
    let processor = BatchProcessor::new();
    let req = make_request("b-dur", vec![fatal_envelope("x")]);
    let resp = processor.process(req);
    // Duration should be non-negative (it's u64, so always >= 0)
    // Just verify the field is populated without panicking.
    let _ = resp.total_duration_ms;
}

#[test]
fn process_indexes_are_sequential() {
    let processor = BatchProcessor::new();
    let envelopes: Vec<Envelope> = (0..5).map(|i| fatal_envelope(&format!("e{i}"))).collect();
    let req = make_request("b-idx", envelopes);
    let resp = processor.process(req);

    let indexes: Vec<usize> = resp.results.iter().map(|r| r.index).collect();
    assert_eq!(indexes, vec![0, 1, 2, 3, 4]);
}

// ---------------------------------------------------------------------------
// BatchProcessor::validate_batch
// ---------------------------------------------------------------------------

#[test]
fn validate_empty_batch() {
    let processor = BatchProcessor::new();
    let req = make_request("v-empty", vec![]);
    let errors = processor.validate_batch(&req);

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0], BatchValidationError::EmptyBatch);
}

#[test]
fn validate_too_many_items() {
    let processor = BatchProcessor::new();
    let envelopes: Vec<Envelope> = (0..MAX_BATCH_SIZE + 1)
        .map(|i| fatal_envelope(&format!("e{i}")))
        .collect();
    let req = make_request("v-big", envelopes);
    let errors = processor.validate_batch(&req);

    assert!(errors.iter().any(|e| matches!(
        e,
        BatchValidationError::TooManyItems { count, max }
        if *count == MAX_BATCH_SIZE + 1 && *max == MAX_BATCH_SIZE
    )));
}

#[test]
fn validate_valid_batch_returns_no_errors() {
    let processor = BatchProcessor::new();
    let req = make_request("v-ok", vec![fatal_envelope("a"), hello_envelope("b")]);
    let errors = processor.validate_batch(&req);
    assert!(errors.is_empty());
}

#[test]
fn validate_single_valid_envelope() {
    let processor = BatchProcessor::new();
    let req = make_request("v-one", vec![fatal_envelope("x")]);
    let errors = processor.validate_batch(&req);
    assert!(errors.is_empty());
}

#[test]
fn validate_at_max_size_is_ok() {
    let processor = BatchProcessor::new();
    let envelopes: Vec<Envelope> = (0..MAX_BATCH_SIZE)
        .map(|i| fatal_envelope(&format!("e{i}")))
        .collect();
    let req = make_request("v-max", envelopes);
    let errors = processor.validate_batch(&req);
    // Exactly at limit should pass the size check.
    assert!(
        !errors
            .iter()
            .any(|e| matches!(e, BatchValidationError::TooManyItems { .. }))
    );
}

// ---------------------------------------------------------------------------
// BatchValidationError Display
// ---------------------------------------------------------------------------

#[test]
fn validation_error_display_empty() {
    let e = BatchValidationError::EmptyBatch;
    assert_eq!(e.to_string(), "batch is empty");
}

#[test]
fn validation_error_display_too_many() {
    let e = BatchValidationError::TooManyItems {
        count: 2000,
        max: 1000,
    };
    let msg = e.to_string();
    assert!(msg.contains("2000"));
    assert!(msg.contains("1000"));
}

#[test]
fn validation_error_display_invalid_envelope() {
    let e = BatchValidationError::InvalidEnvelope {
        index: 3,
        error: "bad json".into(),
    };
    let msg = e.to_string();
    assert!(msg.contains("3"));
    assert!(msg.contains("bad json"));
}

// ---------------------------------------------------------------------------
// Default trait
// ---------------------------------------------------------------------------

#[test]
fn batch_processor_default() {
    let _p: BatchProcessor = Default::default();
}
