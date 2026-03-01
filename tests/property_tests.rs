// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for dialect mapping invariants across all SDK adapters.

use proptest::prelude::*;

use abp_core::{CONTRACT_VERSION, ContextPacket, ContextSnippet, WorkOrderBuilder};

// ---------------------------------------------------------------------------
// Proptest strategies
// ---------------------------------------------------------------------------

/// Strategy for generating non-empty printable strings.
fn nonempty_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_ ./-]{1,64}".prop_map(|s| s.to_string())
}

/// Strategy for generating a well-formed WorkOrder via the builder.
fn arb_work_order() -> impl Strategy<Value = abp_core::WorkOrder> {
    (nonempty_string(), prop::option::of(nonempty_string())).prop_map(|(task, model)| {
        let mut builder = WorkOrderBuilder::new(task);
        if let Some(m) = model {
            builder = builder.model(m);
        }
        builder.build()
    })
}

/// Strategy for a WorkOrder with context snippets attached.
fn arb_work_order_with_snippets() -> impl Strategy<Value = abp_core::WorkOrder> {
    (
        nonempty_string(),
        prop::collection::vec((nonempty_string(), nonempty_string()), 0..4),
    )
        .prop_map(|(task, snippets)| {
            let ctx = ContextPacket {
                files: vec![],
                snippets: snippets
                    .into_iter()
                    .map(|(name, content)| ContextSnippet { name, content })
                    .collect(),
            };
            WorkOrderBuilder::new(task).context(ctx).build()
        })
}

// ============================= CLAUDE =====================================

mod claude {
    use super::*;
    use abp_claude_sdk::dialect::*;

    proptest! {
        /// map_work_order never panics for any well-formed WorkOrder.
        #[test]
        fn map_work_order_never_panics(wo in arb_work_order()) {
            let cfg = ClaudeConfig::default();
            let _ = map_work_order(&wo, &cfg);
        }

        /// The produced request always contains at least one message.
        #[test]
        fn map_work_order_always_has_messages(wo in arb_work_order()) {
            let cfg = ClaudeConfig::default();
            let req = map_work_order(&wo, &cfg);
            prop_assert!(!req.messages.is_empty());
        }

        /// Model canonical roundtrip: from_canonical(to_canonical(m)) == m
        #[test]
        fn model_canonical_roundtrip(model in nonempty_string()) {
            let canonical = to_canonical_model(&model);
            let back = from_canonical_model(&canonical);
            prop_assert_eq!(&back, &model);
        }

        /// ClaudeResponse serialization roundtrip.
        #[test]
        fn claude_response_serde_roundtrip(
            id in nonempty_string(),
            model in nonempty_string(),
            text in nonempty_string(),
        ) {
            let resp = ClaudeResponse {
                id,
                model,
                role: "assistant".into(),
                content: vec![ClaudeContentBlock::Text { text }],
                stop_reason: Some("end_turn".into()),
                usage: None,
            };
            let json = serde_json::to_string(&resp).unwrap();
            let back: ClaudeResponse = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(resp, back);
        }

        /// map_work_order with snippets never panics and preserves task text.
        #[test]
        fn map_work_order_with_snippets(wo in arb_work_order_with_snippets()) {
            let cfg = ClaudeConfig::default();
            let req = map_work_order(&wo, &cfg);
            prop_assert!(req.messages[0].content.contains(&wo.task));
        }

        /// Tool definition roundtrip: from_claude(to_claude(def)) == def
        #[test]
        fn tool_def_roundtrip(
            name in nonempty_string(),
            desc in nonempty_string(),
        ) {
            let canonical = CanonicalToolDef {
                name,
                description: desc,
                parameters_schema: serde_json::json!({"type": "object"}),
            };
            let claude = tool_def_to_claude(&canonical);
            let back = tool_def_from_claude(&claude);
            prop_assert_eq!(canonical, back);
        }
    }
}

// ============================= GEMINI =====================================

mod gemini {
    use super::*;
    use abp_gemini_sdk::dialect::*;

    proptest! {
        /// map_work_order never panics for any well-formed WorkOrder.
        #[test]
        fn map_work_order_never_panics(wo in arb_work_order()) {
            let cfg = GeminiConfig::default();
            let _ = map_work_order(&wo, &cfg);
        }

        /// Model canonical roundtrip: from_canonical(to_canonical(m)) == m
        #[test]
        fn model_canonical_roundtrip(model in nonempty_string()) {
            let canonical = to_canonical_model(&model);
            let back = from_canonical_model(&canonical);
            prop_assert_eq!(&back, &model);
        }

        /// Tool definition roundtrip: from_gemini(to_gemini(def)) == def
        #[test]
        fn tool_def_roundtrip(
            name in nonempty_string(),
            desc in nonempty_string(),
        ) {
            let canonical = CanonicalToolDef {
                name,
                description: desc,
                parameters_schema: serde_json::json!({"type": "object"}),
            };
            let gemini = tool_def_to_gemini(&canonical);
            let back = tool_def_from_gemini(&gemini);
            prop_assert_eq!(canonical, back);
        }

        /// map_work_order with snippets preserves task text in content.
        #[test]
        fn map_work_order_preserves_task(wo in arb_work_order_with_snippets()) {
            let cfg = GeminiConfig::default();
            let req = map_work_order(&wo, &cfg);
            let first_part = &req.contents[0].parts[0];
            if let abp_gemini_sdk::dialect::GeminiPart::Text(t) = first_part {
                prop_assert!(t.contains(&wo.task));
            } else {
                prop_assert!(false, "expected Text part");
            }
        }
    }
}

// ============================= CODEX ======================================

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::*;

    proptest! {
        /// map_work_order never panics for any well-formed WorkOrder.
        #[test]
        fn map_work_order_never_panics(wo in arb_work_order()) {
            let cfg = CodexConfig::default();
            let _ = map_work_order(&wo, &cfg);
        }

        /// Model canonical roundtrip: from_canonical(to_canonical(m)) == m
        #[test]
        fn model_canonical_roundtrip(model in nonempty_string()) {
            let canonical = to_canonical_model(&model);
            let back = from_canonical_model(&canonical);
            prop_assert_eq!(&back, &model);
        }

        /// Tool definition roundtrip: from_codex(to_codex(def)) == def
        #[test]
        fn tool_def_roundtrip(
            name in nonempty_string(),
            desc in nonempty_string(),
        ) {
            let canonical = CanonicalToolDef {
                name,
                description: desc,
                parameters_schema: serde_json::json!({"type": "object"}),
            };
            let codex = tool_def_to_codex(&canonical);
            let back = tool_def_from_codex(&codex);
            prop_assert_eq!(canonical, back);
        }
    }
}

// ============================= KIMI =======================================

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::*;

    proptest! {
        /// map_work_order never panics for any well-formed WorkOrder.
        #[test]
        fn map_work_order_never_panics(wo in arb_work_order()) {
            let cfg = KimiConfig::default();
            let _ = map_work_order(&wo, &cfg);
        }

        /// Model canonical roundtrip: from_canonical(to_canonical(m)) == m
        #[test]
        fn model_canonical_roundtrip(model in nonempty_string()) {
            let canonical = to_canonical_model(&model);
            let back = from_canonical_model(&canonical);
            prop_assert_eq!(&back, &model);
        }

        /// Tool definition roundtrip: from_kimi(to_kimi(def)) == def
        #[test]
        fn tool_def_roundtrip(
            name in nonempty_string(),
            desc in nonempty_string(),
        ) {
            let canonical = CanonicalToolDef {
                name,
                description: desc,
                parameters_schema: serde_json::json!({"type": "object"}),
            };
            let kimi = tool_def_to_kimi(&canonical);
            let back = tool_def_from_kimi(&kimi);
            prop_assert_eq!(canonical, back);
        }
    }
}

// ============================= OPENAI =====================================

mod openai {
    use super::*;
    use abp_openai_sdk::dialect::*;

    proptest! {
        /// map_work_order never panics for any well-formed WorkOrder.
        #[test]
        fn map_work_order_never_panics(wo in arb_work_order()) {
            let cfg = OpenAIConfig::default();
            let _ = map_work_order(&wo, &cfg);
        }

        /// Model canonical roundtrip: from_canonical(to_canonical(m)) == m
        #[test]
        fn model_canonical_roundtrip(model in nonempty_string()) {
            let canonical = to_canonical_model(&model);
            let back = from_canonical_model(&canonical);
            prop_assert_eq!(&back, &model);
        }

        /// Tool definition roundtrip: from_openai(to_openai(def)) == def
        #[test]
        fn tool_def_roundtrip(
            name in nonempty_string(),
            desc in nonempty_string(),
        ) {
            let canonical = CanonicalToolDef {
                name,
                description: desc,
                parameters_schema: serde_json::json!({"type": "object"}),
            };
            let oai = tool_def_to_openai(&canonical);
            let back = tool_def_from_openai(&oai);
            prop_assert_eq!(canonical, back);
        }
    }
}

// ==================== CROSS-DIALECT CAPABILITY INVARIANTS ==================

/// Capability manifests must have non-empty entries and valid structure.
#[test]
fn claude_capability_manifest_is_valid() {
    let m = abp_claude_sdk::dialect::capability_manifest();
    assert!(!m.is_empty(), "manifest must not be empty");
}

#[test]
fn gemini_capability_manifest_is_valid() {
    let m = abp_gemini_sdk::dialect::capability_manifest();
    assert!(!m.is_empty(), "manifest must not be empty");
}

#[test]
fn codex_capability_manifest_is_valid() {
    let m = abp_codex_sdk::dialect::capability_manifest();
    assert!(!m.is_empty(), "manifest must not be empty");
}

#[test]
fn kimi_capability_manifest_is_valid() {
    let m = abp_kimi_sdk::dialect::capability_manifest();
    assert!(!m.is_empty(), "manifest must not be empty");
}

#[test]
fn openai_capability_manifest_is_valid() {
    let m = abp_openai_sdk::dialect::capability_manifest();
    assert!(!m.is_empty(), "manifest must not be empty");
}

/// All dialects must report streaming capability.
#[test]
fn all_dialects_support_streaming() {
    use abp_core::Capability;

    let manifests = [
        abp_claude_sdk::dialect::capability_manifest(),
        abp_gemini_sdk::dialect::capability_manifest(),
        abp_codex_sdk::dialect::capability_manifest(),
        abp_kimi_sdk::dialect::capability_manifest(),
        abp_openai_sdk::dialect::capability_manifest(),
    ];
    for (i, m) in manifests.iter().enumerate() {
        assert!(
            m.contains_key(&Capability::Streaming),
            "dialect {i} missing streaming capability"
        );
    }
}

/// All dialect version strings are non-empty and contain a slash.
#[test]
fn dialect_versions_are_well_formed() {
    let versions = [
        abp_claude_sdk::dialect::DIALECT_VERSION,
        abp_gemini_sdk::dialect::DIALECT_VERSION,
        abp_codex_sdk::dialect::DIALECT_VERSION,
        abp_kimi_sdk::dialect::DIALECT_VERSION,
        abp_openai_sdk::dialect::DIALECT_VERSION,
    ];
    for v in &versions {
        assert!(!v.is_empty(), "dialect version must not be empty");
        assert!(v.contains('/'), "dialect version must contain '/'");
    }
}

/// CONTRACT_VERSION is the expected value (sanity check).
#[test]
fn contract_version_is_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}
