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
use abp_runtime::RuntimeError;
use insta::assert_snapshot;

#[test]
fn snapshot_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    assert_snapshot!("runtime_error_unknown_backend", err.to_string());
}

#[test]
fn snapshot_capability_check_failed_display() {
    let err = RuntimeError::CapabilityCheckFailed(
        "missing capability: mcp_client requires native but got unsupported".into(),
    );
    assert_snapshot!("runtime_error_capability_check_failed", err.to_string());
}

#[test]
fn snapshot_backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("connection refused"));
    assert_snapshot!("runtime_error_backend_failed", err.to_string());
}
