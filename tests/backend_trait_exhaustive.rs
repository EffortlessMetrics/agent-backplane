#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive test suite for the Backend trait, MockBackend, BackendRegistry,
//! SidecarBackend interface, error codes, capability reporting, and concurrency.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use abp_backend_core::{
    Backend, BackendHealth, BackendMetadata, BackendRegistry, HealthStatus, RateLimit,
    ensure_capability_requirements, extract_execution_mode, validate_passthrough_compatibility,
};
use abp_backend_mock::MockBackend;
use abp_backend_mock::scenarios::{
    MockBackendRecorder, MockScenario, RecordedCall, ScenarioMockBackend,
};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ContractError, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn simple_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn work_order_with_requirements(reqs: Vec<CapabilityRequirement>) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test task")
        .requirements(CapabilityRequirements { required: reqs })
        .build()
}

fn work_order_with_model(model: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("model test").model(model).build()
}

fn work_order_passthrough() -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    WorkOrderBuilder::new("passthrough test")
        .config(config)
        .build()
}

fn work_order_passthrough_flat() -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp.mode".to_string(), json!("passthrough"));
    WorkOrderBuilder::new("passthrough flat test")
        .config(config)
        .build()
}

fn metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "1.0".to_string(),
        max_tokens: None,
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    }
}

async fn collect_events(mut rx: mpsc::Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    events
}

// ============================================================================
// Module: Backend Trait Interface
// ============================================================================
mod backend_trait_interface {
    use super::*;

    #[tokio::test]
    async fn mock_backend_identity_id() {
        let b = MockBackend;
        assert_eq!(b.identity().id, "mock");
    }

    #[tokio::test]
    async fn mock_backend_identity_backend_version() {
        let b = MockBackend;
        assert_eq!(b.identity().backend_version.as_deref(), Some("0.1"));
    }

    #[tokio::test]
    async fn mock_backend_identity_adapter_version() {
        let b = MockBackend;
        assert_eq!(b.identity().adapter_version.as_deref(), Some("0.1"));
    }

    #[tokio::test]
    async fn mock_backend_capabilities_not_empty() {
        let b = MockBackend;
        assert!(!b.capabilities().is_empty());
    }

    #[tokio::test]
    async fn mock_backend_has_streaming_native() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(matches!(
            caps.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[tokio::test]
    async fn mock_backend_has_tool_read_emulated() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolRead),
            Some(SupportLevel::Emulated)
        ));
    }

    #[tokio::test]
    async fn mock_backend_has_tool_write_emulated() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolWrite),
            Some(SupportLevel::Emulated)
        ));
    }

    #[tokio::test]
    async fn mock_backend_has_tool_edit_emulated() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolEdit),
            Some(SupportLevel::Emulated)
        ));
    }

    #[tokio::test]
    async fn mock_backend_has_tool_bash_emulated() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(matches!(
            caps.get(&Capability::ToolBash),
            Some(SupportLevel::Emulated)
        ));
    }

    #[tokio::test]
    async fn mock_backend_has_structured_output_emulated() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(matches!(
            caps.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Emulated)
        ));
    }

    #[tokio::test]
    async fn mock_backend_missing_capability_returns_none() {
        let b = MockBackend;
        let caps = b.capabilities();
        assert!(caps.get(&Capability::Vision).is_none());
    }

    #[tokio::test]
    async fn mock_backend_capabilities_count() {
        let b = MockBackend;
        assert_eq!(b.capabilities().len(), 6);
    }

    #[tokio::test]
    async fn backend_trait_is_object_safe() {
        let b: Box<dyn Backend> = Box::new(MockBackend);
        assert_eq!(b.identity().id, "mock");
    }

    #[tokio::test]
    async fn backend_trait_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockBackend>();
    }
}

// ============================================================================
// Module: MockBackend Execute
// ============================================================================
mod mock_backend_execute {
    use super::*;

    #[tokio::test]
    async fn run_returns_receipt() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn run_receipt_has_hash() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn run_receipt_hash_length() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[tokio::test]
    async fn run_receipt_contract_version() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn run_receipt_backend_id_is_mock() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }

    #[tokio::test]
    async fn run_receipt_work_order_id_matches() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let wo_id = wo.id;
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.meta.work_order_id, wo_id);
    }

    #[tokio::test]
    async fn run_receipt_run_id_matches() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let run_id = Uuid::new_v4();
        let receipt = b.run(run_id, wo, tx).await.unwrap();
        assert_eq!(receipt.meta.run_id, run_id);
    }

    #[tokio::test]
    async fn run_receipt_timestamps_valid() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert!(receipt.meta.finished_at >= receipt.meta.started_at);
    }

    #[tokio::test]
    async fn run_receipt_mode_defaults_to_mapped() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[tokio::test]
    async fn run_receipt_usage_zero_tokens() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.usage.input_tokens, Some(0));
        assert_eq!(receipt.usage.output_tokens, Some(0));
    }

    #[tokio::test]
    async fn run_receipt_usage_zero_cost() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
    }

    #[tokio::test]
    async fn run_receipt_verification_harness_ok() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert!(receipt.verification.harness_ok);
    }

    #[tokio::test]
    async fn run_receipt_empty_artifacts() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert!(receipt.artifacts.is_empty());
    }

    #[tokio::test]
    async fn run_receipt_trace_has_events() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert!(receipt.trace.len() >= 3);
    }

    #[tokio::test]
    async fn run_receipt_capabilities_match_backend() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.capabilities.len(), b.capabilities().len());
    }
}

// ============================================================================
// Module: Agent Event Streaming
// ============================================================================
mod agent_event_streaming {
    use super::*;

    #[tokio::test]
    async fn events_stream_to_channel() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn first_event_is_run_started() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    }

    #[tokio::test]
    async fn last_event_is_run_completed() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        assert!(matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[tokio::test]
    async fn events_contain_assistant_messages() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        let msg_count = events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
            .count();
        assert!(msg_count >= 1);
    }

    #[tokio::test]
    async fn events_have_timestamps() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let before = Utc::now();
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        for ev in &events {
            assert!(ev.ts >= before);
        }
    }

    #[tokio::test]
    async fn events_timestamps_non_decreasing() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        for w in events.windows(2) {
            assert!(w[1].ts >= w[0].ts);
        }
    }

    #[tokio::test]
    async fn events_ext_is_none() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        for ev in &events {
            assert!(ev.ext.is_none());
        }
    }

    #[tokio::test]
    async fn run_started_contains_task_name() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("my special task");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        if let AgentEventKind::RunStarted { message } = &events[0].kind {
            assert!(message.contains("my special task"));
        } else {
            panic!("expected RunStarted");
        }
    }

    #[tokio::test]
    async fn mock_emits_four_events() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        assert_eq!(events.len(), 4);
    }

    #[tokio::test]
    async fn trace_matches_streamed_events() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("hello");
        let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        let events = collect_events(rx).await;
        assert_eq!(receipt.trace.len(), events.len());
    }
}

// ============================================================================
// Module: BackendRegistry
// ============================================================================
mod backend_registry_tests {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = BackendRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_and_lookup_metadata() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("openai", metadata("openai", "openai"));
        assert!(reg.contains("openai"));
        let m = reg.metadata("openai").unwrap();
        assert_eq!(m.name, "openai");
    }

    #[test]
    fn register_multiple_backends() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("a", metadata("a", "openai"));
        reg.register_with_metadata("b", metadata("b", "anthropic"));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn list_returns_sorted_names() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("zeta", metadata("zeta", "z"));
        reg.register_with_metadata("alpha", metadata("alpha", "a"));
        let list = reg.list();
        assert_eq!(list, vec!["alpha", "zeta"]);
    }

    #[test]
    fn remove_backend() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("test", metadata("test", "x"));
        let removed = reg.remove("test");
        assert!(removed.is_some());
        assert!(!reg.contains("test"));
        assert!(reg.is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut reg = BackendRegistry::new();
        assert!(reg.remove("ghost").is_none());
    }

    #[test]
    fn contains_false_for_missing() {
        let reg = BackendRegistry::new();
        assert!(!reg.contains("nope"));
    }

    #[test]
    fn metadata_none_for_missing() {
        let reg = BackendRegistry::new();
        assert!(reg.metadata("nope").is_none());
    }

    #[test]
    fn health_none_for_unregistered() {
        let reg = BackendRegistry::new();
        assert!(reg.health("nope").is_none());
    }

    #[test]
    fn register_creates_default_health() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("x", metadata("x", "d"));
        let h = reg.health("x").unwrap();
        assert_eq!(h.status, HealthStatus::Unknown);
    }

    #[test]
    fn update_health() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("x", metadata("x", "d"));
        reg.update_health(
            "x",
            BackendHealth {
                status: HealthStatus::Healthy,
                last_check: Some(Utc::now()),
                latency_ms: Some(42),
                error_rate: 0.0,
                consecutive_failures: 0,
            },
        );
        let h = reg.health("x").unwrap();
        assert_eq!(h.status, HealthStatus::Healthy);
        assert_eq!(h.latency_ms, Some(42));
    }

    #[test]
    fn healthy_backends_filter() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("a", metadata("a", "x"));
        reg.register_with_metadata("b", metadata("b", "x"));
        reg.update_health(
            "a",
            BackendHealth {
                status: HealthStatus::Healthy,
                ..Default::default()
            },
        );
        reg.update_health(
            "b",
            BackendHealth {
                status: HealthStatus::Unhealthy,
                ..Default::default()
            },
        );
        let healthy = reg.healthy_backends();
        assert_eq!(healthy, vec!["a"]);
    }

    #[test]
    fn by_dialect_filter() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("a", metadata("a", "openai"));
        reg.register_with_metadata("b", metadata("b", "anthropic"));
        reg.register_with_metadata("c", metadata("c", "openai"));
        let openai = reg.by_dialect("openai");
        assert_eq!(openai, vec!["a", "c"]);
    }

    #[test]
    fn by_dialect_empty_for_unknown() {
        let reg = BackendRegistry::new();
        assert!(reg.by_dialect("unknown").is_empty());
    }

    #[test]
    fn register_replaces_previous() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("x", metadata("x", "old"));
        reg.register_with_metadata("x", metadata("x", "new"));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.metadata("x").unwrap().dialect, "new");
    }

    #[test]
    fn metadata_fields() {
        let m = BackendMetadata {
            name: "test".into(),
            dialect: "openai".into(),
            version: "2.0".into(),
            max_tokens: Some(128_000),
            supports_streaming: true,
            supports_tools: false,
            rate_limit: Some(RateLimit {
                requests_per_minute: 60,
                tokens_per_minute: 100_000,
                concurrent_requests: 5,
            }),
        };
        assert_eq!(m.max_tokens, Some(128_000));
        assert!(m.supports_streaming);
        assert!(!m.supports_tools);
        let rl = m.rate_limit.unwrap();
        assert_eq!(rl.requests_per_minute, 60);
        assert_eq!(rl.tokens_per_minute, 100_000);
        assert_eq!(rl.concurrent_requests, 5);
    }
}

// ============================================================================
// Module: Health Status
// ============================================================================
mod health_status_tests {
    use super::*;

    #[test]
    fn default_health_status_is_unknown() {
        let h = BackendHealth::default();
        assert_eq!(h.status, HealthStatus::Unknown);
    }

    #[test]
    fn default_health_no_check() {
        let h = BackendHealth::default();
        assert!(h.last_check.is_none());
        assert!(h.latency_ms.is_none());
    }

    #[test]
    fn default_health_zero_errors() {
        let h = BackendHealth::default();
        assert_eq!(h.error_rate, 0.0);
        assert_eq!(h.consecutive_failures, 0);
    }

    #[test]
    fn health_status_variants_exist() {
        let _h = HealthStatus::Healthy;
        let _d = HealthStatus::Degraded;
        let _u = HealthStatus::Unhealthy;
        let _k = HealthStatus::Unknown;
    }

    #[test]
    fn health_status_equality() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Degraded);
    }

    #[test]
    fn health_status_default_is_unknown() {
        let s: HealthStatus = Default::default();
        assert_eq!(s, HealthStatus::Unknown);
    }
}

// ============================================================================
// Module: SidecarBackend Interface
// ============================================================================
mod sidecar_backend_interface {
    use super::*;

    #[test]
    fn sidecar_backend_new() {
        let spec = SidecarSpec::new("node");
        let sb = SidecarBackend::new(spec);
        assert_eq!(sb.spec.command, "node");
    }

    #[test]
    fn sidecar_backend_identity_id() {
        let sb = SidecarBackend::new(SidecarSpec::new("python"));
        assert_eq!(sb.identity().id, "sidecar");
    }

    #[test]
    fn sidecar_backend_identity_adapter_version() {
        let sb = SidecarBackend::new(SidecarSpec::new("python"));
        assert_eq!(sb.identity().adapter_version.as_deref(), Some("0.1"));
    }

    #[test]
    fn sidecar_backend_identity_no_backend_version() {
        let sb = SidecarBackend::new(SidecarSpec::new("python"));
        assert!(sb.identity().backend_version.is_none());
    }

    #[test]
    fn sidecar_backend_empty_capabilities() {
        let sb = SidecarBackend::new(SidecarSpec::new("x"));
        assert!(sb.capabilities().is_empty());
    }

    #[test]
    fn sidecar_backend_clone() {
        let sb = SidecarBackend::new(SidecarSpec::new("node"));
        let sb2 = sb.clone();
        assert_eq!(sb2.spec.command, "node");
    }

    #[test]
    fn sidecar_spec_with_args() {
        let mut spec = SidecarSpec::new("node");
        spec.args = vec!["script.js".to_string()];
        let sb = SidecarBackend::new(spec);
        assert_eq!(sb.spec.args, vec!["script.js"]);
    }

    #[test]
    fn sidecar_spec_with_env() {
        let mut spec = SidecarSpec::new("python");
        spec.env.insert("API_KEY".to_string(), "secret".to_string());
        let sb = SidecarBackend::new(spec);
        assert_eq!(sb.spec.env.get("API_KEY").unwrap(), "secret");
    }

    #[test]
    fn sidecar_spec_with_cwd() {
        let mut spec = SidecarSpec::new("node");
        spec.cwd = Some("/tmp".to_string());
        let sb = SidecarBackend::new(spec);
        assert_eq!(sb.spec.cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn sidecar_backend_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SidecarBackend>();
    }
}

// ============================================================================
// Module: Receipt Structures
// ============================================================================
mod receipt_structure_tests {
    use super::*;

    #[test]
    fn receipt_builder_basic() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.backend.id, "test");
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_builder_with_hash() {
        let r = ReceiptBuilder::new("test").with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_builder_outcome() {
        let r = ReceiptBuilder::new("x").outcome(Outcome::Failed).build();
        assert_eq!(r.outcome, Outcome::Failed);
    }

    #[test]
    fn receipt_builder_partial_outcome() {
        let r = ReceiptBuilder::new("x").outcome(Outcome::Partial).build();
        assert_eq!(r.outcome, Outcome::Partial);
    }

    #[test]
    fn receipt_builder_mode_passthrough() {
        let r = ReceiptBuilder::new("x")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn receipt_builder_backend_version() {
        let r = ReceiptBuilder::new("x").backend_version("1.2.3").build();
        assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn receipt_builder_adapter_version() {
        let r = ReceiptBuilder::new("x").adapter_version("0.5").build();
        assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5"));
    }

    #[test]
    fn receipt_builder_work_order_id() {
        let id = Uuid::new_v4();
        let r = ReceiptBuilder::new("x").work_order_id(id).build();
        assert_eq!(r.meta.work_order_id, id);
    }

    #[test]
    fn receipt_builder_add_trace_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "test".into(),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("x").add_trace_event(ev).build();
        assert_eq!(r.trace.len(), 1);
    }

    #[test]
    fn receipt_builder_add_artifact() {
        let artifact = ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        };
        let r = ReceiptBuilder::new("x").add_artifact(artifact).build();
        assert_eq!(r.artifacts.len(), 1);
        assert_eq!(r.artifacts[0].kind, "patch");
    }

    #[test]
    fn receipt_builder_contract_version() {
        let r = ReceiptBuilder::new("x").build();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_hash_deterministic() {
        let r = ReceiptBuilder::new("x").outcome(Outcome::Complete).build();
        let h1 = r.clone().with_hash().unwrap().receipt_sha256.unwrap();
        let h2 = r.with_hash().unwrap().receipt_sha256.unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_hash_changes_with_outcome() {
        let r1 = ReceiptBuilder::new("x")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let r2 = ReceiptBuilder::new("x")
            .outcome(Outcome::Failed)
            .with_hash()
            .unwrap();
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }
}

// ============================================================================
// Module: Error Handling & ErrorCode Classification
// ============================================================================
mod error_code_tests {
    use super::*;

    #[test]
    fn error_code_as_str_snake_case() {
        assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    }

    #[test]
    fn error_code_backend_not_found() {
        assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    }

    #[test]
    fn error_code_backend_unavailable() {
        assert_eq!(
            ErrorCode::BackendUnavailable.as_str(),
            "backend_unavailable"
        );
    }

    #[test]
    fn error_code_backend_rate_limited() {
        assert_eq!(
            ErrorCode::BackendRateLimited.as_str(),
            "backend_rate_limited"
        );
    }

    #[test]
    fn error_code_backend_auth_failed() {
        assert_eq!(ErrorCode::BackendAuthFailed.as_str(), "backend_auth_failed");
    }

    #[test]
    fn error_code_backend_crashed() {
        assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
    }

    #[test]
    fn error_code_backend_model_not_found() {
        assert_eq!(
            ErrorCode::BackendModelNotFound.as_str(),
            "backend_model_not_found"
        );
    }

    #[test]
    fn error_code_internal() {
        assert_eq!(ErrorCode::Internal.as_str(), "internal");
    }

    #[test]
    fn error_code_policy_denied() {
        assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    }

    #[test]
    fn error_code_capability_unsupported() {
        assert_eq!(
            ErrorCode::CapabilityUnsupported.as_str(),
            "capability_unsupported"
        );
    }

    #[test]
    fn error_code_category_backend() {
        assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
        assert_eq!(
            ErrorCode::BackendNotFound.category(),
            ErrorCategory::Backend
        );
        assert_eq!(
            ErrorCode::BackendUnavailable.category(),
            ErrorCategory::Backend
        );
        assert_eq!(
            ErrorCode::BackendRateLimited.category(),
            ErrorCategory::Backend
        );
        assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    }

    #[test]
    fn error_code_category_protocol() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolHandshakeFailed.category(),
            ErrorCategory::Protocol
        );
    }

    #[test]
    fn error_code_category_policy() {
        assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
        assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
    }

    #[test]
    fn error_code_category_capability() {
        assert_eq!(
            ErrorCode::CapabilityUnsupported.category(),
            ErrorCategory::Capability
        );
    }

    #[test]
    fn error_code_category_workspace() {
        assert_eq!(
            ErrorCode::WorkspaceInitFailed.category(),
            ErrorCategory::Workspace
        );
        assert_eq!(
            ErrorCode::WorkspaceStagingFailed.category(),
            ErrorCategory::Workspace
        );
    }

    #[test]
    fn error_code_retryable() {
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendCrashed.is_retryable());
    }

    #[test]
    fn error_code_not_retryable() {
        assert!(!ErrorCode::BackendNotFound.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::Internal.is_retryable());
        assert!(!ErrorCode::ConfigInvalid.is_retryable());
    }

    #[test]
    fn error_code_message_not_empty() {
        let codes = [
            ErrorCode::BackendTimeout,
            ErrorCode::BackendNotFound,
            ErrorCode::PolicyDenied,
            ErrorCode::Internal,
            ErrorCode::CapabilityUnsupported,
        ];
        for code in codes {
            assert!(!code.message().is_empty());
        }
    }

    #[test]
    fn error_code_display_is_message() {
        let code = ErrorCode::BackendTimeout;
        assert_eq!(format!("{}", code), code.message());
    }

    #[test]
    fn error_info_construction() {
        let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
        assert_eq!(info.code, ErrorCode::BackendTimeout);
        assert_eq!(info.message, "timed out");
        assert!(info.is_retryable);
    }

    #[test]
    fn error_info_with_detail() {
        let info =
            ErrorInfo::new(ErrorCode::BackendTimeout, "timeout").with_detail("backend", "openai");
        assert_eq!(info.details["backend"], json!("openai"));
    }

    #[test]
    fn error_info_not_retryable() {
        let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
        assert!(!info.is_retryable);
    }

    #[test]
    fn abp_error_construction() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        assert_eq!(err.message, "timed out");
    }

    #[test]
    fn abp_error_with_context() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
            .with_context("backend", "openai")
            .with_context("timeout_ms", 30_000);
        assert_eq!(err.context.len(), 2);
    }

    #[test]
    fn abp_error_category() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "test");
        assert_eq!(err.category(), ErrorCategory::Backend);
    }

    #[test]
    fn error_code_mapping_category() {
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingLossyConversion.category(),
            ErrorCategory::Mapping
        );
    }

    #[test]
    fn error_code_receipt_category() {
        assert_eq!(
            ErrorCode::ReceiptHashMismatch.category(),
            ErrorCategory::Receipt
        );
        assert_eq!(
            ErrorCode::ReceiptChainBroken.category(),
            ErrorCategory::Receipt
        );
    }

    #[test]
    fn error_code_dialect_category() {
        assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
        assert_eq!(
            ErrorCode::DialectMappingFailed.category(),
            ErrorCategory::Dialect
        );
    }

    #[test]
    fn error_code_ir_category() {
        assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
        assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
    }

    #[test]
    fn error_code_execution_category() {
        assert_eq!(
            ErrorCode::ExecutionToolFailed.category(),
            ErrorCategory::Execution
        );
        assert_eq!(
            ErrorCode::ExecutionPermissionDenied.category(),
            ErrorCategory::Execution
        );
    }

    #[test]
    fn error_code_config_category() {
        assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    }

    #[test]
    fn error_code_contract_category() {
        assert_eq!(
            ErrorCode::ContractVersionMismatch.category(),
            ErrorCategory::Contract
        );
        assert_eq!(
            ErrorCode::ContractSchemaViolation.category(),
            ErrorCategory::Contract
        );
    }
}

// ============================================================================
// Module: Capability Reporting
// ============================================================================
mod capability_reporting {
    use super::*;

    #[test]
    fn support_level_native_satisfies_native() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_level_native_satisfies_emulated() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_emulated_does_not_satisfy_native() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_level_emulated_satisfies_emulated() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_unsupported_satisfies_nothing() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_restricted_satisfies_emulated() {
        let restricted = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(restricted.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_restricted_not_satisfies_native() {
        let restricted = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(!restricted.satisfies(&MinSupport::Native));
    }

    #[tokio::test]
    async fn ensure_requirements_empty_ok() {
        let caps = MockBackend.capabilities();
        let reqs = CapabilityRequirements { required: vec![] };
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[tokio::test]
    async fn ensure_requirements_satisfied() {
        let caps = MockBackend.capabilities();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[tokio::test]
    async fn ensure_requirements_emulated_accepted() {
        let caps = MockBackend.capabilities();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }

    #[tokio::test]
    async fn ensure_requirements_unsatisfied_fails() {
        let caps = MockBackend.capabilities();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Vision,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[tokio::test]
    async fn ensure_requirements_native_not_met_by_emulated() {
        let caps = MockBackend.capabilities();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_err());
    }

    #[tokio::test]
    async fn run_fails_with_unsatisfied_requirements() {
        let b = MockBackend;
        let (tx, _rx) = mpsc::channel(32);
        let wo = work_order_with_requirements(vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }]);
        let result = b.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_err());
    }
}

// ============================================================================
// Module: Backend Config & Execution Mode
// ============================================================================
mod backend_config_metadata {
    use super::*;

    #[test]
    fn extract_mode_default_is_mapped() {
        let wo = simple_work_order("test");
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
    }

    #[test]
    fn extract_mode_nested_passthrough() {
        let wo = work_order_passthrough();
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
    }

    #[test]
    fn extract_mode_flat_passthrough() {
        let wo = work_order_passthrough_flat();
        assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
    }

    #[test]
    fn validate_passthrough_ok() {
        let wo = simple_work_order("test");
        assert!(validate_passthrough_compatibility(&wo).is_ok());
    }

    #[test]
    fn execution_mode_default() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn work_order_builder_model() {
        let wo = work_order_with_model("gpt-4");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn work_order_builder_max_turns() {
        let wo = WorkOrderBuilder::new("t").max_turns(5).build();
        assert_eq!(wo.config.max_turns, Some(5));
    }

    #[test]
    fn work_order_builder_max_budget() {
        let wo = WorkOrderBuilder::new("t").max_budget_usd(10.0).build();
        assert_eq!(wo.config.max_budget_usd, Some(10.0));
    }

    #[test]
    fn work_order_builder_lane() {
        let wo = WorkOrderBuilder::new("t")
            .lane(ExecutionLane::WorkspaceFirst)
            .build();
        assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    }

    #[test]
    fn work_order_builder_root() {
        let wo = WorkOrderBuilder::new("t").root("/custom").build();
        assert_eq!(wo.workspace.root, "/custom");
    }
}

// ============================================================================
// Module: ScenarioMockBackend
// ============================================================================
mod scenario_mock_tests {
    use super::*;

    #[tokio::test]
    async fn scenario_success() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "done".into(),
        });
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("task");
        let receipt = sb.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        let events = collect_events(rx).await;
        assert!(events.iter().any(
            |e| matches!(&e.kind, AgentEventKind::AssistantMessage { text } if text == "done")
        ));
    }

    #[tokio::test]
    async fn scenario_streaming_success() {
        let sb = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
            chunks: vec!["a".into(), "b".into(), "c".into()],
            chunk_delay_ms: 0,
        });
        let (tx, rx) = mpsc::channel(32);
        let wo = simple_work_order("streaming");
        let receipt = sb.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        let events = collect_events(rx).await;
        let deltas: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
            .collect();
        assert_eq!(deltas.len(), 3);
    }

    #[tokio::test]
    async fn scenario_permanent_error() {
        let sb = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "ERR-001".into(),
            message: "fatal".into(),
        });
        let (tx, _rx) = mpsc::channel(32);
        let wo = simple_work_order("fail");
        let result = sb.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("ERR-001"));
        assert!(err_str.contains("fatal"));
    }

    #[tokio::test]
    async fn scenario_transient_then_success() {
        let sb = ScenarioMockBackend::new(MockScenario::TransientError {
            fail_count: 2,
            then: Box::new(MockScenario::Success {
                delay_ms: 0,
                text: "recovered".into(),
            }),
        });
        let (tx1, _) = mpsc::channel(32);
        let wo1 = simple_work_order("attempt1");
        assert!(sb.run(Uuid::new_v4(), wo1, tx1).await.is_err());

        let (tx2, _) = mpsc::channel(32);
        let wo2 = simple_work_order("attempt2");
        assert!(sb.run(Uuid::new_v4(), wo2, tx2).await.is_err());

        let (tx3, _) = mpsc::channel(32);
        let wo3 = simple_work_order("attempt3");
        let receipt = sb.run(Uuid::new_v4(), wo3, tx3).await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn scenario_call_count() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        });
        assert_eq!(sb.call_count(), 0);
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("x");
        let _ = sb.run(Uuid::new_v4(), wo, tx).await;
        assert_eq!(sb.call_count(), 1);
    }

    #[tokio::test]
    async fn scenario_last_error_none_on_success() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        });
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("x");
        let _ = sb.run(Uuid::new_v4(), wo, tx).await;
        assert!(sb.last_error().await.is_none());
    }

    #[tokio::test]
    async fn scenario_last_error_set_on_failure() {
        let sb = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "X".into(),
            message: "boom".into(),
        });
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("x");
        let _ = sb.run(Uuid::new_v4(), wo, tx).await;
        assert!(sb.last_error().await.is_some());
    }

    #[tokio::test]
    async fn scenario_recorded_calls() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        });
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("tracked");
        let _ = sb.run(Uuid::new_v4(), wo, tx).await;
        let calls = sb.calls().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].work_order.task, "tracked");
    }

    #[tokio::test]
    async fn scenario_last_call() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        });
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("last");
        let _ = sb.run(Uuid::new_v4(), wo, tx).await;
        let lc = sb.last_call().await.unwrap();
        assert_eq!(lc.work_order.task, "last");
        assert!(lc.result.is_ok());
    }

    #[tokio::test]
    async fn scenario_rate_limited() {
        let sb = ScenarioMockBackend::new(MockScenario::RateLimited {
            retry_after_ms: 1000,
        });
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("rl");
        let result = sb.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limited"));
    }

    #[tokio::test]
    async fn scenario_timeout() {
        let sb = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 10 });
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("to");
        let result = sb.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timeout"));
    }

    #[tokio::test]
    async fn scenario_identity() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        });
        assert_eq!(sb.identity().id, "scenario-mock");
    }

    #[tokio::test]
    async fn scenario_capabilities_match_mock() {
        let sb = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        });
        assert_eq!(sb.capabilities().len(), MockBackend.capabilities().len());
    }
}

// ============================================================================
// Module: MockBackendRecorder
// ============================================================================
mod recorder_tests {
    use super::*;

    #[tokio::test]
    async fn recorder_wraps_mock() {
        let rec = MockBackendRecorder::new(MockBackend);
        assert_eq!(rec.identity().id, "mock");
    }

    #[tokio::test]
    async fn recorder_capabilities() {
        let rec = MockBackendRecorder::new(MockBackend);
        assert_eq!(rec.capabilities().len(), MockBackend.capabilities().len());
    }

    #[tokio::test]
    async fn recorder_records_calls() {
        let rec = MockBackendRecorder::new(MockBackend);
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("recorded");
        let _ = rec.run(Uuid::new_v4(), wo, tx).await;
        assert_eq!(rec.call_count().await, 1);
    }

    #[tokio::test]
    async fn recorder_last_call() {
        let rec = MockBackendRecorder::new(MockBackend);
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("last recorded");
        let _ = rec.run(Uuid::new_v4(), wo, tx).await;
        let lc = rec.last_call().await.unwrap();
        assert_eq!(lc.work_order.task, "last recorded");
    }

    #[tokio::test]
    async fn recorder_multiple_calls() {
        let rec = MockBackendRecorder::new(MockBackend);
        for i in 0..3 {
            let (tx, _) = mpsc::channel(32);
            let wo = simple_work_order(&format!("call-{i}"));
            let _ = rec.run(Uuid::new_v4(), wo, tx).await;
        }
        assert_eq!(rec.call_count().await, 3);
        let calls = rec.calls().await;
        assert_eq!(calls[0].work_order.task, "call-0");
        assert_eq!(calls[2].work_order.task, "call-2");
    }

    #[tokio::test]
    async fn recorder_call_result_ok() {
        let rec = MockBackendRecorder::new(MockBackend);
        let (tx, _) = mpsc::channel(32);
        let wo = simple_work_order("ok");
        let _ = rec.run(Uuid::new_v4(), wo, tx).await;
        let calls = rec.calls().await;
        assert!(calls[0].result.is_ok());
        assert_eq!(calls[0].result.as_ref().unwrap(), &Outcome::Complete);
    }
}

// ============================================================================
// Module: Concurrent Backend Execution
// ============================================================================
mod concurrent_execution {
    use super::*;

    #[tokio::test]
    async fn parallel_mock_runs() {
        let b = Arc::new(MockBackend);
        let mut handles = Vec::new();
        for _ in 0..10 {
            let b = Arc::clone(&b);
            handles.push(tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(32);
                let wo = simple_work_order("parallel");
                b.run(Uuid::new_v4(), wo, tx).await.unwrap()
            }));
        }
        let mut receipts = Vec::new();
        for h in handles {
            receipts.push(h.await.unwrap());
        }
        assert_eq!(receipts.len(), 10);
        for r in &receipts {
            assert_eq!(r.outcome, Outcome::Complete);
        }
    }

    #[tokio::test]
    async fn parallel_runs_unique_run_ids() {
        let b = Arc::new(MockBackend);
        let mut handles = Vec::new();
        for _ in 0..5 {
            let b = Arc::clone(&b);
            handles.push(tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(32);
                let wo = simple_work_order("unique");
                let run_id = Uuid::new_v4();
                let receipt = b.run(run_id, wo, tx).await.unwrap();
                receipt.meta.run_id
            }));
        }
        let mut ids = Vec::new();
        for h in handles {
            ids.push(h.await.unwrap());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }

    #[tokio::test]
    async fn concurrent_scenario_mock() {
        let sb = Arc::new(ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "concurrent".into(),
        }));
        let mut handles = Vec::new();
        for _ in 0..5 {
            let sb = Arc::clone(&sb);
            handles.push(tokio::spawn(async move {
                let (tx, _) = mpsc::channel(32);
                let wo = simple_work_order("c");
                sb.run(Uuid::new_v4(), wo, tx).await.unwrap()
            }));
        }
        for h in handles {
            let r = h.await.unwrap();
            assert_eq!(r.outcome, Outcome::Complete);
        }
        assert_eq!(sb.call_count(), 5);
    }

    #[tokio::test]
    async fn concurrent_registry_no_conflict() {
        let mut reg = BackendRegistry::new();
        for i in 0..10 {
            reg.register_with_metadata(
                &format!("backend-{i}"),
                metadata(&format!("backend-{i}"), "test"),
            );
        }
        assert_eq!(reg.len(), 10);
        assert_eq!(reg.list().len(), 10);
    }
}

// ============================================================================
// Module: Backend Cleanup / Shutdown
// ============================================================================
mod backend_cleanup {
    use super::*;

    #[tokio::test]
    async fn channel_closes_after_run() {
        let b = MockBackend;
        let (tx, mut rx) = mpsc::channel(32);
        let wo = simple_work_order("cleanup");
        let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
        // Drain all events
        let mut count = 0;
        while let Some(_) = rx.recv().await {
            count += 1;
        }
        // Channel should be closed (sender dropped)
        assert!(rx.recv().await.is_none());
        assert!(count > 0);
    }

    #[tokio::test]
    async fn registry_remove_cleans_health() {
        let mut reg = BackendRegistry::new();
        reg.register_with_metadata("x", metadata("x", "d"));
        reg.update_health(
            "x",
            BackendHealth {
                status: HealthStatus::Healthy,
                ..Default::default()
            },
        );
        reg.remove("x");
        assert!(reg.health("x").is_none());
    }

    #[tokio::test]
    async fn multiple_runs_same_backend() {
        let b = MockBackend;
        for _ in 0..5 {
            let (tx, _) = mpsc::channel(32);
            let wo = simple_work_order("reuse");
            let r = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
            assert_eq!(r.outcome, Outcome::Complete);
        }
    }

    #[tokio::test]
    async fn drop_receiver_does_not_panic() {
        let b = MockBackend;
        let (tx, rx) = mpsc::channel(1);
        drop(rx); // Drop receiver before run
        let wo = simple_work_order("orphan");
        // Should not panic even though receiver is gone
        let result = b.run(Uuid::new_v4(), wo, tx).await;
        assert!(result.is_ok());
    }
}

// ============================================================================
// Module: AgentEventKind variants
// ============================================================================
mod agent_event_kind_tests {
    use super::*;

    #[test]
    fn run_started_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        };
        assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
    }

    #[test]
    fn run_completed_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
    }

    #[test]
    fn assistant_delta_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        };
        assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    }

    #[test]
    fn tool_call_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "file.rs"}),
            },
            ext: None,
        };
        if let AgentEventKind::ToolCall { tool_name, .. } = &ev.kind {
            assert_eq!(tool_name, "read");
        }
    }

    #[test]
    fn tool_result_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                output: json!("file contents"),
                is_error: false,
            },
            ext: None,
        };
        if let AgentEventKind::ToolResult { is_error, .. } = &ev.kind {
            assert!(!is_error);
        }
    }

    #[test]
    fn file_changed_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            ext: None,
        };
        assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
    }

    #[test]
    fn command_executed_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        };
        if let AgentEventKind::CommandExecuted { exit_code, .. } = &ev.kind {
            assert_eq!(*exit_code, Some(0));
        }
    }

    #[test]
    fn warning_event() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "caution".into(),
            },
            ext: None,
        };
        assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
    }

    #[test]
    fn error_event_with_code() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "boom".into(),
                error_code: Some(ErrorCode::BackendTimeout),
            },
            ext: None,
        };
        if let AgentEventKind::Error { error_code, .. } = &ev.kind {
            assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
        }
    }

    #[test]
    fn error_event_without_code() {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "unknown".into(),
                error_code: None,
            },
            ext: None,
        };
        if let AgentEventKind::Error { error_code, .. } = &ev.kind {
            assert!(error_code.is_none());
        }
    }

    #[test]
    fn event_with_ext() {
        let mut ext = BTreeMap::new();
        ext.insert("raw_message".to_string(), json!({"foo": "bar"}));
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        };
        assert!(ev.ext.is_some());
        assert!(ev.ext.unwrap().contains_key("raw_message"));
    }
}

// ============================================================================
// Module: Contract Version and Serialization
// ============================================================================
mod contract_and_serde {
    use super::*;

    #[test]
    fn contract_version_value() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn outcome_serde_roundtrip() {
        let json = serde_json::to_string(&Outcome::Complete).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Outcome::Complete);
    }

    #[test]
    fn outcome_serde_failed() {
        let json = serde_json::to_string(&Outcome::Failed).unwrap();
        assert_eq!(json, "\"failed\"");
    }

    #[test]
    fn outcome_serde_partial() {
        let json = serde_json::to_string(&Outcome::Partial).unwrap();
        assert_eq!(json, "\"partial\"");
    }

    #[test]
    fn execution_mode_serde_roundtrip() {
        let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ExecutionMode::Passthrough);
    }

    #[test]
    fn execution_mode_serde_mapped() {
        let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
        assert_eq!(json, "\"mapped\"");
    }

    #[test]
    fn backend_identity_serde() {
        let id = BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        };
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json["id"], "test");
        assert_eq!(json["backend_version"], "1.0");
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let code = ErrorCode::BackendTimeout;
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }

    #[test]
    fn error_code_serializes_snake_case() {
        let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
        assert_eq!(json, "\"backend_timeout\"");
    }
}
