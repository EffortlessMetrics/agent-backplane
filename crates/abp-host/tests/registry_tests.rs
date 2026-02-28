// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `SidecarRegistry`.

use abp_host::registry::SidecarRegistry;
use abp_host::SidecarSpec;
use std::path::Path;

#[test]
fn register_and_retrieve() {
    let mut reg = SidecarRegistry::default();
    reg.register("node", SidecarSpec::new("node"));
    let spec = reg.get("node").expect("should find registered sidecar");
    assert_eq!(spec.command, "node");
}

#[test]
fn list_sidecars() {
    let mut reg = SidecarRegistry::default();
    reg.register("python", SidecarSpec::new("python"));
    reg.register("node", SidecarSpec::new("node"));
    // BTreeMap keeps sorted order.
    assert_eq!(reg.list(), vec!["node", "python"]);
}

#[test]
fn remove_sidecar() {
    let mut reg = SidecarRegistry::default();
    reg.register("node", SidecarSpec::new("node"));
    let removed = reg.remove("node");
    assert!(removed.is_some());
    assert!(reg.get("node").is_none());
    assert!(reg.remove("nonexistent").is_none());
}

#[test]
fn discover_from_hosts_dir() {
    let hosts = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("hosts");

    if !hosts.is_dir() {
        eprintln!("hosts/ directory not found, skipping discover test");
        return;
    }

    let reg = SidecarRegistry::discover_from_dir(&hosts).expect("discover should succeed");
    let names = reg.list();

    // The hosts/ directory should contain at least `node` and `python`.
    assert!(
        names.contains(&"node"),
        "expected 'node' in discovered sidecars: {names:?}"
    );
    assert!(
        names.contains(&"python"),
        "expected 'python' in discovered sidecars: {names:?}"
    );

    // Each discovered spec should reference the correct interpreter.
    let node_spec = reg.get("node").unwrap();
    assert_eq!(node_spec.command, "node");
    let py_spec = reg.get("python").unwrap();
    assert_eq!(py_spec.command, "python");
}

#[test]
fn discover_nonexistent_dir_is_err() {
    let result = SidecarRegistry::discover_from_dir(Path::new("/no/such/path"));
    assert!(result.is_err());
}
