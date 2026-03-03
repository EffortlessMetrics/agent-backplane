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
