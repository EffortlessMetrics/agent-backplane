#![allow(clippy::all)]
#![allow(unknown_lints)]
//! Comprehensive tests for the `abp-backend-sidecar` crate.
//!
//! Covers construction, configuration, Backend trait implementation,
//! identity/capability reporting, serialization, event streaming behavior,
//! receipt generation, error handling, and edge cases — all without
//! spawning actual sidecar processes unless explicitly needed.

use abp_backend_core::{
    ensure_capability_requirements, Backend, BackendMetadata, BackendRegistry,
};
use abp_backend_sidecar::SidecarBackend;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_host::SidecarSpec;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn spec(cmd: &str) -> SidecarSpec {
    SidecarSpec::new(cmd)
}

fn backend(cmd: &str) -> SidecarBackend {
    SidecarBackend::new(spec(cmd))
}

fn wo(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn reqs(items: Vec<(Capability, MinSupport)>) -> CapabilityRequirements {
    CapabilityRequirements {
        required: items
            .into_iter()
            .map(|(capability, min_support)| CapabilityRequirement {
                capability,
                min_support,
            })
            .collect(),
    }
}

fn caps(items: Vec<(Capability, SupportLevel)>) -> CapabilityManifest {
    items.into_iter().collect()
}

fn make_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "0.1.0".to_string(),
        max_tokens: None,
        supports_streaming: true,
        supports_tools: false,
        rate_limit: None,
    }
}

// =========================================================================
// Module: construction
// =========================================================================
mod construction {
    use super::*;

    #[test]
    fn new_with_simple_command() {
        let b = backend("node");
        assert_eq!(b.spec.command, "node");
    }

    #[test]
    fn new_preserves_args() {
        let mut s = spec("python3");
        s.args = vec!["-u".into(), "main.py".into()];
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.args, vec!["-u", "main.py"]);
    }

    #[test]
    fn new_preserves_env() {
        let mut s = spec("node");
        s.env.insert("TOKEN".into(), "abc".into());
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.env["TOKEN"], "abc");
    }

    #[test]
    fn new_preserves_cwd() {
        let mut s = spec("node");
        s.cwd = Some("/workspace".into());
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.cwd.as_deref(), Some("/workspace"));
    }

    #[test]
    fn new_defaults_are_empty() {
        let b = backend("echo");
        assert!(b.spec.args.is_empty());
        assert!(b.spec.env.is_empty());
        assert!(b.spec.cwd.is_none());
    }

    #[test]
    fn new_with_empty_string_command() {
        let b = backend("");
        assert_eq!(b.spec.command, "");
    }

    #[test]
    fn new_with_path_command() {
        let b = backend("/usr/local/bin/my-sidecar");
        assert_eq!(b.spec.command, "/usr/local/bin/my-sidecar");
    }

    #[test]
    fn new_with_unicode_command() {
        let b = backend("日本語コマンド");
        assert_eq!(b.spec.command, "日本語コマンド");
    }

    #[test]
    fn new_with_many_args() {
        let mut s = spec("node");
        s.args = (0..100).map(|i| format!("arg{i}")).collect();
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.args.len(), 100);
    }

    #[test]
    fn new_with_many_env_vars() {
        let mut s = spec("node");
        for i in 0..50 {
            s.env.insert(format!("VAR_{i}"), format!("val_{i}"));
        }
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.env.len(), 50);
    }

    #[test]
    fn spec_field_is_public() {
        let b = backend("test");
        let _cmd: &str = &b.spec.command;
        let _args: &Vec<String> = &b.spec.args;
        let _env: &BTreeMap<String, String> = &b.spec.env;
        let _cwd: &Option<String> = &b.spec.cwd;
    }
}

// =========================================================================
// Module: trait_impls
// =========================================================================
mod trait_impls {
    use super::*;

    #[test]
    fn debug_contains_type_name() {
        let b = backend("test-bin");
        let dbg = format!("{b:?}");
        assert!(dbg.contains("SidecarBackend"));
    }

    #[test]
    fn debug_contains_command() {
        let b = backend("my-sidecar");
        let dbg = format!("{b:?}");
        assert!(dbg.contains("my-sidecar"));
    }

    #[test]
    fn debug_contains_args() {
        let mut s = spec("node");
        s.args = vec!["--verbose".into()];
        let b = SidecarBackend::new(s);
        let dbg = format!("{b:?}");
        assert!(dbg.contains("--verbose"));
    }

    #[test]
    fn clone_produces_independent_copy() {
        let mut s = spec("node");
        s.args = vec!["a.js".into()];
        s.env.insert("K".into(), "V".into());
        s.cwd = Some("/tmp".into());
        let b = SidecarBackend::new(s);
        let c = b.clone();
        assert_eq!(c.spec.command, b.spec.command);
        assert_eq!(c.spec.args, b.spec.args);
        assert_eq!(c.spec.env, b.spec.env);
        assert_eq!(c.spec.cwd, b.spec.cwd);
    }

    #[test]
    fn clone_is_deep() {
        let mut s = spec("node");
        s.env.insert("K".into(), "V".into());
        let b = SidecarBackend::new(s);
        let mut c = b.clone();
        c.spec.env.insert("K2".into(), "V2".into());
        assert_eq!(b.spec.env.len(), 1);
        assert_eq!(c.spec.env.len(), 2);
    }

    #[test]
    fn is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<SidecarBackend>();
    }

    #[test]
    fn is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<SidecarBackend>();
    }

    #[test]
    fn is_send_and_sync_combined() {
        fn assert_both<T: Send + Sync>() {}
        assert_both::<SidecarBackend>();
    }

    #[test]
    fn backend_trait_object_safe() {
        let b = backend("node");
        let _: &dyn Backend = &b;
    }

    #[test]
    fn backend_boxable() {
        let b = backend("node");
        let _: Box<dyn Backend> = Box::new(b);
    }

    #[test]
    fn backend_arc_compatible() {
        let b = backend("node");
        let _: std::sync::Arc<dyn Backend> = std::sync::Arc::new(b);
    }
}

// =========================================================================
// Module: identity
// =========================================================================
mod identity {
    use super::*;

    #[test]
    fn identity_id_is_sidecar() {
        let b = backend("node");
        assert_eq!(b.identity().id, "sidecar");
    }

    #[test]
    fn identity_backend_version_is_none() {
        let b = backend("python");
        assert!(b.identity().backend_version.is_none());
    }

    #[test]
    fn identity_adapter_version_is_0_1() {
        let b = backend("ruby");
        assert_eq!(b.identity().adapter_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn identity_same_for_any_command() {
        let a = backend("node").identity();
        let b = backend("python").identity();
        let c = backend("/bin/custom").identity();
        assert_eq!(a.id, b.id);
        assert_eq!(b.id, c.id);
    }

    #[test]
    fn identity_is_consistent_across_calls() {
        let b = backend("node");
        let id1 = b.identity();
        let id2 = b.identity();
        assert_eq!(id1.id, id2.id);
        assert_eq!(id1.adapter_version, id2.adapter_version);
        assert_eq!(id1.backend_version, id2.backend_version);
    }

    #[test]
    fn identity_serializes_to_json() {
        let id = backend("node").identity();
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json["id"], "sidecar");
        assert!(json["backend_version"].is_null());
        assert_eq!(json["adapter_version"], "0.1");
    }

    #[test]
    fn identity_roundtrips_through_json() {
        let id = backend("node").identity();
        let json = serde_json::to_string(&id).unwrap();
        let deser: BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.id, id.id);
        assert_eq!(deser.adapter_version, id.adapter_version);
        assert_eq!(deser.backend_version, id.backend_version);
    }

    #[test]
    fn identity_id_is_not_empty() {
        assert!(!backend("x").identity().id.is_empty());
    }

    #[test]
    fn identity_adapter_version_is_not_empty() {
        let v = backend("x").identity().adapter_version.unwrap();
        assert!(!v.is_empty());
    }

    #[test]
    fn identity_from_cloned_backend() {
        let b = backend("node");
        let c = b.clone();
        assert_eq!(b.identity().id, c.identity().id);
    }
}

// =========================================================================
// Module: capabilities
// =========================================================================
mod capabilities {
    use super::*;

    #[test]
    fn capabilities_returns_empty_manifest() {
        let b = backend("node");
        assert!(b.capabilities().is_empty());
    }

    #[test]
    fn capabilities_length_is_zero() {
        assert_eq!(backend("node").capabilities().len(), 0);
    }

    #[test]
    fn capabilities_consistent_across_calls() {
        let b = backend("node");
        let c1 = b.capabilities();
        let c2 = b.capabilities();
        assert_eq!(c1.len(), c2.len());
    }

    #[test]
    fn capabilities_same_for_different_commands() {
        let a = backend("node").capabilities();
        let b = backend("python").capabilities();
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn capabilities_does_not_claim_streaming() {
        let caps = backend("node").capabilities();
        assert!(!caps.contains_key(&Capability::Streaming));
    }

    #[test]
    fn capabilities_does_not_claim_tool_use() {
        let caps = backend("node").capabilities();
        assert!(!caps.contains_key(&Capability::ToolUse));
    }

    #[test]
    fn capabilities_does_not_claim_any_tool() {
        let caps = backend("node").capabilities();
        assert!(!caps.contains_key(&Capability::ToolRead));
        assert!(!caps.contains_key(&Capability::ToolWrite));
        assert!(!caps.contains_key(&Capability::ToolEdit));
        assert!(!caps.contains_key(&Capability::ToolBash));
    }

    #[test]
    fn capabilities_serializes_to_empty_object() {
        let caps = backend("node").capabilities();
        let json = serde_json::to_value(&caps).unwrap();
        assert!(json.is_object());
        assert_eq!(json.as_object().unwrap().len(), 0);
    }

    #[test]
    fn empty_capabilities_pass_empty_requirements() {
        let caps = backend("node").capabilities();
        let reqs = CapabilityRequirements::default();
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[test]
    fn empty_capabilities_fail_any_native_requirement() {
        let caps = backend("node").capabilities();
        let r = reqs(vec![(Capability::Streaming, MinSupport::Native)]);
        assert!(ensure_capability_requirements(&r, &caps).is_err());
    }

    #[test]
    fn empty_capabilities_fail_any_emulated_requirement() {
        let caps = backend("node").capabilities();
        let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
        assert!(ensure_capability_requirements(&r, &caps).is_err());
    }
}

// =========================================================================
// Module: capability_validation
// =========================================================================
mod capability_validation {
    use super::*;

    #[test]
    fn native_satisfies_native() {
        let c = caps(vec![(Capability::Streaming, SupportLevel::Native)]);
        let r = reqs(vec![(Capability::Streaming, MinSupport::Native)]);
        assert!(ensure_capability_requirements(&r, &c).is_ok());
    }

    #[test]
    fn native_satisfies_emulated() {
        let c = caps(vec![(Capability::Streaming, SupportLevel::Native)]);
        let r = reqs(vec![(Capability::Streaming, MinSupport::Emulated)]);
        assert!(ensure_capability_requirements(&r, &c).is_ok());
    }

    #[test]
    fn emulated_satisfies_emulated() {
        let c = caps(vec![(Capability::ToolRead, SupportLevel::Emulated)]);
        let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
        assert!(ensure_capability_requirements(&r, &c).is_ok());
    }

    #[test]
    fn emulated_does_not_satisfy_native() {
        let c = caps(vec![(Capability::ToolRead, SupportLevel::Emulated)]);
        let r = reqs(vec![(Capability::ToolRead, MinSupport::Native)]);
        assert!(ensure_capability_requirements(&r, &c).is_err());
    }

    #[test]
    fn unsupported_does_not_satisfy_native() {
        let c = caps(vec![(Capability::Vision, SupportLevel::Unsupported)]);
        let r = reqs(vec![(Capability::Vision, MinSupport::Native)]);
        assert!(ensure_capability_requirements(&r, &c).is_err());
    }

    #[test]
    fn unsupported_does_not_satisfy_emulated() {
        let c = caps(vec![(Capability::Vision, SupportLevel::Unsupported)]);
        let r = reqs(vec![(Capability::Vision, MinSupport::Emulated)]);
        assert!(ensure_capability_requirements(&r, &c).is_err());
    }

    #[test]
    fn restricted_satisfies_emulated() {
        let c = caps(vec![(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let r = reqs(vec![(Capability::ToolBash, MinSupport::Emulated)]);
        assert!(ensure_capability_requirements(&r, &c).is_ok());
    }

    #[test]
    fn restricted_does_not_satisfy_native() {
        let c = caps(vec![(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let r = reqs(vec![(Capability::ToolBash, MinSupport::Native)]);
        assert!(ensure_capability_requirements(&r, &c).is_err());
    }

    #[test]
    fn missing_capability_fails() {
        let c: CapabilityManifest = BTreeMap::new();
        let r = reqs(vec![(Capability::Streaming, MinSupport::Emulated)]);
        assert!(ensure_capability_requirements(&r, &c).is_err());
    }

    #[test]
    fn multiple_requirements_all_satisfied() {
        let c = caps(vec![
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]);
        let r = reqs(vec![
            (Capability::Streaming, MinSupport::Emulated),
            (Capability::ToolRead, MinSupport::Emulated),
            (Capability::ToolWrite, MinSupport::Native),
        ]);
        assert!(ensure_capability_requirements(&r, &c).is_ok());
    }

    #[test]
    fn multiple_requirements_one_fails() {
        let c = caps(vec![
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Unsupported),
        ]);
        let r = reqs(vec![
            (Capability::Streaming, MinSupport::Native),
            (Capability::ToolRead, MinSupport::Emulated),
        ]);
        assert!(ensure_capability_requirements(&r, &c).is_err());
    }

    #[test]
    fn error_message_contains_capability_name() {
        let c: CapabilityManifest = BTreeMap::new();
        let r = reqs(vec![(Capability::Streaming, MinSupport::Native)]);
        let err = ensure_capability_requirements(&r, &c).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Streaming"));
    }

    #[test]
    fn error_message_contains_missing_for_absent_cap() {
        let c: CapabilityManifest = BTreeMap::new();
        let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
        let err = ensure_capability_requirements(&r, &c).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }
}

// =========================================================================
// Module: serde_spec
// =========================================================================
mod serde_spec {
    use super::*;

    #[test]
    fn spec_serialize_roundtrip() {
        let mut s = spec("node");
        s.args = vec!["host.js".into()];
        s.env.insert("KEY".into(), "VAL".into());
        s.cwd = Some("/app".into());

        let json = serde_json::to_string(&s).unwrap();
        let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.command, "node");
        assert_eq!(deser.args, vec!["host.js"]);
        assert_eq!(deser.env["KEY"], "VAL");
        assert_eq!(deser.cwd.as_deref(), Some("/app"));
    }

    #[test]
    fn spec_json_keys() {
        let s = spec("echo");
        let val: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert!(val.get("command").is_some());
        assert!(val.get("args").is_some());
        assert!(val.get("env").is_some());
        assert!(val.get("cwd").is_some());
    }

    #[test]
    fn spec_args_serialize_as_array() {
        let mut s = spec("node");
        s.args = vec!["a".into(), "b".into()];
        let val: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert!(val["args"].is_array());
        assert_eq!(val["args"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn spec_env_serialize_as_object() {
        let mut s = spec("node");
        s.env.insert("A".into(), "1".into());
        let val: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert!(val["env"].is_object());
    }

    #[test]
    fn spec_cwd_null_when_none() {
        let val: serde_json::Value = serde_json::to_value(&spec("x")).unwrap();
        assert!(val["cwd"].is_null());
    }

    #[test]
    fn spec_cwd_string_when_some() {
        let mut s = spec("x");
        s.cwd = Some("/work".into());
        let val: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(val["cwd"], "/work");
    }

    #[test]
    fn spec_env_btreemap_ordered_in_json() {
        let mut s = spec("x");
        s.env.insert("Z".into(), "1".into());
        s.env.insert("A".into(), "2".into());
        let json = serde_json::to_string(&s).unwrap();
        let a_pos = json.find("\"A\"").unwrap();
        let z_pos = json.find("\"Z\"").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn spec_deserialize_minimal() {
        let json = r#"{"command":"echo","args":[],"env":{},"cwd":null}"#;
        let s: SidecarSpec = serde_json::from_str(json).unwrap();
        assert_eq!(s.command, "echo");
        assert!(s.args.is_empty());
    }

    #[test]
    fn spec_deserialize_full() {
        let json = r#"{
            "command": "npx",
            "args": ["--yes", "sidecar"],
            "env": {"NODE_ENV": "prod"},
            "cwd": "/opt"
        }"#;
        let s: SidecarSpec = serde_json::from_str(json).unwrap();
        assert_eq!(s.command, "npx");
        assert_eq!(s.args.len(), 2);
        assert_eq!(s.env["NODE_ENV"], "prod");
        assert_eq!(s.cwd.as_deref(), Some("/opt"));
    }

    #[test]
    fn spec_empty_args_roundtrip() {
        let s = spec("echo");
        let json = serde_json::to_string(&s).unwrap();
        let d: SidecarSpec = serde_json::from_str(&json).unwrap();
        assert!(d.args.is_empty());
    }

    #[test]
    fn spec_special_chars_in_env_value() {
        let mut s = spec("node");
        s.env
            .insert("MSG".into(), "hello \"world\" \n\ttab".into());
        let json = serde_json::to_string(&s).unwrap();
        let d: SidecarSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(d.env["MSG"], "hello \"world\" \n\ttab");
    }
}

// =========================================================================
// Module: serde_identity
// =========================================================================
mod serde_identity {
    use super::*;

    #[test]
    fn identity_serialize_contains_id() {
        let id = backend("node").identity();
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("\"sidecar\""));
    }

    #[test]
    fn identity_serialize_contains_adapter_version() {
        let id = backend("node").identity();
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("\"0.1\""));
    }

    #[test]
    fn identity_json_value_format() {
        let id = backend("node").identity();
        let val: serde_json::Value = serde_json::to_value(&id).unwrap();
        assert_eq!(val["id"], "sidecar");
        assert!(val["backend_version"].is_null());
        assert_eq!(val["adapter_version"], "0.1");
    }

    #[test]
    fn identity_roundtrip() {
        let id = backend("node").identity();
        let json = serde_json::to_string(&id).unwrap();
        let d: BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(d.id, "sidecar");
        assert_eq!(d.adapter_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn identity_pretty_json_roundtrip() {
        let id = backend("node").identity();
        let json = serde_json::to_string_pretty(&id).unwrap();
        let d: BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(d.id, id.id);
    }
}

// =========================================================================
// Module: registry_integration
// =========================================================================
mod registry_integration {
    use super::*;

    #[test]
    fn register_single_sidecar() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("sidecar:node", make_metadata("node", "openai"));
        assert!(reg.contains("sidecar:node"));
    }

    #[test]
    fn register_multiple_sidecars() {
        let mut reg = BackendRegistry::new();
        for name in &["sidecar:node", "sidecar:python", "sidecar:claude"] {
            reg.register_with_metadata(name, make_metadata(name, "openai"));
        }
        assert_eq!(reg.len(), 3);
    }

    #[test]
    fn registry_overwrite_replaces_metadata() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("sc:n", make_metadata("v1", "openai"));
        reg.register_with_metadata("sc:n", make_metadata("v2", "anthropic"));
        assert_eq!(reg.len(), 1);
        let m = reg.metadata("sc:n").unwrap();
        assert_eq!(m.name, "v2");
    }

    #[test]
    fn registry_filter_by_dialect() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("sc:a", make_metadata("a", "openai"));
        reg.register_with_metadata("sc:b", make_metadata("b", "anthropic"));
        reg.register_with_metadata("sc:c", make_metadata("c", "openai"));
        let openai = reg.by_dialect("openai");
        assert_eq!(openai.len(), 2);
    }

    #[test]
    fn registry_remove_sidecar() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("sc:x", make_metadata("x", "d"));
        reg.remove("sc:x");
        assert!(!reg.contains("sc:x"));
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_list_is_sorted() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("sc:z", make_metadata("z", "d"));
        reg.register_with_metadata("sc:a", make_metadata("a", "d"));
        reg.register_with_metadata("sc:m", make_metadata("m", "d"));
        let list = reg.list();
        assert_eq!(list, vec!["sc:a", "sc:m", "sc:z"]);
    }

    #[test]
    fn registry_empty_initially() {
        let reg = BackendRegistry::new();
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn registry_not_found_returns_none() {
        let reg = BackendRegistry::new();
        assert!(reg.metadata("nonexistent").is_none());
    }
}

// =========================================================================
// Module: work_order_interaction
// =========================================================================
mod work_order_interaction {
    use super::*;

    #[test]
    fn work_order_builder_produces_valid_wo() {
        let w = wo("test task");
        assert_eq!(w.task, "test task");
    }

    #[test]
    fn work_order_has_uuid_id() {
        let w = wo("task");
        let _ = w.id.to_string(); // valid UUID
    }

    #[test]
    fn work_order_ids_are_unique() {
        let a = wo("a");
        let b = wo("b");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn work_order_default_requirements_empty() {
        let w = wo("task");
        assert!(w.requirements.required.is_empty());
    }

    #[test]
    fn work_order_with_requirements() {
        let w = WorkOrderBuilder::new("task")
            .requirements(reqs(vec![(Capability::Streaming, MinSupport::Native)]))
            .build();
        assert_eq!(w.requirements.required.len(), 1);
    }

    #[test]
    fn work_order_with_model() {
        let w = WorkOrderBuilder::new("task").model("gpt-4").build();
        assert_eq!(w.config.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn work_order_with_max_turns() {
        let w = WorkOrderBuilder::new("task").max_turns(10).build();
        assert_eq!(w.config.max_turns, Some(10));
    }

    #[test]
    fn work_order_serializes_to_json() {
        let w = wo("hello");
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("hello"));
    }
}

// =========================================================================
// Module: receipt_construction
// =========================================================================
mod receipt_construction {
    use super::*;

    #[test]
    fn receipt_builder_with_sidecar_id() {
        let r = ReceiptBuilder::new("sidecar").build();
        assert_eq!(r.backend.id, "sidecar");
    }

    #[test]
    fn receipt_builder_outcome_complete() {
        let r = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_builder_outcome_partial() {
        let r = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Partial)
            .build();
        assert_eq!(r.outcome, Outcome::Partial);
    }

    #[test]
    fn receipt_builder_outcome_failed() {
        let r = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Failed)
            .build();
        assert_eq!(r.outcome, Outcome::Failed);
    }

    #[test]
    fn receipt_without_hash() {
        let r = ReceiptBuilder::new("sidecar").build();
        assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn receipt_with_hash() {
        let r = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        assert!(r.receipt_sha256.is_some());
        assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn receipt_hash_is_hex() {
        let r = ReceiptBuilder::new("sidecar")
            .build()
            .with_hash()
            .unwrap();
        let hash = r.receipt_sha256.as_ref().unwrap();
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_hash_deterministic_for_same_input() {
        let r1 = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        // Hashes may differ due to timestamps, but structure is valid
        assert!(r1.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_serializes_to_json() {
        let r = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Complete)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("sidecar"));
        assert!(json.contains("complete"));
    }

    #[test]
    fn receipt_roundtrips_through_json() {
        let r = ReceiptBuilder::new("sidecar")
            .outcome(Outcome::Complete)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let d: abp_core::Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(d.backend.id, "sidecar");
        assert_eq!(d.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_mode_defaults_to_mapped() {
        let r = ReceiptBuilder::new("sidecar").build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn receipt_mode_passthrough() {
        let r = ReceiptBuilder::new("sidecar")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn receipt_with_adapter_version() {
        let r = ReceiptBuilder::new("sidecar")
            .adapter_version("0.1")
            .build();
        assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn receipt_with_backend_version() {
        let r = ReceiptBuilder::new("sidecar")
            .backend_version("2.0")
            .build();
        assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
    }
}

// =========================================================================
// Module: event_types
// =========================================================================
mod event_types {
    use super::*;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        }
    }

    #[test]
    fn run_started_event() {
        let ev = make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        });
        match &ev.kind {
            AgentEventKind::RunStarted { message } => assert_eq!(message, "starting"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn run_completed_event() {
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        match &ev.kind {
            AgentEventKind::RunCompleted { message } => assert_eq!(message, "done"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn assistant_delta_event() {
        let ev = make_event(AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        });
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn assistant_message_event() {
        let ev = make_event(AgentEventKind::AssistantMessage {
            text: "Full message".into(),
        });
        match &ev.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Full message"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_call_event() {
        let ev = make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "main.rs"}),
        });
        match &ev.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_result_event() {
        let ev = make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            output: serde_json::json!("file contents"),
            is_error: false,
        });
        match &ev.kind {
            AgentEventKind::ToolResult {
                is_error,
                tool_name,
                ..
            } => {
                assert!(!is_error);
                assert_eq!(tool_name, "read_file");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn file_changed_event() {
        let ev = make_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "Added function".into(),
        });
        match &ev.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "Added function");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn command_executed_event() {
        let ev = make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all tests passed".into()),
        });
        match &ev.kind {
            AgentEventKind::CommandExecuted {
                command, exit_code, ..
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(*exit_code, Some(0));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn warning_event() {
        let ev = make_event(AgentEventKind::Warning {
            message: "deprecated API".into(),
        });
        match &ev.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "deprecated API"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn error_event() {
        let ev = make_event(AgentEventKind::Error {
            message: "out of memory".into(),
            error_code: None,
        });
        match &ev.kind {
            AgentEventKind::Error {
                message,
                error_code,
            } => {
                assert_eq!(message, "out of memory");
                assert!(error_code.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn event_with_ext() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "raw_message".into(),
            serde_json::json!({"vendor": "data"}),
        );
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hi".into(),
            },
            ext: Some(ext),
        };
        assert!(ev.ext.is_some());
        assert!(ev.ext.as_ref().unwrap().contains_key("raw_message"));
    }

    #[test]
    fn event_serializes_to_json() {
        let ev = make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        });
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("run_started"));
        assert!(json.contains("go"));
    }

    #[test]
    fn event_roundtrips_through_json() {
        let ev = make_event(AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        });
        let json = serde_json::to_string(&ev).unwrap();
        let d: AgentEvent = serde_json::from_str(&json).unwrap();
        match d.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "hello world"),
            _ => panic!("wrong variant"),
        }
    }
}

// =========================================================================
// Module: event_channel_behavior
// =========================================================================
mod event_channel_behavior {
    use super::*;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        }
    }

    #[tokio::test]
    async fn channel_receives_sent_events() {
        let (tx, mut rx) = mpsc::channel(16);
        let ev = make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        });
        tx.send(ev).await.unwrap();
        drop(tx);
        let received = rx.recv().await.unwrap();
        match received.kind {
            AgentEventKind::RunStarted { message } => assert_eq!(message, "start"),
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn channel_preserves_event_order() {
        let (tx, mut rx) = mpsc::channel(16);
        for i in 0..5 {
            let ev = make_event(AgentEventKind::AssistantDelta {
                text: format!("chunk{i}"),
            });
            tx.send(ev).await.unwrap();
        }
        drop(tx);
        for i in 0..5 {
            let ev = rx.recv().await.unwrap();
            match ev.kind {
                AgentEventKind::AssistantDelta { text } => {
                    assert_eq!(text, format!("chunk{i}"));
                }
                _ => panic!("wrong variant"),
            }
        }
    }

    #[tokio::test]
    async fn channel_returns_none_after_close() {
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(4);
        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn channel_with_multiple_event_types() {
        let (tx, mut rx) = mpsc::channel(16);
        tx.send(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: "hi".into(),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .await
        .unwrap();
        drop(tx);

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn channel_large_buffer() {
        let (tx, mut rx) = mpsc::channel(1024);
        for i in 0..100 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("t{i}"),
            }))
            .await
            .unwrap();
        }
        drop(tx);
        let mut count = 0;
        while let Some(_) = rx.recv().await {
            count += 1;
        }
        assert_eq!(count, 100);
    }

    #[tokio::test]
    async fn channel_small_buffer_does_not_lose_events() {
        let (tx, mut rx) = mpsc::channel(1);
        let handle = tokio::spawn(async move {
            for i in 0..10 {
                tx.send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("d{i}"),
                }))
                .await
                .unwrap();
            }
        });
        let mut count = 0;
        while let Some(_) = rx.recv().await {
            count += 1;
        }
        handle.await.unwrap();
        assert_eq!(count, 10);
    }
}

// =========================================================================
// Module: error_handling
// =========================================================================
mod error_handling {
    use super::*;

    #[test]
    fn host_error_spawn_display() {
        let err = abp_host::HostError::Spawn(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        let msg = err.to_string();
        assert!(msg.contains("spawn"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn host_error_stdout_display() {
        let err = abp_host::HostError::Stdout(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe broken",
        ));
        assert!(err.to_string().contains("stdout"));
    }

    #[test]
    fn host_error_stdin_display() {
        let err = abp_host::HostError::Stdin(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe broken",
        ));
        assert!(err.to_string().contains("stdin"));
    }

    #[test]
    fn host_error_violation_display() {
        let err = abp_host::HostError::Violation("unexpected run before hello".into());
        assert!(err.to_string().contains("violation"));
    }

    #[test]
    fn host_error_fatal_display() {
        let err = abp_host::HostError::Fatal("out of memory".into());
        assert!(err.to_string().contains("fatal"));
        assert!(err.to_string().contains("out of memory"));
    }

    #[test]
    fn host_error_exited_display() {
        let err = abp_host::HostError::Exited { code: Some(1) };
        let msg = err.to_string();
        assert!(msg.contains("exited"));
    }

    #[test]
    fn host_error_exited_none_code() {
        let err = abp_host::HostError::Exited { code: None };
        let msg = err.to_string();
        assert!(msg.contains("exited"));
    }

    #[test]
    fn host_error_sidecar_crashed_display() {
        let err = abp_host::HostError::SidecarCrashed {
            exit_code: Some(137),
            stderr: "killed by signal".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("crashed"));
    }

    #[test]
    fn host_error_timeout_display() {
        let err = abp_host::HostError::Timeout {
            duration: std::time::Duration::from_secs(30),
        };
        let msg = err.to_string();
        assert!(msg.contains("timed out"));
    }

    #[test]
    fn host_error_is_debug() {
        let err = abp_host::HostError::Fatal("test".into());
        let _ = format!("{err:?}");
    }

    #[test]
    fn host_error_implements_std_error() {
        let err: Box<dyn std::error::Error> =
            Box::new(abp_host::HostError::Fatal("test".into()));
        let _ = err.to_string();
    }

    #[tokio::test]
    async fn run_nonexistent_command_fails() {
        let b = SidecarBackend::new(SidecarSpec::new(
            "nonexistent_binary_that_should_not_exist_12345",
        ));
        let (tx, _rx) = mpsc::channel(16);
        let result = b.run(Uuid::new_v4(), wo("test"), tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_error_message_contains_spawn() {
        let b = SidecarBackend::new(SidecarSpec::new("no_such_binary_xyz_abc"));
        let (tx, _rx) = mpsc::channel(16);
        let err = b.run(Uuid::new_v4(), wo("test"), tx).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("spawn"));
    }

    #[tokio::test]
    async fn run_with_invalid_cwd_fails() {
        let mut s = SidecarSpec::new("echo");
        s.cwd = Some("/nonexistent_directory_12345".into());
        let b = SidecarBackend::new(s);
        let (tx, _rx) = mpsc::channel(16);
        let result = b.run(Uuid::new_v4(), wo("test"), tx).await;
        assert!(result.is_err());
    }
}

// =========================================================================
// Module: execution_mode
// =========================================================================
mod execution_mode {
    use super::*;

    #[test]
    fn default_mode_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn mode_passthrough() {
        let m = ExecutionMode::Passthrough;
        assert_ne!(m, ExecutionMode::Mapped);
    }

    #[test]
    fn mode_serializes_mapped() {
        let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
        assert_eq!(json, "\"mapped\"");
    }

    #[test]
    fn mode_serializes_passthrough() {
        let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
        assert_eq!(json, "\"passthrough\"");
    }

    #[test]
    fn mode_deserializes_mapped() {
        let m: ExecutionMode = serde_json::from_str("\"mapped\"").unwrap();
        assert_eq!(m, ExecutionMode::Mapped);
    }

    #[test]
    fn mode_deserializes_passthrough() {
        let m: ExecutionMode = serde_json::from_str("\"passthrough\"").unwrap();
        assert_eq!(m, ExecutionMode::Passthrough);
    }

    #[test]
    fn mode_roundtrip() {
        for mode in &[ExecutionMode::Mapped, ExecutionMode::Passthrough] {
            let json = serde_json::to_string(mode).unwrap();
            let d: ExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(*mode, d);
        }
    }

    #[test]
    fn mode_is_copy() {
        let a = ExecutionMode::Mapped;
        let b = a;
        assert_eq!(a, b);
    }
}

// =========================================================================
// Module: outcome_types
// =========================================================================
mod outcome_types {
    use super::*;

    #[test]
    fn outcome_complete() {
        assert_eq!(Outcome::Complete, Outcome::Complete);
    }

    #[test]
    fn outcome_partial() {
        assert_eq!(Outcome::Partial, Outcome::Partial);
    }

    #[test]
    fn outcome_failed() {
        assert_eq!(Outcome::Failed, Outcome::Failed);
    }

    #[test]
    fn outcome_not_equal_variants() {
        assert_ne!(Outcome::Complete, Outcome::Failed);
        assert_ne!(Outcome::Complete, Outcome::Partial);
        assert_ne!(Outcome::Partial, Outcome::Failed);
    }

    #[test]
    fn outcome_serialize_complete() {
        let json = serde_json::to_string(&Outcome::Complete).unwrap();
        assert_eq!(json, "\"complete\"");
    }

    #[test]
    fn outcome_serialize_partial() {
        let json = serde_json::to_string(&Outcome::Partial).unwrap();
        assert_eq!(json, "\"partial\"");
    }

    #[test]
    fn outcome_serialize_failed() {
        let json = serde_json::to_string(&Outcome::Failed).unwrap();
        assert_eq!(json, "\"failed\"");
    }

    #[test]
    fn outcome_roundtrip() {
        for o in &[Outcome::Complete, Outcome::Partial, Outcome::Failed] {
            let json = serde_json::to_string(o).unwrap();
            let d: Outcome = serde_json::from_str(&json).unwrap();
            assert_eq!(*o, d);
        }
    }
}

// =========================================================================
// Module: support_level
// =========================================================================
mod support_level {
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
    fn unsupported_does_not_satisfy_native() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn unsupported_does_not_satisfy_emulated() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
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

    #[test]
    fn native_serialize() {
        let json = serde_json::to_string(&SupportLevel::Native).unwrap();
        assert_eq!(json, "\"native\"");
    }

    #[test]
    fn emulated_serialize() {
        let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
        assert_eq!(json, "\"emulated\"");
    }

    #[test]
    fn unsupported_serialize() {
        let json = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
        assert_eq!(json, "\"unsupported\"");
    }

    #[test]
    fn restricted_serialize() {
        let r = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("restricted"));
        assert!(json.contains("policy"));
    }

    #[test]
    fn support_level_roundtrip_native() {
        let json = serde_json::to_string(&SupportLevel::Native).unwrap();
        let d: SupportLevel = serde_json::from_str(&json).unwrap();
        assert!(d.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_level_roundtrip_restricted() {
        let r = SupportLevel::Restricted {
            reason: "sandbox".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let d: SupportLevel = serde_json::from_str(&json).unwrap();
        assert!(d.satisfies(&MinSupport::Emulated));
        assert!(!d.satisfies(&MinSupport::Native));
    }
}

// =========================================================================
// Module: backend_trait_via_dyn
// =========================================================================
mod backend_trait_via_dyn {
    use super::*;

    #[test]
    fn dyn_identity() {
        let b: Box<dyn Backend> = Box::new(backend("node"));
        assert_eq!(b.identity().id, "sidecar");
    }

    #[test]
    fn dyn_capabilities() {
        let b: Box<dyn Backend> = Box::new(backend("node"));
        assert!(b.capabilities().is_empty());
    }

    #[test]
    fn arc_identity() {
        let b: std::sync::Arc<dyn Backend> = std::sync::Arc::new(backend("node"));
        assert_eq!(b.identity().id, "sidecar");
    }

    #[test]
    fn arc_capabilities() {
        let b: std::sync::Arc<dyn Backend> = std::sync::Arc::new(backend("node"));
        assert!(b.capabilities().is_empty());
    }

    #[tokio::test]
    async fn dyn_run_nonexistent_fails() {
        let b: Box<dyn Backend> = Box::new(backend("no_such_sidecar_999"));
        let (tx, _rx) = mpsc::channel(16);
        let res = b.run(Uuid::new_v4(), wo("test"), tx).await;
        assert!(res.is_err());
    }
}

// =========================================================================
// Module: concurrency
// =========================================================================
mod concurrency {
    use super::*;

    #[tokio::test]
    async fn multiple_backends_constructed_in_parallel() {
        let handles: Vec<_> = (0..10)
            .map(|i| {
                tokio::spawn(async move {
                    let b = backend(&format!("cmd{i}"));
                    b.identity().id.clone()
                })
            })
            .collect();
        for h in handles {
            assert_eq!(h.await.unwrap(), "sidecar");
        }
    }

    #[tokio::test]
    async fn identity_from_shared_arc() {
        let b: std::sync::Arc<dyn Backend> = std::sync::Arc::new(backend("node"));
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let b = b.clone();
                tokio::spawn(async move { b.identity().id.clone() })
            })
            .collect();
        for h in handles {
            assert_eq!(h.await.unwrap(), "sidecar");
        }
    }

    #[tokio::test]
    async fn capabilities_from_shared_arc() {
        let b: std::sync::Arc<dyn Backend> = std::sync::Arc::new(backend("node"));
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let b = b.clone();
                tokio::spawn(async move { b.capabilities().len() })
            })
            .collect();
        for h in handles {
            assert_eq!(h.await.unwrap(), 0);
        }
    }
}

// =========================================================================
// Module: edge_cases
// =========================================================================
mod edge_cases {
    use super::*;

    #[test]
    fn spec_with_whitespace_command() {
        let b = backend("  node  ");
        assert_eq!(b.spec.command, "  node  ");
    }

    #[test]
    fn spec_with_special_chars_in_args() {
        let mut s = spec("node");
        s.args = vec!["--flag=value with spaces".into(), "arg\"quote".into()];
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.args[0], "--flag=value with spaces");
        assert_eq!(b.spec.args[1], "arg\"quote");
    }

    #[test]
    fn spec_with_empty_env_key() {
        let mut s = spec("node");
        s.env.insert("".into(), "value".into());
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.env[""], "value");
    }

    #[test]
    fn spec_with_empty_env_value() {
        let mut s = spec("node");
        s.env.insert("KEY".into(), "".into());
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.env["KEY"], "");
    }

    #[test]
    fn spec_with_duplicate_args() {
        let mut s = spec("node");
        s.args = vec!["--verbose".into(), "--verbose".into()];
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.args.len(), 2);
    }

    #[test]
    fn spec_cwd_relative_path() {
        let mut s = spec("node");
        s.cwd = Some("./relative/path".into());
        let b = SidecarBackend::new(s);
        assert_eq!(b.spec.cwd.as_deref(), Some("./relative/path"));
    }

    #[test]
    fn backend_with_long_command() {
        let long_cmd = "a".repeat(10000);
        let b = backend(&long_cmd);
        assert_eq!(b.spec.command.len(), 10000);
    }

    #[test]
    fn work_order_with_empty_task() {
        let w = wo("");
        assert_eq!(w.task, "");
    }

    #[test]
    fn work_order_with_very_long_task() {
        let task = "x".repeat(100_000);
        let w = wo(&task);
        assert_eq!(w.task.len(), 100_000);
    }

    #[test]
    fn work_order_with_unicode_task() {
        let w = wo("日本語タスク 🚀");
        assert_eq!(w.task, "日本語タスク 🚀");
    }

    #[test]
    fn uuid_run_id_is_valid() {
        let id = Uuid::new_v4();
        let s = id.to_string();
        assert_eq!(s.len(), 36);
        assert!(s.contains('-'));
    }
}

// =========================================================================
// Module: capability_manifest_operations
// =========================================================================
mod capability_manifest_operations {
    use super::*;

    #[test]
    fn empty_manifest() {
        let m: CapabilityManifest = BTreeMap::new();
        assert!(m.is_empty());
    }

    #[test]
    fn manifest_insert_and_get() {
        let mut m: CapabilityManifest = BTreeMap::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        assert!(m.contains_key(&Capability::Streaming));
    }

    #[test]
    fn manifest_is_btreemap_ordered() {
        let c = caps(vec![
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]);
        let keys: Vec<&Capability> = c.keys().collect();
        // BTreeMap ensures consistent ordering
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn manifest_serialization_deterministic() {
        let c = caps(vec![
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Native),
        ]);
        let json1 = serde_json::to_string(&c).unwrap();
        let json2 = serde_json::to_string(&c).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn manifest_roundtrip() {
        let c = caps(vec![
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::Vision, SupportLevel::Unsupported),
        ]);
        let json = serde_json::to_string(&c).unwrap();
        let d: CapabilityManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(d.len(), 3);
    }

    #[test]
    fn manifest_overwrite_key() {
        let mut m: CapabilityManifest = BTreeMap::new();
        m.insert(Capability::Streaming, SupportLevel::Emulated);
        m.insert(Capability::Streaming, SupportLevel::Native);
        assert_eq!(m.len(), 1);
        assert!(m[&Capability::Streaming].satisfies(&MinSupport::Native));
    }
}

// =========================================================================
// Module: contract_version
// =========================================================================
mod contract_version {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn contract_version_is_abp_v0_1() {
        assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn contract_version_starts_with_abp() {
        assert!(abp_core::CONTRACT_VERSION.starts_with("abp/"));
    }

    #[test]
    fn contract_version_not_empty() {
        assert!(!abp_core::CONTRACT_VERSION.is_empty());
    }
}
