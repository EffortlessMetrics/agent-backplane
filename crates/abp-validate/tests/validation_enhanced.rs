// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for dialect validation, custom rule builder, composite validator,
//! and validation result aggregation.

use abp_validate::{
    CompositeValidator, DialectRequestValidator, DialectResponseValidator, RuleBuilder,
    SchemaValidator, ValidationErrorKind, ValidationErrors, Validator, WorkOrderValidator,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// DialectRequestValidator — OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_valid_request_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);
    let req = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    assert!(v.validate(&req).is_ok());
}

#[test]
fn openai_missing_model_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);
    let req = json!({
        "messages": [{"role": "user", "content": "hello"}]
    });
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "model"));
}

#[test]
fn openai_missing_messages_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);
    let req = json!({"model": "gpt-4"});
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "messages"));
}

#[test]
fn openai_message_without_role_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);
    let req = json!({
        "model": "gpt-4",
        "messages": [{"content": "hello"}]
    });
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("role")));
}

#[test]
fn openai_messages_not_array_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);
    let req = json!({"model": "gpt-4", "messages": "not-array"});
    let err = v.validate(&req).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "messages" && e.kind == ValidationErrorKind::InvalidFormat)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectRequestValidator — Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_valid_request_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Claude);
    let req = json!({
        "model": "claude-3-opus-20240229",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hello"}]
    });
    assert!(v.validate(&req).is_ok());
}

#[test]
fn claude_missing_max_tokens_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Claude);
    let req = json!({
        "model": "claude-3-opus-20240229",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "max_tokens"));
}

#[test]
fn claude_content_wrong_type_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Claude);
    let req = json!({
        "model": "claude-3-opus-20240229",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": 42}]
    });
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("content")));
}

#[test]
fn claude_content_array_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Claude);
    let req = json!({
        "model": "claude-3-opus-20240229",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    assert!(v.validate(&req).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectRequestValidator — Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_valid_request_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Gemini);
    let req = json!({
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    assert!(v.validate(&req).is_ok());
}

#[test]
fn gemini_missing_contents_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Gemini);
    let req = json!({"model": "gemini-pro"});
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "contents"));
}

#[test]
fn gemini_content_missing_parts_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Gemini);
    let req = json!({
        "contents": [{"role": "user"}]
    });
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("parts")));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectRequestValidator — Codex
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_valid_request_with_input_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Codex);
    let req = json!({"model": "codex", "input": "do stuff"});
    assert!(v.validate(&req).is_ok());
}

#[test]
fn codex_valid_request_with_messages_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Codex);
    let req = json!({"model": "codex", "messages": [{"role": "user", "content": "hi"}]});
    assert!(v.validate(&req).is_ok());
}

#[test]
fn codex_missing_input_and_messages_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Codex);
    let req = json!({"model": "codex"});
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "input"));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectRequestValidator — Kimi & Copilot
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_valid_request_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Kimi);
    let req = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "hello"}]
    });
    assert!(v.validate(&req).is_ok());
}

#[test]
fn kimi_missing_model_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Kimi);
    let req = json!({"messages": [{"role": "user", "content": "hello"}]});
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "model"));
}

#[test]
fn copilot_valid_request_passes() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Copilot);
    let req = json!({
        "messages": [{"role": "user", "content": "hello"}]
    });
    assert!(v.validate(&req).is_ok());
}

#[test]
fn copilot_missing_messages_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Copilot);
    let req = json!({"agent_mode": true});
    let err = v.validate(&req).unwrap_err();
    assert!(err.iter().any(|e| e.path == "messages"));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectRequestValidator — Non-object input
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_request_non_object_fails() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);
    let req = json!("just a string");
    let err = v.validate(&req).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn dialect_accessor_returns_correct_dialect() {
    let v = DialectRequestValidator::new(abp_dialect::Dialect::Claude);
    assert_eq!(v.dialect(), abp_dialect::Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectResponseValidator — OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_valid_response_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::OpenAi);
    let resp = json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}]
    });
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn openai_response_missing_choices_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::OpenAi);
    let resp = json!({"model": "gpt-4"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "choices"));
}

#[test]
fn openai_response_error_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::OpenAi);
    let resp = json!({"error": {"message": "rate limited"}});
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn openai_response_choice_missing_message_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::OpenAi);
    let resp = json!({
        "choices": [{"index": 0}]
    });
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("choices")));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectResponseValidator — Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_valid_response_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Claude);
    let resp = json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "hi"}]
    });
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn claude_response_missing_type_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Claude);
    let resp = json!({"content": [{"type": "text", "text": "hi"}]});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "type"));
}

#[test]
fn claude_response_error_type_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Claude);
    let resp = json!({"type": "error", "error": {"message": "bad request"}});
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn claude_response_streaming_type_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Claude);
    for t in &[
        "message_start",
        "content_block_start",
        "content_block_delta",
        "content_block_stop",
        "message_delta",
        "message_stop",
        "ping",
    ] {
        let resp = json!({"type": t});
        assert!(
            v.validate(&resp).is_ok(),
            "streaming type '{t}' should pass"
        );
    }
}

#[test]
fn claude_response_unknown_type_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Claude);
    let resp = json!({"type": "bogus"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "type"));
}

#[test]
fn claude_message_missing_content_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Claude);
    let resp = json!({"type": "message", "role": "assistant"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "content"));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectResponseValidator — Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_valid_response_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Gemini);
    let resp = json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
    });
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn gemini_response_missing_candidates_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Gemini);
    let resp = json!({"model": "gemini-pro"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "candidates"));
}

#[test]
fn gemini_response_candidate_missing_content_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Gemini);
    let resp = json!({
        "candidates": [{"index": 0}]
    });
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("content")));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectResponseValidator — Codex
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_valid_response_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Codex);
    let resp = json!({
        "output": [{"type": "message", "content": "done"}],
        "status": "completed"
    });
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn codex_response_missing_output_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Codex);
    let resp = json!({"status": "completed"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "output"));
}

#[test]
fn codex_response_bad_status_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Codex);
    let resp = json!({"output": [], "status": "bogus"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "status"));
}

// ═══════════════════════════════════════════════════════════════════════════
// DialectResponseValidator — Kimi & Copilot
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_valid_response_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Kimi);
    let resp = json!({"choices": [{"message": {"role": "assistant"}}]});
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn copilot_valid_response_passes() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Copilot);
    let resp = json!({"choices": [{"message": {"role": "assistant"}}]});
    assert!(v.validate(&resp).is_ok());
}

#[test]
fn copilot_response_missing_choices_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Copilot);
    let resp = json!({"model": "copilot"});
    let err = v.validate(&resp).unwrap_err();
    assert!(err.iter().any(|e| e.path == "choices"));
}

#[test]
fn dialect_response_non_object_fails() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::OpenAi);
    let resp = json!(42);
    let err = v.validate(&resp).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn dialect_response_accessor_returns_dialect() {
    let v = DialectResponseValidator::new(abp_dialect::Dialect::Gemini);
    assert_eq!(v.dialect(), abp_dialect::Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════════
// RuleBuilder & CustomValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rule_builder_require_field_passes() {
    let v = RuleBuilder::new().require_field("name").build();
    let val = json!({"name": "test"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_require_field_missing_fails() {
    let v = RuleBuilder::new().require_field("name").build();
    let val = json!({"other": "value"});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "name"));
}

#[test]
fn rule_builder_require_field_null_fails() {
    let v = RuleBuilder::new().require_field("name").build();
    let val = json!({"name": null});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "name"));
}

#[test]
fn rule_builder_require_string_passes() {
    let v = RuleBuilder::new().require_string("version").build();
    let val = json!({"version": "1.0"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_require_string_wrong_type_fails() {
    let v = RuleBuilder::new().require_string("version").build();
    let val = json!({"version": 42});
    let err = v.validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "version" && e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn rule_builder_require_array_passes() {
    let v = RuleBuilder::new().require_array("items").build();
    let val = json!({"items": [1, 2, 3]});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_require_array_wrong_type_fails() {
    let v = RuleBuilder::new().require_array("items").build();
    let val = json!({"items": "not-array"});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "items"));
}

#[test]
fn rule_builder_require_array_missing_fails() {
    let v = RuleBuilder::new().require_array("items").build();
    let val = json!({"other": true});
    let err = v.validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "items" && e.kind == ValidationErrorKind::Required)
    );
}

#[test]
fn rule_builder_number_range_passes() {
    let v = RuleBuilder::new()
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"score": 50});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_number_range_below_min_fails() {
    let v = RuleBuilder::new()
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"score": -1});
    let err = v.validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "score" && e.kind == ValidationErrorKind::OutOfRange)
    );
}

#[test]
fn rule_builder_number_range_above_max_fails() {
    let v = RuleBuilder::new()
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"score": 101});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "score"));
}

#[test]
fn rule_builder_number_range_not_number_fails() {
    let v = RuleBuilder::new()
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"score": "high"});
    let err = v.validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "score" && e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn rule_builder_number_range_null_skipped() {
    let v = RuleBuilder::new()
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"score": null});
    // null is silently skipped for range check (field not required by this rule)
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_number_range_absent_skipped() {
    let v = RuleBuilder::new()
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"other": 1});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_one_of_passes() {
    let v = RuleBuilder::new()
        .require_one_of("status", &["active", "inactive"])
        .build();
    let val = json!({"status": "active"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_one_of_fails() {
    let v = RuleBuilder::new()
        .require_one_of("status", &["active", "inactive"])
        .build();
    let val = json!({"status": "unknown"});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "status"));
}

#[test]
fn rule_builder_custom_rule_passes() {
    let v = RuleBuilder::new()
        .custom("positive_count", |val, errs| {
            if let Some(n) = val.get("count").and_then(|v| v.as_i64()) {
                if n <= 0 {
                    errs.add(
                        "count",
                        ValidationErrorKind::OutOfRange,
                        "count must be positive",
                    );
                }
            }
        })
        .build();
    let val = json!({"count": 5});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn rule_builder_custom_rule_fails() {
    let v = RuleBuilder::new()
        .custom("positive_count", |val, errs| {
            if let Some(n) = val.get("count").and_then(|v| v.as_i64()) {
                if n <= 0 {
                    errs.add(
                        "count",
                        ValidationErrorKind::OutOfRange,
                        "count must be positive",
                    );
                }
            }
        })
        .build();
    let val = json!({"count": -3});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "count"));
}

#[test]
fn rule_builder_multiple_rules_accumulate() {
    let v = RuleBuilder::new()
        .require_string("name")
        .require_array("items")
        .require_number_range("score", 0.0, 100.0)
        .build();
    let val = json!({"name": 42, "items": "not-array", "score": 200});
    let err = v.validate(&val).unwrap_err();
    assert!(err.len() >= 3);
}

#[test]
fn rule_builder_non_object_input_fails() {
    let v = RuleBuilder::new().require_field("name").build();
    let val = json!("string");
    let err = v.validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn rule_builder_empty_produces_valid() {
    let v = RuleBuilder::new().build();
    let val = json!({"anything": "goes"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn custom_validator_rule_count() {
    let v = RuleBuilder::new()
        .require_field("a")
        .require_string("b")
        .require_array("c")
        .build();
    assert_eq!(v.rule_count(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// CompositeValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn composite_empty_passes() {
    let cv: CompositeValidator<serde_json::Value> = CompositeValidator::new();
    let val = json!({"anything": true});
    assert!(cv.validate(&val).is_ok());
}

#[test]
fn composite_single_validator_passes() {
    let cv = CompositeValidator::new().add(SchemaValidator::new(vec![(
        "name".into(),
        abp_validate::JsonType::String,
    )]));
    let val = json!({"name": "test"});
    assert!(cv.validate(&val).is_ok());
}

#[test]
fn composite_single_validator_fails() {
    let cv = CompositeValidator::new().add(SchemaValidator::new(vec![(
        "name".into(),
        abp_validate::JsonType::String,
    )]));
    let val = json!({"other": true});
    let err = cv.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "name"));
}

#[test]
fn composite_multiple_validators_aggregate() {
    let cv = CompositeValidator::new()
        .add(SchemaValidator::new(vec![(
            "id".into(),
            abp_validate::JsonType::String,
        )]))
        .add(SchemaValidator::new(vec![(
            "count".into(),
            abp_validate::JsonType::Number,
        )]));

    let val = json!({"unrelated": true});
    let err = cv.validate(&val).unwrap_err();
    // Both "id" missing and "count" missing
    assert!(err.iter().any(|e| e.path == "id"));
    assert!(err.iter().any(|e| e.path == "count"));
    assert!(err.len() >= 2);
}

#[test]
fn composite_all_pass_returns_ok() {
    let cv = CompositeValidator::new()
        .add(SchemaValidator::new(vec![(
            "id".into(),
            abp_validate::JsonType::String,
        )]))
        .add(SchemaValidator::new(vec![(
            "count".into(),
            abp_validate::JsonType::Number,
        )]));
    let val = json!({"id": "abc", "count": 42});
    assert!(cv.validate(&val).is_ok());
}

#[test]
fn composite_len_and_is_empty() {
    let cv: CompositeValidator<serde_json::Value> = CompositeValidator::new();
    assert!(cv.is_empty());
    assert_eq!(cv.len(), 0);

    let cv = cv.add(SchemaValidator::new(vec![]));
    assert!(!cv.is_empty());
    assert_eq!(cv.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// ValidationErrors — merge, filter, format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_errors_merge() {
    let mut a = ValidationErrors::new();
    a.add("field_a", ValidationErrorKind::Required, "missing a");

    let mut b = ValidationErrors::new();
    b.add("field_b", ValidationErrorKind::Custom, "bad b");
    b.add("field_c", ValidationErrorKind::OutOfRange, "too big c");

    a.merge(b);
    assert_eq!(a.len(), 3);
    assert!(a.iter().any(|e| e.path == "field_a"));
    assert!(a.iter().any(|e| e.path == "field_b"));
    assert!(a.iter().any(|e| e.path == "field_c"));
}

#[test]
fn validation_errors_merge_empty() {
    let mut a = ValidationErrors::new();
    a.add("x", ValidationErrorKind::Required, "missing");
    a.merge(ValidationErrors::new());
    assert_eq!(a.len(), 1);
}

#[test]
fn validation_errors_filter_by_kind() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "missing a");
    errs.add("b", ValidationErrorKind::InvalidFormat, "bad b");
    errs.add("c", ValidationErrorKind::Required, "missing c");

    let required = errs.filter_by_kind(&ValidationErrorKind::Required);
    assert_eq!(required.len(), 2);
    assert!(
        required
            .iter()
            .all(|e| e.kind == ValidationErrorKind::Required)
    );
}

#[test]
fn validation_errors_filter_by_path_prefix() {
    let mut errs = ValidationErrors::new();
    errs.add("config.budget", ValidationErrorKind::OutOfRange, "too high");
    errs.add(
        "config.model",
        ValidationErrorKind::InvalidFormat,
        "bad model",
    );
    errs.add("task", ValidationErrorKind::Required, "missing task");

    let config_errs = errs.filter_by_path_prefix("config");
    assert_eq!(config_errs.len(), 2);
}

#[test]
fn validation_errors_format_report_empty() {
    let errs = ValidationErrors::new();
    assert_eq!(errs.format_report(), "No validation errors.");
}

#[test]
fn validation_errors_format_report_with_errors() {
    let mut errs = ValidationErrors::new();
    errs.add("name", ValidationErrorKind::Required, "missing name");
    errs.add(
        "score",
        ValidationErrorKind::OutOfRange,
        "score out of range",
    );

    let report = errs.format_report();
    assert!(report.contains("2 validation error(s):"));
    assert!(report.contains("[name]"));
    assert!(report.contains("[score]"));
    assert!(report.contains("(required)"));
    assert!(report.contains("(out_of_range)"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Schema validation — additional coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn schema_validator_combined_with_dialect() {
    // Schema checks structure, dialect checks semantics
    let schema = SchemaValidator::new(vec![
        ("model".into(), abp_validate::JsonType::String),
        ("messages".into(), abp_validate::JsonType::Array),
    ]);
    let dialect = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);

    let val = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });

    assert!(schema.validate(&val).is_ok());
    assert!(dialect.validate(&val).is_ok());
}

#[test]
fn schema_plus_dialect_both_find_errors() {
    let schema = SchemaValidator::new(vec![
        ("model".into(), abp_validate::JsonType::String),
        ("messages".into(), abp_validate::JsonType::Array),
    ]);
    let dialect = DialectRequestValidator::new(abp_dialect::Dialect::OpenAi);

    let val = json!({"unrelated": true});

    let schema_err = schema.validate(&val).unwrap_err();
    let dialect_err = dialect.validate(&val).unwrap_err();

    assert!(schema_err.len() >= 2);
    assert!(dialect_err.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Composite with WorkOrderValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn composite_with_work_order_validator() {
    use abp_core::WorkOrderBuilder;

    let cv = CompositeValidator::new().add(WorkOrderValidator);

    let wo = WorkOrderBuilder::new("do stuff").build();
    assert!(cv.validate(&wo).is_ok());

    let bad_wo = WorkOrderBuilder::new("").build();
    let err = cv.validate(&bad_wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
}

// ═══════════════════════════════════════════════════════════════════════════
// All dialects request validation — ensure every dialect handles non-object
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_dialect_request_validators_reject_non_object() {
    for &d in abp_dialect::Dialect::all() {
        let v = DialectRequestValidator::new(d);
        let err = v.validate(&json!(null)).unwrap_err();
        assert!(
            !err.is_empty(),
            "dialect {d:?} should reject non-object input"
        );
    }
}

#[test]
fn all_dialect_response_validators_reject_non_object() {
    for &d in abp_dialect::Dialect::all() {
        let v = DialectResponseValidator::new(d);
        let err = v.validate(&json!([])).unwrap_err();
        assert!(
            !err.is_empty(),
            "dialect {d:?} should reject non-object input"
        );
    }
}
