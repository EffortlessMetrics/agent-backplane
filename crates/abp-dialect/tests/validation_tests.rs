// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for `abp_dialect::validate`.

use abp_dialect::Dialect;
use abp_dialect::validate::{RequestValidator, Severity, ValidationIssue, ValidationResult};
use serde_json::json;

fn v() -> RequestValidator {
    RequestValidator::new()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Non-object inputs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn non_object_string_is_error() {
    let r = v().validate(Dialect::OpenAi, &json!("hello"));
    assert!(!r.is_valid());
    assert_eq!(r.issues[0].code, "invalid_type");
}

#[test]
fn non_object_number_is_error() {
    let r = v().validate(Dialect::Claude, &json!(42));
    assert!(!r.is_valid());
}

#[test]
fn non_object_null_is_error() {
    let r = v().validate(Dialect::Gemini, &json!(null));
    assert!(!r.is_valid());
}

#[test]
fn non_object_array_is_error() {
    let r = v().validate(Dialect::Codex, &json!([1, 2, 3]));
    assert!(!r.is_valid());
    assert_eq!(r.error_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. OpenAI validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_valid_request() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        }),
    );
    assert!(r.is_valid());
    assert_eq!(r.error_count(), 0);
}

#[test]
fn openai_missing_model() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field == "model" && i.code == "missing_required_field")
    );
}

#[test]
fn openai_missing_messages() {
    let r = v().validate(Dialect::OpenAi, &json!({"model": "gpt-4"}));
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "messages"));
}

#[test]
fn openai_messages_not_array() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": "oops"}),
    );
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.code == "invalid_field_type" && i.field == "messages")
    );
}

#[test]
fn openai_message_missing_role() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field.contains("role")));
}

#[test]
fn openai_empty_messages_warning() {
    let r = v().validate(Dialect::OpenAi, &json!({"model": "gpt-4", "messages": []}));
    assert!(r.is_valid()); // warning only
    assert!(r.has_warnings());
    assert!(r.issues.iter().any(|i| i.code == "empty_messages"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Claude validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_valid_request() {
    let r = v().validate(
        Dialect::Claude,
        &json!({
            "model": "claude-3-opus-20240229",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024
        }),
    );
    assert!(r.is_valid());
}

#[test]
fn claude_missing_max_tokens() {
    let r = v().validate(
        Dialect::Claude,
        &json!({
            "model": "claude-3-opus",
            "messages": [{"role": "user", "content": "hi"}]
        }),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "max_tokens"));
}

#[test]
fn claude_max_tokens_wrong_type() {
    let r = v().validate(
        Dialect::Claude,
        &json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": "lots"
        }),
    );
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field == "max_tokens" && i.code == "invalid_field_type")
    );
}

#[test]
fn claude_bad_content_type() {
    let r = v().validate(
        Dialect::Claude,
        &json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": 42}],
            "max_tokens": 100
        }),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field.contains("content")));
}

#[test]
fn claude_content_as_array_is_valid() {
    let r = v().validate(
        Dialect::Claude,
        &json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
            "max_tokens": 100
        }),
    );
    assert!(r.is_valid());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Gemini validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_valid_request() {
    let r = v().validate(
        Dialect::Gemini,
        &json!({
            "model": "gemini-pro",
            "contents": [{"parts": [{"text": "hello"}]}]
        }),
    );
    assert!(r.is_valid());
}

#[test]
fn gemini_missing_contents() {
    let r = v().validate(Dialect::Gemini, &json!({"model": "gemini-pro"}));
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "contents"));
}

#[test]
fn gemini_content_missing_parts() {
    let r = v().validate(
        Dialect::Gemini,
        &json!({"model": "gemini-pro", "contents": [{"role": "user"}]}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field.contains("parts")));
}

#[test]
fn gemini_parts_not_array() {
    let r = v().validate(
        Dialect::Gemini,
        &json!({"model": "gemini-pro", "contents": [{"parts": "oops"}]}),
    );
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field.contains("parts") && i.code == "invalid_field_type")
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Codex validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_valid_request() {
    let r = v().validate(
        Dialect::Codex,
        &json!({
            "model": "codex-mini",
            "messages": [{"role": "user", "content": "fix bug"}]
        }),
    );
    assert!(r.is_valid());
}

#[test]
fn codex_missing_model_and_messages() {
    let r = v().validate(Dialect::Codex, &json!({}));
    assert!(!r.is_valid());
    assert!(r.error_count() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Kimi validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_valid_request() {
    let r = v().validate(
        Dialect::Kimi,
        &json!({
            "model": "kimi",
            "messages": [{"role": "user", "content": "search"}]
        }),
    );
    assert!(r.is_valid());
}

#[test]
fn kimi_missing_messages() {
    let r = v().validate(Dialect::Kimi, &json!({"model": "kimi"}));
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "messages"));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Copilot validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_valid_request() {
    let r = v().validate(
        Dialect::Copilot,
        &json!({
            "model": "copilot-chat",
            "messages": [{"role": "user", "content": "help"}]
        }),
    );
    assert!(r.is_valid());
}

#[test]
fn copilot_missing_model() {
    let r = v().validate(
        Dialect::Copilot,
        &json!({"messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "model"));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Model name validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn model_name_empty_is_error() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "empty_model_name"));
}

#[test]
fn model_name_with_spaces_is_warning() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt 4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.is_valid()); // warning only
    assert!(r.has_warnings());
    assert!(r.issues.iter().any(|i| i.code == "model_name_has_spaces"));
}

#[test]
fn model_not_a_string_is_error() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": 42, "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field == "model" && i.code == "invalid_field_type")
    );
}

#[test]
fn model_name_long_is_warning() {
    let long = "a".repeat(300);
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": long, "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "long_model_name"));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_result_serde_roundtrip() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    let json = serde_json::to_string(&r).unwrap();
    let back: ValidationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.issues.len(), r.issues.len());
}

#[test]
fn validation_issue_serde_roundtrip() {
    let issue = ValidationIssue {
        severity: Severity::Error,
        field: "model".into(),
        message: "missing".into(),
        code: "missing_required_field".into(),
    };
    let json = serde_json::to_string(&issue).unwrap();
    let back: ValidationIssue = serde_json::from_str(&json).unwrap();
    assert_eq!(back, issue);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Counting helpers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_and_warning_counts() {
    // Claude: missing max_tokens (error), empty messages (warning)
    let r = v().validate(
        Dialect::Claude,
        &json!({"model": "claude-3", "messages": []}),
    );
    assert!(r.error_count() >= 1);
    assert!(r.warning_count() >= 1);
}

#[test]
fn info_count_is_zero_by_default() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(r.info_count(), 0);
}

#[test]
fn role_not_a_string_is_error() {
    let r = v().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"role": 42, "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field.contains("role") && i.code == "invalid_field_type")
    );
}
