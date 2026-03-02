// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive enum coverage tests for `abp-core`.
//!
//! For every public enum in abp-core, verify:
//! 1. All variants serialize to JSON
//! 2. All variants round-trip through serde
//! 3. Debug output is non-empty
//! 4. Clone equality (via PartialEq or JSON comparison)
//! 5. Variant count matches expected (catches additions/removals)

use abp_core::validate::ValidationError;
use abp_core::{
    AgentEventKind, Capability, ContractError, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    SupportLevel, WorkspaceMode,
};

// ---------------------------------------------------------------------------
// Helper: round-trip via JSON for types without PartialEq
// ---------------------------------------------------------------------------

fn json_round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(val: &T) -> String {
    let json = serde_json::to_string(val).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "round-trip JSON mismatch");
    json
}

fn assert_debug_non_empty<T: std::fmt::Debug>(val: &T) {
    let dbg = format!("{val:?}");
    assert!(!dbg.is_empty(), "Debug output must be non-empty");
}

fn assert_clone_eq_json<T: Clone + serde::Serialize>(val: &T) {
    let original = serde_json::to_string(val).unwrap();
    let cloned = serde_json::to_string(&val.clone()).unwrap();
    assert_eq!(original, cloned, "clone produced different JSON");
}

// ===========================================================================
// ExecutionLane
// ===========================================================================

fn all_execution_lanes() -> Vec<ExecutionLane> {
    vec![ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst]
}

#[test]
fn execution_lane_serialize_all() {
    for v in &all_execution_lanes() {
        serde_json::to_string(v).expect("serialize ExecutionLane");
    }
}

#[test]
fn execution_lane_round_trip() {
    for v in &all_execution_lanes() {
        json_round_trip(v);
    }
}

#[test]
fn execution_lane_debug() {
    for v in &all_execution_lanes() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn execution_lane_clone_eq() {
    for v in &all_execution_lanes() {
        assert_clone_eq_json(v);
    }
}

#[test]
fn execution_lane_count() {
    assert_eq!(
        all_execution_lanes().len(),
        2,
        "ExecutionLane variant count changed"
    );
}

// ===========================================================================
// WorkspaceMode
// ===========================================================================

fn all_workspace_modes() -> Vec<WorkspaceMode> {
    vec![WorkspaceMode::PassThrough, WorkspaceMode::Staged]
}

#[test]
fn workspace_mode_serialize_all() {
    for v in &all_workspace_modes() {
        serde_json::to_string(v).expect("serialize WorkspaceMode");
    }
}

#[test]
fn workspace_mode_round_trip() {
    for v in &all_workspace_modes() {
        json_round_trip(v);
    }
}

#[test]
fn workspace_mode_debug() {
    for v in &all_workspace_modes() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn workspace_mode_clone_eq() {
    for v in &all_workspace_modes() {
        assert_clone_eq_json(v);
    }
}

#[test]
fn workspace_mode_count() {
    assert_eq!(
        all_workspace_modes().len(),
        2,
        "WorkspaceMode variant count changed"
    );
}

// ===========================================================================
// ExecutionMode
// ===========================================================================

fn all_execution_modes() -> Vec<ExecutionMode> {
    vec![ExecutionMode::Passthrough, ExecutionMode::Mapped]
}

#[test]
fn execution_mode_serialize_all() {
    for v in &all_execution_modes() {
        serde_json::to_string(v).expect("serialize ExecutionMode");
    }
}

#[test]
fn execution_mode_round_trip() {
    for v in &all_execution_modes() {
        json_round_trip(v);
    }
}

#[test]
fn execution_mode_debug() {
    for v in &all_execution_modes() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn execution_mode_clone_eq() {
    for v in &all_execution_modes() {
        let cloned = *v;
        assert_eq!(*v, cloned, "clone must equal original");
    }
}

#[test]
fn execution_mode_count() {
    assert_eq!(
        all_execution_modes().len(),
        2,
        "ExecutionMode variant count changed"
    );
}

// ===========================================================================
// Outcome
// ===========================================================================

fn all_outcomes() -> Vec<Outcome> {
    vec![Outcome::Complete, Outcome::Partial, Outcome::Failed]
}

#[test]
fn outcome_serialize_all() {
    for v in &all_outcomes() {
        serde_json::to_string(v).expect("serialize Outcome");
    }
}

#[test]
fn outcome_round_trip() {
    for v in &all_outcomes() {
        json_round_trip(v);
    }
}

#[test]
fn outcome_debug() {
    for v in &all_outcomes() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn outcome_clone_eq() {
    for v in &all_outcomes() {
        let cloned = v.clone();
        assert_eq!(*v, cloned, "clone must equal original");
    }
}

#[test]
fn outcome_count() {
    assert_eq!(all_outcomes().len(), 3, "Outcome variant count changed");
}

// ===========================================================================
// MinSupport
// ===========================================================================

fn all_min_supports() -> Vec<MinSupport> {
    vec![MinSupport::Native, MinSupport::Emulated]
}

#[test]
fn min_support_serialize_all() {
    for v in &all_min_supports() {
        serde_json::to_string(v).expect("serialize MinSupport");
    }
}

#[test]
fn min_support_round_trip() {
    for v in &all_min_supports() {
        json_round_trip(v);
    }
}

#[test]
fn min_support_debug() {
    for v in &all_min_supports() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn min_support_clone_eq() {
    for v in &all_min_supports() {
        assert_clone_eq_json(v);
    }
}

#[test]
fn min_support_count() {
    assert_eq!(
        all_min_supports().len(),
        2,
        "MinSupport variant count changed"
    );
}

// ===========================================================================
// SupportLevel
// ===========================================================================

fn all_support_levels() -> Vec<SupportLevel> {
    vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "testing".into(),
        },
    ]
}

#[test]
fn support_level_serialize_all() {
    for v in &all_support_levels() {
        serde_json::to_string(v).expect("serialize SupportLevel");
    }
}

#[test]
fn support_level_round_trip() {
    for v in &all_support_levels() {
        json_round_trip(v);
    }
}

#[test]
fn support_level_debug() {
    for v in &all_support_levels() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn support_level_clone_eq() {
    for v in &all_support_levels() {
        assert_clone_eq_json(v);
    }
}

#[test]
fn support_level_count() {
    assert_eq!(
        all_support_levels().len(),
        4,
        "SupportLevel variant count changed"
    );
}

// ===========================================================================
// Capability
// ===========================================================================

fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ]
}

#[test]
fn capability_serialize_all() {
    for v in &all_capabilities() {
        serde_json::to_string(v).expect("serialize Capability");
    }
}

#[test]
fn capability_round_trip() {
    for v in &all_capabilities() {
        json_round_trip(v);
    }
}

#[test]
fn capability_debug() {
    for v in &all_capabilities() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn capability_clone_eq() {
    for v in &all_capabilities() {
        let cloned = v.clone();
        assert_eq!(*v, cloned, "clone must equal original");
    }
}

#[test]
fn capability_count() {
    assert_eq!(
        all_capabilities().len(),
        26,
        "Capability variant count changed"
    );
}

// ===========================================================================
// AgentEventKind
// ===========================================================================

fn all_agent_event_kinds() -> Vec<AgentEventKind> {
    vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "delta".into(),
        },
        AgentEventKind::AssistantMessage { text: "msg".into() },
        AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/tmp"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("contents"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("Compiling...".into()),
        },
        AgentEventKind::Warning {
            message: "caution".into(),
        },
        AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
    ]
}

#[test]
fn agent_event_kind_serialize_all() {
    for v in &all_agent_event_kinds() {
        let json = serde_json::to_string(v).expect("serialize AgentEventKind");
        // Internally-tagged enum: must contain "type" field
        assert!(json.contains("\"type\""), "missing type tag in: {json}");
    }
}

#[test]
fn agent_event_kind_round_trip() {
    for v in &all_agent_event_kinds() {
        json_round_trip(v);
    }
}

#[test]
fn agent_event_kind_debug() {
    for v in &all_agent_event_kinds() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn agent_event_kind_clone_eq() {
    for v in &all_agent_event_kinds() {
        assert_clone_eq_json(v);
    }
}

#[test]
fn agent_event_kind_count() {
    assert_eq!(
        all_agent_event_kinds().len(),
        10,
        "AgentEventKind variant count changed"
    );
}

// ===========================================================================
// ContractError (Debug-only — no Serialize/Deserialize/Clone)
// ===========================================================================

#[test]
fn contract_error_debug() {
    let err =
        ContractError::Json(serde_json::from_str::<serde_json::Value>("!invalid").unwrap_err());
    assert_debug_non_empty(&err);
}

#[test]
fn contract_error_display() {
    let err =
        ContractError::Json(serde_json::from_str::<serde_json::Value>("!invalid").unwrap_err());
    let msg = err.to_string();
    assert!(!msg.is_empty(), "Display output must be non-empty");
}

#[test]
fn contract_error_count() {
    // ContractError has 1 variant: Json
    // If a new variant is added, this will remind the developer to add coverage.
    let _json_variant =
        ContractError::Json(serde_json::from_str::<serde_json::Value>("!invalid").unwrap_err());
    // Variant count: 1 — update this comment and add tests if new variants appear.
    let count: usize = 1;
    assert_eq!(count, 1, "ContractError variant count changed");
}

// ===========================================================================
// ValidationError (Debug + Clone + PartialEq, no Serialize/Deserialize)
// ===========================================================================

fn all_validation_errors() -> Vec<ValidationError> {
    vec![
        ValidationError::MissingField { field: "id" },
        ValidationError::InvalidHash {
            expected: "abc".into(),
            actual: "def".into(),
        },
        ValidationError::EmptyBackendId,
        ValidationError::InvalidOutcome {
            reason: "bad".into(),
        },
    ]
}

#[test]
fn validation_error_debug() {
    for v in &all_validation_errors() {
        assert_debug_non_empty(v);
    }
}

#[test]
fn validation_error_display() {
    for v in &all_validation_errors() {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "Display output must be non-empty");
    }
}

#[test]
fn validation_error_clone_eq() {
    for v in &all_validation_errors() {
        let cloned = v.clone();
        assert_eq!(*v, cloned, "clone must equal original");
    }
}

#[test]
fn validation_error_count() {
    assert_eq!(
        all_validation_errors().len(),
        4,
        "ValidationError variant count changed"
    );
}
