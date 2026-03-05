#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end integration tests for dialect translation through the projection matrix.
//!
//! Verifies: dialect detection, TranslationEngine registration, request/response
//! translation, passthrough mode, receipt metadata, and all 6 dialect combinations.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    CapabilityRequirements, ExecutionLane, Outcome, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_dialect::Dialect;
use abp_projection::translate::{TranslationEngine, TranslationMode};
use abp_runtime::{Runtime, RuntimeError};

// ── Helpers ─────────────────────────────────────────────────────────────

fn work_order_with_dialect(dialect: &str) -> WorkOrder {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".to_string(), serde_json::json!({ "dialect": dialect }));
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "translation test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: None,
            vendor,
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        },
    }
}

fn work_order_with_flat_dialect(dialect: &str) -> WorkOrder {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp.dialect".to_string(),
        serde_json::Value::String(dialect.to_string()),
    );
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "flat dialect test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: None,
            vendor,
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        },
    }
}

fn plain_work_order() -> WorkOrder {
    WorkOrderBuilder::new("no dialect test").build()
}

fn simple_conv() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")])
}

async fn run_and_get_receipt(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> Result<abp_core::Receipt, RuntimeError> {
    let handle = rt.run_streaming(backend, wo).await?;
    handle.receipt.await.unwrap()
}

// =========================================================================
// 1. Dialect extraction tests
// =========================================================================

#[test]
fn extract_dialect_from_nested_vendor_config() {
    let wo = work_order_with_dialect("claude");
    let dialect = abp_runtime::extract_dialect(&wo);
    assert_eq!(dialect, Some(Dialect::Claude));
}

#[test]
fn extract_dialect_from_flat_vendor_config() {
    let wo = work_order_with_flat_dialect("openai");
    let dialect = abp_runtime::extract_dialect(&wo);
    assert_eq!(dialect, Some(Dialect::OpenAi));
}

#[test]
fn extract_dialect_returns_none_when_absent() {
    let wo = plain_work_order();
    assert_eq!(abp_runtime::extract_dialect(&wo), None);
}

#[test]
fn extract_dialect_openai() {
    let wo = work_order_with_dialect("openai");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::OpenAi));
}

#[test]
fn extract_dialect_gemini() {
    let wo = work_order_with_dialect("gemini");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::Gemini));
}

#[test]
fn extract_dialect_codex() {
    let wo = work_order_with_dialect("codex");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::Codex));
}

#[test]
fn extract_dialect_kimi() {
    let wo = work_order_with_dialect("kimi");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::Kimi));
}

#[test]
fn extract_dialect_copilot() {
    let wo = work_order_with_dialect("copilot");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::Copilot));
}

#[test]
fn extract_dialect_case_insensitive() {
    let wo = work_order_with_dialect("Claude");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::Claude));
}

#[test]
fn extract_dialect_open_ai_alias() {
    let wo = work_order_with_dialect("open_ai");
    assert_eq!(abp_runtime::extract_dialect(&wo), Some(Dialect::OpenAi));
}

#[test]
fn extract_dialect_unknown_returns_none() {
    let wo = work_order_with_dialect("unknown_vendor");
    assert_eq!(abp_runtime::extract_dialect(&wo), None);
}

// =========================================================================
// 2. Backend dialect inference tests
// =========================================================================

#[test]
fn infer_dialect_from_sidecar_claude() {
    assert_eq!(
        abp_runtime::infer_dialect_from_backend("sidecar:claude"),
        Some(Dialect::Claude)
    );
}

#[test]
fn infer_dialect_from_openai_backend() {
    assert_eq!(
        abp_runtime::infer_dialect_from_backend("openai"),
        Some(Dialect::OpenAi)
    );
}

#[test]
fn infer_dialect_from_gemini_backend() {
    assert_eq!(
        abp_runtime::infer_dialect_from_backend("sidecar:gemini"),
        Some(Dialect::Gemini)
    );
}

#[test]
fn infer_dialect_from_codex_backend() {
    assert_eq!(
        abp_runtime::infer_dialect_from_backend("codex-api"),
        Some(Dialect::Codex)
    );
}

#[test]
fn infer_dialect_from_kimi_backend() {
    assert_eq!(
        abp_runtime::infer_dialect_from_backend("sidecar:kimi"),
        Some(Dialect::Kimi)
    );
}

#[test]
fn infer_dialect_from_copilot_backend() {
    assert_eq!(
        abp_runtime::infer_dialect_from_backend("sidecar:copilot"),
        Some(Dialect::Copilot)
    );
}

#[test]
fn infer_dialect_from_mock_returns_none() {
    assert_eq!(abp_runtime::infer_dialect_from_backend("mock"), None);
}

// =========================================================================
// 3. TranslationEngine registration tests
// =========================================================================

#[test]
fn translation_engine_with_defaults_has_all_6_identity_pairs() {
    let engine = TranslationEngine::with_defaults();
    for &d in Dialect::all() {
        assert!(engine.supports(d, d), "missing identity pair for {d}");
    }
}

#[test]
fn translation_engine_supports_openai_claude_bidirectional() {
    let engine = TranslationEngine::with_defaults();
    assert!(engine.supports(Dialect::OpenAi, Dialect::Claude));
    assert!(engine.supports(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn translation_engine_supports_openai_gemini_bidirectional() {
    let engine = TranslationEngine::with_defaults();
    assert!(engine.supports(Dialect::OpenAi, Dialect::Gemini));
    assert!(engine.supports(Dialect::Gemini, Dialect::OpenAi));
}

#[test]
fn translation_engine_supports_openai_codex_bidirectional() {
    let engine = TranslationEngine::with_defaults();
    assert!(engine.supports(Dialect::OpenAi, Dialect::Codex));
    assert!(engine.supports(Dialect::Codex, Dialect::OpenAi));
}

#[test]
fn translation_engine_supports_openai_kimi_bidirectional() {
    let engine = TranslationEngine::with_defaults();
    assert!(engine.supports(Dialect::OpenAi, Dialect::Kimi));
    assert!(engine.supports(Dialect::Kimi, Dialect::OpenAi));
}

#[test]
fn translation_engine_supports_openai_copilot_bidirectional() {
    let engine = TranslationEngine::with_defaults();
    assert!(engine.supports(Dialect::OpenAi, Dialect::Copilot));
    assert!(engine.supports(Dialect::Copilot, Dialect::OpenAi));
}

#[test]
fn translation_engine_classify_passthrough_for_same_dialect() {
    let engine = TranslationEngine::with_defaults();
    for &d in Dialect::all() {
        assert_eq!(engine.classify(d, d), TranslationMode::Passthrough);
    }
}

#[test]
fn translation_engine_classify_mapped_for_cross_dialect() {
    let engine = TranslationEngine::with_defaults();
    assert_eq!(
        engine.classify(Dialect::OpenAi, Dialect::Claude),
        TranslationMode::Mapped
    );
}

// =========================================================================
// 4. Translation request/response round-trip tests
// =========================================================================

#[test]
fn translate_passthrough_preserves_conversation() {
    let engine = TranslationEngine::with_defaults();
    let conv = simple_conv();
    let result = engine
        .translate(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Passthrough);
    assert_eq!(result.conversation.messages.len(), conv.messages.len());
    assert!(result.gaps.is_empty());
}

#[test]
fn translate_openai_to_claude_succeeds() {
    let engine = TranslationEngine::with_defaults();
    let conv = simple_conv();
    let result = engine
        .translate(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Mapped);
    assert_eq!(result.from, Dialect::OpenAi);
    assert_eq!(result.to, Dialect::Claude);
    assert!(!result.conversation.messages.is_empty());
}

#[test]
fn translate_claude_to_openai_succeeds() {
    let engine = TranslationEngine::with_defaults();
    let conv = simple_conv();
    let result = engine
        .translate(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Mapped);
}

#[test]
fn translate_openai_to_gemini_succeeds() {
    let engine = TranslationEngine::with_defaults();
    let conv = simple_conv();
    let result = engine
        .translate(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Mapped);
}

#[test]
fn translate_response_passthrough_works() {
    let engine = TranslationEngine::with_defaults();
    let conv =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Response text")]);
    let result = engine
        .translate_response(Dialect::Claude, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Passthrough);
}

#[test]
fn translate_response_cross_dialect_works() {
    let engine = TranslationEngine::with_defaults();
    let conv = IrConversation::from_messages(vec![IrMessage::text(
        IrRole::Assistant,
        "Translated response",
    )]);
    let result = engine
        .translate_response(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Mapped);
}

// =========================================================================
// 5. Runtime integration — TranslationEngine is wired in
// =========================================================================

#[test]
fn runtime_has_translation_engine_by_default() {
    let rt = Runtime::with_default_backends();
    let engine = rt.translation_engine();
    assert!(engine.translator_count() > 0);
}

#[test]
fn runtime_with_custom_translation_engine() {
    let engine = TranslationEngine::new();
    assert_eq!(engine.translator_count(), 0);

    let rt = Runtime::new().with_translation_engine(engine);
    assert_eq!(rt.translation_engine().translator_count(), 0);
}

#[test]
fn runtime_default_engine_supports_all_6_dialects() {
    let rt = Runtime::with_default_backends();
    let engine = rt.translation_engine();
    for &d in Dialect::all() {
        assert!(engine.supports(d, d), "identity pair missing for {d:?}");
    }
}

// =========================================================================
// 6. Full pipeline e2e: request → translate → backend → receipt
// =========================================================================

#[tokio::test]
async fn e2e_passthrough_no_dialect_specified() {
    let rt = Runtime::with_default_backends();
    let wo = plain_work_order();
    let receipt = run_and_get_receipt(&rt, "mock", wo).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    // No translation metadata when no dialect is specified.
    let usage = receipt.usage_raw.as_object().unwrap();
    assert!(!usage.contains_key("dialect_translation"));
}

#[tokio::test]
async fn e2e_same_dialect_passthrough_records_metadata() {
    // When source dialect = target dialect, translation is passthrough.
    // Backend is "mock" which has no inferred dialect, so we need to use
    // a projection matrix to register a dialect for the mock backend.
    use abp_core::{Capability, SupportLevel};

    let mut manifest = abp_core::CapabilityManifest::default();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let mut matrix = abp_projection::ProjectionMatrix::new();
    matrix.register_backend("mock", manifest, Dialect::OpenAi, 50);

    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = work_order_with_dialect("openai");
    let receipt = run_and_get_receipt(&rt, "mock", wo).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    let usage = receipt.usage_raw.as_object().unwrap();
    let translation = usage
        .get("dialect_translation")
        .expect("should have translation metadata");
    assert_eq!(translation["translation_mode"], "passthrough");
    assert_eq!(translation["source_dialect"], "OpenAI");
    assert_eq!(translation["target_dialect"], "OpenAI");
}

#[tokio::test]
async fn e2e_cross_dialect_mapped_records_metadata() {
    use abp_core::{Capability, SupportLevel};

    let mut manifest = abp_core::CapabilityManifest::default();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let mut matrix = abp_projection::ProjectionMatrix::new();
    matrix.register_backend("mock", manifest, Dialect::Claude, 50);

    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = work_order_with_dialect("openai");
    let receipt = run_and_get_receipt(&rt, "mock", wo).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    let usage = receipt.usage_raw.as_object().unwrap();
    let translation = usage
        .get("dialect_translation")
        .expect("should have translation metadata");
    assert_eq!(translation["translation_mode"], "mapped");
    assert_eq!(translation["source_dialect"], "OpenAI");
    assert_eq!(translation["target_dialect"], "Claude");
}

#[tokio::test]
async fn e2e_receipt_has_capability_gaps_when_relevant() {
    use abp_core::{Capability, SupportLevel};

    let mut manifest = abp_core::CapabilityManifest::default();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let mut matrix = abp_projection::ProjectionMatrix::new();
    matrix.register_backend("mock", manifest, Dialect::Gemini, 50);

    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = work_order_with_dialect("openai");
    let receipt = run_and_get_receipt(&rt, "mock", wo).await.unwrap();

    let usage = receipt.usage_raw.as_object().unwrap();
    let translation = usage
        .get("dialect_translation")
        .expect("should have translation metadata");
    // capability_gaps should be an array (may be empty for simple text).
    assert!(translation["capability_gaps"].is_array());
}

#[tokio::test]
async fn e2e_receipt_hash_is_present_with_translation() {
    use abp_core::{Capability, SupportLevel};

    let mut manifest = abp_core::CapabilityManifest::default();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let mut matrix = abp_projection::ProjectionMatrix::new();
    matrix.register_backend("mock", manifest, Dialect::Claude, 50);

    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = work_order_with_dialect("openai");
    let receipt = run_and_get_receipt(&rt, "mock", wo).await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

// =========================================================================
// 7. Mode matrix and supported pairs coverage
// =========================================================================

#[test]
fn mode_matrix_covers_all_dialect_combinations() {
    let engine = TranslationEngine::with_defaults();
    let matrix = engine.mode_matrix();
    // 6 dialects × 6 = 36 entries.
    assert_eq!(matrix.len(), 36);
    // All identity pairs are passthrough.
    for &d in Dialect::all() {
        assert_eq!(matrix[&(d, d)], TranslationMode::Passthrough);
    }
}

#[test]
fn supported_pairs_includes_cross_dialect() {
    let engine = TranslationEngine::with_defaults();
    let pairs = engine.supported_pairs();
    // Should include at least the 6 identity pairs plus several cross-dialect.
    assert!(pairs.len() >= 6 + 10);
}

// =========================================================================
// 8. Translation with complex conversations
// =========================================================================

#[test]
fn translate_multi_turn_conversation() {
    let engine = TranslationEngine::with_defaults();
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant"),
        IrMessage::text(IrRole::User, "Question 1"),
        IrMessage::text(IrRole::Assistant, "Answer 1"),
        IrMessage::text(IrRole::User, "Question 2"),
    ]);
    let result = engine
        .translate(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Mapped);
    assert!(result.conversation.messages.len() >= 2);
}

#[test]
fn translate_tool_use_conversation() {
    let engine = TranslationEngine::with_defaults();
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Search for X"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: serde_json::json!({"query": "X"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Result for X".into(),
                }],
                is_error: false,
            }],
        ),
    ]);
    let result = engine
        .translate(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Mapped);
}

#[test]
fn translate_empty_conversation_passthrough() {
    let engine = TranslationEngine::with_defaults();
    let conv = IrConversation::new();
    let result = engine
        .translate(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.mode, TranslationMode::Passthrough);
    assert!(result.conversation.messages.is_empty());
}

// =========================================================================
// 9. All 6 dialects as source
// =========================================================================

#[test]
fn translate_from_all_6_dialects_to_openai() {
    let engine = TranslationEngine::with_defaults();
    let conv = simple_conv();
    for &src in Dialect::all() {
        let result = engine.translate(src, Dialect::OpenAi, &conv);
        assert!(
            result.is_ok(),
            "translation {src} → OpenAI failed: {:?}",
            result.err()
        );
    }
}

#[test]
fn translate_from_openai_to_all_6_dialects() {
    let engine = TranslationEngine::with_defaults();
    let conv = simple_conv();
    for &tgt in Dialect::all() {
        let result = engine.translate(Dialect::OpenAi, tgt, &conv);
        assert!(
            result.is_ok(),
            "translation OpenAI → {tgt} failed: {:?}",
            result.err()
        );
    }
}

// =========================================================================
// 10. Error handling
// =========================================================================

#[test]
fn runtime_default_translation_engine_has_nonzero_translators() {
    let rt = Runtime::new();
    assert!(rt.translation_engine().translator_count() > 0);
}

#[test]
fn translate_unsupported_pair_returns_error() {
    // An engine with no translators can't map cross-dialect.
    let engine = TranslationEngine::new();
    let conv = simple_conv();
    let result = engine.translate(Dialect::OpenAi, Dialect::Claude, &conv);
    assert!(result.is_err());
}

#[test]
fn translate_unsupported_pair_passthrough_still_works() {
    let engine = TranslationEngine::new();
    let conv = simple_conv();
    let result = engine.translate(Dialect::OpenAi, Dialect::OpenAi, &conv);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().mode, TranslationMode::Passthrough);
}
