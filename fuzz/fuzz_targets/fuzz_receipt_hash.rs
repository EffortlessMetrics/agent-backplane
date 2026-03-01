// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz receipt_hash() with random Receipt data.
//!
//! Constructs Receipt structs from structured fuzzer input and verifies:
//! 1. `receipt_hash()` never panics on any input.
//! 2. The returned hash is always valid lowercase hex (64 chars for SHA-256).
//! 3. Hashing the same receipt twice produces identical output (determinism).
//! 4. `with_hash()` embeds the hash and re-hashing is consistent.
//! 5. `validate_receipt()` never panics on hashed or unhashed receipts.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct FuzzReceipt {
    backend_id: String,
    outcome_idx: u8,
    has_diff: bool,
    diff: String,
    has_status: bool,
    status: String,
    harness_ok: bool,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cost: Option<f64>,
    delta_texts: Vec<String>,
    tool_name: Option<String>,
    tool_input: Option<String>,
    artifact_paths: Vec<String>,
    raw_json: String,
}

fuzz_target!(|input: FuzzReceipt| {
    use abp_core::*;
    use chrono::Utc;

    let outcome = match input.outcome_idx % 3 {
        0 => Outcome::Complete,
        1 => Outcome::Partial,
        _ => Outcome::Failed,
    };

    let now = Utc::now();
    let mut builder = ReceiptBuilder::new(&input.backend_id).outcome(outcome);

    // Add assistant delta events.
    for text in &input.delta_texts {
        builder = builder.add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta { text: text.clone() },
            ext: None,
        });
    }

    // Add tool call event if present.
    if let (Some(tool_name), Some(tool_input)) = (&input.tool_name, &input.tool_input) {
        builder = builder.add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: tool_name.clone(),
                tool_use_id: Some("fuzz-tool-id".to_string()),
                parent_tool_use_id: None,
                input: serde_json::Value::String(tool_input.clone()),
            },
            ext: None,
        });
    }

    // Add artifacts.
    for path in &input.artifact_paths {
        builder = builder.add_artifact(ArtifactRef {
            kind: "file".to_string(),
            path: path.clone(),
        });
    }

    // Set verification.
    builder = builder.verification(VerificationReport {
        git_diff: if input.has_diff { Some(input.diff.clone()) } else { None },
        git_status: if input.has_status { Some(input.status.clone()) } else { None },
        harness_ok: input.harness_ok,
    });

    // Set usage.
    builder = builder.usage(UsageNormalized {
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: input.cost.filter(|c| c.is_finite()),
    });

    // Set raw usage.
    if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&input.raw_json) {
        builder = builder.usage_raw(raw);
    }

    let receipt = builder.build();

    // --- Property 1: receipt_hash never panics ---
    let hash1 = receipt_hash(&receipt);

    // --- Property 2: hash is valid lowercase hex, 64 chars ---
    if let Ok(ref h) = hash1 {
        assert_eq!(h.len(), 64, "SHA-256 hex must be 64 chars");
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "hash must be lowercase hex"
        );
    }

    // --- Property 3: deterministic ---
    let hash2 = receipt_hash(&receipt);
    assert_eq!(hash1.as_ref().ok(), hash2.as_ref().ok(), "hashing same receipt must be deterministic");

    // --- Property 4: with_hash embeds correctly ---
    if let Ok(hashed) = receipt.clone().with_hash() {
        let embedded = hashed.receipt_sha256.as_ref().expect("with_hash must set sha256");
        assert_eq!(embedded.len(), 64);
        // Re-hash and compare.
        let rehash = receipt_hash(&hashed).expect("rehash must succeed");
        assert_eq!(embedded, &rehash, "embedded hash must match re-computed hash");
    }

    // --- Property 5: validate never panics ---
    let _ = abp_core::validate::validate_receipt(&receipt);

    // --- Property 6: abp-receipt crate functions agree with abp-core ---
    let receipt_crate_hash = abp_receipt::compute_hash(&receipt);
    if let (Ok(core_h), Ok(crate_h)) = (&hash1, &receipt_crate_hash) {
        assert_eq!(core_h, crate_h, "abp-core and abp-receipt hashes must agree");
    }

    // --- Property 7: abp-receipt canonicalize never panics ---
    let _ = abp_receipt::canonicalize(&receipt);

    // --- Property 8: abp-receipt verify_hash is consistent ---
    // Unhashed receipt (receipt_sha256 is None) should verify as true.
    assert!(
        abp_receipt::verify_hash(&receipt),
        "verify_hash must return true for unhashed receipt"
    );

    if let Ok(hashed) = receipt.clone().with_hash() {
        // Hashed receipt must verify.
        assert!(
            abp_receipt::verify_hash(&hashed),
            "verify_hash must return true for correctly hashed receipt"
        );

        // Tampered hash must fail.
        let mut tampered = hashed.clone();
        tampered.receipt_sha256 = Some("deadbeef".into());
        assert!(
            !abp_receipt::verify_hash(&tampered),
            "verify_hash must return false for tampered hash"
        );
    }

    // --- Property 9: abp-receipt diff never panics ---
    let receipt2 = abp_core::ReceiptBuilder::new("fuzz-other")
        .outcome(Outcome::Complete)
        .build();
    let diff = abp_receipt::diff_receipts(&receipt, &receipt2);
    let _ = diff.len();
    let _ = diff.is_empty();

    // --- Property 10: abp-receipt ReceiptChain never panics on push ---
    let mut chain = abp_receipt::ReceiptChain::new();
    // Push requires hashed receipts; try with_hash first.
    if let Ok(hashed) = receipt.clone().with_hash() {
        let _ = chain.push(hashed);
    }
});
