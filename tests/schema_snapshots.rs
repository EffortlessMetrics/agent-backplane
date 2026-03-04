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
