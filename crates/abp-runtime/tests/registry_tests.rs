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
