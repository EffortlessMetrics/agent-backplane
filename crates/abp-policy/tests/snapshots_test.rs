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
