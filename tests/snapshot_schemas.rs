#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Snapshot tests for JSON schema stability of core contract types.
//!
//! These tests use `schemars::schema_for!` to generate JSON schemas and
//! `insta::assert_snapshot!` to detect unintended schema changes.

// ── Core types ──────────────────────────────────────────────────────────────

#[test]
fn work_order_schema() {
    let schema = schemars::schema_for!(abp_core::WorkOrder);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn receipt_schema() {
    let schema = schemars::schema_for!(abp_core::Receipt);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn agent_event_schema() {
    let schema = schemars::schema_for!(abp_core::AgentEvent);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn agent_event_kind_schema() {
    let schema = schemars::schema_for!(abp_core::AgentEventKind);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn capability_schema() {
    let schema = schemars::schema_for!(abp_core::Capability);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn support_level_schema() {
    let schema = schemars::schema_for!(abp_core::SupportLevel);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn policy_profile_schema() {
    let schema = schemars::schema_for!(abp_core::PolicyProfile);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn execution_mode_schema() {
    let schema = schemars::schema_for!(abp_core::ExecutionMode);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn outcome_schema() {
    let schema = schemars::schema_for!(abp_core::Outcome);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn backend_identity_schema() {
    let schema = schemars::schema_for!(abp_core::BackendIdentity);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

// ── Error types ─────────────────────────────────────────────────────────────

#[test]
fn error_code_schema() {
    let schema = schemars::schema_for!(abp_error::ErrorCode);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

// ── IR types ────────────────────────────────────────────────────────────────

#[test]
fn ir_message_schema() {
    let schema = schemars::schema_for!(abp_core::ir::IrMessage);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn ir_conversation_schema() {
    let schema = schemars::schema_for!(abp_core::ir::IrConversation);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn ir_role_schema() {
    let schema = schemars::schema_for!(abp_core::ir::IrRole);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn ir_content_block_schema() {
    let schema = schemars::schema_for!(abp_core::ir::IrContentBlock);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}

#[test]
fn ir_usage_schema() {
    let schema = schemars::schema_for!(abp_core::ir::IrUsage);
    insta::assert_snapshot!(serde_json::to_string_pretty(&schema).unwrap());
}
