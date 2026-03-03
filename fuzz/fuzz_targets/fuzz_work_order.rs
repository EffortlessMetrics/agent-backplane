// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrder deserialization from arbitrary bytes and structured input.
//!
//! Verifies:
//! 1. `serde_json` deserialization of WorkOrder never panics on any input.
//! 2. Successfully parsed WorkOrders survive JSON round-trips deterministically.
//! 3. `WorkOrderBuilder` never panics with arbitrary parameters.
//! 4. Builder-produced WorkOrders round-trip through JSON.
//! 5. WorkOrder serialization is deterministic (BTreeMap ordering).
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct WorkOrderFuzzInput {
    raw_json: Vec<u8>,
    // Builder fields.
    task: String,
    lane_idx: u8,
    root: String,
    workspace_mode_idx: u8,
    include: Vec<String>,
    exclude: Vec<String>,
    model: Option<String>,
    max_budget_usd: Option<f64>,
    max_turns: Option<u32>,
    context_files: Vec<String>,
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
}

fuzz_target!(|input: WorkOrderFuzzInput| {
    use abp_core::*;

    // --- Property 1: JSON deserialization never panics ---
    if let Ok(s) = std::str::from_utf8(&input.raw_json) {
        if let Ok(wo) = serde_json::from_str::<WorkOrder>(s) {
            // --- Property 2: round-trip preserves structure ---
            if let Ok(json) = serde_json::to_string(&wo) {
                let rt = serde_json::from_str::<WorkOrder>(&json);
                assert!(rt.is_ok(), "WorkOrder JSON round-trip must succeed");

                // Parsed WorkOrder serialization is deterministic.
                let json2 = serde_json::to_string(&wo).unwrap();
                assert_eq!(json, json2, "parsed WorkOrder re-serialization must be deterministic");
            }
        }
    }

    // --- Property 3: WorkOrderBuilder never panics ---
    let lane = match input.lane_idx % 2 {
        0 => ExecutionLane::PatchFirst,
        _ => ExecutionLane::WorkspaceFirst,
    };
    let ws_mode = match input.workspace_mode_idx % 2 {
        0 => WorkspaceMode::PassThrough,
        _ => WorkspaceMode::Staged,
    };

    let policy = PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };

    let ctx = ContextPacket {
        files: input.context_files,
        snippets: vec![],
    };

    let mut builder = WorkOrderBuilder::new(&input.task)
        .lane(lane)
        .root(&input.root)
        .workspace_mode(ws_mode)
        .include(input.include)
        .exclude(input.exclude)
        .policy(policy)
        .context(ctx);

    if let Some(ref model) = input.model {
        builder = builder.model(model);
    }
    if let Some(budget) = input.max_budget_usd {
        if budget.is_finite() {
            builder = builder.max_budget_usd(budget);
        }
    }
    if let Some(turns) = input.max_turns {
        builder = builder.max_turns(turns);
    }

    let wo = builder.build();

    // --- Property 5: builder output serializes and round-trips ---
    let json_rt = serde_json::to_string(&wo).and_then(|j| serde_json::from_str::<WorkOrder>(&j));
    assert!(json_rt.is_ok(), "builder WorkOrder must round-trip through JSON");

    // --- Property 6: serialization is deterministic ---
    let json1 = serde_json::to_string(&wo);
    let json2 = serde_json::to_string(&wo);
    if let (Ok(j1), Ok(j2)) = (&json1, &json2) {
        assert_eq!(j1, j2, "WorkOrder serialization must be deterministic");
    }
});
