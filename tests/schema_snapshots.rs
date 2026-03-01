// SPDX-License-Identifier: MIT OR Apache-2.0
//! Insta snapshot tests for JSON schemas of all major contract types.
//! These catch contract drift by snapshotting the generated JSON schema.

use schemars::schema_for;

#[test]
fn work_order_schema() {
    let schema = schema_for!(abp_core::WorkOrder);
    insta::assert_json_snapshot!("work_order_schema", schema);
}

#[test]
fn receipt_schema() {
    let schema = schema_for!(abp_core::Receipt);
    insta::assert_json_snapshot!("receipt_schema", schema);
}

#[test]
fn agent_event_schema() {
    let schema = schema_for!(abp_core::AgentEvent);
    insta::assert_json_snapshot!("agent_event_schema", schema);
}

#[test]
fn agent_event_kind_schema() {
    let schema = schema_for!(abp_core::AgentEventKind);
    insta::assert_json_snapshot!("agent_event_kind_schema", schema);
}

#[test]
fn policy_profile_schema() {
    let schema = schema_for!(abp_core::PolicyProfile);
    insta::assert_json_snapshot!("policy_profile_schema", schema);
}

#[test]
fn capability_manifest_schema() {
    let schema = schema_for!(abp_core::CapabilityManifest);
    insta::assert_json_snapshot!("capability_manifest_schema", schema);
}

#[test]
fn capability_requirements_schema() {
    let schema = schema_for!(abp_core::CapabilityRequirements);
    insta::assert_json_snapshot!("capability_requirements_schema", schema);
}

#[test]
fn runtime_config_schema() {
    let schema = schema_for!(abp_core::RuntimeConfig);
    insta::assert_json_snapshot!("runtime_config_schema", schema);
}

#[test]
fn workspace_spec_schema() {
    let schema = schema_for!(abp_core::WorkspaceSpec);
    insta::assert_json_snapshot!("workspace_spec_schema", schema);
}

#[test]
fn backend_identity_schema() {
    let schema = schema_for!(abp_core::BackendIdentity);
    insta::assert_json_snapshot!("backend_identity_schema", schema);
}

#[test]
fn execution_lane_schema() {
    let schema = schema_for!(abp_core::ExecutionLane);
    insta::assert_json_snapshot!("execution_lane_schema", schema);
}

#[test]
fn outcome_schema() {
    let schema = schema_for!(abp_core::Outcome);
    insta::assert_json_snapshot!("outcome_schema", schema);
}
