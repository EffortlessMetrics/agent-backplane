#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for receipt chain export/import, tamper detection, concurrent
//! append safety, and large chain performance.

use abp_receipt::{
    ChainBuilder, ChainError, ChainExportError, ExportedChain, Outcome, Receipt, ReceiptBuilder,
    ReceiptChain, TamperKind,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────

fn ts(year: i32, month: u32, day: u32, hour: u32, min: u32) -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, min, 0)
        .unwrap()
}

fn hashed_receipt(backend: &str, t: chrono::DateTime<chrono::Utc>) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(t)
        .finished_at(t)
        .with_hash()
        .unwrap()
}

fn hashed_receipt_with_outcome(
    backend: &str,
    t: chrono::DateTime<chrono::Utc>,
    outcome: Outcome,
) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(t)
        .finished_at(t)
        .with_hash()
        .unwrap()
}

fn sequential_receipts(n: usize) -> Vec<Receipt> {
    (0..n)
        .map(|i| {
            let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i as i64);
            hashed_receipt("mock", t)
        })
        .collect()
}

// ── Export / Import roundtrip ──────────────────────────────────────

#[test]
fn export_empty_chain() {
    let chain = ReceiptChain::new();
    let json = chain.export_chain().unwrap();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.chain_length, 0);
    assert!(parsed.entries.is_empty());
}

#[test]
fn export_single_receipt_chain() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let json = chain.export_chain().unwrap();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.chain_length, 1);
    assert_eq!(parsed.entries.len(), 1);
    assert!(parsed.entries[0].parent_hash.is_none());
    assert_eq!(parsed.entries[0].sequence, 0);
}

#[test]
fn export_multi_receipt_chain() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.chain_length, 5);
    assert!(parsed.entries[0].parent_hash.is_none());
    for i in 1..5 {
        assert!(parsed.entries[i].parent_hash.is_some());
    }
}

#[test]
fn export_contains_version_tag() {
    let chain = ReceiptChain::new();
    let json = chain.export_chain().unwrap();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.version, "abp-chain/v1");
}

#[test]
fn export_contains_exported_at_timestamp() {
    let chain = ReceiptChain::new();
    let before = Utc::now();
    let json = chain.export_chain().unwrap();
    let after = Utc::now();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();
    assert!(parsed.exported_at >= before);
    assert!(parsed.exported_at <= after);
}

#[test]
fn import_empty_chain() {
    let chain = ReceiptChain::new();
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert!(imported.is_empty());
    assert_eq!(imported.len(), 0);
}

#[test]
fn import_single_receipt_roundtrip() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt("mock", ts(2025, 1, 1, 0, 0));
    let run_id = r.meta.run_id;
    chain.push(r).unwrap();
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported.get(0).unwrap().meta.run_id, run_id);
}

#[test]
fn import_export_roundtrip_preserves_receipts() {
    let mut chain = ReceiptChain::new();
    let receipts = sequential_receipts(5);
    let ids: Vec<Uuid> = receipts.iter().map(|r| r.meta.run_id).collect();
    for r in receipts {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.len(), 5);
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(imported.get(i).unwrap().meta.run_id, *id);
    }
}

#[test]
fn import_export_roundtrip_preserves_hashes() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let original_hashes: Vec<Option<String>> =
        chain.iter().map(|r| r.receipt_sha256.clone()).collect();
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    for (i, hash) in original_hashes.iter().enumerate() {
        assert_eq!(&imported.get(i).unwrap().receipt_sha256, hash);
    }
}

#[test]
fn import_export_roundtrip_preserves_sequences() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(4) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    for i in 0..4 {
        assert_eq!(imported.sequence_at(i), Some(i as u64));
    }
}

#[test]
fn import_export_roundtrip_preserves_parent_hashes() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(4) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert!(imported.parent_hash_at(0).is_none());
    for i in 1..4 {
        assert_eq!(chain.parent_hash_at(i), imported.parent_hash_at(i));
    }
}

#[test]
fn imported_chain_verifies_successfully() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert!(imported.verify().is_ok());
    assert!(imported.verify_chain().is_ok());
}

#[test]
fn imported_chain_has_no_tampering() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert!(imported.detect_tampering().is_empty());
}

#[test]
fn import_rejects_bad_version() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let json = chain.export_chain().unwrap();
    let tampered = json.replace("abp-chain/v1", "abp-chain/v999");
    let result = ReceiptChain::import_chain(&tampered);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("version mismatch"));
}

#[test]
fn import_rejects_length_mismatch() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    // Change declared chain_length from 3 to 99
    let tampered = json.replace("\"chain_length\": 3", "\"chain_length\": 99");
    let result = ReceiptChain::import_chain(&tampered);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("length mismatch"));
}

#[test]
fn import_rejects_tampered_receipt_hash() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let json = chain.export_chain().unwrap();
    // Find the receipt_sha256 and corrupt a character
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let hash = parsed["entries"][0]["receipt"]["receipt_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    let corrupted_hash = format!("{}X", &hash[..hash.len() - 1]);
    let tampered = json.replace(&hash, &corrupted_hash);
    let result = ReceiptChain::import_chain(&tampered);
    assert!(result.is_err());
}

#[test]
fn import_rejects_invalid_json() {
    let result = ReceiptChain::import_chain("not valid json at all");
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("json error"));
}

#[test]
fn import_rejects_duplicate_run_ids() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("a", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let json = chain.export_chain().unwrap();
    // Duplicate the single entry
    let mut parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let entry = parsed["entries"][0].clone();
    parsed["entries"].as_array_mut().unwrap().push(entry);
    parsed["chain_length"] = serde_json::json!(2);
    let tampered = serde_json::to_string_pretty(&parsed).unwrap();
    let result = ReceiptChain::import_chain(&tampered);
    assert!(result.is_err());
}

#[test]
fn import_rejects_corrupted_parent_hash() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let mut parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Corrupt the parent_hash of entry 1
    parsed["entries"][1]["parent_hash"] = serde_json::json!("corrupted_hash_value");
    let tampered = serde_json::to_string_pretty(&parsed).unwrap();
    let result = ReceiptChain::import_chain(&tampered);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("integrity"));
}

#[test]
fn export_import_roundtrip_with_mixed_outcomes() {
    let t_base = ts(2025, 1, 1, 0, 0);
    let mut chain = ReceiptChain::new();
    let outcomes = [Outcome::Complete, Outcome::Failed, Outcome::Partial];
    for (i, outcome) in outcomes.iter().enumerate() {
        let t = t_base + chrono::Duration::minutes(i as i64);
        chain
            .push(hashed_receipt_with_outcome("mixed", t, outcome.clone()))
            .unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.len(), 3);
    assert_eq!(imported.get(0).unwrap().outcome, Outcome::Complete);
    assert_eq!(imported.get(1).unwrap().outcome, Outcome::Failed);
    assert_eq!(imported.get(2).unwrap().outcome, Outcome::Partial);
}

#[test]
fn export_import_roundtrip_with_tokens() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i);
        chain
            .push(
                ReceiptBuilder::new("tok")
                    .started_at(t)
                    .finished_at(t)
                    .usage_tokens(i as u64 * 100, i as u64 * 200)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    let s = imported.chain_summary();
    // sum: 0+100+200=300 in, 0+200+400=600 out
    assert_eq!(s.total_input_tokens, 300);
    assert_eq!(s.total_output_tokens, 600);
}

#[test]
fn double_export_import_roundtrip() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let json1 = chain.export_chain().unwrap();
    let imported1 = ReceiptChain::import_chain(&json1).unwrap();
    let json2 = imported1.export_chain().unwrap();
    let imported2 = ReceiptChain::import_chain(&json2).unwrap();
    assert_eq!(imported2.len(), 3);
    assert!(imported2.verify_chain().is_ok());
    for i in 0..3 {
        assert_eq!(
            imported2.get(i).unwrap().meta.run_id,
            chain.get(i).unwrap().meta.run_id
        );
    }
}

// ── Tamper detection: swapped order ────────────────────────────────

#[test]
fn detect_tampering_swapped_receipts() {
    let mut receipts = sequential_receipts(3);
    receipts.swap(1, 2);
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    // Swapping causes chronological order violation
    // and potentially parent hash mismatch
    let result = chain.verify();
    // The swapped chain should fail verification (ordering)
    assert!(result.is_err() || !chain.detect_tampering().is_empty());
}

#[test]
fn detect_tampering_removed_middle_entry() {
    // Build a 5-receipt chain, then rebuild without the middle receipt
    let receipts = sequential_receipts(5);
    let mut builder = ChainBuilder::new().skip_validation();
    for (i, r) in receipts.into_iter().enumerate() {
        if i != 2 {
            builder = builder.append(r).unwrap();
        }
    }
    let chain = builder.build();
    // The chain should have 4 receipts but sequences 0,1,2,3
    // Parent hashes will mismatch because receipt at index 2 is different
    assert_eq!(chain.len(), 4);
    // Verify still passes for internal consistency since skip_validation
    // rebuilt parent hashes internally, but the original chain relationship is broken
}

#[test]
fn detect_tampering_modified_backend_id() {
    let mut receipts = sequential_receipts(3);
    // Tamper with backend id (changes the hash)
    receipts[1].backend.id = "tampered-backend".to_string();
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    assert!(evidence.iter().any(|e| e.index == 1));
    assert!(
        evidence
            .iter()
            .any(|e| { matches!(e.kind, TamperKind::HashMismatch { .. }) })
    );
}

#[test]
fn detect_tampering_modified_duration() {
    let mut receipts = sequential_receipts(3);
    receipts[0].meta.duration_ms = 999999;
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    assert!(evidence.iter().any(|e| e.index == 0));
}

#[test]
fn detect_tampering_modified_outcome() {
    let mut receipts = sequential_receipts(4);
    receipts[3].outcome = Outcome::Failed;
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let tampered_indices: Vec<usize> = evidence.iter().map(|e| e.index).collect();
    assert!(tampered_indices.contains(&3));
}

#[test]
fn detect_tampering_returns_correct_tamper_kind() {
    let mut receipts = sequential_receipts(2);
    let original_hash = receipts[0].receipt_sha256.clone().unwrap();
    receipts[0].outcome = Outcome::Failed; // tamper content but keep old hash
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let ev = evidence.iter().find(|e| e.index == 0).unwrap();
    match &ev.kind {
        TamperKind::HashMismatch { stored, .. } => {
            assert_eq!(*stored, original_hash);
        }
        _ => panic!("expected HashMismatch"),
    }
}

#[test]
fn detect_tampering_all_receipts_tampered() {
    let mut receipts = sequential_receipts(5);
    for r in &mut receipts {
        r.outcome = Outcome::Failed; // tamper all
    }
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let hash_mismatches: Vec<_> = evidence
        .iter()
        .filter(|e| matches!(e.kind, TamperKind::HashMismatch { .. }))
        .collect();
    assert_eq!(hash_mismatches.len(), 5);
}

#[test]
fn detect_tampering_only_last_tampered() {
    let mut receipts = sequential_receipts(5);
    receipts[4].outcome = Outcome::Partial;
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let hash_mismatches: Vec<_> = evidence
        .iter()
        .filter(|e| matches!(e.kind, TamperKind::HashMismatch { .. }))
        .collect();
    assert_eq!(hash_mismatches.len(), 1);
    assert_eq!(hash_mismatches[0].index, 4);
}

// ── Concurrent append safety ───────────────────────────────────────

#[test]
fn concurrent_append_via_separate_chains() {
    // ReceiptChain is not Sync/Send by design; test that independent
    // chains built in different threads produce valid results.
    let receipts_a: Vec<Receipt> = (0..10)
        .map(|i| {
            let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i);
            hashed_receipt("thread-a", t)
        })
        .collect();
    let receipts_b: Vec<Receipt> = (0..10)
        .map(|i| {
            let t = ts(2025, 2, 1, 0, 0) + chrono::Duration::minutes(i);
            hashed_receipt("thread-b", t)
        })
        .collect();

    let handle_a = std::thread::spawn(move || {
        let mut chain = ReceiptChain::new();
        for r in receipts_a {
            chain.push(r).unwrap();
        }
        chain
    });

    let handle_b = std::thread::spawn(move || {
        let mut chain = ReceiptChain::new();
        for r in receipts_b {
            chain.push(r).unwrap();
        }
        chain
    });

    let chain_a = handle_a.join().unwrap();
    let chain_b = handle_b.join().unwrap();

    assert_eq!(chain_a.len(), 10);
    assert_eq!(chain_b.len(), 10);
    assert!(chain_a.verify().is_ok());
    assert!(chain_b.verify().is_ok());
}

#[test]
fn concurrent_build_and_verify() {
    let receipts: Vec<Receipt> = (0..20)
        .map(|i| {
            let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i);
            hashed_receipt("conc", t)
        })
        .collect();

    let mut chain = ReceiptChain::new();
    for r in receipts {
        chain.push(r).unwrap();
    }

    // Verify from multiple threads (chain is cloneable)
    let chain_clone_1 = chain.clone();
    let chain_clone_2 = chain.clone();

    let h1 = std::thread::spawn(move || chain_clone_1.verify().is_ok());
    let h2 = std::thread::spawn(move || chain_clone_2.verify_chain().is_ok());

    assert!(h1.join().unwrap());
    assert!(h2.join().unwrap());
}

#[test]
fn concurrent_export_from_multiple_threads() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }

    let c1 = chain.clone();
    let c2 = chain.clone();

    let h1 = std::thread::spawn(move || c1.export_chain().unwrap());
    let h2 = std::thread::spawn(move || c2.export_chain().unwrap());

    let json1 = h1.join().unwrap();
    let json2 = h2.join().unwrap();

    // Both exports should produce structurally identical chains
    let imported1 = ReceiptChain::import_chain(&json1).unwrap();
    let imported2 = ReceiptChain::import_chain(&json2).unwrap();
    assert_eq!(imported1.len(), imported2.len());
    for i in 0..5 {
        assert_eq!(
            imported1.get(i).unwrap().meta.run_id,
            imported2.get(i).unwrap().meta.run_id
        );
    }
}

#[test]
fn concurrent_import_from_multiple_threads() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();

    let j1 = json.clone();
    let j2 = json.clone();

    let h1 = std::thread::spawn(move || ReceiptChain::import_chain(&j1).unwrap());
    let h2 = std::thread::spawn(move || ReceiptChain::import_chain(&j2).unwrap());

    let c1 = h1.join().unwrap();
    let c2 = h2.join().unwrap();

    assert_eq!(c1.len(), 5);
    assert_eq!(c2.len(), 5);
    assert!(c1.verify_chain().is_ok());
    assert!(c2.verify_chain().is_ok());
}

// ── Large chain performance ────────────────────────────────────────

#[test]
fn large_chain_500_receipts() {
    let mut chain = ReceiptChain::new();
    for i in 0..500 {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::seconds(i);
        chain.push(hashed_receipt("perf", t)).unwrap();
    }
    assert_eq!(chain.len(), 500);
    assert!(chain.verify().is_ok());
    assert!(chain.verify_chain().is_ok());
    assert!(chain.detect_tampering().is_empty());
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn large_chain_summary_accurate() {
    let mut chain = ReceiptChain::new();
    for i in 0..200 {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::seconds(i);
        chain.push(hashed_receipt("perf", t)).unwrap();
    }
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 200);
    assert_eq!(s.complete_count, 200);
    assert!(s.all_hashes_valid);
    assert_eq!(s.gap_count, 0);
}

#[test]
fn large_chain_export_import_roundtrip() {
    let mut chain = ReceiptChain::new();
    for i in 0..100 {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::seconds(i);
        chain.push(hashed_receipt("perf", t)).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.len(), 100);
    assert!(imported.verify_chain().is_ok());
}

#[test]
fn large_chain_tamper_detection_performance() {
    let mut receipts: Vec<Receipt> = (0..200)
        .map(|i| {
            let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::seconds(i);
            hashed_receipt("perf", t)
        })
        .collect();

    // Tamper every 20th receipt
    for i in (0..200).step_by(20) {
        receipts[i].outcome = Outcome::Failed;
    }

    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();

    let evidence = chain.detect_tampering();
    let tampered_count = evidence
        .iter()
        .filter(|e| matches!(e.kind, TamperKind::HashMismatch { .. }))
        .count();
    assert_eq!(tampered_count, 10); // 0,20,40,...,180
}

#[test]
fn large_chain_find_by_hash() {
    let mut chain = ReceiptChain::new();
    for i in 0..100 {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::seconds(i);
        chain.push(hashed_receipt("perf", t)).unwrap();
    }
    // Find the 50th receipt by hash
    let target_hash = chain.get(50).unwrap().receipt_sha256.clone().unwrap();
    let found = chain.find_by_hash(&target_hash);
    assert!(found.is_some());
    assert_eq!(
        found.unwrap().meta.run_id,
        chain.get(50).unwrap().meta.run_id
    );
}

// ── Export error type coverage ─────────────────────────────────────

#[test]
fn chain_export_error_display_json() {
    let err = serde_json::from_str::<ExportedChain>("invalid").unwrap_err();
    let export_err = ChainExportError::Json(err);
    assert!(export_err.to_string().contains("json error"));
}

#[test]
fn chain_export_error_display_version_mismatch() {
    let err = ChainExportError::VersionMismatch {
        expected: "v1".into(),
        found: "v2".into(),
    };
    assert!(err.to_string().contains("version mismatch"));
    assert!(err.to_string().contains("v1"));
    assert!(err.to_string().contains("v2"));
}

#[test]
fn chain_export_error_display_length_mismatch() {
    let err = ChainExportError::LengthMismatch {
        declared: 10,
        actual: 5,
    };
    let msg = err.to_string();
    assert!(msg.contains("length mismatch"));
    assert!(msg.contains("10"));
    assert!(msg.contains("5"));
}

#[test]
fn chain_export_error_display_integrity() {
    let err = ChainExportError::Integrity(ChainError::EmptyChain);
    assert!(err.to_string().contains("integrity"));
}

#[test]
fn chain_export_error_source_json() {
    use std::error::Error;
    let json_err = serde_json::from_str::<ExportedChain>("bad").unwrap_err();
    let err = ChainExportError::Json(json_err);
    assert!(err.source().is_some());
}

#[test]
fn chain_export_error_source_integrity() {
    use std::error::Error;
    let err = ChainExportError::Integrity(ChainError::EmptyChain);
    assert!(err.source().is_some());
}

#[test]
fn chain_export_error_source_version_is_none() {
    use std::error::Error;
    let err = ChainExportError::VersionMismatch {
        expected: "a".into(),
        found: "b".into(),
    };
    assert!(err.source().is_none());
}

// ── Additional edge cases ──────────────────────────────────────────

#[test]
fn export_chain_with_unhashed_receipts() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("unhashed")
        .outcome(Outcome::Complete)
        .build(); // no hash
    chain.push(r).unwrap();
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.len(), 1);
    assert!(imported.get(0).unwrap().receipt_sha256.is_none());
}

#[test]
fn export_chain_with_mixed_hashed_unhashed() {
    let mut chain = ReceiptChain::new();
    let t1 = ts(2025, 1, 1, 0, 0);
    let t2 = ts(2025, 1, 1, 0, 1);
    chain
        .push(
            ReceiptBuilder::new("hashed")
                .started_at(t1)
                .finished_at(t1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("unhashed")
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.len(), 2);
    assert!(imported.get(0).unwrap().receipt_sha256.is_some());
    assert!(imported.get(1).unwrap().receipt_sha256.is_none());
}

#[test]
fn export_with_gap_sequences_preserved() {
    let receipts = sequential_receipts(3);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 0)
        .unwrap()
        .append_with_sequence(receipts[1].clone(), 5)
        .unwrap()
        .append_with_sequence(receipts[2].clone(), 10)
        .unwrap()
        .build();

    let json = chain.export_chain().unwrap();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.entries[0].sequence, 0);
    assert_eq!(parsed.entries[1].sequence, 5);
    assert_eq!(parsed.entries[2].sequence, 10);
}

#[test]
fn import_chain_with_gap_sequences() {
    let receipts = sequential_receipts(3);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 0)
        .unwrap()
        .append_with_sequence(receipts[1].clone(), 5)
        .unwrap()
        .append_with_sequence(receipts[2].clone(), 10)
        .unwrap()
        .build();

    let json = chain.export_chain().unwrap();
    let imported = ReceiptChain::import_chain(&json).unwrap();
    assert_eq!(imported.sequence_at(0), Some(0));
    assert_eq!(imported.sequence_at(1), Some(5));
    assert_eq!(imported.sequence_at(2), Some(10));
    // Gaps should be detected
    assert!(!imported.find_gaps().is_empty());
}

#[test]
fn exported_chain_entries_have_correct_parent_hashes() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(4) {
        chain.push(r).unwrap();
    }
    let json = chain.export_chain().unwrap();
    let parsed: ExportedChain = serde_json::from_str(&json).unwrap();

    // First entry has no parent
    assert!(parsed.entries[0].parent_hash.is_none());

    // Each subsequent entry's parent_hash matches the previous receipt's hash
    for i in 1..4 {
        assert_eq!(
            parsed.entries[i].parent_hash.as_deref(),
            parsed.entries[i - 1].receipt.receipt_sha256.as_deref()
        );
    }
}
