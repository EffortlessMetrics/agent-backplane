#![allow(clippy::all)]
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
use abp_core::PolicyProfile;
use insta::assert_json_snapshot;

#[test]
fn snapshot_policy_profile_default() {
    let policy = PolicyProfile::default();
    assert_json_snapshot!("policy_profile_default", policy);
}

#[test]
fn snapshot_policy_profile_tool_patterns() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "glob".into(), "grep".into()],
        disallowed_tools: vec!["bash".into(), "exec".into()],
        ..Default::default()
    };
    assert_json_snapshot!("policy_profile_tool_patterns", policy);
}

#[test]
fn snapshot_policy_profile_path_patterns() {
    let policy = PolicyProfile {
        deny_read: vec![".env".into(), "**/*.key".into(), "secrets/**".into()],
        deny_write: vec!["Cargo.lock".into(), "dist/**".into()],
        ..Default::default()
    };
    assert_json_snapshot!("policy_profile_path_patterns", policy);
}
