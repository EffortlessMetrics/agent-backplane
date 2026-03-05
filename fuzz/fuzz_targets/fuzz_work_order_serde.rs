// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder roundtrip: deserialize arbitrary JSON → serialize → compare.
//!
//! Exercises both raw-bytes and structured-Arbitrary paths to verify:
//! 1. `serde_json::from_slice` / `from_str` never panics on any input.
//! 2. Successfully parsed WorkOrders survive JSON round-trips losslessly.
//! 3. `canonical_json` produces identical output on identical WorkOrders.
//! 4. Structured fuzzing constructs valid WorkOrders and round-trips them.
//! 5. Adjacent contract types (RuntimeConfig, WorkspaceSpec, ContextPacket)
//!    never panic on arbitrary JSON.
//! 6. WorkOrder fields are consistent after deserialization.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_core::{canonical_json, ContextPacket, RuntimeConfig, WorkOrder, WorkspaceSpec};

#[derive(Debug, Arbitrary)]
struct WorkOrderFuzzInput {
    /// Raw bytes for unstructured deserialization.
    raw_bytes: Vec<u8>,
    /// Structured fields for constructing a WorkOrder via JSON.
    task: String,
    backend_id: String,
    model: Option<String>,
    workspace_root: Option<String>,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    context_files: Vec<String>,
    context_text: Option<String>,
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    env_keys: Vec<String>,
    env_vals: Vec<String>,
    extra_json: String,
}

fuzz_target!(|input: WorkOrderFuzzInput| {
    // ===== Path 1: raw bytes deserialization =====
    let from_bytes: Result<WorkOrder, _> = serde_json::from_slice(&input.raw_bytes);

    if let Ok(ref wo) = from_bytes {
        // Round-trip: serialize → deserialize must not panic.
        if let Ok(json) = serde_json::to_string(wo) {
            let rt: Result<WorkOrder, _> = serde_json::from_str(&json);
            assert!(rt.is_ok(), "byte-path round-trip must succeed");
        }
        // canonical_json must not panic.
        let _ = canonical_json(wo);
    }

    // ===== Path 2: UTF-8 string deserialization =====
    if let Ok(s) = std::str::from_utf8(&input.raw_bytes) {
        let from_str: Result<WorkOrder, _> = serde_json::from_str(s);

        if let Ok(wo) = from_str {
            // --- Property 2: JSON round-trip ---
            let json1 = serde_json::to_string(&wo).expect("serialize must succeed");
            let rt: WorkOrder = serde_json::from_str(&json1).expect("round-trip must succeed");
            let json2 = serde_json::to_string(&rt).expect("re-serialize must succeed");

            // --- Property 3: canonical_json determinism ---
            if let (Ok(c1), Ok(c2)) = (canonical_json(&wo), canonical_json(&rt)) {
                assert_eq!(
                    c1, c2,
                    "canonical JSON must be deterministic across round-trips"
                );
            }

            // Serialized JSON must be identical after round-trip.
            assert_eq!(json1, json2, "JSON round-trip must be lossless");
        }

        // --- Property 5: adjacent types never panic ---
        let _ = serde_json::from_str::<RuntimeConfig>(s);
        let _ = serde_json::from_str::<WorkspaceSpec>(s);
        let _ = serde_json::from_str::<ContextPacket>(s);
    }

    // ===== Path 3: structured construction via JSON =====
    // Build a JSON object from structured fields and try to deserialize.
    let mut env_map = serde_json::Map::new();
    for (k, v) in input.env_keys.iter().zip(input.env_vals.iter()) {
        env_map.insert(k.clone(), serde_json::Value::String(v.clone()));
    }

    let extra = serde_json::from_str::<serde_json::Value>(&input.extra_json)
        .unwrap_or(serde_json::Value::Null);

    let wo_json = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "contract_version": "abp/v0.1",
        "task": input.task,
        "config": {
            "backend": input.backend_id,
            "model": input.model,
            "workspace": {
                "root": input.workspace_root,
                "include": input.include_patterns,
                "exclude": input.exclude_patterns,
            },
            "context": {
                "files": input.context_files,
                "text": input.context_text,
            },
            "policy": {
                "allowed_tools": input.allowed_tools,
                "disallowed_tools": input.disallowed_tools,
            },
            "env": env_map,
        },
        "ext": extra,
    });

    if let Ok(wo) = serde_json::from_value::<WorkOrder>(wo_json) {
        // --- Property 6: field consistency ---
        assert_eq!(wo.task, input.task, "task must match after deser");

        // Round-trip the constructed WorkOrder.
        let json = serde_json::to_string(&wo).expect("serialize constructed WO");
        let rt: Result<WorkOrder, _> = serde_json::from_str(&json);
        assert!(rt.is_ok(), "constructed WO round-trip must succeed");

        // canonical_json on constructed WO.
        let c1 = canonical_json(&wo);
        let c2 = canonical_json(&rt.unwrap());
        if let (Ok(a), Ok(b)) = (c1, c2) {
            assert_eq!(a, b, "canonical JSON must match for constructed WO");
        }
    }
});
