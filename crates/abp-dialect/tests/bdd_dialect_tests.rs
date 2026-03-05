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
// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style tests for dialect detection, properties, and registry operations.

use abp_dialect::ir::IrRequest;
use abp_dialect::registry::{DialectEntry, DialectError, DialectRegistry};
use abp_dialect::{Dialect, DialectDetector};
use serde_json::{json, Value};
use std::collections::HashSet;

// ═══════════════════════════════════════════════════════════════════════
// Given a known dialect — when detecting from a request payload
// ═══════════════════════════════════════════════════════════════════════

mod given_a_known_dialect {
    use super::*;

    mod when_detecting_from_model_name {
        use super::*;

        fn openai_request(model: &str) -> Value {
            json!({
                "model": model,
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.7
            })
        }

        #[test]
        fn gpt_4o_is_detected_as_openai() {
            let det = DialectDetector::new();
            let result = det.detect(&openai_request("gpt-4o")).unwrap();
            assert_eq!(result.dialect, Dialect::OpenAi);
            assert!(result.confidence > 0.3);
        }

        #[test]
        fn claude_3_opus_request_is_detected_as_claude() {
            let det = DialectDetector::new();
            let msg = json!({
                "model": "claude-3-opus",
                "type": "message",
                "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Claude);
        }

        #[test]
        fn gemini_request_is_detected_as_gemini() {
            let det = DialectDetector::new();
            let msg = json!({
                "contents": [{"parts": [{"text": "hi"}]}],
                "generationConfig": {"temperature": 0.5}
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Gemini);
        }

        #[test]
        fn codex_mini_request_is_detected_as_codex() {
            let det = DialectDetector::new();
            let msg = json!({
                "model": "codex-mini",
                "items": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
                "status": "completed",
                "object": "response"
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Codex);
        }

        #[test]
        fn kimi_k1_request_is_detected_as_kimi() {
            let det = DialectDetector::new();
            let msg = json!({
                "model": "kimi-k1",
                "messages": [{"role": "user", "content": "hi"}],
                "refs": ["doc1"],
                "search_plus": true
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Kimi);
        }

        #[test]
        fn copilot_gpt_4o_request_is_detected_as_copilot() {
            let det = DialectDetector::new();
            let msg = json!({
                "model": "copilot/gpt-4o",
                "messages": [{"role": "user", "content": "hi"}],
                "references": [{"type": "file", "path": "main.rs"}],
                "agent_mode": true
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Copilot);
        }
    }

    mod when_detecting_from_request {
        use super::*;

        #[test]
        fn openai_response_with_choices_is_detected() {
            let det = DialectDetector::new();
            let msg = json!({
                "id": "chatcmpl-abc",
                "model": "gpt-4o",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "hello"}}]
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::OpenAi);
            assert!(result.confidence >= 0.5);
        }

        #[test]
        fn claude_response_with_type_message_is_detected() {
            let det = DialectDetector::new();
            let msg = json!({
                "type": "message",
                "content": [{"type": "text", "text": "hi"}],
                "stop_reason": "end_turn"
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Claude);
        }

        #[test]
        fn gemini_response_with_candidates_is_detected() {
            let det = DialectDetector::new();
            let msg = json!({
                "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Gemini);
        }

        #[test]
        fn codex_response_with_items_and_status_is_detected() {
            let det = DialectDetector::new();
            let msg = json!({
                "object": "response",
                "status": "completed",
                "items": [{"type": "message", "content": []}]
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Codex);
        }

        #[test]
        fn copilot_request_with_confirmations_is_detected() {
            let det = DialectDetector::new();
            let msg = json!({
                "references": [{"type": "snippet"}],
                "confirmations": [{"id": "c1"}],
                "agent_mode": true
            });
            let result = det.detect(&msg).unwrap();
            assert_eq!(result.dialect, Dialect::Copilot);
        }

        #[test]
        fn detection_returns_evidence_strings() {
            let det = DialectDetector::new();
            let msg = json!({
                "model": "gpt-4o",
                "choices": [{"index": 0}]
            });
            let result = det.detect(&msg).unwrap();
            assert!(!result.evidence.is_empty(), "should include evidence");
        }

        #[test]
        fn detect_all_returns_multiple_candidates_sorted() {
            let det = DialectDetector::new();
            // This request has OpenAI-like and could partially match others
            let msg = json!({
                "model": "gpt-4o",
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.7
            });
            let results = det.detect_all(&msg);
            assert!(!results.is_empty());
            // Results should be sorted by descending confidence
            for w in results.windows(2) {
                assert!(w[0].confidence >= w[1].confidence);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Given an unknown dialect
// ═══════════════════════════════════════════════════════════════════════

mod given_an_unknown_dialect {
    use super::*;

    mod when_detecting_from_model_name {
        use super::*;

        #[test]
        fn empty_object_returns_none() {
            let det = DialectDetector::new();
            let result = det.detect(&json!({}));
            assert!(
                result.is_none(),
                "empty object should not match any dialect"
            );
        }

        #[test]
        fn array_input_returns_none() {
            let det = DialectDetector::new();
            let result = det.detect(&json!([1, 2, 3]));
            assert!(result.is_none(), "non-object should return None");
        }

        #[test]
        fn string_input_returns_none() {
            let det = DialectDetector::new();
            let result = det.detect(&json!("just a string"));
            assert!(result.is_none());
        }

        #[test]
        fn null_input_returns_none() {
            let det = DialectDetector::new();
            let result = det.detect(&Value::Null);
            assert!(result.is_none());
        }

        #[test]
        fn unrecognized_structure_returns_none() {
            let det = DialectDetector::new();
            let msg = json!({"foo": "bar", "baz": 42});
            let result = det.detect(&msg);
            assert!(result.is_none());
        }

        #[test]
        fn detect_all_returns_empty_for_unknown() {
            let det = DialectDetector::new();
            let results = det.detect_all(&json!({}));
            assert!(results.is_empty());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Given dialect properties
// ═══════════════════════════════════════════════════════════════════════

mod given_dialect_properties {
    use super::*;

    #[test]
    fn each_dialect_has_a_display_name() {
        for &d in Dialect::all() {
            let label = d.label();
            assert!(!label.is_empty(), "{d:?} should have a non-empty label");
        }
    }

    #[test]
    fn display_trait_uses_label() {
        assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
        assert_eq!(format!("{}", Dialect::Claude), "Claude");
        assert_eq!(format!("{}", Dialect::Gemini), "Gemini");
        assert_eq!(format!("{}", Dialect::Codex), "Codex");
        assert_eq!(format!("{}", Dialect::Kimi), "Kimi");
        assert_eq!(format!("{}", Dialect::Copilot), "Copilot");
    }

    #[test]
    fn each_dialect_serializes_to_snake_case() {
        let expected = [
            (Dialect::OpenAi, "\"open_ai\""),
            (Dialect::Claude, "\"claude\""),
            (Dialect::Gemini, "\"gemini\""),
            (Dialect::Codex, "\"codex\""),
            (Dialect::Kimi, "\"kimi\""),
            (Dialect::Copilot, "\"copilot\""),
        ];
        for (dialect, json_str) in expected {
            let serialized = serde_json::to_string(&dialect).unwrap();
            assert_eq!(serialized, json_str, "serialization of {dialect:?}");
        }
    }

    #[test]
    fn dialects_deserialize_from_snake_case() {
        let d: Dialect = serde_json::from_str("\"open_ai\"").unwrap();
        assert_eq!(d, Dialect::OpenAi);
        let d: Dialect = serde_json::from_str("\"claude\"").unwrap();
        assert_eq!(d, Dialect::Claude);
    }

    #[test]
    fn all_dialects_are_comparable_and_hashable() {
        let mut set = HashSet::new();
        for &d in Dialect::all() {
            assert!(set.insert(d), "{d:?} should be insertable into HashSet");
        }
        assert_eq!(set.len(), Dialect::all().len());
    }

    #[test]
    fn dialect_clone_equals_original() {
        for &d in Dialect::all() {
            let cloned = d;
            assert_eq!(d, cloned);
        }
    }

    #[test]
    fn all_returns_six_known_dialects() {
        assert_eq!(Dialect::all().len(), 6);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Given a dialect registry
// ═══════════════════════════════════════════════════════════════════════

mod given_dialect_registry {
    use super::*;

    mod when_registering_new_dialects {
        use super::*;

        fn noop_parser(_v: &Value) -> Result<IrRequest, DialectError> {
            Ok(IrRequest::new(vec![]))
        }

        fn noop_serializer(_ir: &IrRequest) -> Result<Value, DialectError> {
            Ok(json!({}))
        }

        #[test]
        fn register_replaces_existing_entry() {
            let mut reg = DialectRegistry::with_builtins();
            let original = reg.get(Dialect::OpenAi).unwrap().version;

            reg.register(DialectEntry {
                dialect: Dialect::OpenAi,
                name: "openai",
                version: "v2-custom",
                parser: noop_parser,
                serializer: noop_serializer,
            });

            let updated = reg.get(Dialect::OpenAi).unwrap();
            assert_eq!(updated.version, "v2-custom");
            assert_ne!(updated.version, original);
            assert_eq!(reg.len(), 6, "count should not change on replace");
        }

        #[test]
        fn register_on_empty_registry_adds_entry() {
            let mut reg = DialectRegistry::new();
            assert!(reg.is_empty());

            reg.register(DialectEntry {
                dialect: Dialect::Kimi,
                name: "kimi",
                version: "v1",
                parser: noop_parser,
                serializer: noop_serializer,
            });

            assert_eq!(reg.len(), 1);
            assert!(reg.get(Dialect::Kimi).is_some());
        }

        #[test]
        fn parse_on_unregistered_dialect_returns_error() {
            let reg = DialectRegistry::new();
            let err = reg.parse(Dialect::OpenAi, &json!({})).unwrap_err();
            assert_eq!(err.dialect, Dialect::OpenAi);
            assert!(err.message.contains("not registered"));
        }

        #[test]
        fn serialize_on_unregistered_dialect_returns_error() {
            let reg = DialectRegistry::new();
            let ir = IrRequest::new(vec![]);
            let err = reg.serialize(Dialect::Claude, &ir).unwrap_err();
            assert_eq!(err.dialect, Dialect::Claude);
        }
    }

    mod when_querying_supported_pairs {
        use super::*;

        #[test]
        fn default_registry_has_standard_dialects() {
            let reg = DialectRegistry::with_builtins();
            let dialects = reg.list_dialects();
            assert!(dialects.contains(&Dialect::OpenAi));
            assert!(dialects.contains(&Dialect::Claude));
            assert!(dialects.contains(&Dialect::Gemini));
            assert!(dialects.contains(&Dialect::Codex));
            assert!(dialects.contains(&Dialect::Kimi));
            assert!(dialects.contains(&Dialect::Copilot));
        }

        #[test]
        fn supports_pair_when_both_registered() {
            let reg = DialectRegistry::with_builtins();
            assert!(reg.supports_pair(Dialect::OpenAi, Dialect::Claude));
            assert!(reg.supports_pair(Dialect::Claude, Dialect::Gemini));
            assert!(reg.supports_pair(Dialect::Codex, Dialect::Kimi));
        }

        #[test]
        fn supports_pair_is_false_when_source_missing() {
            let mut reg = DialectRegistry::new();
            reg.register(DialectEntry {
                dialect: Dialect::OpenAi,
                name: "openai",
                version: "v1",
                parser: |_| Ok(IrRequest::new(vec![])),
                serializer: |_| Ok(json!({})),
            });
            assert!(!reg.supports_pair(Dialect::Claude, Dialect::OpenAi));
        }

        #[test]
        fn supports_pair_is_false_when_target_missing() {
            let mut reg = DialectRegistry::new();
            reg.register(DialectEntry {
                dialect: Dialect::OpenAi,
                name: "openai",
                version: "v1",
                parser: |_| Ok(IrRequest::new(vec![])),
                serializer: |_| Ok(json!({})),
            });
            assert!(!reg.supports_pair(Dialect::OpenAi, Dialect::Claude));
        }

        #[test]
        fn supports_pair_reflexive() {
            let reg = DialectRegistry::with_builtins();
            for &d in Dialect::all() {
                assert!(reg.supports_pair(d, d), "{d:?} → {d:?} should be supported");
            }
        }

        #[test]
        fn empty_registry_supports_no_pairs() {
            let reg = DialectRegistry::new();
            assert!(!reg.supports_pair(Dialect::OpenAi, Dialect::Claude));
        }

        #[test]
        fn list_dialects_is_deterministic() {
            let reg = DialectRegistry::with_builtins();
            let a = reg.list_dialects();
            let b = reg.list_dialects();
            assert_eq!(a, b, "list_dialects should be deterministic");
        }

        #[test]
        fn builtin_entries_have_correct_names() {
            let reg = DialectRegistry::with_builtins();
            let cases = [
                (Dialect::OpenAi, "openai"),
                (Dialect::Claude, "claude"),
                (Dialect::Gemini, "gemini"),
                (Dialect::Codex, "codex"),
                (Dialect::Kimi, "kimi"),
                (Dialect::Copilot, "copilot"),
            ];
            for (dialect, expected_name) in cases {
                let entry = reg.get(dialect).unwrap();
                assert_eq!(entry.name, expected_name);
            }
        }
    }
}
