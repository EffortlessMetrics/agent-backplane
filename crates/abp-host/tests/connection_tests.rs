// SPDX-License-Identifier: MIT OR Apache-2.0
//! Connection lifecycle and edge-case tests for abp-host.
//!
//! Covers SidecarSpec serialization, SidecarClient timeout / reconnect
//! behaviour, and SidecarHello parsing edge cases.

use abp_core::{
    BackendIdentity, Capability, CapabilityManifest, CONTRACT_VERSION, SupportLevel,
};
use abp_host::{SidecarClient, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn python_cmd() -> Option<String> {
    for cmd in &["python3", "python"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(cmd.to_string());
        }
    }
    None
}

macro_rules! require_python {
    () => {
        match python_cmd() {
            Some(cmd) => cmd,
            None => {
                eprintln!("SKIP: python not found");
                return;
            }
        }
    };
}

// ---------------------------------------------------------------------------
// 1. SidecarSpec serialization — round-trips through serde
// ---------------------------------------------------------------------------

#[test]
fn sidecar_spec_round_trips_through_serde() {
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["index.js".into(), "--port".into(), "3000".into()],
        env: BTreeMap::new(),
        cwd: Some("/tmp/workspace".into()),
    };

    let json = serde_json::to_string(&spec).expect("serialize");
    let deserialized: SidecarSpec = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.command, "node");
    assert_eq!(deserialized.args, vec!["index.js", "--port", "3000"]);
    assert_eq!(deserialized.cwd.as_deref(), Some("/tmp/workspace"));
    assert!(deserialized.env.is_empty());
}

// ---------------------------------------------------------------------------
// 2. SidecarSpec with env vars — env map serializes correctly
// ---------------------------------------------------------------------------

#[test]
fn sidecar_spec_with_env_vars_round_trips() {
    let mut spec = SidecarSpec::new("python3");
    spec.args = vec!["sidecar.py".into()];
    spec.env.insert("API_KEY".into(), "secret-123".into());
    spec.env.insert("DEBUG".into(), "true".into());
    spec.env.insert("TIMEOUT".into(), "30".into());

    let json = serde_json::to_string(&spec).expect("serialize");

    // Verify env entries appear in the JSON.
    assert!(json.contains("API_KEY"));
    assert!(json.contains("secret-123"));
    assert!(json.contains("DEBUG"));

    let deserialized: SidecarSpec = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.env.len(), 3);
    assert_eq!(deserialized.env["API_KEY"], "secret-123");
    assert_eq!(deserialized.env["DEBUG"], "true");
    assert_eq!(deserialized.env["TIMEOUT"], "30");
}

// ---------------------------------------------------------------------------
// 3. SidecarClient timeout — sidecar that sleeps forever is handled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_client_timeout_on_no_response() {
    let py = require_python!();

    // Sidecar that sleeps without producing any output.
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "import time; time.sleep(5)".into()];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        SidecarClient::spawn(spec),
    )
    .await;

    // The outer timeout fires because the sidecar never sends hello.
    assert!(result.is_err(), "spawn should timeout when sidecar is silent");
}

// ---------------------------------------------------------------------------
// 4. SidecarClient reconnect — after a failed connection, can create new one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_client_reconnect_after_failure() {
    let py = require_python!();
    let mock_script = {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest
            .join("tests")
            .join("mock_sidecar.py")
            .to_string_lossy()
            .into_owned()
    };

    // First attempt: bad command that fails.
    let bad_spec = SidecarSpec::new("nonexistent-binary-abp-reconnect-xyz");
    let result = SidecarClient::spawn(bad_spec).await;
    assert!(result.is_err(), "first spawn should fail");

    // Second attempt: valid command succeeds.
    let mut good_spec = SidecarSpec::new(&py);
    good_spec.args = vec![mock_script];
    let client = SidecarClient::spawn(good_spec)
        .await
        .expect("second spawn should succeed after prior failure");

    assert_eq!(client.hello.backend.id, "mock-test");
}

// ---------------------------------------------------------------------------
// 5. SidecarHello parsing — parse various hello message formats
// ---------------------------------------------------------------------------

#[test]
fn sidecar_hello_parsing_basic() {
    let json = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": {
            "id": "test-backend",
            "backend_version": "1.0.0",
            "adapter_version": null
        },
        "capabilities": {
            "streaming": "native",
            "tool_read": "emulated"
        }
    });

    let line = serde_json::to_string(&json).unwrap();
    let env = JsonlCodec::decode(&line).unwrap();

    match env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-backend");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            assert!(backend.adapter_version.is_none());
            assert_eq!(capabilities.len(), 2);
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Emulated)
            ));
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn sidecar_hello_parsing_minimal() {
    let json = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {
            "id": "minimal",
            "backend_version": null,
            "adapter_version": null
        },
        "capabilities": {}
    });

    let line = serde_json::to_string(&json).unwrap();
    let env = JsonlCodec::decode(&line).unwrap();

    match env {
        Envelope::Hello {
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "minimal");
            assert!(capabilities.is_empty());
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 6. SidecarHello with extra fields — forward-compatible
// ---------------------------------------------------------------------------

#[test]
fn sidecar_hello_with_extra_fields_tolerated() {
    // Simulates a newer sidecar sending fields the host doesn't know about.
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"future-sidecar","backend_version":"2.0","adapter_version":null,"extra_field":"ignored"},"capabilities":{},"unknown_top_level":"also_ignored"}"#;

    // Parsing should succeed — unknown fields are silently dropped.
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "future-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 7. Large capability manifest — hello with many capabilities
// ---------------------------------------------------------------------------

#[test]
fn sidecar_hello_large_capability_manifest() {
    let mut caps = BTreeMap::new();
    caps.insert("streaming".to_string(), "native");
    caps.insert("tool_read".to_string(), "native");
    caps.insert("tool_write".to_string(), "native");
    caps.insert("tool_edit".to_string(), "native");
    caps.insert("tool_bash".to_string(), "native");
    caps.insert("tool_glob".to_string(), "emulated");
    caps.insert("tool_grep".to_string(), "emulated");
    caps.insert("tool_web_search".to_string(), "unsupported");
    caps.insert("tool_web_fetch".to_string(), "unsupported");
    caps.insert("tool_ask_user".to_string(), "native");
    caps.insert("hooks_pre_tool_use".to_string(), "native");
    caps.insert("hooks_post_tool_use".to_string(), "native");
    caps.insert("session_resume".to_string(), "emulated");
    caps.insert("session_fork".to_string(), "unsupported");
    caps.insert("checkpointing".to_string(), "native");
    caps.insert("structured_output_json_schema".to_string(), "native");
    caps.insert("mcp_client".to_string(), "native");
    caps.insert("mcp_server".to_string(), "emulated");

    let caps_json: BTreeMap<String, serde_json::Value> = caps
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v.into())))
        .collect();

    let json = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": {
            "id": "full-featured",
            "backend_version": "3.0",
            "adapter_version": "1.5"
        },
        "capabilities": caps_json
    });

    let line = serde_json::to_string(&json).unwrap();
    let env = JsonlCodec::decode(&line).unwrap();

    match env {
        Envelope::Hello {
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "full-featured");
            assert_eq!(
                capabilities.len(),
                18,
                "all 18 capabilities should parse; got {}",
                capabilities.len()
            );
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolGrep),
                Some(SupportLevel::Emulated)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolWebSearch),
                Some(SupportLevel::Unsupported)
            ));
            assert!(matches!(
                capabilities.get(&Capability::McpServer),
                Some(SupportLevel::Emulated)
            ));
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 8. Empty capability manifest — hello with no capabilities
// ---------------------------------------------------------------------------

#[test]
fn sidecar_hello_empty_capability_manifest() {
    let json = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": {
            "id": "bare-bones",
            "backend_version": null,
            "adapter_version": null
        },
        "capabilities": {}
    });

    let line = serde_json::to_string(&json).unwrap();
    let env = JsonlCodec::decode(&line).unwrap();

    match env {
        Envelope::Hello {
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "bare-bones");
            assert!(
                capabilities.is_empty(),
                "empty capabilities should parse as empty map"
            );
        }
        other => panic!("expected Hello, got {:?}", other),
    }

    // Also verify SidecarHello can be constructed from the parsed data.
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "bare-bones".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };
    assert!(hello.capabilities.is_empty());
    assert_eq!(hello.contract_version, CONTRACT_VERSION);
}
