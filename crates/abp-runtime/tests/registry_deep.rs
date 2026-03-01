// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for [`BackendRegistry`] — registration, lookup, removal, and edge cases.

use abp_core::{AgentEvent, BackendIdentity, CapabilityManifest, Receipt, WorkOrder};
use abp_integrations::{Backend, MockBackend};
use abp_runtime::registry::BackendRegistry;
use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A custom backend that returns a configurable identity, used to distinguish
/// entries in the registry.
#[derive(Clone)]
struct NamedBackend {
    name: String,
}

#[async_trait]
impl Backend for NamedBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("test".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("NamedBackend::run not implemented")
    }
}

// ── 1. Register and lookup ──────────────────────────────────────────

#[test]
fn register_and_lookup_by_name() {
    let mut reg = BackendRegistry::default();
    reg.register(
        "alpha",
        NamedBackend {
            name: "alpha".into(),
        },
    );
    let b = reg.get("alpha").expect("should find alpha");
    assert_eq!(b.identity().id, "alpha");
}

// ── 2. Remove returns the backend ───────────────────────────────────

#[test]
fn remove_returns_backend_and_clears_entry() {
    let mut reg = BackendRegistry::default();
    reg.register("rm-me", MockBackend);
    assert!(reg.contains("rm-me"));

    let removed = reg.remove("rm-me");
    assert!(removed.is_some());
    assert!(!reg.contains("rm-me"));
    assert!(reg.get("rm-me").is_none());

    // The removed backend still works.
    let b = removed.unwrap();
    assert_eq!(b.identity().id, "mock");
}

// ── 3. Remove nonexistent returns None ──────────────────────────────

#[test]
fn remove_nonexistent_returns_none() {
    let mut reg = BackendRegistry::default();
    assert!(reg.remove("ghost").is_none());
}

// ── 4. Duplicate registration replaces ──────────────────────────────

#[test]
fn duplicate_registration_replaces_previous() {
    let mut reg = BackendRegistry::default();
    reg.register(
        "dup",
        NamedBackend {
            name: "first".into(),
        },
    );
    reg.register(
        "dup",
        NamedBackend {
            name: "second".into(),
        },
    );

    assert_eq!(reg.list().len(), 1);
    let b = reg.get("dup").unwrap();
    assert_eq!(b.identity().id, "second");
}

// ── 5. Lookup is case-sensitive ─────────────────────────────────────

#[test]
fn lookup_is_case_sensitive() {
    let mut reg = BackendRegistry::default();
    reg.register("Mock", MockBackend);

    assert!(reg.get("Mock").is_some());
    assert!(reg.get("mock").is_none());
    assert!(reg.get("MOCK").is_none());
    assert!(reg.contains("Mock"));
    assert!(!reg.contains("mock"));
}

// ── 6. List all backends sorted ─────────────────────────────────────

#[test]
fn list_returns_sorted_names() {
    let mut reg = BackendRegistry::default();
    reg.register("charlie", MockBackend);
    reg.register("alpha", MockBackend);
    reg.register("bravo", MockBackend);

    assert_eq!(reg.list(), vec!["alpha", "bravo", "charlie"]);
}

// ── 7. Empty registry operations ────────────────────────────────────

#[test]
fn empty_registry_operations() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
    assert!(reg.get("anything").is_none());
    assert!(!reg.contains("anything"));
}

#[test]
fn empty_registry_get_arc_returns_none() {
    let reg = BackendRegistry::default();
    assert!(reg.get_arc("anything").is_none());
}

// ── 8. Registry with many backends ──────────────────────────────────

#[test]
fn registry_with_many_backends() {
    let mut reg = BackendRegistry::default();
    for i in 0..50 {
        let name = format!("backend-{i:03}");
        reg.register(&name, NamedBackend { name: name.clone() });
    }
    assert_eq!(reg.list().len(), 50);

    // Spot-check lookup
    let b = reg.get("backend-025").unwrap();
    assert_eq!(b.identity().id, "backend-025");

    // Remove one and verify
    reg.remove("backend-000");
    assert_eq!(reg.list().len(), 49);
    assert!(!reg.contains("backend-000"));
}

// ── 9. get_arc returns cloneable handle ─────────────────────────────

#[test]
fn get_arc_returns_cloneable_handle() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);

    let arc1 = reg.get_arc("mock").expect("should find mock");
    let arc2 = reg.get_arc("mock").expect("should find mock again");
    assert_eq!(arc1.identity().id, arc2.identity().id);
}

// ── 10. Run via registry with tokio ─────────────────────────────────

#[tokio::test]
async fn run_backend_via_registry() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);

    let backend = reg.get_arc("mock").expect("mock registered");
    let (tx, mut rx) = mpsc::channel(16);

    let wo = abp_core::WorkOrder {
        id: Uuid::new_v4(),
        task: "registry run test".into(),
        lane: abp_core::ExecutionLane::PatchFirst,
        workspace: abp_core::WorkspaceSpec {
            root: ".".into(),
            mode: abp_core::WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: abp_core::PolicyProfile::default(),
        requirements: abp_core::CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    };

    let run_id = Uuid::new_v4();
    let receipt = backend
        .run(run_id, wo, tx)
        .await
        .expect("run should succeed");

    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    assert!(receipt.receipt_sha256.is_some());

    // Drain any remaining events from the channel.
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    // We got a valid receipt; event count is backend-dependent.
    let _ = events;
}

// ── 11. Register after remove re-adds ───────────────────────────────

#[test]
fn register_after_remove() {
    let mut reg = BackendRegistry::default();
    reg.register("flip", NamedBackend { name: "v1".into() });
    reg.remove("flip");
    assert!(!reg.contains("flip"));

    reg.register("flip", NamedBackend { name: "v2".into() });
    assert!(reg.contains("flip"));
    assert_eq!(reg.get("flip").unwrap().identity().id, "v2");
}
