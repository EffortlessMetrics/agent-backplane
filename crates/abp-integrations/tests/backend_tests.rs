use abp_core::{
    AgentEventKind, Capability, CapabilityManifest, CapabilityRequirements, ExecutionMode,
    SupportLevel,
};
use abp_integrations::{
    ensure_capability_requirements, extract_execution_mode, validate_passthrough_compatibility,
    Backend, MockBackend,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

fn test_work_order() -> abp_core::WorkOrder {
    abp_core::WorkOrder {
        id: Uuid::nil(),
        task: "test task".into(),
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
    }
}

// 1. MockBackend::identity
#[test]
fn mock_backend_identity() {
    let backend = MockBackend;
    let id = backend.identity();
    assert_eq!(id.id, "mock");
    assert_eq!(id.backend_version.as_deref(), Some("0.1"));
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

// 2. MockBackend::capabilities
#[test]
fn mock_backend_capabilities() {
    let backend = MockBackend;
    let caps = backend.capabilities();

    assert!(matches!(caps.get(&Capability::Streaming), Some(SupportLevel::Native)));
    assert!(matches!(caps.get(&Capability::ToolRead), Some(SupportLevel::Emulated)));
    assert!(matches!(caps.get(&Capability::ToolWrite), Some(SupportLevel::Emulated)));
    assert!(matches!(caps.get(&Capability::ToolEdit), Some(SupportLevel::Emulated)));
    assert!(matches!(caps.get(&Capability::ToolBash), Some(SupportLevel::Emulated)));
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

// 3. MockBackend::run produces receipt
#[tokio::test]
async fn mock_backend_run_produces_receipt() {
    let backend = MockBackend;
    let wo = test_work_order();
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);

    let receipt = backend.run(run_id, wo.clone(), tx).await.unwrap();

    assert_eq!(receipt.meta.run_id, run_id);
    assert_eq!(receipt.meta.work_order_id, wo.id);
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    assert!(receipt.receipt_sha256.is_some());
}

// 4. MockBackend::run streams events
#[tokio::test]
async fn mock_backend_run_streams_events() {
    let backend = MockBackend;
    let wo = test_work_order();
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(16);

    let _receipt = backend.run(run_id, wo, tx).await.unwrap();

    let mut kinds = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        kinds.push(ev.kind);
    }

    assert!(
        kinds.iter().any(|k| matches!(k, AgentEventKind::RunStarted { .. })),
        "expected RunStarted event"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, AgentEventKind::AssistantMessage { .. })),
        "expected AssistantMessage event"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, AgentEventKind::RunCompleted { .. })),
        "expected RunCompleted event"
    );
}

// 5. MockBackend::run includes trace in receipt
#[tokio::test]
async fn mock_backend_run_includes_trace() {
    let backend = MockBackend;
    let wo = test_work_order();
    let (tx, _rx) = mpsc::channel(16);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    assert!(!receipt.trace.is_empty(), "receipt trace should be non-empty");
}

// 6. extract_execution_mode defaults to Mapped
#[test]
fn extract_execution_mode_defaults_to_mapped() {
    let wo = test_work_order();
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

// 7. extract_execution_mode nested abp.mode
#[test]
fn extract_execution_mode_nested_key() {
    let mut wo = test_work_order();
    wo.config.vendor.insert(
        "abp".to_string(),
        json!({"mode": "passthrough"}),
    );
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

// 8. extract_execution_mode dotted key
#[test]
fn extract_execution_mode_dotted_key() {
    let mut wo = test_work_order();
    wo.config
        .vendor
        .insert("abp.mode".to_string(), json!("passthrough"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

// 9. validate_passthrough_compatibility
#[test]
fn validate_passthrough_compatibility_ok() {
    let wo = test_work_order();
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

// 10. ensure_capability_requirements with empty reqs
#[test]
fn ensure_capability_requirements_empty_reqs() {
    let reqs = CapabilityRequirements::default();
    let caps = CapabilityManifest::default();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

// 11. MockBackend receipt has valid hash
#[tokio::test]
async fn mock_backend_receipt_has_valid_hash() {
    let backend = MockBackend;
    let wo = test_work_order();
    let (tx, _rx) = mpsc::channel(16);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    let stored_hash = receipt.receipt_sha256.clone().expect("hash should be present");
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored_hash, recomputed, "stored hash should match recomputed hash");
}
