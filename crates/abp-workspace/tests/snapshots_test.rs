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
