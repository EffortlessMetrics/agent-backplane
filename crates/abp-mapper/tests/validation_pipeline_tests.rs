// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests for the mapping validation pipeline.

use abp_dialect::Dialect;
use abp_mapper::validation::{
    DefaultMappingValidator, MappingValidator, RoundtripResult, ValidationPipeline,
    ValidationResult, ValidationSeverity,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn v() -> DefaultMappingValidator {
    DefaultMappingValidator::new()
}

fn valid_openai() -> serde_json::Value {
    json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    })
}

fn valid_claude() -> serde_json::Value {
    json!({
        "model": "claude-3-sonnet",
        "messages": [{"role": "user", "content": "hello"}],
        "max_tokens": 1024
    })
}

fn valid_gemini() -> serde_json::Value {
    json!({
        "model": "gemini-pro",
        "contents": [{"parts": [{"text": "hello"}]}]
    })
}

// ── Pre-mapping validation ──────────────────────────────────────────────

#[test]
fn pre_mapping_openai_valid() {
    let r = v().validate_pre_mapping(Dialect::OpenAi, &valid_openai());
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
    assert_eq!(r.error_count(), 0);
}

#[test]
fn pre_mapping_claude_valid() {
    let r = v().validate_pre_mapping(Dialect::Claude, &valid_claude());
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn pre_mapping_gemini_valid() {
    let r = v().validate_pre_mapping(Dialect::Gemini, &valid_gemini());
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn pre_mapping_openai_missing_model() {
    let req = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = v().validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "model"));
    assert_eq!(r.field_coverage, 50.0);
}

#[test]
fn pre_mapping_openai_missing_messages() {
    let req = json!({"model": "gpt-4"});
    let r = v().validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "messages"));
}

#[test]
fn pre_mapping_claude_missing_max_tokens() {
    let req = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]});
    let r = v().validate_pre_mapping(Dialect::Claude, &req);
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field == "max_tokens" && i.code == "missing_required_field")
    );
    // 2 out of 3 required fields
    let expected_coverage = (2.0 / 3.0) * 100.0;
    assert!((r.field_coverage - expected_coverage).abs() < 0.1);
}

#[test]
fn pre_mapping_gemini_missing_contents() {
    let req = json!({"model": "gemini-pro"});
    let r = v().validate_pre_mapping(Dialect::Gemini, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "contents"));
}

#[test]
fn pre_mapping_non_object_rejected() {
    let r = v().validate_pre_mapping(Dialect::OpenAi, &json!("string"));
    assert!(!r.is_valid());
    assert_eq!(r.field_coverage, 0.0);
    assert_eq!(r.issues[0].code, "invalid_type");
}

#[test]
fn pre_mapping_empty_messages_warns() {
    let req = json!({"model": "gpt-4", "messages": []});
    let r = v().validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid()); // only warning, not error
    assert!(r.warning_count() > 0);
    assert!(
        r.issues
            .iter()
            .any(|i| i.severity == ValidationSeverity::Warning && i.code == "empty_messages")
    );
}

#[test]
fn pre_mapping_codex_valid() {
    let req = json!({"model": "codex", "messages": [{"role": "user", "content": "code"}]});
    let r = v().validate_pre_mapping(Dialect::Codex, &req);
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn pre_mapping_kimi_valid() {
    let req = json!({"model": "kimi", "messages": [{"role": "user", "content": "hello"}]});
    let r = v().validate_pre_mapping(Dialect::Kimi, &req);
    assert!(r.is_valid());
}

#[test]
fn pre_mapping_copilot_valid() {
    let req = json!({"model": "copilot", "messages": [{"role": "user", "content": "hi"}]});
    let r = v().validate_pre_mapping(Dialect::Copilot, &req);
    assert!(r.is_valid());
}

// ── Post-mapping validation ─────────────────────────────────────────────

#[test]
fn post_mapping_openai_valid() {
    let r = v().validate_post_mapping(Dialect::OpenAi, &valid_openai());
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn post_mapping_claude_missing_field() {
    let mapped = json!({"model": "claude-3", "messages": []});
    let r = v().validate_post_mapping(Dialect::Claude, &mapped);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "max_tokens"));
}

#[test]
fn post_mapping_non_object() {
    let r = v().validate_post_mapping(Dialect::OpenAi, &json!(null));
    assert!(!r.is_valid());
    assert_eq!(r.field_coverage, 0.0);
}

// ── Roundtrip validation ────────────────────────────────────────────────

#[test]
fn roundtrip_identical_objects() {
    let val = valid_openai();
    let r = v().validate_roundtrip(&val, &val);
    assert!(r.is_lossless());
    assert!(r.equivalent);
    assert!(r.lost_fields.is_empty());
    assert!(r.added_fields.is_empty());
    assert!(r.changed_fields.is_empty());
}

#[test]
fn roundtrip_lost_field() {
    let orig = json!({"model": "gpt-4", "temperature": 0.7, "top_p": 0.9});
    let rt = json!({"model": "gpt-4"});
    let r = v().validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.lost_fields.contains(&"temperature".to_string()));
    assert!(r.lost_fields.contains(&"top_p".to_string()));
}

#[test]
fn roundtrip_added_field() {
    let orig = json!({"model": "gpt-4"});
    let rt = json!({"model": "gpt-4", "extra_field": "added"});
    let r = v().validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.added_fields.contains(&"extra_field".to_string()));
}

#[test]
fn roundtrip_changed_value() {
    let orig = json!({"model": "gpt-4", "temperature": 0.7});
    let rt = json!({"model": "gpt-4", "temperature": 1.0});
    let r = v().validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"temperature".to_string()));
    assert!(r.lost_fields.is_empty());
    assert!(r.added_fields.is_empty());
}

#[test]
fn roundtrip_nested_diff() {
    let orig = json!({"config": {"a": 1, "b": 2}});
    let rt = json!({"config": {"a": 1, "b": 3}});
    let r = v().validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"config.b".to_string()));
}

#[test]
fn roundtrip_array_diff() {
    let orig = json!({"items": [1, 2, 3]});
    let rt = json!({"items": [1, 2, 4]});
    let r = v().validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"items[2]".to_string()));
}

#[test]
fn roundtrip_empty_objects() {
    let r = v().validate_roundtrip(&json!({}), &json!({}));
    assert!(r.is_lossless());
}

// ── ValidationPipeline ──────────────────────────────────────────────────

#[test]
fn pipeline_openai_identity_pass() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::OpenAi);
    let req = valid_openai();
    let result = pipe.run(&req, |v| Ok(v.clone()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_pre_fail_skips_mapping() {
    let pipe = ValidationPipeline::new(v(), Dialect::Claude, Dialect::OpenAi);
    // Missing max_tokens → pre-validation fails
    let req = json!({"model": "claude-3", "messages": []});
    let result = pipe.run(&req, |_| panic!("map_fn should not be called"));
    assert!(!result.pre.is_valid());
    assert!(result.mapped.is_none());
    assert!(result.post.is_none());
}

#[test]
fn pipeline_mapping_error_reported() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Claude);
    let req = valid_openai();
    let result = pipe.run(&req, |_| Err("unsupported".into()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_none());
    let post = result.post.unwrap();
    assert!(!post.is_valid());
    assert_eq!(post.issues[0].code, "mapping_failed");
    assert!(post.issues[0].message.contains("unsupported"));
}

#[test]
fn pipeline_post_validation_catches_bad_output() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Claude);
    let req = valid_openai();
    // Map to something that's missing Claude's required max_tokens
    let result = pipe.run(&req, |_| {
        Ok(json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]}))
    });
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    let post = result.post.unwrap();
    assert!(!post.is_valid());
    assert!(post.issues.iter().any(|i| i.field == "max_tokens"));
}

#[test]
fn pipeline_cross_dialect_success() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Claude);
    let req = valid_openai();
    let result = pipe.run(&req, |_| Ok(valid_claude()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_gemini_source() {
    let pipe = ValidationPipeline::new(v(), Dialect::Gemini, Dialect::OpenAi);
    let req = valid_gemini();
    let result = pipe.run(&req, |_| Ok(valid_openai()));
    assert!(result.pre.is_valid());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_validator_accessor() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Claude);
    let _validator = pipe.validator();
    assert_eq!(pipe.source, Dialect::OpenAi);
    assert_eq!(pipe.target, Dialect::Claude);
}

// ── Serde roundtrips on result types ────────────────────────────────────

#[test]
fn validation_result_serde_roundtrip() {
    let result = ValidationResult {
        valid: true,
        issues: vec![],
        field_coverage: 100.0,
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: ValidationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.valid, true);
    assert_eq!(back.field_coverage, 100.0);
}

#[test]
fn roundtrip_result_serde() {
    let result = RoundtripResult {
        equivalent: false,
        lost_fields: vec!["temperature".into()],
        added_fields: vec![],
        changed_fields: vec!["model".into()],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: RoundtripResult = serde_json::from_str(&json).unwrap();
    assert!(!back.equivalent);
    assert_eq!(back.lost_fields, vec!["temperature"]);
    assert_eq!(back.changed_fields, vec!["model"]);
}

// ── Edge cases ──────────────────────────────────────────────────────────

#[test]
fn pre_mapping_all_fields_missing() {
    let r = v().validate_pre_mapping(Dialect::OpenAi, &json!({}));
    assert!(!r.is_valid());
    assert_eq!(r.field_coverage, 0.0);
    assert_eq!(r.error_count(), 2); // model + messages
}

#[test]
fn roundtrip_deeply_nested() {
    let orig = json!({"a": {"b": {"c": {"d": 42}}}});
    let rt = json!({"a": {"b": {"c": {"d": 99}}}});
    let r = v().validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"a.b.c.d".to_string()));
}

#[test]
fn gemini_empty_contents_warns() {
    let req = json!({"model": "gemini-pro", "contents": []});
    let r = v().validate_pre_mapping(Dialect::Gemini, &req);
    assert!(r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.severity == ValidationSeverity::Warning && i.code == "empty_messages")
    );
}
