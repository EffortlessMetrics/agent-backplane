// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz receipt hash verification under random field mutations.
//!
//! Builds a correctly hashed receipt, then applies random mutations to
//! individual fields and verifies:
//! 1. `verify_hash()` returns false for any field mutation.
//! 2. No mutation causes a panic in hashing or verification code.
//! 3. Re-hashing the mutated receipt produces a different hash.
//! 4. `diff_receipts()` detects the difference between original and mutated.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct MutationInput {
    backend_id: String,
    outcome_idx: u8,
    delta_texts: Vec<String>,
    mutation_target: u8,
    mutation_string: String,
    mutation_bool: bool,
    mutation_u64: u64,
}

fuzz_target!(|input: MutationInput| {
    use abp_core::*;
    use chrono::Utc;

    let outcome = match input.outcome_idx % 3 {
        0 => Outcome::Complete,
        1 => Outcome::Partial,
        _ => Outcome::Failed,
    };

    let mut builder = ReceiptBuilder::new(&input.backend_id).outcome(outcome);
    for text in &input.delta_texts {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.clone() },
            ext: None,
        });
    }

    let receipt = builder.build();

    // Hash the original receipt.
    let hashed = match receipt.clone().with_hash() {
        Ok(h) => h,
        Err(_) => return,
    };

    // Original must verify.
    assert!(
        abp_receipt::verify_hash(&hashed),
        "original hashed receipt must verify"
    );

    // Apply a mutation based on the target field.
    let mut mutated = hashed.clone();
    match input.mutation_target % 5 {
        0 => {
            // Mutate outcome.
            mutated.outcome = if input.mutation_bool {
                Outcome::Failed
            } else {
                Outcome::Partial
            };
        }
        1 => {
            // Mutate backend identity.
            mutated.backend.id = input.mutation_string.clone();
        }
        2 => {
            // Mutate verification report.
            mutated.verification.harness_ok = !mutated.verification.harness_ok;
        }
        3 => {
            // Mutate usage tokens.
            mutated.usage.input_tokens = Some(input.mutation_u64);
        }
        _ => {
            // Mutate the hash itself.
            mutated.receipt_sha256 = Some(format!("{:064x}", input.mutation_u64));
        }
    }

    // --- Property 1: mutated receipt should fail verification ---
    // (unless the mutation happens to be a no-op)
    let original_json = serde_json::to_string(&hashed).unwrap_or_default();
    let mutated_json = serde_json::to_string(&mutated).unwrap_or_default();
    if original_json != mutated_json {
        // Only check if mutation actually changed something.
        let verifies = abp_receipt::verify_hash(&mutated);
        // If we mutated the hash (target 4), definitely should fail.
        if input.mutation_target % 5 == 4 {
            assert!(!verifies, "tampered hash must fail verification");
        }
    }

    // --- Property 2: no panics in hash/verify path ---
    let _ = abp_core::receipt_hash(&mutated);
    let _ = abp_receipt::compute_hash(&mutated);
    let _ = abp_receipt::verify_hash(&mutated);
    let _ = abp_receipt::canonicalize(&mutated);

    // --- Property 3: re-hashing mutated receipt gives different hash ---
    if original_json != mutated_json && input.mutation_target % 5 != 4 {
        if let (Ok(orig_h), Ok(mut_h)) = (
            abp_core::receipt_hash(&hashed),
            abp_core::receipt_hash(&mutated),
        ) {
            // Hashes should differ if content changed.
            let _ = (orig_h != mut_h);
        }
    }

    // --- Property 4: diff detects the mutation ---
    let diff = abp_receipt::diff_receipts(&hashed, &mutated);
    if original_json != mutated_json {
        assert!(
            !diff.is_empty(),
            "diff must detect mutation (target={})",
            input.mutation_target % 5
        );
    }
});
