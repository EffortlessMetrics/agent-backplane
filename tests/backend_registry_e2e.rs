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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! End-to-end tests for the backend registry and trait system.

use std::sync::Arc;

use abp_backend_core::{
    Backend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, SupportLevel,
    WorkOrder, WorkOrderBuilder,
};
use abp_runtime::registry::BackendRegistry;
use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Custom test backends
// ---------------------------------------------------------------------------

/// A configurable backend for testing different capability combinations.
#[derive(Debug, Clone)]
struct CustomBackend {
    name: String,
    caps: CapabilityManifest,
}

impl CustomBackend {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            caps: CapabilityManifest::default(),
        }
    }

    fn with_cap(mut self, cap: Capability, level: SupportLevel) -> Self {
        self.caps.insert(cap, level);
        self
    }
}

#[async_trait]
impl Backend for CustomBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("test".to_string()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        self.caps.clone()
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<abp_core::Receipt> {
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: format!("{} done", self.name),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;
        let receipt = abp_core::ReceiptBuilder::new(&self.name)
            .outcome(abp_core::Outcome::Complete)
            .build()
            .with_hash()?;
        Ok(receipt)
    }
}

/// Helper to create a simple work order.
fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

/// Helper to create a work order with capability requirements.
fn work_order_with_requirements(reqs: Vec<CapabilityRequirement>) -> WorkOrder {
    WorkOrderBuilder::new("test task")
        .requirements(CapabilityRequirements { required: reqs })
        .build()
}

// ===========================================================================
// 1. Backend trait implementation (using MockBackend)
// ===========================================================================

mod backend_trait {
    use super::*;

    #[test]
    fn mock_identity_id() {
        let b = MockBackend;
        assert_eq!(b.identity().id, "mock");
    }

    #[test]
    fn mock_identity_backend_version() {
        let b = MockBackend;
        assert_eq!(b.identity().backend_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn mock_identity_adapter_version() {
        let b = MockBackend;
        assert_eq!(b.identity().adapter_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn mock_has_streaming_native() {
        let caps = MockBackend.capabilities();
        assert!(matches!(
            caps.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn mock_has_tool_read_emulated() {
        let caps = MockBackend.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolRead),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn mock_has_tool_write_emulated() {
        let caps = MockBackend.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolWrite),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn mock_has_tool_edit_emulated() {
        let caps = MockBackend.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolEdit),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn mock_has_tool_bash_emulated() {
        let caps = MockBackend.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolBash),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn mock_has_structured_output_emulated() {
        let caps = MockBackend.capabilities();
        assert!(matches!(
            caps.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn mock_lacks_mcp_client() {
        let caps = MockBackend.capabilities();
        assert!(!caps.contains_key(&Capability::McpClient));
    }

    #[test]
    fn mock_capability_count() {
        let caps = MockBackend.capabilities();
        assert_eq!(caps.len(), 6);
    }

    #[tokio::test]
    async fn mock_run_produces_receipt() {
        let (tx, _rx) = mpsc::channel(16);
        let wo = simple_work_order("hello");
        let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.backend.id, "mock");
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn mock_run_streams_events() {
        let (tx, mut rx) = mpsc::channel(64);
        let wo = simple_work_order("test");
        let _receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();

        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count >= 3, "expected at least 3 events, got {count}");
    }

    #[tokio::test]
    async fn mock_run_first_event_is_run_started() {
        let (tx, mut rx) = mpsc::channel(64);
        let wo = simple_work_order("test");
        let _receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();

        let first = rx.try_recv().unwrap();
        assert!(
            matches!(first.kind, AgentEventKind::RunStarted { .. }),
            "expected RunStarted, got {:?}",
            first.kind
        );
    }

    #[tokio::test]
    async fn mock_run_last_event_is_run_completed() {
        let (tx, mut rx) = mpsc::channel(64);
        let wo = simple_work_order("test");
        let _receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();

        let mut last = None;
        while let Ok(ev) = rx.try_recv() {
            last = Some(ev);
        }
        let last = last.unwrap();
        assert!(
            matches!(last.kind, AgentEventKind::RunCompleted { .. }),
            "expected RunCompleted, got {:?}",
            last.kind
        );
    }

    #[tokio::test]
    async fn mock_receipt_outcome_is_complete() {
        let (tx, _rx) = mpsc::channel(16);
        let wo = simple_work_order("test");
        let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.outcome, abp_core::Outcome::Complete);
    }

    #[tokio::test]
    async fn mock_receipt_has_trace() {
        let (tx, _rx) = mpsc::channel(16);
        let wo = simple_work_order("test");
        let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert!(!receipt.trace.is_empty());
    }

    #[tokio::test]
    async fn mock_receipt_mode_default_is_mapped() {
        let (tx, _rx) = mpsc::channel(16);
        let wo = simple_work_order("test");
        let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[tokio::test]
    async fn custom_backend_run_returns_receipt() {
        let b = CustomBackend::new("alpha");
        let (tx, _rx) = mpsc::channel(16);
        let wo = simple_work_order("test");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.backend.id, "alpha");
    }

    #[test]
    fn custom_backend_identity() {
        let b = CustomBackend::new("beta");
        let id = b.identity();
        assert_eq!(id.id, "beta");
        assert_eq!(id.backend_version.as_deref(), Some("test"));
        assert!(id.adapter_version.is_none());
    }

    #[test]
    fn custom_backend_empty_capabilities() {
        let b = CustomBackend::new("empty");
        assert!(b.capabilities().is_empty());
    }

    #[test]
    fn custom_backend_with_capabilities() {
        let b = CustomBackend::new("rich")
            .with_cap(Capability::Streaming, SupportLevel::Native)
            .with_cap(Capability::ToolRead, SupportLevel::Emulated);
        let caps = b.capabilities();
        assert_eq!(caps.len(), 2);
        assert!(matches!(
            caps.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }
}

// ===========================================================================
// 2. Backend registry: register, lookup, list, remove
// ===========================================================================

mod registry_operations {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = BackendRegistry::default();
        assert!(reg.list().is_empty());
    }

    #[test]
    fn register_single_backend() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        assert!(reg.contains("mock"));
    }

    #[test]
    fn get_registered_backend() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let b = reg.get("mock").unwrap();
        assert_eq!(b.identity().id, "mock");
    }

    #[test]
    fn get_arc_registered_backend() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();
        assert_eq!(arc.identity().id, "mock");
    }

    #[test]
    fn get_arc_is_cloneable() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc1 = reg.get_arc("mock").unwrap();
        let arc2 = arc1.clone();
        assert_eq!(arc1.identity().id, arc2.identity().id);
    }

    #[test]
    fn list_returns_sorted_names() {
        let mut reg = BackendRegistry::default();
        reg.register("zebra", MockBackend);
        reg.register("alpha", MockBackend);
        reg.register("middle", MockBackend);
        let names = reg.list();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn contains_returns_true_for_registered() {
        let mut reg = BackendRegistry::default();
        reg.register("present", MockBackend);
        assert!(reg.contains("present"));
    }

    #[test]
    fn contains_returns_false_for_missing() {
        let reg = BackendRegistry::default();
        assert!(!reg.contains("nonexistent"));
    }

    #[test]
    fn remove_returns_backend() {
        let mut reg = BackendRegistry::default();
        reg.register("removable", MockBackend);
        let removed = reg.remove("removable");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().identity().id, "mock");
    }

    #[test]
    fn remove_deletes_from_registry() {
        let mut reg = BackendRegistry::default();
        reg.register("removable", MockBackend);
        reg.remove("removable");
        assert!(!reg.contains("removable"));
        assert!(reg.list().is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut reg = BackendRegistry::default();
        assert!(reg.remove("ghost").is_none());
    }

    #[test]
    fn register_multiple_backends() {
        let mut reg = BackendRegistry::default();
        reg.register("a", MockBackend);
        reg.register("b", CustomBackend::new("b"));
        reg.register("c", CustomBackend::new("c"));
        assert_eq!(reg.list().len(), 3);
    }

    #[test]
    fn get_returns_correct_backend_among_many() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        reg.register("custom", CustomBackend::new("custom"));
        let b = reg.get("custom").unwrap();
        assert_eq!(b.identity().id, "custom");
    }

    #[test]
    fn remove_preserves_other_backends() {
        let mut reg = BackendRegistry::default();
        reg.register("keep", MockBackend);
        reg.register("drop", CustomBackend::new("drop"));
        reg.remove("drop");
        assert!(reg.contains("keep"));
        assert!(!reg.contains("drop"));
        assert_eq!(reg.list().len(), 1);
    }

    #[tokio::test]
    async fn removed_backend_still_works() {
        let mut reg = BackendRegistry::default();
        reg.register("temp", MockBackend);
        let removed = reg.remove("temp").unwrap();
        let (tx, _rx) = mpsc::channel(16);
        let receipt = removed
            .run(Uuid::new_v4(), simple_work_order("test"), tx)
            .await
            .unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }

    #[tokio::test]
    async fn get_arc_backend_runs_successfully() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();
        let (tx, _rx) = mpsc::channel(16);
        let receipt = arc
            .run(Uuid::new_v4(), simple_work_order("arc test"), tx)
            .await
            .unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }
}

// ===========================================================================
// 3. Capability enforcement in backend selection
// ===========================================================================

mod capability_enforcement {
    use super::*;

    #[test]
    fn no_requirements_passes() {
        let reqs = CapabilityRequirements::default();
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn satisfied_native_requirement() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn emulated_satisfies_emulated_requirement() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn native_satisfies_emulated_requirement() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        };
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn emulated_does_not_satisfy_native_requirement() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        };
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[test]
    fn missing_capability_fails() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Emulated,
            }],
        };
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[test]
    fn multiple_satisfied_requirements() {
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
                CapabilityRequirement {
                    capability: Capability::ToolBash,
                    min_support: MinSupport::Emulated,
                },
            ],
        };
        let caps = MockBackend.capabilities();
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn one_of_many_requirements_fails() {
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::McpServer,
                    min_support: MinSupport::Emulated,
                },
            ],
        };
        let caps = MockBackend.capabilities();
        let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsatisfied"), "error: {msg}");
    }

    #[test]
    fn error_message_contains_capability_name() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ImageInput,
                min_support: MinSupport::Emulated,
            }],
        };
        let caps = MockBackend.capabilities();
        let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ImageInput"), "error: {msg}");
    }

    #[test]
    fn restricted_satisfies_emulated_requirement() {
        let mut caps = CapabilityManifest::default();
        caps.insert(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        );
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn restricted_does_not_satisfy_native_requirement() {
        let mut caps = CapabilityManifest::default();
        caps.insert(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        );
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[test]
    fn unsupported_does_not_satisfy_emulated() {
        let mut caps = CapabilityManifest::default();
        caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[test]
    fn empty_manifest_fails_any_requirement() {
        let caps = CapabilityManifest::default();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[tokio::test]
    async fn mock_run_fails_with_unsatisfied_requirements() {
        let (tx, _rx) = mpsc::channel(16);
        let wo = work_order_with_requirements(vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }]);
        let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_err());
    }
}

// ===========================================================================
// 4. Backend metadata queries
// ===========================================================================

mod metadata_queries {
    use super::*;

    #[test]
    fn identity_fields_are_accessible() {
        let b = CustomBackend::new("test-backend");
        let id = b.identity();
        assert!(!id.id.is_empty());
    }

    #[test]
    fn capabilities_are_btreemap() {
        let caps = MockBackend.capabilities();
        // BTreeMap iteration is sorted by key
        let keys: Vec<_> = caps.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "CapabilityManifest should iterate in order");
    }

    #[test]
    fn registry_backend_metadata_via_get() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let b = reg.get("mock").unwrap();
        let id = b.identity();
        assert_eq!(id.id, "mock");
        assert!(id.backend_version.is_some());
    }

    #[test]
    fn registry_backend_capabilities_via_get() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let b = reg.get("mock").unwrap();
        let caps = b.capabilities();
        assert!(caps.contains_key(&Capability::Streaming));
    }

    #[test]
    fn arc_backend_metadata_accessible() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();
        assert_eq!(arc.identity().id, "mock");
        assert!(!arc.capabilities().is_empty());
    }

    #[test]
    fn custom_backend_metadata_in_registry() {
        let mut reg = BackendRegistry::default();
        let b =
            CustomBackend::new("my-backend").with_cap(Capability::Streaming, SupportLevel::Native);
        reg.register("my-backend", b);
        let retrieved = reg.get("my-backend").unwrap();
        assert_eq!(retrieved.identity().id, "my-backend");
        assert!(matches!(
            retrieved.capabilities().get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn multiple_backends_have_distinct_identities() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        reg.register("custom", CustomBackend::new("custom"));
        let mock_id = reg.get("mock").unwrap().identity().id;
        let custom_id = reg.get("custom").unwrap().identity().id;
        assert_ne!(mock_id, custom_id);
    }

    #[test]
    fn backend_version_and_adapter_version_optionality() {
        let b = CustomBackend::new("no-adapter");
        let id = b.identity();
        assert!(id.backend_version.is_some());
        assert!(id.adapter_version.is_none());
    }
}

// ===========================================================================
// 5. Error handling: unknown backend, duplicate registration
// ===========================================================================

mod error_handling {
    use super::*;

    #[test]
    fn get_unknown_backend_returns_none() {
        let reg = BackendRegistry::default();
        assert!(reg.get("unknown").is_none());
    }

    #[test]
    fn get_arc_unknown_backend_returns_none() {
        let reg = BackendRegistry::default();
        assert!(reg.get_arc("unknown").is_none());
    }

    #[test]
    fn duplicate_registration_replaces_previous() {
        let mut reg = BackendRegistry::default();
        reg.register("dup", CustomBackend::new("first"));
        reg.register("dup", CustomBackend::new("second"));
        let b = reg.get("dup").unwrap();
        assert_eq!(b.identity().id, "second");
    }

    #[test]
    fn duplicate_registration_does_not_increase_count() {
        let mut reg = BackendRegistry::default();
        reg.register("dup", MockBackend);
        reg.register("dup", MockBackend);
        assert_eq!(reg.list().len(), 1);
    }

    #[test]
    fn remove_unknown_returns_none() {
        let mut reg = BackendRegistry::default();
        assert!(reg.remove("nonexistent").is_none());
    }

    #[test]
    fn contains_after_remove() {
        let mut reg = BackendRegistry::default();
        reg.register("temp", MockBackend);
        reg.remove("temp");
        assert!(!reg.contains("temp"));
    }

    #[test]
    fn get_after_remove_returns_none() {
        let mut reg = BackendRegistry::default();
        reg.register("temp", MockBackend);
        reg.remove("temp");
        assert!(reg.get("temp").is_none());
    }

    #[test]
    fn case_sensitive_lookup() {
        let mut reg = BackendRegistry::default();
        reg.register("Mock", MockBackend);
        assert!(reg.get("Mock").is_some());
        assert!(reg.get("mock").is_none());
        assert!(reg.get("MOCK").is_none());
    }

    #[test]
    fn empty_string_name_is_valid() {
        let mut reg = BackendRegistry::default();
        reg.register("", MockBackend);
        assert!(reg.contains(""));
        assert_eq!(reg.get("").unwrap().identity().id, "mock");
    }

    #[test]
    fn whitespace_name_is_valid() {
        let mut reg = BackendRegistry::default();
        reg.register("  spaces  ", MockBackend);
        assert!(reg.contains("  spaces  "));
        assert!(reg.get("spaces").is_none());
    }

    #[test]
    fn special_chars_in_name() {
        let mut reg = BackendRegistry::default();
        reg.register("sidecar:node", MockBackend);
        reg.register("a/b/c", MockBackend);
        reg.register("back-end.v2", MockBackend);
        assert!(reg.contains("sidecar:node"));
        assert!(reg.contains("a/b/c"));
        assert!(reg.contains("back-end.v2"));
    }

    #[test]
    fn unicode_name() {
        let mut reg = BackendRegistry::default();
        reg.register("バックエンド", MockBackend);
        assert!(reg.contains("バックエンド"));
    }
}

// ===========================================================================
// 6. Thread safety of registry
// ===========================================================================

mod thread_safety {
    use super::*;

    #[test]
    fn backend_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<MockBackend>();
        assert_send::<CustomBackend>();
    }

    #[test]
    fn backend_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<MockBackend>();
        assert_sync::<CustomBackend>();
    }

    #[test]
    fn arc_backend_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<dyn Backend>>();
    }

    #[tokio::test]
    async fn arc_backend_across_tasks() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();
        let arc2 = arc.clone();

        let h1 = tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(16);
            arc.run(Uuid::new_v4(), simple_work_order("task1"), tx)
                .await
                .unwrap()
        });
        let h2 = tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(16);
            arc2.run(Uuid::new_v4(), simple_work_order("task2"), tx)
                .await
                .unwrap()
        });

        let (r1, r2) = tokio::join!(h1, h2);
        assert_eq!(r1.unwrap().backend.id, "mock");
        assert_eq!(r2.unwrap().backend.id, "mock");
    }

    #[tokio::test]
    async fn concurrent_arc_reads() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();

        let mut handles = Vec::new();
        for i in 0..10 {
            let a = arc.clone();
            handles.push(tokio::spawn(async move {
                let id = a.identity();
                assert_eq!(id.id, "mock");
                let caps = a.capabilities();
                assert!(caps.contains_key(&Capability::Streaming));
                i
            }));
        }

        for h in handles {
            h.await.unwrap();
        }
    }

    #[tokio::test]
    async fn concurrent_runs_produce_distinct_receipts() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();

        let mut handles = Vec::new();
        for _ in 0..5 {
            let a = arc.clone();
            handles.push(tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                a.run(Uuid::new_v4(), simple_work_order("concurrent"), tx)
                    .await
                    .unwrap()
            }));
        }

        let mut hashes = std::collections::HashSet::new();
        for h in handles {
            let receipt = h.await.unwrap();
            // Each receipt should have a hash
            assert!(receipt.receipt_sha256.is_some());
            hashes.insert(receipt.receipt_sha256.unwrap());
        }
        // Different runs should produce different hashes (different timestamps)
        assert!(
            hashes.len() > 1,
            "expected diverse hashes from concurrent runs"
        );
    }
}

// ===========================================================================
// 7. Edge cases: empty registry, many backends
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_registry_list() {
        let reg = BackendRegistry::default();
        assert!(reg.list().is_empty());
    }

    #[test]
    fn empty_registry_contains_false() {
        let reg = BackendRegistry::default();
        assert!(!reg.contains("anything"));
    }

    #[test]
    fn empty_registry_get_none() {
        let reg = BackendRegistry::default();
        assert!(reg.get("anything").is_none());
    }

    #[test]
    fn empty_registry_get_arc_none() {
        let reg = BackendRegistry::default();
        assert!(reg.get_arc("anything").is_none());
    }

    #[test]
    fn empty_registry_remove_none() {
        let mut reg = BackendRegistry::default();
        assert!(reg.remove("anything").is_none());
    }

    #[test]
    fn register_50_backends() {
        let mut reg = BackendRegistry::default();
        for i in 0..50 {
            reg.register(
                format!("backend-{i:03}"),
                CustomBackend::new(&format!("b{i}")),
            );
        }
        assert_eq!(reg.list().len(), 50);
    }

    #[test]
    fn list_50_backends_is_sorted() {
        let mut reg = BackendRegistry::default();
        for i in (0..50).rev() {
            reg.register(format!("backend-{i:03}"), MockBackend);
        }
        let names = reg.list();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn lookup_among_50_backends() {
        let mut reg = BackendRegistry::default();
        for i in 0..50 {
            reg.register(
                format!("backend-{i:03}"),
                CustomBackend::new(&format!("b{i}")),
            );
        }
        let b = reg.get("backend-025").unwrap();
        assert_eq!(b.identity().id, "b25");
    }

    #[test]
    fn remove_all_backends() {
        let mut reg = BackendRegistry::default();
        for i in 0..10 {
            reg.register(format!("b{i}"), MockBackend);
        }
        for i in 0..10 {
            reg.remove(&format!("b{i}"));
        }
        assert!(reg.list().is_empty());
    }

    #[test]
    fn re_register_after_remove() {
        let mut reg = BackendRegistry::default();
        reg.register("recycled", CustomBackend::new("v1"));
        reg.remove("recycled");
        reg.register("recycled", CustomBackend::new("v2"));
        let b = reg.get("recycled").unwrap();
        assert_eq!(b.identity().id, "v2");
    }

    #[test]
    fn register_same_backend_type_multiple_names() {
        let mut reg = BackendRegistry::default();
        reg.register("mock1", MockBackend);
        reg.register("mock2", MockBackend);
        reg.register("mock3", MockBackend);
        assert_eq!(reg.list().len(), 3);
        // All point to same type but are distinct entries
        assert_eq!(reg.get("mock1").unwrap().identity().id, "mock");
        assert_eq!(reg.get("mock2").unwrap().identity().id, "mock");
    }
}

// ===========================================================================
// 8. Execution mode extraction
// ===========================================================================

mod execution_mode {
    use super::*;

    #[test]
    fn default_mode_is_mapped() {
        let wo = simple_work_order("test");
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
    }

    #[test]
    fn passthrough_from_nested_vendor_config() {
        let mut wo = simple_work_order("test");
        wo.config.vendor.insert(
            "abp".to_string(),
            serde_json::json!({"mode": "passthrough"}),
        );
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
    }

    #[test]
    fn mapped_from_nested_vendor_config() {
        let mut wo = simple_work_order("test");
        wo.config
            .vendor
            .insert("abp".to_string(), serde_json::json!({"mode": "mapped"}));
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
    }

    #[test]
    fn passthrough_from_flat_vendor_config() {
        let mut wo = simple_work_order("test");
        wo.config
            .vendor
            .insert("abp.mode".to_string(), serde_json::json!("passthrough"));
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
    }

    #[test]
    fn invalid_mode_falls_back_to_default() {
        let mut wo = simple_work_order("test");
        wo.config.vendor.insert(
            "abp".to_string(),
            serde_json::json!({"mode": "invalid_xyz"}),
        );
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
    }

    #[test]
    fn no_vendor_config_defaults_to_mapped() {
        let wo = simple_work_order("test");
        assert!(wo.config.vendor.is_empty());
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
    }

    #[test]
    fn validate_passthrough_compatibility_succeeds() {
        let wo = simple_work_order("test");
        assert!(validate_passthrough_compatibility(&wo).is_ok());
    }
}

// ===========================================================================
// 9. Support-level satisfaction matrix
// ===========================================================================

mod support_level_matrix {
    use super::*;

    #[test]
    fn native_satisfies_native() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_satisfies_emulated() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_does_not_satisfy_native() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn unsupported_does_not_satisfy_emulated() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn unsupported_does_not_satisfy_native() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn restricted_satisfies_emulated() {
        let r = SupportLevel::Restricted {
            reason: "test".into(),
        };
        assert!(r.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_does_not_satisfy_native() {
        let r = SupportLevel::Restricted {
            reason: "test".into(),
        };
        assert!(!r.satisfies(&MinSupport::Native));
    }
}

// ===========================================================================
// 10. Integration: registry + capability checking
// ===========================================================================

mod integration {
    use super::*;

    #[test]
    fn registry_backend_satisfies_requirements() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);

        let b = reg.get("mock").unwrap();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &b.capabilities()).is_ok());
    }

    #[test]
    fn registry_backend_fails_unsatisfied_requirements() {
        let mut reg = BackendRegistry::default();
        reg.register("limited", CustomBackend::new("limited"));

        let b = reg.get("limited").unwrap();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &b.capabilities()).is_err());
    }

    #[test]
    fn find_capable_backend_in_registry() {
        let mut reg = BackendRegistry::default();
        reg.register("limited", CustomBackend::new("limited"));
        reg.register("mock", MockBackend);
        reg.register(
            "streamer",
            CustomBackend::new("streamer").with_cap(Capability::Streaming, SupportLevel::Native),
        );

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };

        let capable: Vec<&str> = reg
            .list()
            .into_iter()
            .filter(|name| {
                let b = reg.get(name).unwrap();
                ensure_capability_requirements(&reqs, &b.capabilities()).is_ok()
            })
            .collect();

        assert!(capable.contains(&"mock"));
        assert!(capable.contains(&"streamer"));
        assert!(!capable.contains(&"limited"));
    }

    #[tokio::test]
    async fn full_lifecycle_register_check_run() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);

        // Check capabilities
        let b = reg.get("mock").unwrap();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &b.capabilities()).is_ok());

        // Run via arc handle
        let arc = reg.get_arc("mock").unwrap();
        let (tx, mut rx) = mpsc::channel(64);
        let wo = simple_work_order("lifecycle test");
        let receipt = arc.run(Uuid::new_v4(), wo, tx).await.unwrap();

        assert_eq!(receipt.backend.id, "mock");
        assert_eq!(receipt.outcome, abp_core::Outcome::Complete);
        assert!(receipt.receipt_sha256.is_some());

        // Verify events were streamed
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn removed_backend_arc_still_alive() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock").unwrap();

        // Remove from registry
        reg.remove("mock");
        assert!(!reg.contains("mock"));

        // Arc still works
        let (tx, _rx) = mpsc::channel(16);
        let receipt = arc
            .run(Uuid::new_v4(), simple_work_order("outlived"), tx)
            .await
            .unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }

    #[test]
    fn all_registered_backends_have_identities() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        reg.register("alpha", CustomBackend::new("alpha"));
        reg.register("beta", CustomBackend::new("beta"));

        for name in reg.list() {
            let b = reg.get(name).unwrap();
            let id = b.identity();
            assert!(!id.id.is_empty(), "backend {name} has empty identity id");
        }
    }
}
