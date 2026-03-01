// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz receipt_hash() with structurally varied Receipt inputs.
//!
//! Constructs Receipt structs from fuzzer-derived fields to ensure
//! receipt_hash() and with_hash() never panic regardless of field values.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct ReceiptInput {
    backend_id: String,
    outcome_idx: u8,
    trace_count: u8,
    has_git_diff: bool,
    git_diff: String,
    has_git_status: bool,
    git_status: String,
    harness_ok: bool,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    estimated_cost: Option<f64>,
    artifact_kind: String,
    artifact_path: String,
    extra_json: String,
}

fuzz_target!(|input: ReceiptInput| {
    use abp_core::*;
    use chrono::Utc;

    let outcome = match input.outcome_idx % 3 {
        0 => Outcome::Complete,
        1 => Outcome::Partial,
        _ => Outcome::Failed,
    };

    let mut builder = ReceiptBuilder::new(&input.backend_id).outcome(outcome);

    // Add trace events.
    let now = Utc::now();
    for i in 0..input.trace_count.min(8) {
        builder = builder.add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: format!("fuzz-{i}"),
            },
            ext: None,
        });
    }

    // Add artifact.
    if !input.artifact_kind.is_empty() {
        builder = builder.add_artifact(ArtifactRef {
            kind: input.artifact_kind.clone(),
            path: input.artifact_path.clone(),
        });
    }

    // Set verification.
    builder = builder.verification(VerificationReport {
        git_diff: if input.has_git_diff {
            Some(input.git_diff.clone())
        } else {
            None
        },
        git_status: if input.has_git_status {
            Some(input.git_status.clone())
        } else {
            None
        },
        harness_ok: input.harness_ok,
    });

    // Set usage.
    builder = builder.usage(UsageNormalized {
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: input.estimated_cost.filter(|c| c.is_finite()),
    });

    // Set raw usage from arbitrary JSON.
    if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&input.extra_json) {
        builder = builder.usage_raw(raw);
    }

    let receipt = builder.build();

    // receipt_hash must never panic.
    let _ = receipt_hash(&receipt);

    // with_hash round-trip must never panic.
    if let Ok(hashed) = receipt.with_hash() {
        assert!(hashed.receipt_sha256.is_some());
        // Re-hashing the same logical receipt should be deterministic.
        let h2 = receipt_hash(&hashed).unwrap();
        assert_eq!(hashed.receipt_sha256.as_ref().unwrap(), &h2);
    }
});
