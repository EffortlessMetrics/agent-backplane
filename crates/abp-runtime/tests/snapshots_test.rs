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
