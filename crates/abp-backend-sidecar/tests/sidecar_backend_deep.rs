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
//! Deep tests for the `abp-backend-sidecar` crate.
//!
//! These tests exercise construction, configuration, trait implementations,
//! serde round-trips, and registry integration — all without spawning
//! actual sidecar processes.

use abp_backend_core::{Backend, BackendMetadata, BackendRegistry};
use abp_backend_sidecar::SidecarBackend;
use abp_host::SidecarSpec;

// ---------------------------------------------------------------------------
// Module: backend_construction
// ---------------------------------------------------------------------------
mod backend_construction {
    use super::*;

    #[test]
    fn create_with_command_only() {
        let spec = SidecarSpec::new("node");
        let backend = SidecarBackend::new(spec);
        assert_eq!(backend.spec.command, "node");
        assert!(backend.spec.args.is_empty());
        assert!(backend.spec.env.is_empty());
        assert!(backend.spec.cwd.is_none());
    }

    #[test]
    fn create_with_args() {
        let mut spec = SidecarSpec::new("python3");
        spec.args = vec!["-m".into(), "sidecar".into()];
        let backend = SidecarBackend::new(spec);
        assert_eq!(backend.spec.args, vec!["-m", "sidecar"]);
    }

    #[test]
    fn create_with_environment_variables() {
        let mut spec = SidecarSpec::new("node");
        spec.env.insert("API_KEY".into(), "sk-test-123".into());
        spec.env.insert("LOG_LEVEL".into(), "debug".into());
        let backend = SidecarBackend::new(spec);
        assert_eq!(backend.spec.env.len(), 2);
        assert_eq!(backend.spec.env["API_KEY"], "sk-test-123");
        assert_eq!(backend.spec.env["LOG_LEVEL"], "debug");
    }

    #[test]
    fn create_with_working_directory() {
        let mut spec = SidecarSpec::new("node");
        spec.cwd = Some("/tmp/workspace".into());
        let backend = SidecarBackend::new(spec);
        assert_eq!(backend.spec.cwd.as_deref(), Some("/tmp/workspace"));
    }

    #[test]
    fn backend_debug_trait() {
        let spec = SidecarSpec::new("echo");
        let backend = SidecarBackend::new(spec);
        let debug_str = format!("{backend:?}");
        assert!(debug_str.contains("SidecarBackend"));
        assert!(debug_str.contains("echo"));
    }

    #[test]
    fn backend_clone_trait() {
        let mut spec = SidecarSpec::new("node");
        spec.args = vec!["index.js".into()];
        spec.env.insert("KEY".into(), "VAL".into());
        let backend = SidecarBackend::new(spec);
        let cloned = backend.clone();
        assert_eq!(cloned.spec.command, backend.spec.command);
        assert_eq!(cloned.spec.args, backend.spec.args);
        assert_eq!(cloned.spec.env, backend.spec.env);
    }

    #[test]
    fn backend_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<SidecarBackend>();
        assert_sync::<SidecarBackend>();
    }

    #[test]
    fn backend_identity_returns_sidecar() {
        let backend = SidecarBackend::new(SidecarSpec::new("node"));
        let id = backend.identity();
        assert_eq!(id.id, "sidecar");
        assert!(id.backend_version.is_none());
        assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn backend_capabilities_default_empty() {
        let backend = SidecarBackend::new(SidecarSpec::new("node"));
        let caps = backend.capabilities();
        assert!(caps.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Module: backend_configuration
// ---------------------------------------------------------------------------
mod backend_configuration {
    use super::*;

    #[test]
    fn spec_new_sets_command() {
        let spec = SidecarSpec::new("python3");
        assert_eq!(spec.command, "python3");
    }

    #[test]
    fn spec_new_defaults_are_empty() {
        let spec = SidecarSpec::new("node");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn spec_with_multiple_args() {
        let mut spec = SidecarSpec::new("npx");
        spec.args = vec![
            "--yes".into(),
            "my-sidecar@latest".into(),
            "--port".into(),
            "3000".into(),
        ];
        assert_eq!(spec.args.len(), 4);
        assert_eq!(spec.args[0], "--yes");
        assert_eq!(spec.args[3], "3000");
    }

    #[test]
    fn spec_env_uses_btreemap_ordered() {
        let mut spec = SidecarSpec::new("node");
        spec.env.insert("ZEBRA".into(), "1".into());
        spec.env.insert("ALPHA".into(), "2".into());
        spec.env.insert("MIDDLE".into(), "3".into());
        let keys: Vec<&String> = spec.env.keys().collect();
        assert_eq!(keys, vec!["ALPHA", "MIDDLE", "ZEBRA"]);
    }

    #[test]
    fn spec_cwd_can_be_set_and_cleared() {
        let mut spec = SidecarSpec::new("node");
        spec.cwd = Some("/workspace".into());
        assert_eq!(spec.cwd.as_deref(), Some("/workspace"));
        spec.cwd = None;
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn spec_accepts_string_types_via_into() {
        let owned = String::from("my-binary");
        let spec = SidecarSpec::new(owned);
        assert_eq!(spec.command, "my-binary");

        let spec2 = SidecarSpec::new("literal-str");
        assert_eq!(spec2.command, "literal-str");
    }
}

// ---------------------------------------------------------------------------
// Module: backend_registration
// ---------------------------------------------------------------------------
mod backend_registration {
    use super::*;

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

    #[test]
    fn register_single_sidecar() {
        let mut registry = BackendRegistry::new();
        let meta = make_metadata("sidecar:node", "openai");
        registry.register_with_metadata("sidecar:node", meta);
        assert!(registry.contains("sidecar:node"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn register_multiple_sidecars() {
        let mut registry = BackendRegistry::new();
        for name in &["sidecar:node", "sidecar:python", "sidecar:claude"] {
            registry.register_with_metadata(name, make_metadata(name, "openai"));
        }
        assert_eq!(registry.len(), 3);
        let list = registry.list();
        assert!(list.contains(&"sidecar:node"));
        assert!(list.contains(&"sidecar:python"));
        assert!(list.contains(&"sidecar:claude"));
    }

    #[test]
    fn backend_name_uniqueness_overwrites() {
        let mut registry = BackendRegistry::new();
        registry.register_with_metadata("sidecar:node", make_metadata("node-v1", "openai"));
        registry.register_with_metadata("sidecar:node", make_metadata("node-v2", "anthropic"));
        assert_eq!(registry.len(), 1);
        let meta = registry.metadata("sidecar:node").unwrap();
        assert_eq!(meta.name, "node-v2");
        assert_eq!(meta.dialect, "anthropic");
    }

    #[test]
    fn backend_discovery_by_dialect() {
        let mut registry = BackendRegistry::new();
        registry.register_with_metadata("sidecar:node", make_metadata("node", "openai"));
        registry.register_with_metadata("sidecar:claude", make_metadata("claude", "anthropic"));
        registry.register_with_metadata("sidecar:gpt", make_metadata("gpt", "openai"));

        let openai_backends = registry.by_dialect("openai");
        assert_eq!(openai_backends.len(), 2);
        assert!(openai_backends.contains(&"sidecar:node"));
        assert!(openai_backends.contains(&"sidecar:gpt"));

        let anthropic_backends = registry.by_dialect("anthropic");
        assert_eq!(anthropic_backends.len(), 1);
        assert!(anthropic_backends.contains(&"sidecar:claude"));
    }

    #[test]
    fn backend_listing_is_sorted() {
        let mut registry = BackendRegistry::new();
        registry.register_with_metadata("sidecar:zulu", make_metadata("z", "x"));
        registry.register_with_metadata("sidecar:alpha", make_metadata("a", "x"));
        registry.register_with_metadata("sidecar:middle", make_metadata("m", "x"));
        let list = registry.list();
        assert_eq!(
            list,
            vec!["sidecar:alpha", "sidecar:middle", "sidecar:zulu"]
        );
    }

    #[test]
    fn backend_removal() {
        let mut registry = BackendRegistry::new();
        registry.register_with_metadata("sidecar:temp", make_metadata("temp", "x"));
        assert!(registry.contains("sidecar:temp"));
        registry.remove("sidecar:temp");
        assert!(!registry.contains("sidecar:temp"));
        assert_eq!(registry.len(), 0);
    }
}

// ---------------------------------------------------------------------------
// Module: backend_serde
// ---------------------------------------------------------------------------
mod backend_serde {
    use super::*;

    #[test]
    fn sidecar_spec_serialize_roundtrip() {
        let mut spec = SidecarSpec::new("node");
        spec.args = vec!["host.js".into()];
        spec.env.insert("API_KEY".into(), "test".into());
        spec.cwd = Some("/app".into());

        let json = serde_json::to_string(&spec).unwrap();
        let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.command, "node");
        assert_eq!(deser.args, vec!["host.js"]);
        assert_eq!(deser.env["API_KEY"], "test");
        assert_eq!(deser.cwd.as_deref(), Some("/app"));
    }

    #[test]
    fn sidecar_spec_json_format() {
        let spec = SidecarSpec::new("python3");
        let val: serde_json::Value = serde_json::to_value(&spec).unwrap();
        assert_eq!(val["command"], "python3");
        assert!(val["args"].is_array());
        assert!(val["args"].as_array().unwrap().is_empty());
        assert!(val["env"].is_object());
        assert!(val["cwd"].is_null());
    }

    #[test]
    fn sidecar_spec_optional_cwd_absent_in_json() {
        let spec = SidecarSpec::new("node");
        let val: serde_json::Value = serde_json::to_value(&spec).unwrap();
        // cwd is None, serialized as null by default
        assert!(val["cwd"].is_null());
    }

    #[test]
    fn sidecar_spec_optional_cwd_present_in_json() {
        let mut spec = SidecarSpec::new("node");
        spec.cwd = Some("/work".into());
        let val: serde_json::Value = serde_json::to_value(&spec).unwrap();
        assert_eq!(val["cwd"], "/work");
    }

    #[test]
    fn sidecar_spec_env_deterministic_order() {
        let mut spec = SidecarSpec::new("node");
        spec.env.insert("Z_KEY".into(), "last".into());
        spec.env.insert("A_KEY".into(), "first".into());
        let json = serde_json::to_string(&spec).unwrap();
        let a_pos = json.find("A_KEY").unwrap();
        let z_pos = json.find("Z_KEY").unwrap();
        assert!(
            a_pos < z_pos,
            "BTreeMap should serialize A_KEY before Z_KEY"
        );
    }

    #[test]
    fn sidecar_spec_deserialize_minimal_json() {
        let json = r#"{"command":"echo","args":[],"env":{},"cwd":null}"#;
        let spec: SidecarSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.command, "echo");
        assert!(spec.args.is_empty());
        assert!(spec.env.is_empty());
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn sidecar_spec_deserialize_with_all_fields() {
        let json = r#"{
            "command": "npx",
            "args": ["--yes", "my-sidecar"],
            "env": {"NODE_ENV": "production", "PORT": "8080"},
            "cwd": "/opt/app"
        }"#;
        let spec: SidecarSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.command, "npx");
        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.env.len(), 2);
        assert_eq!(spec.env["NODE_ENV"], "production");
        assert_eq!(spec.cwd.as_deref(), Some("/opt/app"));
    }

    #[test]
    fn backend_identity_serialize_roundtrip() {
        let backend = SidecarBackend::new(SidecarSpec::new("test"));
        let id = backend.identity();
        let json = serde_json::to_string(&id).unwrap();
        let deser: abp_core::BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.id, "sidecar");
        assert_eq!(deser.adapter_version.as_deref(), Some("0.1"));
    }
}
