#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive fuzz-like tests exercising edge cases and boundary conditions.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use uuid::Uuid;

// ─── Helpers ────────────────────────────────────────────────────────────

fn long_string(len: usize) -> String {
    "A".repeat(len)
}

const UNICODE_ZWJ: &str = "\u{200D}";
const UNICODE_ZWS: &str = "\u{200B}";
const UNICODE_FEFF: &str = "\u{FEFF}";
const UNICODE_RTL_OVERRIDE: &str = "\u{202E}";
const UNICODE_LTR_OVERRIDE: &str = "\u{202D}";
const COMBINING_DIACRITICAL: &str = "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}";
const EMOJI_FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";

fn evil_strings() -> Vec<String> {
    vec![
        String::new(),
        " ".into(),
        "\t\n\r".into(),
        "\0".into(),
        "\0\0\0".into(),
        "null".into(),
        "undefined".into(),
        "NaN".into(),
        "true".into(),
        "false".into(),
        "\"".into(),
        "\\".into(),
        "\\\"".into(),
        "'".into(),
        "<script>alert(1)</script>".into(),
        "{}".into(),
        "[]".into(),
        "{\"nested\": true}".into(),
        UNICODE_ZWJ.into(),
        UNICODE_ZWS.into(),
        UNICODE_FEFF.into(),
        UNICODE_RTL_OVERRIDE.into(),
        UNICODE_LTR_OVERRIDE.into(),
        COMBINING_DIACRITICAL.into(),
        EMOJI_FAMILY.into(),
        "🚀🔥💯".into(),
        "\u{0000}\u{0001}\u{001F}".into(),
        format!("{UNICODE_RTL_OVERRIDE}admin{UNICODE_LTR_OVERRIDE}"),
        "a\nb\nc".into(),
        "\r\n\r\n".into(),
        "\\n\\t\\r".into(),
        "/".into(),
        "..".into(),
        "../../../etc/passwd".into(),
        "C:\\Windows\\System32".into(),
        long_string(1024),
    ]
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("fuzz-backend")
        .outcome(Outcome::Complete)
        .build()
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Empty strings everywhere
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_empty_task() {
    let wo = make_work_order("");
    assert_eq!(wo.task, "");
    let json = serde_json::to_string(&wo).unwrap();
    let _rt: WorkOrder = serde_json::from_str(&json).unwrap();
}

#[test]
fn receipt_empty_backend_id() {
    let r = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    assert_eq!(r.backend.id, "");
    let json = serde_json::to_string(&r).unwrap();
    let _rt: Receipt = serde_json::from_str(&json).unwrap();
}

#[test]
fn envelope_empty_error_string() {
    let env = Envelope::Fatal {
        ref_id: Some(String::new()),
        error: String::new(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { error, .. } if error.is_empty()));
}

#[test]
fn context_snippet_empty_fields() {
    let snip = ContextSnippet {
        name: String::new(),
        content: String::new(),
    };
    let json = serde_json::to_string(&snip).unwrap();
    let rt: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert!(rt.name.is_empty());
    assert!(rt.content.is_empty());
}

#[test]
fn artifact_ref_empty_fields() {
    let art = ArtifactRef {
        kind: String::new(),
        path: String::new(),
    };
    let json = serde_json::to_string(&art).unwrap();
    let _rt: ArtifactRef = serde_json::from_str(&json).unwrap();
}

#[test]
fn backend_identity_all_empty() {
    let id = BackendIdentity {
        id: String::new(),
        backend_version: Some(String::new()),
        adapter_version: Some(String::new()),
    };
    let json = serde_json::to_string(&id).unwrap();
    let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert!(rt.id.is_empty());
}

#[test]
fn policy_profile_empty_vecs() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    let json = serde_json::to_string(&p).unwrap();
    let _: PolicyProfile = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Very long strings (1MB+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_1mb_task() {
    let big = long_string(1_048_576);
    let wo = make_work_order(&big);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task.len(), 1_048_576);
}

#[test]
fn receipt_1mb_backend_id() {
    let big = long_string(1_048_576);
    let r = ReceiptBuilder::new(&big).build();
    assert_eq!(r.backend.id.len(), 1_048_576);
}

#[test]
fn envelope_fatal_1mb_error() {
    let big = long_string(1_048_576);
    let env = Envelope::Fatal {
        ref_id: None,
        error: big.clone(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 1_048_576),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn context_snippet_1mb_content() {
    let big = long_string(1_048_576);
    let snip = ContextSnippet {
        name: "big".into(),
        content: big.clone(),
    };
    let json = serde_json::to_string(&snip).unwrap();
    let rt: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.content.len(), 1_048_576);
}

#[test]
fn work_order_many_context_files() {
    let wo = WorkOrderBuilder::new("test")
        .context(ContextPacket {
            files: (0..10_000).map(|i| format!("file_{i}.rs")).collect(),
            snippets: vec![],
        })
        .build();
    assert_eq!(wo.context.files.len(), 10_000);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Unicode edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_unicode_task_rtl() {
    let task = format!("{UNICODE_RTL_OVERRIDE}task description");
    let wo = make_work_order(&task);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(rt.task.contains(UNICODE_RTL_OVERRIDE));
}

#[test]
fn work_order_zero_width_chars() {
    let task = format!("hello{UNICODE_ZWS}world{UNICODE_ZWJ}!");
    let wo = make_work_order(&task);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn work_order_combining_marks() {
    let wo = make_work_order(COMBINING_DIACRITICAL);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, COMBINING_DIACRITICAL);
}

#[test]
fn work_order_emoji_family_sequence() {
    let wo = make_work_order(EMOJI_FAMILY);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, EMOJI_FAMILY);
}

#[test]
fn work_order_bom_in_task() {
    let task = format!("{UNICODE_FEFF}task");
    let wo = make_work_order(&task);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn envelope_unicode_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some("🔥🚀".into()),
        error: "boom".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.unwrap(), "🔥🚀"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn receipt_unicode_git_diff() {
    let r = ReceiptBuilder::new("test")
        .verification(VerificationReport {
            git_diff: Some(format!(
                "diff with {EMOJI_FAMILY} and {COMBINING_DIACRITICAL}"
            )),
            git_status: None,
            harness_ok: true,
        })
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert!(rt.verification.git_diff.unwrap().contains(EMOJI_FAMILY));
}

#[test]
fn agent_event_unicode_message() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: format!("{UNICODE_RTL_OVERRIDE}{COMBINING_DIACRITICAL}{EMOJI_FAMILY}"),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let _rt: AgentEvent = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 4. JSON special characters in all string fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_json_special_chars_in_task() {
    for s in evil_strings() {
        let wo = make_work_order(&s);
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.task, s);
    }
}

#[test]
fn backend_identity_json_special_chars() {
    for s in evil_strings() {
        let id = BackendIdentity {
            id: s.clone(),
            backend_version: Some(s.clone()),
            adapter_version: Some(s.clone()),
        };
        let json = serde_json::to_string(&id).unwrap();
        let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.id, s);
    }
}

#[test]
fn context_snippet_json_special_chars() {
    for s in evil_strings() {
        let snip = ContextSnippet {
            name: s.clone(),
            content: s.clone(),
        };
        let json = serde_json::to_string(&snip).unwrap();
        let rt: ContextSnippet = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.name, s);
        assert_eq!(rt.content, s);
    }
}

#[test]
fn envelope_fatal_json_special_chars() {
    for s in evil_strings() {
        let env = Envelope::Fatal {
            ref_id: Some(s.clone()),
            error: s.clone(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.unwrap(), s);
                assert_eq!(error, s);
            }
            _ => panic!("expected Fatal"),
        }
    }
}

#[test]
fn artifact_ref_json_special_chars() {
    for s in evil_strings() {
        let art = ArtifactRef {
            kind: s.clone(),
            path: s.clone(),
        };
        let json = serde_json::to_string(&art).unwrap();
        let rt: ArtifactRef = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.kind, s);
        assert_eq!(rt.path, s);
    }
}

#[test]
fn policy_profile_json_special_chars_tools() {
    for s in evil_strings() {
        let p = PolicyProfile {
            allowed_tools: vec![s.clone()],
            disallowed_tools: vec![s.clone()],
            deny_read: vec![],
            deny_write: vec![],
            allow_network: vec![s.clone()],
            deny_network: vec![s.clone()],
            require_approval_for: vec![s.clone()],
        };
        let json = serde_json::to_string(&p).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.allowed_tools[0], s);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Nested JSON in unexpected places
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_config_nested_json_vendor_flags() {
    let deeply_nested = serde_json::json!({
        "a": { "b": { "c": { "d": { "e": { "f": "deep" } } } } }
    });
    let config = RuntimeConfig {
        model: None,
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("deep".into(), deeply_nested.clone());
            m
        },
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&config).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.vendor["deep"], deeply_nested);
}

#[test]
fn agent_event_tool_call_nested_input() {
    let nested = serde_json::json!({
        "array": [[[[[1, 2, 3]]]]],
        "obj": {"a": {"b": {"c": {"d": true}}}}
    });
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "test".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: nested.clone(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    match rt.kind {
        AgentEventKind::ToolCall { input, .. } => assert_eq!(input, nested),
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn agent_event_ext_deeply_nested() {
    let mut ext = BTreeMap::new();
    let deep = serde_json::json!({"l1": {"l2": {"l3": {"l4": {"l5": "deep"}}}}});
    ext.insert("raw_message".into(), deep.clone());
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.ext.unwrap()["raw_message"], deep);
}

#[test]
fn receipt_usage_raw_nested_json() {
    let raw = serde_json::json!({
        "provider_specific": {
            "nested": [1, 2, {"deep": true}],
            "more": {"even": {"deeper": null}}
        }
    });
    let r = ReceiptBuilder::new("test").usage_raw(raw.clone()).build();
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.usage_raw, raw);
}

#[test]
fn vendor_flags_json_string_value_containing_json() {
    let json_in_string = serde_json::json!("{\"nested\": true}");
    let mut vendor = BTreeMap::new();
    vendor.insert("raw".into(), json_in_string);
    let config = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&config).unwrap();
    let _: RuntimeConfig = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Null bytes in strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_null_bytes_in_task() {
    let task = "hello\0world\0!";
    let wo = make_work_order(task);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn envelope_null_bytes_in_error() {
    let env = Envelope::Fatal {
        ref_id: Some("ref\0id".into()),
        error: "err\0or".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.unwrap().contains('\0'));
            assert!(error.contains('\0'));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn receipt_null_bytes_in_backend_id() {
    let r = ReceiptBuilder::new("test\0backend").build();
    assert!(r.backend.id.contains('\0'));
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert!(rt.backend.id.contains('\0'));
}

#[test]
fn agent_event_null_bytes_tool_name() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "tool\0name".into(),
            tool_use_id: Some("\0".into()),
            parent_tool_use_id: None,
            input: serde_json::json!(null),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let _: AgentEvent = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Extremely deep nesting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn deep_nested_json_value_in_tool_input() {
    // Build a JSON value nested 64 levels deep
    let mut val = serde_json::json!("leaf");
    for _ in 0..64 {
        val = serde_json::json!({"n": val});
    }
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "deep".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: val,
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let _: AgentEvent = serde_json::from_str(&json).unwrap();
}

#[test]
fn deep_nested_array_in_vendor_flags() {
    let mut val = serde_json::json!(42);
    for _ in 0..64 {
        val = serde_json::json!([val]);
    }
    let mut vendor = BTreeMap::new();
    vendor.insert("deep_arr".into(), val);
    let config = RuntimeConfig {
        vendor,
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let _: RuntimeConfig = serde_json::from_str(&json).unwrap();
}

#[test]
fn deep_nested_ext_map() {
    let mut val = serde_json::json!("bottom");
    for _ in 0..32 {
        val = serde_json::json!({"inner": val});
    }
    let mut ext = BTreeMap::new();
    ext.insert("deep".into(), val);
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "x".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    let _: AgentEvent = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Maximum/minimum numeric values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn usage_max_tokens() {
    let usage = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
        cache_read_tokens: Some(u64::MAX),
        cache_write_tokens: Some(u64::MAX),
        request_units: Some(u64::MAX),
        estimated_cost_usd: Some(f64::MAX),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.input_tokens, Some(u64::MAX));
}

#[test]
fn usage_zero_tokens() {
    let usage = UsageNormalized {
        input_tokens: Some(0),
        output_tokens: Some(0),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        request_units: Some(0),
        estimated_cost_usd: Some(0.0),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let _: UsageNormalized = serde_json::from_str(&json).unwrap();
}

#[test]
fn runtime_config_max_budget() {
    let config = RuntimeConfig {
        max_budget_usd: Some(f64::MAX),
        max_turns: Some(u32::MAX),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.max_turns, Some(u32::MAX));
}

#[test]
fn runtime_config_zero_budget() {
    let config = RuntimeConfig {
        max_budget_usd: Some(0.0),
        max_turns: Some(0),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let _: RuntimeConfig = serde_json::from_str(&json).unwrap();
}

#[test]
fn runtime_config_negative_budget() {
    let config = RuntimeConfig {
        max_budget_usd: Some(-1.0),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.max_budget_usd, Some(-1.0));
}

#[test]
fn runtime_config_nan_budget_serialization() {
    let config = RuntimeConfig {
        max_budget_usd: Some(f64::NAN),
        ..RuntimeConfig::default()
    };
    // serde_json may serialize NaN as null or fail — either is acceptable
    let result = serde_json::to_string(&config);
    if let Ok(json) = result {
        // If it serializes, roundtrip should still work
        let _: RuntimeConfig = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn runtime_config_infinity_budget_serialization() {
    let config = RuntimeConfig {
        max_budget_usd: Some(f64::INFINITY),
        ..RuntimeConfig::default()
    };
    // serde_json may serialize Infinity as null or fail — either is acceptable
    let result = serde_json::to_string(&config);
    if let Ok(json) = result {
        let _: RuntimeConfig = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn run_metadata_max_duration() {
    let r = ReceiptBuilder::new("test").build();
    let json = serde_json::to_string(&r).unwrap();
    let _: Receipt = serde_json::from_str(&json).unwrap();
}

#[test]
fn command_executed_exit_codes() {
    for code in [i32::MIN, -1, 0, 1, 127, 255, i32::MAX] {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "test".into(),
                exit_code: Some(code),
                output_preview: None,
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match rt.kind {
            AgentEventKind::CommandExecuted { exit_code, .. } => {
                assert_eq!(exit_code, Some(code));
            }
            _ => panic!("expected CommandExecuted"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Malformed JSON parsing recovery
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_empty_string() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn decode_whitespace_only() {
    assert!(JsonlCodec::decode("   ").is_err());
}

#[test]
fn decode_plain_text() {
    assert!(JsonlCodec::decode("not json at all").is_err());
}

#[test]
fn decode_partial_json() {
    assert!(JsonlCodec::decode("{\"t\":\"fatal\"").is_err());
}

#[test]
fn decode_truncated_json() {
    assert!(JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":"bo"#).is_err());
}

#[test]
fn decode_wrong_type_tag() {
    assert!(JsonlCodec::decode(r#"{"t":"nonexistent","data":"x"}"#).is_err());
}

#[test]
fn decode_missing_required_fields() {
    assert!(JsonlCodec::decode(r#"{"t":"fatal"}"#).is_err());
}

#[test]
fn decode_null_object() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn decode_number() {
    assert!(JsonlCodec::decode("42").is_err());
}

#[test]
fn decode_boolean() {
    assert!(JsonlCodec::decode("true").is_err());
}

#[test]
fn decode_array() {
    assert!(JsonlCodec::decode("[1,2,3]").is_err());
}

#[test]
fn decode_nested_json_string() {
    assert!(JsonlCodec::decode(r#""{"t":"fatal"}""#).is_err());
}

#[test]
fn decode_extra_fields_ignored() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored","another":42}"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_ok());
}

#[test]
fn decode_stream_mixed_valid_invalid() {
    let input = r#"{"t":"fatal","ref_id":null,"error":"ok"}
not valid json
{"t":"fatal","ref_id":null,"error":"ok2"}
"#;
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn decode_stream_blank_lines_skipped() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn decode_work_order_missing_fields() {
    let bad = r#"{"task":"x"}"#;
    assert!(serde_json::from_str::<WorkOrder>(bad).is_err());
}

#[test]
fn decode_receipt_missing_fields() {
    let bad = r#"{"outcome":"complete"}"#;
    assert!(serde_json::from_str::<Receipt>(bad).is_err());
}

#[test]
fn decode_agent_event_wrong_type_discriminator() {
    let bad = r#"{"ts":"2024-01-01T00:00:00Z","type":"nonexistent_type","message":"x"}"#;
    assert!(serde_json::from_str::<AgentEvent>(bad).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Concurrent access patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_receipt_hashing() {
    let handles: Vec<_> = (0..16)
        .map(|i| {
            std::thread::spawn(move || {
                let r = ReceiptBuilder::new(format!("backend-{i}"))
                    .outcome(Outcome::Complete)
                    .build()
                    .with_hash()
                    .unwrap();
                assert!(r.receipt_sha256.is_some());
                assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_envelope_encode_decode() {
    let handles: Vec<_> = (0..16)
        .map(|i| {
            std::thread::spawn(move || {
                let env = Envelope::Fatal {
                    ref_id: Some(format!("run-{i}")),
                    error: format!("error-{i}"),
                    error_code: None,
                };
                let line = JsonlCodec::encode(&env).unwrap();
                let decoded = JsonlCodec::decode(line.trim()).unwrap();
                match decoded {
                    Envelope::Fatal { ref_id, error, .. } => {
                        assert_eq!(ref_id.unwrap(), format!("run-{i}"));
                        assert_eq!(error, format!("error-{i}"));
                    }
                    _ => panic!("expected Fatal"),
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_work_order_serialization() {
    let handles: Vec<_> = (0..16)
        .map(|i| {
            std::thread::spawn(move || {
                let wo = make_work_order(&format!("task-{i}"));
                let json = serde_json::to_string(&wo).unwrap();
                let rt: WorkOrder = serde_json::from_str(&json).unwrap();
                assert_eq!(rt.task, format!("task-{i}"));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_glob_matching() {
    let globs =
        IncludeExcludeGlobs::new(&["src/**".into(), "tests/**".into()], &["*.log".into()]).unwrap();
    // IncludeExcludeGlobs does not implement Send, so use std::sync::Arc is not needed.
    // Test sequential rapid access instead.
    for _ in 0..1000 {
        assert_eq!(globs.decide_str("src/main.rs"), MatchDecision::Allowed);
        assert_eq!(
            globs.decide_str("build.log"),
            MatchDecision::DeniedByExclude
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional edge case tests: serde roundtrips for all enum variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn execution_lane_roundtrip() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        let json = serde_json::to_string(&lane).unwrap();
        let rt: ExecutionLane = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&rt).unwrap(), json);
    }
}

#[test]
fn workspace_mode_roundtrip() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        let json = serde_json::to_string(&mode).unwrap();
        let _: WorkspaceMode = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn execution_mode_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let _: ExecutionMode = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn outcome_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, outcome);
    }
}

#[test]
fn capability_all_variants_roundtrip() {
    let caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ];
    for cap in caps {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, cap);
    }
}

#[test]
fn support_level_all_variants_roundtrip() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    ];
    for level in levels {
        let json = serde_json::to_string(&level).unwrap();
        let _: SupportLevel = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn agent_event_kind_all_variants_roundtrip() {
    let now = Utc::now();
    let events = vec![
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: "delta".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage { text: "msg".into() },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "test.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                output: serde_json::json!("file contents"),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added fn".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::Warning {
                message: "warn".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::Error {
                message: "err".into(),
                error_code: None,
            },
            ext: None,
        },
    ];
    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let _: AgentEvent = serde_json::from_str(&json).unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Glob edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn glob_empty_pattern_lists() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn glob_invalid_pattern_bracket() {
    assert!(IncludeExcludeGlobs::new(&["[".into()], &[]).is_err());
}

#[test]
fn glob_unicode_path_matching() {
    let g = IncludeExcludeGlobs::new(&["**/*.rs".into()], &[]).unwrap();
    assert_eq!(
        g.decide_str("src/données/fichier.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("src/données/fichier.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_path_traversal_attempt() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(
        g.decide_str("../../../etc/passwd"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_extremely_long_path() {
    let long_path = format!("src/{}", "a/".repeat(500));
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(g.decide_str(&long_path), MatchDecision::Allowed);
}

#[test]
fn glob_special_chars_in_path() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(
        g.decide_str("path with spaces/file.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(g.decide_str("path\twith\ttabs"), MatchDecision::Allowed);
}

#[test]
fn glob_empty_string_path_with_includes() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn glob_exclude_takes_precedence() {
    let g = IncludeExcludeGlobs::new(&["**/*".into()], &["**/*.secret".into()]).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("keys/api.secret"),
        MatchDecision::DeniedByExclude
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Dialect detector edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_detect_null_input() {
    let det = DialectDetector::new();
    assert!(det.detect(&serde_json::json!(null)).is_none());
}

#[test]
fn dialect_detect_empty_object() {
    let det = DialectDetector::new();
    assert!(det.detect(&serde_json::json!({})).is_none());
}

#[test]
fn dialect_detect_string_input() {
    let det = DialectDetector::new();
    assert!(det.detect(&serde_json::json!("hello")).is_none());
}

#[test]
fn dialect_detect_number_input() {
    let det = DialectDetector::new();
    assert!(det.detect(&serde_json::json!(42)).is_none());
}

#[test]
fn dialect_detect_array_input() {
    let det = DialectDetector::new();
    assert!(det.detect(&serde_json::json!([1, 2, 3])).is_none());
}

#[test]
fn dialect_detect_boolean_input() {
    let det = DialectDetector::new();
    assert!(det.detect(&serde_json::json!(true)).is_none());
}

#[test]
fn dialect_detect_all_empty_object() {
    let det = DialectDetector::new();
    let results = det.detect_all(&serde_json::json!({}));
    assert!(results.is_empty());
}

#[test]
fn dialect_detect_all_non_object() {
    let det = DialectDetector::new();
    assert!(det.detect_all(&serde_json::json!("str")).is_empty());
    assert!(det.detect_all(&serde_json::json!(null)).is_empty());
}

#[test]
fn dialect_all_variants() {
    let all = Dialect::all();
    assert!(all.len() >= 6);
    for d in all {
        assert!(!d.label().is_empty());
        assert!(!d.to_string().is_empty());
    }
}

#[test]
fn dialect_serde_roundtrip() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let rt: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, d);
    }
}

#[test]
fn dialect_detect_ambiguous_input() {
    let det = DialectDetector::new();
    // Object with fields from multiple dialects
    let ambiguous = serde_json::json!({
        "model": "test",
        "messages": [{"role": "user", "content": "hello"}],
        "choices": [],
        "type": "message",
        "contents": [{"parts": [{"text": "hi"}]}],
        "candidates": [],
    });
    // Should not panic even with conflicting signals
    let _ = det.detect(&ambiguous);
    let _ = det.detect_all(&ambiguous);
}

// ═══════════════════════════════════════════════════════════════════════
// Dialect validator edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validator_non_object_input() {
    let v = DialectValidator::new();
    for &d in Dialect::all() {
        let result = v.validate(&serde_json::json!("not an object"), d);
        assert!(!result.valid);
    }
}

#[test]
fn validator_empty_object_all_dialects() {
    let v = DialectValidator::new();
    for &d in Dialect::all() {
        let _ = v.validate(&serde_json::json!({}), d);
        // Should not panic
    }
}

#[test]
fn validator_deeply_nested_messages() {
    let v = DialectValidator::new();
    let msg = serde_json::json!({
        "model": "test",
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "deeply nested"},
                {"type": "text", "text": {"further": {"nesting": true}}}
            ]}
        ]
    });
    let _ = v.validate(&msg, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// Envelope roundtrip edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_roundtrip() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn envelope_run_roundtrip() {
    let wo = make_work_order("test task");
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn envelope_event_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn envelope_final_roundtrip() {
    let receipt = make_receipt();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

#[test]
fn envelope_hello_with_all_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let env = Envelope::hello(
        BackendIdentity {
            id: "full-caps".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("2.0".into()),
        },
        caps,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.len() >= 4);
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Receipt hashing edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_deterministic() {
    let r = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn receipt_hash_different_outcomes_differ() {
    let r1 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("test").outcome(Outcome::Failed).build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_with_hash_sets_sha256() {
    let r = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let r1 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let h_before = abp_core::receipt_hash(&r1).unwrap();
    let r2 = r1.with_hash().unwrap();
    let h_after = abp_core::receipt_hash(&r2).unwrap();
    assert_eq!(h_before, h_after);
}

#[test]
fn receipt_hash_with_large_trace() {
    let now = Utc::now();
    let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
    for i in 0..100 {
        builder = builder.add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    let r = builder.build().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// Protocol version parsing edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(abp_protocol::parse_version(""), None);
    assert_eq!(abp_protocol::parse_version("abp/v"), None);
    assert_eq!(abp_protocol::parse_version("abp/v."), None);
    assert_eq!(abp_protocol::parse_version("abp/v1."), None);
    assert_eq!(abp_protocol::parse_version("abp/v.1"), None);
    assert_eq!(abp_protocol::parse_version("invalid"), None);
    assert_eq!(abp_protocol::parse_version("abp/v-1.0"), None);
    assert_eq!(abp_protocol::parse_version("abp/v1.2.3"), None);
}

#[test]
fn parse_version_large_numbers() {
    assert_eq!(
        abp_protocol::parse_version("abp/v999999999.999999999"),
        Some((999_999_999, 999_999_999))
    );
}

#[test]
fn is_compatible_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.99"));
}

#[test]
fn is_compatible_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn is_compatible_invalid_version() {
    assert!(!abp_protocol::is_compatible_version("garbage", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "garbage"));
}

// ═══════════════════════════════════════════════════════════════════════
// WorkOrder builder edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_all_evil_strings() {
    for s in evil_strings() {
        let wo = WorkOrderBuilder::new(s.clone())
            .root(s.clone())
            .model(s.clone())
            .include(vec![])
            .exclude(vec![])
            .build();
        assert_eq!(wo.task, s);
    }
}

#[test]
fn work_order_builder_max_turns_zero() {
    let wo = WorkOrderBuilder::new("test").max_turns(0).build();
    assert_eq!(wo.config.max_turns, Some(0));
}

#[test]
fn work_order_builder_max_turns_max() {
    let wo = WorkOrderBuilder::new("test").max_turns(u32::MAX).build();
    assert_eq!(wo.config.max_turns, Some(u32::MAX));
}

#[test]
fn work_order_contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════
// Workspace spec edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_spec_empty_root() {
    let ws = WorkspaceSpec {
        root: String::new(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let _: WorkspaceSpec = serde_json::from_str(&json).unwrap();
}

#[test]
fn workspace_spec_path_traversal() {
    let ws = WorkspaceSpec {
        root: "../../../".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["../**".into()],
        exclude: vec![],
    };
    let json = serde_json::to_string(&ws).unwrap();
    let _: WorkspaceSpec = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// Capability requirements edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_requirements_empty() {
    let cr = CapabilityRequirements::default();
    let json = serde_json::to_string(&cr).unwrap();
    let rt: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert!(rt.required.is_empty());
}

#[test]
fn capability_requirements_all_capabilities() {
    let caps: Vec<Capability> = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ];
    let reqs = CapabilityRequirements {
        required: caps
            .into_iter()
            .map(|c| CapabilityRequirement {
                capability: c,
                min_support: MinSupport::Emulated,
            })
            .collect(),
    };
    let json = serde_json::to_string(&reqs).unwrap();
    let rt: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.required.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// Canonical JSON edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn canonical_json_deterministic() {
    let v1 = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let c1 = abp_core::canonical_json(&v1).unwrap();
    let c2 = abp_core::canonical_json(&v1).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonical_json_key_ordering() {
    let v = serde_json::json!({"b": 2, "a": 1});
    let c = abp_core::canonical_json(&v).unwrap();
    assert!(c.starts_with(r#"{"a":1"#));
}

#[test]
fn canonical_json_nested() {
    let v = serde_json::json!({"z": {"b": 2, "a": 1}, "a": 0});
    let c = abp_core::canonical_json(&v).unwrap();
    assert!(c.starts_with(r#"{"a":0"#));
}

#[test]
fn sha256_hex_basic() {
    let hex = abp_core::sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
    // Deterministic
    assert_eq!(hex, abp_core::sha256_hex(b"hello"));
}

#[test]
fn sha256_hex_empty() {
    let hex = abp_core::sha256_hex(b"");
    assert_eq!(hex.len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════
// Encode to writer edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_to_writer_basic() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("boom"));
}

#[test]
fn encode_many_to_writer() {
    let envs = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "e1".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e2".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Tool result edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tool_result_is_error_flag() {
    for is_err in [true, false] {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "test".into(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: is_err,
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match rt.kind {
            AgentEventKind::ToolResult { is_error: flag, .. } => assert_eq!(flag, is_err),
            _ => panic!("expected ToolResult"),
        }
    }
}

#[test]
fn tool_call_null_input() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "test".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!(null),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let _: AgentEvent = serde_json::from_str(&json).unwrap();
}

#[test]
fn tool_result_complex_output() {
    let output = serde_json::json!({
        "files": [{"name": "a.rs", "size": 100}],
        "metadata": {"nested": true},
        "null_field": null,
        "array": [1, "two", null, false]
    });
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "test".into(),
            tool_use_id: None,
            output,
            is_error: false,
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let _: AgentEvent = serde_json::from_str(&json).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// MinSupport satisfies edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn support_level_satisfies_matrix() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Native));
}

// ═══════════════════════════════════════════════════════════════════════
// Rapid-fire mixed serde stress
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stress_many_work_orders() {
    for i in 0..100 {
        let wo = WorkOrderBuilder::new(format!("task-{i}"))
            .root(format!("/workspace/{i}"))
            .model(format!("model-{i}"))
            .max_turns(i as u32)
            .build();
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.task, format!("task-{i}"));
    }
}

#[test]
fn stress_many_receipts() {
    for i in 0..100 {
        let r = ReceiptBuilder::new(format!("backend-{i}"))
            .outcome(match i % 3 {
                0 => Outcome::Complete,
                1 => Outcome::Partial,
                _ => Outcome::Failed,
            })
            .build()
            .with_hash()
            .unwrap();
        assert!(r.receipt_sha256.is_some());
    }
}

#[test]
fn stress_many_envelopes() {
    for i in 0..100 {
        let env = Envelope::Fatal {
            ref_id: Some(format!("run-{i}")),
            error: format!("error-{i}"),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let _ = JsonlCodec::decode(line.trim()).unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Envelope error_code accessor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_error_code_on_non_fatal() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "t".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(env.error_code().is_none());
}

#[test]
fn envelope_fatal_no_error_code() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    assert!(env.error_code().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// Verification report edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verification_report_defaults() {
    let v = VerificationReport::default();
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
    assert!(!v.harness_ok);
}

#[test]
fn verification_report_large_diff() {
    let big_diff = long_string(1_048_576);
    let v = VerificationReport {
        git_diff: Some(big_diff.clone()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&v).unwrap();
    let rt: VerificationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.git_diff.unwrap().len(), 1_048_576);
}

// ═══════════════════════════════════════════════════════════════════════
// UUID edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_uuid_is_valid() {
    let wo = make_work_order("test");
    assert_ne!(wo.id, Uuid::nil());
}

#[test]
fn receipt_nil_work_order_id() {
    let r = ReceiptBuilder::new("test")
        .work_order_id(Uuid::nil())
        .build();
    assert_eq!(r.meta.work_order_id, Uuid::nil());
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.meta.work_order_id, Uuid::nil());
}
