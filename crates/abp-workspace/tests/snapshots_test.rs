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
use abp_core::{WorkspaceMode, WorkspaceSpec};
use insta::assert_json_snapshot;

#[test]
fn snapshot_workspace_spec_pass_through() {
    let spec = WorkspaceSpec {
        root: "/home/user/project".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    assert_json_snapshot!("workspace_spec_pass_through", spec);
}

#[test]
fn snapshot_workspace_spec_staged() {
    let spec = WorkspaceSpec {
        root: "/tmp/workspace".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    assert_json_snapshot!("workspace_spec_staged", spec);
}

#[test]
fn snapshot_workspace_spec_with_globs() {
    let spec = WorkspaceSpec {
        root: "/home/user/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into(), "Cargo.toml".into()],
        exclude: vec!["target/**".into(), "*.log".into(), ".git/**".into()],
    };
    assert_json_snapshot!("workspace_spec_with_globs", spec);
}
