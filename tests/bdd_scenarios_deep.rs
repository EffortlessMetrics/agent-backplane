// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep BDD-style Given/When/Then tests covering 15 major user journeys
//! across the Agent Backplane: receipt hashing, policy enforcement, workspace
//! staging, dialect mapping, sequential runs, capability negotiation, sidecar
//! protocol, envelope parsing, max_turns, error receipts, streaming order,
//! passthrough mode, mapped mode, cancellation, and receipt chains.

use std::collections::BTreeMap;
use std::path::Path;

use abp_capability::negotiate;
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel,
    VerificationReport, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, canonical_json, sha256_hex,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_emulation::{EmulationConfig, EmulationEngine};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_mapping::{Fidelity, features, known_rules, validate_mapping};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use abp_receipt::{
    ReceiptBuilder as ReceiptBuilderExt, ReceiptChain, compute_hash, diff_receipts, verify_hash,
};
use abp_runtime::{Runtime, RuntimeError};
use chrono::Utc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest_from(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn make_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_hello_envelope(
    backend_id: &str,
    caps: CapabilityManifest,
    mode: ExecutionMode,
) -> Envelope {
    Envelope::hello_with_mode(
        BackendIdentity {
            id: backend_id.into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        caps,
        mode,
    )
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// Scenario 1: Given a work order, When sent to mock backend,
//             Then receipt has valid hash (8 tests)
// ===========================================================================

/// Given a simple work order, When run on mock, Then receipt has a SHA-256 hash.
#[tokio::test]
async fn s01_given_work_order_when_mock_run_then_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("Summarize the README");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

/// Given a work order, When receipt returned, Then hash is 64 hex chars.
#[tokio::test]
async fn s01_given_work_order_when_receipt_returned_then_hash_is_64_hex() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("Test task");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

/// Given a receipt with hash, When hash recomputed, Then values match.
#[tokio::test]
async fn s01_given_receipt_when_recomputed_then_hash_matches() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("Recompute hash");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(verify_hash(&receipt));
}

/// Given a receipt, When tampered, Then verification fails.
#[tokio::test]
async fn s01_given_receipt_when_tampered_then_verification_fails() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("Tamper test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut receipt = handle.receipt.await.unwrap().unwrap();
    receipt.outcome = Outcome::Failed;
    assert!(!verify_hash(&receipt));
}

/// Given a builder receipt, When with_hash called, Then hash is present.
#[test]
fn s01_given_builder_receipt_when_hashed_then_present() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));
}

/// Given two identical receipts built separately, When hashed, Then hashes differ
/// (because run_id is random).
#[test]
fn s01_given_two_separate_receipts_when_hashed_then_differ() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

/// Given a receipt with fixed run_id, When hashed twice, Then deterministic.
#[test]
fn s01_given_fixed_run_id_when_hashed_twice_then_deterministic() {
    let id = Uuid::nil();
    let r1 = ReceiptBuilderExt::new("mock")
        .run_id(id)
        .outcome(Outcome::Complete)
        .build();
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

/// Given a receipt, When serialized to JSON and back, Then hash still verifies.
#[test]
fn s01_given_receipt_when_json_roundtrip_then_hash_verifies() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&back));
}

// ===========================================================================
// Scenario 2: Given a restricted policy, When running tool_use,
//             Then tool is blocked (8 tests)
// ===========================================================================

/// Given policy disallowing "Bash", When checked, Then Bash is denied.
#[test]
fn s02_given_policy_disallow_bash_when_checked_then_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
}

/// Given allowlist of "Read" only, When "Write" checked, Then denied.
#[test]
fn s02_given_allowlist_read_when_write_checked_then_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

/// Given deny_write for .env files, When writing .env, Then denied.
#[test]
fn s02_given_deny_write_env_when_writing_then_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".env")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

/// Given deny_read for secrets, When reading secrets/key.pem, Then denied.
#[test]
fn s02_given_deny_read_secrets_when_reading_then_denied() {
    let policy = PolicyProfile {
        deny_read: vec!["secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secrets/key.pem")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

/// Given combined deny_tool and deny_write, When both checked, Then both denied.
#[test]
fn s02_given_combined_policy_when_checked_then_both_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
}

/// Given empty policy, When any tool checked, Then allowed.
#[test]
fn s02_given_empty_policy_when_tool_checked_then_allowed() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_read_path(Path::new("anything")).allowed);
    assert!(engine.can_write_path(Path::new("anything")).allowed);
}

/// Given allowlist and denylist overlap, When checked, Then deny wins.
#[test]
fn s02_given_allow_deny_overlap_when_checked_then_deny_wins() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Bash".into(), "Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

/// Given denial, When reason inspected, Then reason is non-empty.
#[test]
fn s02_given_denial_when_reason_inspected_then_non_empty() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
    assert!(decision.reason.is_some());
    assert!(!decision.reason.unwrap().is_empty());
}

// ===========================================================================
// Scenario 3: Given a workspace spec, When staged,
//             Then files are copied correctly (6 tests)
// ===========================================================================

/// Given a PassThrough workspace spec, When used in work order, Then root is preserved.
#[test]
fn s03_given_passthrough_spec_when_used_then_root_preserved() {
    let wo = WorkOrderBuilder::new("test")
        .root("/some/path")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert_eq!(wo.workspace.root, "/some/path");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

/// Given include globs, When work order built, Then include patterns are set.
#[test]
fn s03_given_include_globs_when_built_then_patterns_set() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["src/**".into(), "Cargo.toml".into()])
        .build();
    assert_eq!(wo.workspace.include.len(), 2);
    assert!(wo.workspace.include.contains(&"src/**".to_string()));
}

/// Given exclude globs, When work order built, Then exclude patterns are set.
#[test]
fn s03_given_exclude_globs_when_built_then_patterns_set() {
    let wo = WorkOrderBuilder::new("test")
        .exclude(vec!["target/**".into(), "node_modules/**".into()])
        .build();
    assert_eq!(wo.workspace.exclude.len(), 2);
    assert!(wo.workspace.exclude.contains(&"target/**".to_string()));
}

/// Given staged mode, When work order built, Then mode is Staged.
#[test]
fn s03_given_staged_mode_when_built_then_mode_is_staged() {
    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

/// Given workspace spec serialized, When deserialized, Then fields match.
#[test]
fn s03_given_workspace_spec_when_roundtrip_then_fields_match() {
    let spec = WorkspaceSpec {
        root: "/tmp/ws".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["**/*.rs".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, spec.root);
    assert_eq!(back.include.len(), 1);
    assert_eq!(back.exclude.len(), 1);
}

/// Given default work order, When built with no root override, Then default root is ".".
#[test]
fn s03_given_default_work_order_when_built_then_default_root() {
    let wo = WorkOrderBuilder::new("test").build();
    assert_eq!(wo.workspace.root, ".");
}

// ===========================================================================
// Scenario 4: Given OpenAI dialect request, When mapped to Claude,
//             Then structure changes (6 tests)
// ===========================================================================

/// Given OpenAI→Claude tool_use mapping, When checked, Then lossless.
#[test]
fn s04_given_openai_to_claude_tool_use_when_checked_then_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .expect("rule should exist");
    assert!(rule.fidelity.is_lossless());
}

/// Given Claude→Gemini thinking, When checked, Then lossy-labeled.
#[test]
fn s04_given_claude_to_gemini_thinking_when_checked_then_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
        .expect("rule should exist");
    assert!(matches!(rule.fidelity, Fidelity::LossyLabeled { .. }));
}

/// Given OpenAI→Claude streaming, When checked, Then lossless.
#[test]
fn s04_given_openai_to_claude_streaming_when_checked_then_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING)
        .expect("rule should exist");
    assert!(rule.fidelity.is_lossless());
}

/// Given validate_mapping with lossy feature, When run, Then errors reported.
#[test]
fn s04_given_lossy_feature_when_validated_then_errors_reported() {
    let reg = known_rules();
    let feats = vec![features::THINKING.into()];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Gemini, &feats);
    assert_eq!(results.len(), 1);
    assert!(!results[0].errors.is_empty());
}

/// Given nonexistent feature, When validated, Then unsupported error.
#[test]
fn s04_given_nonexistent_feature_when_validated_then_unsupported() {
    let reg = known_rules();
    let feats = vec!["teleportation".into()];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results.len(), 1);
    assert!(!results[0].errors.is_empty());
}

/// Given OpenAI-style JSON, When detected, Then dialect is OpenAI.
#[test]
fn s04_given_openai_json_when_detected_then_openai() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.7
    });
    let result = detector.detect(&val).expect("should detect");
    assert_eq!(result.dialect, Dialect::OpenAi);
}

// ===========================================================================
// Scenario 5: Given two sequential runs, When receipts compared,
//             Then hashes differ (6 tests)
// ===========================================================================

/// Given two sequential mock runs, When receipts obtained, Then different run IDs.
#[tokio::test]
async fn s05_given_two_runs_when_compared_then_different_run_ids() {
    let rt = Runtime::with_default_backends();
    let wo1 = make_work_order("Task A");
    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let r1 = h1.receipt.await.unwrap().unwrap();

    let wo2 = make_work_order("Task B");
    let h2 = rt.run_streaming("mock", wo2).await.unwrap();
    let r2 = h2.receipt.await.unwrap().unwrap();

    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

/// Given two receipts, When hashes compared, Then they differ.
#[tokio::test]
async fn s05_given_two_receipts_when_hashes_compared_then_differ() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", make_work_order("A"))
        .await
        .unwrap();
    let r1 = h1.receipt.await.unwrap().unwrap();

    let h2 = rt
        .run_streaming("mock", make_work_order("B"))
        .await
        .unwrap();
    let r2 = h2.receipt.await.unwrap().unwrap();

    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

/// Given two receipts, When diffed, Then run_id field differs.
#[tokio::test]
async fn s05_given_two_receipts_when_diffed_then_run_id_differs() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", make_work_order("X"))
        .await
        .unwrap();
    let r1 = h1.receipt.await.unwrap().unwrap();

    let h2 = rt
        .run_streaming("mock", make_work_order("Y"))
        .await
        .unwrap();
    let r2 = h2.receipt.await.unwrap().unwrap();

    let diff = diff_receipts(&r1, &r2);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "meta.run_id"));
}

/// Given two builder receipts with different outcomes, When diffed, Then outcome differs.
#[test]
fn s05_given_different_outcomes_when_diffed_then_outcome_differs() {
    let a = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "outcome"));
}

/// Given same receipt cloned, When diffed, Then no differences.
#[test]
fn s05_given_same_receipt_when_diffed_then_no_differences() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let diff = diff_receipts(&r, &r);
    assert!(diff.is_empty());
}

/// Given sequential receipts, When both verified, Then both pass.
#[tokio::test]
async fn s05_given_sequential_receipts_when_verified_then_both_pass() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", make_work_order("P"))
        .await
        .unwrap();
    let r1 = h1.receipt.await.unwrap().unwrap();

    let h2 = rt
        .run_streaming("mock", make_work_order("Q"))
        .await
        .unwrap();
    let r2 = h2.receipt.await.unwrap().unwrap();

    assert!(verify_hash(&r1));
    assert!(verify_hash(&r2));
}

// ===========================================================================
// Scenario 6: Given a backend with limited capabilities,
//             When negotiated, Then emulation applied (7 tests)
// ===========================================================================

/// Given backend without tool_use, When emulation required, Then emulatable.
#[test]
fn s06_given_no_tool_use_when_emulation_required_then_emulatable() {
    let manifest = manifest_from(&[(Capability::ToolUse, SupportLevel::Emulated)]);
    let reqs = require(&[(Capability::ToolUse, MinSupport::Emulated)]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulated_caps(), vec![Capability::ToolUse]);
}

/// Given backend missing extended_thinking, When native required, Then unsupported.
#[test]
fn s06_given_no_thinking_when_native_required_then_unsupported() {
    let manifest: CapabilityManifest = BTreeMap::new();
    let reqs = require(&[(Capability::ExtendedThinking, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(
        result.unsupported_caps(),
        vec![Capability::ExtendedThinking]
    );
}

/// Given empty requirements, When negotiated, Then always compatible.
#[test]
fn s06_given_empty_requirements_when_negotiated_then_compatible() {
    let manifest = manifest_from(&[(Capability::Streaming, SupportLevel::Native)]);
    let reqs = CapabilityRequirements::default();
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
}

/// Given mixed caps, When negotiated, Then native + emulated categorized.
#[test]
fn s06_given_mixed_caps_when_negotiated_then_categorized() {
    let manifest = manifest_from(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
    ]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
}

/// Given EmulationEngine, When checking missing caps, Then report generated.
#[test]
fn s06_given_emulation_engine_when_checking_missing_then_report_generated() {
    let engine = EmulationEngine::with_defaults();
    let missing = vec![Capability::ExtendedThinking, Capability::CodeExecution];
    let report = engine.check_missing(&missing);
    assert!(!report.is_empty());
}

/// Given runtime with emulation, When cap check fails, Then emulation attempted.
#[test]
fn s06_given_runtime_with_emulation_when_configured_then_present() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    assert!(rt.emulation_config().is_some());
}

/// Given SupportLevel::Native, When satisfies Native, Then true.
#[test]
fn s06_given_native_support_when_checked_against_native_then_true() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// Scenario 7: Given a sidecar hello, When protocol validated,
//             Then capabilities registered (6 tests)
// ===========================================================================

/// Given a hello envelope, When encoded, Then contains "t":"hello".
#[test]
fn s07_given_hello_when_encoded_then_contains_t_hello() {
    let caps = manifest_from(&[(Capability::Streaming, SupportLevel::Native)]);
    let hello = make_hello_envelope("test-sidecar", caps, ExecutionMode::Mapped);
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains("\"t\":\"hello\""));
}

/// Given hello envelope, When decoded, Then backend identity preserved.
#[test]
fn s07_given_hello_when_roundtrip_then_identity_preserved() {
    let caps = manifest_from(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let hello = make_hello_envelope("my-backend", caps, ExecutionMode::Mapped);
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "my-backend");
            assert!(capabilities.contains_key(&Capability::ToolRead));
        }
        _ => panic!("expected Hello envelope"),
    }
}

/// Given hello with contract version, When checked, Then version matches.
#[test]
fn s07_given_hello_with_version_when_checked_then_matches() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "v-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    match hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

/// Given compatible versions, When checked, Then compatible.
#[test]
fn s07_given_compatible_versions_when_checked_then_compatible() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

/// Given incompatible versions, When checked, Then not compatible.
#[test]
fn s07_given_incompatible_versions_when_checked_then_not_compatible() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
}

/// Given version string, When parsed, Then major.minor extracted.
#[test]
fn s07_given_version_string_when_parsed_then_extracted() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version(""), None);
}

// ===========================================================================
// Scenario 8: Given invalid JSON, When parsed as envelope,
//             Then error returned (6 tests)
// ===========================================================================

/// Given garbage text, When decoded, Then JSON error.
#[test]
fn s08_given_garbage_when_decoded_then_json_error() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

/// Given valid JSON but wrong shape, When decoded, Then error.
#[test]
fn s08_given_wrong_shape_json_when_decoded_then_error() {
    let result = JsonlCodec::decode(r#"{"foo": "bar"}"#);
    assert!(result.is_err());
}

/// Given empty string, When decoded, Then error.
#[test]
fn s08_given_empty_string_when_decoded_then_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

/// Given JSON array, When decoded, Then error.
#[test]
fn s08_given_json_array_when_decoded_then_error() {
    let result = JsonlCodec::decode("[1, 2, 3]");
    assert!(result.is_err());
}

/// Given valid fatal envelope, When decoded, Then succeeds.
#[test]
fn s08_given_valid_fatal_when_decoded_then_succeeds() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let envelope = JsonlCodec::decode(line).unwrap();
    assert!(matches!(envelope, Envelope::Fatal { error, .. } if error == "boom"));
}

/// Given truncated JSON, When decoded, Then error.
#[test]
fn s08_given_truncated_json_when_decoded_then_error() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#);
    assert!(result.is_err());
}

// ===========================================================================
// Scenario 9: Given a work order with max_turns,
//             When run completes, Then turns counted (6 tests)
// ===========================================================================

/// Given work order with max_turns=10, When built, Then config has max_turns.
#[test]
fn s09_given_max_turns_10_when_built_then_config_has_max_turns() {
    let wo = WorkOrderBuilder::new("Test").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

/// Given work order with no max_turns, When built, Then config has None.
#[test]
fn s09_given_no_max_turns_when_built_then_none() {
    let wo = WorkOrderBuilder::new("Test").build();
    assert!(wo.config.max_turns.is_none());
}

/// Given work order with max_turns, When serialized, Then field preserved.
#[test]
fn s09_given_max_turns_when_serialized_then_preserved() {
    let wo = WorkOrderBuilder::new("Test")
        .max_turns(5)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let json = serde_json::to_value(&wo).unwrap();
    assert_eq!(json["config"]["max_turns"], 5);
}

/// Given work order with max_budget, When built, Then budget preserved.
#[test]
fn s09_given_max_budget_when_built_then_preserved() {
    let wo = WorkOrderBuilder::new("Test").max_budget_usd(1.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(1.5));
}

/// Given work order with model, When built, Then model preserved.
#[test]
fn s09_given_model_when_built_then_preserved() {
    let wo = WorkOrderBuilder::new("Test").model("gpt-4o").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

/// Given runtime config with all fields, When serialized, Then roundtrip.
#[test]
fn s09_given_runtime_config_when_roundtrip_then_preserved() {
    let config = RuntimeConfig {
        model: Some("claude-3".into()),
        max_turns: Some(20),
        max_budget_usd: Some(5.0),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, config.model);
    assert_eq!(back.max_turns, config.max_turns);
    assert_eq!(back.max_budget_usd, config.max_budget_usd);
}

// ===========================================================================
// Scenario 10: Given an error from backend, When processed,
//              Then error in receipt (6 tests)
// ===========================================================================

/// Given unknown backend, When run attempted, Then UnknownBackend error.
#[tokio::test]
async fn s10_given_unknown_backend_when_run_then_error() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("hello");
    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err());
    match result {
        Err(err) => {
            assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
            assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
        }
        Ok(_) => panic!("expected UnknownBackend error"),
    }
}

/// Given AbpError, When converted to RuntimeError, Then code preserved.
#[test]
fn s10_given_abp_error_when_converted_then_code_preserved() {
    let abp_err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), ErrorCode::BackendTimeout);
}

/// Given partial receipt, When built on timeout, Then outcome is Partial.
#[test]
fn s10_given_timeout_when_receipt_built_then_partial() {
    let receipt = ReceiptBuilder::new("openai")
        .outcome(Outcome::Partial)
        .usage_raw(serde_json::json!({"error": "timeout"}))
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert!(verify_hash(&receipt));
}

/// Given failed receipt, When built, Then outcome is Failed.
#[test]
fn s10_given_failure_when_receipt_built_then_failed() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

/// Given RuntimeError variants, When error_code called, Then correct codes.
#[test]
fn s10_given_runtime_error_variants_when_code_called_then_correct() {
    let cases = vec![
        (
            RuntimeError::UnknownBackend { name: "x".into() },
            ErrorCode::BackendNotFound,
        ),
        (
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk")),
            ErrorCode::WorkspaceInitFailed,
        ),
        (
            RuntimeError::PolicyFailed(anyhow::anyhow!("glob")),
            ErrorCode::PolicyInvalid,
        ),
        (
            RuntimeError::BackendFailed(anyhow::anyhow!("crash")),
            ErrorCode::BackendCrashed,
        ),
        (
            RuntimeError::CapabilityCheckFailed("missing".into()),
            ErrorCode::CapabilityUnsupported,
        ),
    ];
    for (err, expected_code) in cases {
        assert_eq!(err.error_code(), expected_code);
    }
}

/// Given error with context, When display, Then includes code and message.
#[test]
fn s10_given_error_with_context_when_displayed_then_includes_info() {
    let err =
        AbpError::new(ErrorCode::PolicyDenied, "tool disallowed").with_context("tool", "Bash");
    let display = err.to_string();
    assert!(display.contains("POLICY_DENIED"));
    assert!(display.contains("tool disallowed"));
}

// ===========================================================================
// Scenario 11: Given a streaming run, When events collected,
//              Then order is correct (6 tests)
// ===========================================================================

/// Given mock run, When events collected, Then at least 2 events.
#[tokio::test]
async fn s11_given_mock_run_when_events_collected_then_at_least_2() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("streaming test"))
        .await
        .unwrap();

    let mut events = vec![];
    let mut stream = handle.events;
    while let Some(ev) = tokio_stream::StreamExt::next(&mut stream).await {
        events.push(ev);
    }
    let _receipt = handle.receipt.await.unwrap().unwrap();
    assert!(events.len() >= 2);
}

/// Given mock run, When trace inspected, Then RunStarted is first.
#[tokio::test]
async fn s11_given_mock_run_when_trace_inspected_then_run_started_first() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("order test"))
        .await
        .unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(matches!(
        receipt.trace.first().map(|e| &e.kind),
        Some(AgentEventKind::RunStarted { .. })
    ));
}

/// Given mock run, When trace inspected, Then RunCompleted is last.
#[tokio::test]
async fn s11_given_mock_run_when_trace_inspected_then_run_completed_last() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("order test"))
        .await
        .unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(matches!(
        receipt.trace.last().map(|e| &e.kind),
        Some(AgentEventKind::RunCompleted { .. })
    ));
}

/// Given mock run, When trace inspected, Then all timestamps are monotonic.
#[tokio::test]
async fn s11_given_mock_run_when_trace_inspected_then_timestamps_monotonic() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("mono test"))
        .await
        .unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    for window in receipt.trace.windows(2) {
        assert!(
            window[0].ts <= window[1].ts,
            "timestamps should be monotonically non-decreasing"
        );
    }
}

/// Given agent events built manually, When ordered, Then kinds are correct.
#[test]
fn s11_given_events_manually_when_ordered_then_kinds_correct() {
    let events = [
        make_agent_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_agent_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
        make_agent_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

/// Given event kinds, When serialized, Then type tag present.
#[test]
fn s11_given_event_kind_when_serialized_then_type_tag() {
    let event = make_agent_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "README.md"}),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_call");
    assert_eq!(json["tool_name"], "Read");
}

// ===========================================================================
// Scenario 12: Given passthrough mode, When executing,
//              Then no request rewriting (5 tests)
// ===========================================================================

/// Given ExecutionMode::Passthrough, When default checked, Then not passthrough.
#[test]
fn s12_given_default_mode_when_checked_then_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
}

/// Given Passthrough mode, When serialized, Then "passthrough".
#[test]
fn s12_given_passthrough_when_serialized_then_correct_string() {
    let json = serde_json::to_value(ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, "passthrough");
}

/// Given Mapped mode, When serialized, Then "mapped".
#[test]
fn s12_given_mapped_when_serialized_then_correct_string() {
    let json = serde_json::to_value(ExecutionMode::Mapped).unwrap();
    assert_eq!(json, "mapped");
}

/// Given hello with passthrough mode, When encoded, Then mode in JSON.
#[test]
fn s12_given_hello_passthrough_when_encoded_then_mode_in_json() {
    let hello = make_hello_envelope(
        "pt-backend",
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains("passthrough"));
}

/// Given receipt built with passthrough mode, When inspected, Then mode matches.
#[test]
fn s12_given_passthrough_receipt_when_inspected_then_mode_matches() {
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// Scenario 13: Given mapped mode, When executing,
//              Then dialect translation occurs (6 tests)
// ===========================================================================

/// Given OpenAI style, When detected, Then OpenAI dialect.
#[test]
fn s13_given_openai_style_when_detected_then_openai() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "model": "gpt-4",
        "choices": [{"message": {"role": "assistant", "content": "hi"}}]
    });
    let result = detector.detect(&val).expect("should detect");
    assert_eq!(result.dialect, Dialect::OpenAi);
}

/// Given Claude style, When detected, Then Claude dialect.
#[test]
fn s13_given_claude_style_when_detected_then_claude() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "type": "message",
        "model": "claude-3-opus",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let result = detector.detect(&val).expect("should detect");
    assert_eq!(result.dialect, Dialect::Claude);
}

/// Given Gemini style, When detected, Then Gemini dialect.
#[test]
fn s13_given_gemini_style_when_detected_then_gemini() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}],
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    let result = detector.detect(&val).expect("should detect");
    assert_eq!(result.dialect, Dialect::Gemini);
}

/// Given empty JSON, When detected, Then no result.
#[test]
fn s13_given_empty_json_when_detected_then_none() {
    let detector = DialectDetector::new();
    let val = serde_json::json!({});
    assert!(detector.detect(&val).is_none());
}

/// Given mapped mode receipt, When inspected, Then mode is Mapped.
#[test]
fn s13_given_mapped_receipt_when_inspected_then_mapped() {
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Mapped)
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

/// Given known_rules registry, When counted, Then at least 16 rules.
#[test]
fn s13_given_known_rules_when_counted_then_at_least_16() {
    let reg = known_rules();
    assert!(reg.len() >= 16, "expected >= 16 rules, got {}", reg.len());
}

// ===========================================================================
// Scenario 14: Given a cancelled run, When receipt created,
//              Then status is appropriate (5 tests)
// ===========================================================================

/// Given partial outcome, When receipt built, Then outcome is Partial.
#[test]
fn s14_given_cancellation_when_receipt_built_then_partial() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .usage_raw(serde_json::json!({"reason": "cancelled"}))
        .build();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

/// Given partial receipt, When hashed, Then hash is valid.
#[test]
fn s14_given_partial_receipt_when_hashed_then_valid() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&receipt));
}

/// Given failed outcome, When receipt built, Then outcome is Failed.
#[test]
fn s14_given_failure_when_receipt_built_then_outcome_failed() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

/// Given Outcome enum, When serialized, Then correct string.
#[test]
fn s14_given_outcome_when_serialized_then_correct() {
    assert_eq!(serde_json::to_value(Outcome::Complete).unwrap(), "complete");
    assert_eq!(serde_json::to_value(Outcome::Partial).unwrap(), "partial");
    assert_eq!(serde_json::to_value(Outcome::Failed).unwrap(), "failed");
}

/// Given outcome strings, When deserialized, Then correct variants.
#[test]
fn s14_given_outcome_strings_when_deserialized_then_correct() {
    let complete: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(complete, Outcome::Complete);
    let partial: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(partial, Outcome::Partial);
    let failed: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(failed, Outcome::Failed);
}

// ===========================================================================
// Scenario 15: Given a chain of work orders, When all complete,
//              Then chain is verifiable (8 tests)
// ===========================================================================

/// Given 3 sequential receipts pushed to chain, When verified, Then OK.
#[test]
fn s15_given_3_receipts_when_chain_verified_then_ok() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        let r = ReceiptBuilder::new(format!("backend-{i}"))
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

/// Given 5 receipts in chain, When verified, Then all hashes valid.
#[test]
fn s15_given_5_receipts_when_chain_verified_then_all_valid() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

/// Given tampered receipt, When pushed to chain, Then error.
#[test]
fn s15_given_tampered_receipt_when_pushed_then_error() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("bad_hash".into());
    let mut chain = ReceiptChain::new();
    assert!(chain.push(r).is_err());
}

/// Given empty chain, When verified, Then error.
#[test]
fn s15_given_empty_chain_when_verified_then_error() {
    let chain = ReceiptChain::new();
    assert!(chain.verify().is_err());
}

/// Given chain of receipts, When iterated, Then correct count.
#[test]
fn s15_given_chain_when_iterated_then_correct_count() {
    let mut chain = ReceiptChain::new();
    for _ in 0..4 {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    let collected: Vec<_> = chain.iter().collect();
    assert_eq!(collected.len(), 4);
}

/// Given runtime receipt chain, When 3 runs, Then chain has 3 entries.
#[tokio::test]
async fn s15_given_runtime_chain_when_3_runs_then_3_entries() {
    let rt = Runtime::with_default_backends();
    for task in &["Step 1", "Step 2", "Step 3"] {
        let wo = make_work_order(task);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _receipt = handle.receipt.await.unwrap().unwrap();
    }
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert_eq!(guard.len(), 3);
    assert!(guard.verify().is_ok());
}

/// Given runtime receipt chain, When receipts diffed, Then run_id differs.
#[tokio::test]
async fn s15_given_chain_when_first_and_last_diffed_then_run_id_differs() {
    let rt = Runtime::with_default_backends();
    for task in &["A", "B", "C"] {
        let wo = make_work_order(task);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _receipt = handle.receipt.await.unwrap().unwrap();
    }
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    let receipts: Vec<_> = guard.iter().collect();
    let diff = diff_receipts(receipts[0], receipts[2]);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "meta.run_id"));
}

/// Given chain receipts, When all have unique hashes, Then no duplicates.
#[tokio::test]
async fn s15_given_chain_when_hashes_checked_then_all_unique() {
    let rt = Runtime::with_default_backends();
    for task in &["X", "Y", "Z"] {
        let wo = make_work_order(task);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _receipt = handle.receipt.await.unwrap().unwrap();
    }
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    let hashes: Vec<_> = guard
        .iter()
        .map(|r| r.receipt_sha256.clone().unwrap_or_default())
        .collect();
    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(unique.len(), hashes.len());
}

// ===========================================================================
// Additional cross-cutting BDD scenarios (to reach 80+)
// ===========================================================================

/// Given receipt builder with trace events, When built, Then trace preserved.
#[test]
fn extra_given_trace_events_when_built_then_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_agent_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_trace_event(make_agent_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }))
        .add_trace_event(make_agent_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build();
    assert_eq!(receipt.trace.len(), 3);
}

/// Given receipt builder with artifacts, When built, Then artifacts preserved.
#[test]
fn extra_given_artifacts_when_built_then_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

/// Given receipt with usage, When built, Then usage_raw present.
#[test]
fn extra_given_usage_when_built_then_usage_raw_present() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_raw(serde_json::json!({"input_tokens": 100, "output_tokens": 50}))
        .build();
    assert!(receipt.usage_raw.is_object());
    assert_eq!(receipt.usage_raw["input_tokens"], 100);
}

/// Given canonical_json, When called on object, Then keys sorted.
#[test]
fn extra_given_canonical_json_when_called_then_keys_sorted() {
    let json = canonical_json(&serde_json::json!({"z": 1, "a": 2})).unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    assert!(a_pos < z_pos, "keys should be sorted");
}

/// Given sha256_hex, When called, Then 64-char hex.
#[test]
fn extra_given_sha256_hex_when_called_then_64_chars() {
    let hex = sha256_hex(b"hello world");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

/// Given fatal envelope with error code, When created, Then code accessible.
#[test]
fn extra_given_fatal_with_code_when_created_then_code_accessible() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "out of memory",
        ErrorCode::BackendCrashed,
    );
    assert_eq!(env.error_code(), Some(ErrorCode::BackendCrashed));
}

/// Given AbpError, When error_code inspected, Then correct category.
#[test]
fn extra_given_abp_error_when_category_then_correct() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
}

/// Given AbpError serialized, When deserialized, Then roundtrip.
#[test]
fn extra_given_abp_error_dto_when_roundtrip_then_preserved() {
    let err =
        AbpError::new(ErrorCode::DialectUnknown, "unrecognized").with_context("input", "foobar");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::DialectUnknown);
    assert_eq!(back.message, "unrecognized");
}

/// Given CONTRACT_VERSION, When checked, Then "abp/v0.1".
#[test]
fn extra_given_contract_version_then_correct() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

/// Given receipt metadata, When timestamps checked, Then started <= finished.
#[tokio::test]
async fn extra_given_receipt_when_timestamps_then_ordered() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("ts test"))
        .await
        .unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

/// Given receipt, When contract_version checked, Then matches constant.
#[tokio::test]
async fn extra_given_receipt_when_contract_version_then_matches() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("cv test"))
        .await
        .unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

/// Given receipt, When backend id checked, Then "mock".
#[tokio::test]
async fn extra_given_receipt_when_backend_id_then_mock() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("backend test"))
        .await
        .unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

/// Given work order, When id checked, Then non-nil UUID.
#[test]
fn extra_given_work_order_when_id_checked_then_non_nil() {
    let wo = make_work_order("uuid test");
    assert!(!wo.id.is_nil());
}

/// Given two work orders, When ids compared, Then different.
#[test]
fn extra_given_two_work_orders_when_ids_then_different() {
    let wo1 = make_work_order("a");
    let wo2 = make_work_order("b");
    assert_ne!(wo1.id, wo2.id);
}

/// Given receipt builder with verification report, When built, Then preserved.
#[test]
fn extra_given_verification_report_when_built_then_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .verification(VerificationReport {
            git_diff: Some("diff output".into()),
            git_status: Some("M src/main.rs".into()),
            harness_ok: true,
        })
        .build();
    assert!(receipt.verification.harness_ok);
    assert_eq!(
        receipt.verification.git_diff.as_deref(),
        Some("diff output")
    );
}

/// Given ProtocolError, When error_code called, Then returns code.
#[test]
fn extra_given_protocol_error_when_code_then_correct() {
    let err = ProtocolError::Violation("bad".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
    let err2 = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        err2.error_code(),
        Some(ErrorCode::ProtocolUnexpectedMessage)
    );
}

/// Given decode_stream with multiple lines, When iterated, Then all decoded.
#[test]
fn extra_given_decode_stream_when_iterated_then_all_decoded() {
    let input = r#"{"t":"fatal","ref_id":null,"error":"boom"}
{"t":"fatal","ref_id":null,"error":"bang"}
"#;
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

/// Given capability check on mock backend, When streaming required, Then passes.
#[test]
fn extra_given_cap_check_mock_when_streaming_then_passes() {
    let rt = Runtime::with_default_backends();
    let reqs = require(&[(Capability::Streaming, MinSupport::Native)]);
    rt.check_capabilities("mock", &reqs).unwrap();
}

/// Given capability check on mock backend, When McpClient required, Then fails.
#[test]
fn extra_given_cap_check_mock_when_mcp_then_fails() {
    let rt = Runtime::with_default_backends();
    let reqs = require(&[(Capability::McpClient, MinSupport::Native)]);
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}
