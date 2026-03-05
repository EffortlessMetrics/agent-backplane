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
//! Integration tests for the cross-dialect conversion module.

use abp_sdk_types::convert::{
    ConversionError, ConversionReport, DialectConverter, Message, RoleMapper, ToolDefinition,
};
use abp_sdk_types::Dialect;

// ── Helper: a concrete converter for testing the trait ──────────────────

/// Trivial pass-through converter used to exercise the trait interface.
struct PassthroughConverter {
    source: Dialect,
    target: Dialect,
}

impl DialectConverter for PassthroughConverter {
    fn source_dialect(&self) -> Dialect {
        self.source
    }
    fn target_dialect(&self) -> Dialect {
        self.target
    }
    fn convert_message(&self, msg: &Message) -> Result<Message, ConversionError> {
        let role = RoleMapper::map_role(&msg.role, self.source, self.target)?;
        Ok(Message {
            role,
            content: msg.content.clone(),
            tool_call_id: msg.tool_call_id.clone(),
        })
    }
    fn convert_tool(&self, tool: &ToolDefinition) -> Result<ToolDefinition, ConversionError> {
        Ok(tool.clone())
    }
    fn convert_response(
        &self,
        resp: &serde_json::Value,
    ) -> Result<serde_json::Value, ConversionError> {
        Ok(resp.clone())
    }
}

// ── Message serde ───────────────────────────────────────────────────────

#[test]
fn message_serde_roundtrip() {
    let msg = Message {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn message_serde_with_tool_call_id() {
    let msg = Message {
        role: "tool".into(),
        content: Some("result".into()),
        tool_call_id: Some("call_abc".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("call_abc"));
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn message_serde_omits_none_fields() {
    let msg = Message {
        role: "user".into(),
        content: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("content"));
    assert!(!json.contains("tool_call_id"));
}

// ── ToolDefinition serde ────────────────────────────────────────────────

#[test]
fn tool_definition_serde_roundtrip() {
    let tool = ToolDefinition {
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn tool_definition_empty_params() {
    let tool = ToolDefinition {
        name: "noop".into(),
        description: "Does nothing".into(),
        parameters: serde_json::json!({}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// ── ConversionError serde ───────────────────────────────────────────────

#[test]
fn conversion_error_unsupported_field_roundtrip() {
    let err = ConversionError::UnsupportedField {
        field: "system".into(),
        dialect: Dialect::Claude,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_incompatible_type_roundtrip() {
    let err = ConversionError::IncompatibleType {
        source_type: "array".into(),
        target_type: "string".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_missing_field_roundtrip() {
    let err = ConversionError::MissingRequiredField {
        field: "content".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_content_too_long_roundtrip() {
    let err = ConversionError::ContentTooLong {
        max: 4096,
        actual: 8192,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_is_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(ConversionError::MissingRequiredField {
        field: "role".into(),
    });
    assert!(err.to_string().contains("role"));
}

// ── ConversionReport ────────────────────────────────────────────────────

#[test]
fn report_serde_roundtrip() {
    let report = ConversionReport {
        source: Dialect::OpenAi,
        target: Dialect::Gemini,
        conversions: 5,
        warnings: vec!["truncated".into()],
        errors: vec![ConversionError::MissingRequiredField {
            field: "model".into(),
        }],
        is_lossless: false,
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: ConversionReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, back);
}

#[test]
fn report_new_is_clean() {
    let r = ConversionReport::new(Dialect::Claude, Dialect::OpenAi);
    assert_eq!(r.conversions, 0);
    assert!(r.warnings.is_empty());
    assert!(r.errors.is_empty());
    assert!(r.is_lossless);
    assert!(r.is_ok());
}

#[test]
fn report_is_ok_false_when_errors() {
    let mut r = ConversionReport::new(Dialect::Gemini, Dialect::Claude);
    assert!(r.is_ok());
    r.errors.push(ConversionError::ContentTooLong {
        max: 10,
        actual: 20,
    });
    assert!(!r.is_ok());
}

#[test]
fn report_is_ok_true_with_warnings_only() {
    let mut r = ConversionReport::new(Dialect::OpenAi, Dialect::Kimi);
    r.warnings.push("minor issue".into());
    assert!(r.is_ok());
}

// ── RoleMapper ──────────────────────────────────────────────────────────

#[test]
fn role_openai_user_to_claude_user() {
    assert_eq!(
        RoleMapper::map_role("user", Dialect::OpenAi, Dialect::Claude).unwrap(),
        "user"
    );
}

#[test]
fn role_openai_assistant_to_claude_assistant() {
    assert_eq!(
        RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::Claude).unwrap(),
        "assistant"
    );
}

#[test]
fn role_openai_system_to_claude_error() {
    let err = RoleMapper::map_role("system", Dialect::OpenAi, Dialect::Claude).unwrap_err();
    matches!(err, ConversionError::UnsupportedField { .. });
}

#[test]
fn role_openai_assistant_to_gemini_model() {
    assert_eq!(
        RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::Gemini).unwrap(),
        "model"
    );
}

#[test]
fn role_gemini_model_to_openai_assistant() {
    assert_eq!(
        RoleMapper::map_role("model", Dialect::Gemini, Dialect::OpenAi).unwrap(),
        "assistant"
    );
}

#[test]
fn role_gemini_user_to_claude_user() {
    assert_eq!(
        RoleMapper::map_role("user", Dialect::Gemini, Dialect::Claude).unwrap(),
        "user"
    );
}

#[test]
fn role_openai_tool_to_gemini_error() {
    let err = RoleMapper::map_role("tool", Dialect::OpenAi, Dialect::Gemini).unwrap_err();
    assert!(matches!(err, ConversionError::UnsupportedField { .. }));
}

#[test]
fn role_unknown_in_openai_error() {
    let err = RoleMapper::map_role("narrator", Dialect::OpenAi, Dialect::Claude).unwrap_err();
    assert!(matches!(err, ConversionError::IncompatibleType { .. }));
}

#[test]
fn role_unknown_in_gemini_error() {
    let err = RoleMapper::map_role("system", Dialect::Gemini, Dialect::OpenAi).unwrap_err();
    assert!(matches!(err, ConversionError::IncompatibleType { .. }));
}

#[test]
fn role_codex_uses_openai_roles() {
    assert_eq!(
        RoleMapper::map_role("system", Dialect::Codex, Dialect::Codex).unwrap(),
        "system"
    );
    assert_eq!(
        RoleMapper::map_role("tool", Dialect::Codex, Dialect::Copilot).unwrap(),
        "tool"
    );
}

#[test]
fn role_kimi_uses_openai_roles() {
    assert_eq!(
        RoleMapper::map_role("assistant", Dialect::Kimi, Dialect::Gemini).unwrap(),
        "model"
    );
}

// ── DialectConverter trait ──────────────────────────────────────────────

#[test]
fn converter_source_target_dialects() {
    let c = PassthroughConverter {
        source: Dialect::OpenAi,
        target: Dialect::Gemini,
    };
    assert_eq!(c.source_dialect(), Dialect::OpenAi);
    assert_eq!(c.target_dialect(), Dialect::Gemini);
}

#[test]
fn converter_message_role_mapping() {
    let c = PassthroughConverter {
        source: Dialect::OpenAi,
        target: Dialect::Gemini,
    };
    let msg = Message {
        role: "assistant".into(),
        content: Some("Hello".into()),
        tool_call_id: None,
    };
    let out = c.convert_message(&msg).unwrap();
    assert_eq!(out.role, "model");
    assert_eq!(out.content, Some("Hello".into()));
}

#[test]
fn converter_message_unsupported_role() {
    let c = PassthroughConverter {
        source: Dialect::OpenAi,
        target: Dialect::Claude,
    };
    let msg = Message {
        role: "system".into(),
        content: Some("Be helpful".into()),
        tool_call_id: None,
    };
    assert!(c.convert_message(&msg).is_err());
}

#[test]
fn converter_tool_passthrough() {
    let c = PassthroughConverter {
        source: Dialect::Claude,
        target: Dialect::OpenAi,
    };
    let tool = ToolDefinition {
        name: "search".into(),
        description: "Search the web".into(),
        parameters: serde_json::json!({"type": "object"}),
    };
    let out = c.convert_tool(&tool).unwrap();
    assert_eq!(out, tool);
}

#[test]
fn converter_response_passthrough() {
    let c = PassthroughConverter {
        source: Dialect::Gemini,
        target: Dialect::OpenAi,
    };
    let resp = serde_json::json!({"id": "123", "result": "ok"});
    let out = c.convert_response(&resp).unwrap();
    assert_eq!(out, resp);
}
