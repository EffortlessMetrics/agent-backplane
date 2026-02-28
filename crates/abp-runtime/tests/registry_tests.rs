// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for [`BackendRegistry`].

use abp_integrations::MockBackend;
use abp_runtime::registry::BackendRegistry;

#[test]
fn register_and_retrieve() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    let backend = reg.get("mock").expect("should find mock");
    assert_eq!(backend.identity().id, "mock");
}

#[test]
fn list_backends() {
    let mut reg = BackendRegistry::default();
    reg.register("beta", MockBackend);
    reg.register("alpha", MockBackend);
    assert_eq!(reg.list(), vec!["alpha", "beta"]);
}

#[test]
fn remove_backend() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.contains("mock"));

    let removed = reg.remove("mock");
    assert!(removed.is_some());
    assert!(!reg.contains("mock"));
    assert!(reg.get("mock").is_none());
}

#[test]
fn get_nonexistent_returns_none() {
    let reg = BackendRegistry::default();
    assert!(reg.get("nope").is_none());
    assert!(!reg.contains("nope"));
}

#[test]
fn register_same_name_replaces() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    reg.register("mock", MockBackend);
    // Still only one entry
    assert_eq!(reg.list().len(), 1);
    assert!(reg.get("mock").is_some());
}
