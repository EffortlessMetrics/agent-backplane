use abp_core::{
    BackendIdentity, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport, receipt_hash,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

#[test]
fn receipt_hash_matches_stored_hash() {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    let receipt = Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".to_string(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: Default::default(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let receipt = receipt.with_hash().expect("hash receipt");
    let recomputed = receipt_hash(&receipt).expect("recompute receipt hash");

    assert_eq!(receipt.receipt_sha256.as_deref(), Some(recomputed.as_str()));
}
