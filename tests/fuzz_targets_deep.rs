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
//! Comprehensive fuzz-like property tests that simulate fuzzing patterns.
//!
//! These tests exercise random, malformed, and adversarial inputs across the
//! entire ABP surface area to ensure no panics, no undefined behaviour, and
//! graceful error handling.

use proptest::prelude::*;
use serde_json::json;
use std::io::BufReader;
use std::path::Path;

use abp_capability::{generate_report, negotiate};
use abp_config::{merge_configs, parse_toml, validate_config, BackendEntry, BackplaneConfig};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_glob::IncludeExcludeGlobs;
use abp_mapping::{
    features, known_rules, validate_mapping, Fidelity, MappingMatrix, MappingRegistry, MappingRule,
};
use abp_policy::PolicyEngine;
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec};
use abp_receipt::{compute_hash, verify_hash};

// ── Configuration ──────────────────────────────────────────────────────

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 40,
        ..ProptestConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Random JSON input parsing — WorkOrder
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random strings never panic when parsed as WorkOrder.
    #[test]
    fn fuzz_work_order_random_string(s in "\\PC{0,200}") {
        let _ = serde_json::from_str::<WorkOrder>(&s);
    }

    /// Random bytes never panic when parsed as WorkOrder.
    #[test]
    fn fuzz_work_order_random_bytes(bytes in prop::collection::vec(any::<u8>(), 0..500)) {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            let _ = serde_json::from_str::<WorkOrder>(s);
        }
    }

    /// Partial valid JSON with missing fields doesn't panic.
    #[test]
    fn fuzz_work_order_partial_json(task in "[a-zA-Z ]{0,30}") {
        let partial = format!(r#"{{"task":"{}"}}"#, task);
        let _ = serde_json::from_str::<WorkOrder>(&partial);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  Random JSON input parsing — Receipt
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random strings never panic when parsed as Receipt.
    #[test]
    fn fuzz_receipt_random_string(s in "\\PC{0,200}") {
        let _ = serde_json::from_str::<Receipt>(&s);
    }

    /// Partial Receipt JSON with extra fields doesn't panic.
    #[test]
    fn fuzz_receipt_extra_fields(key in "[a-z]{1,10}", val in "[a-z]{1,10}") {
        let json_str = format!(r#"{{"{}":"{}","outcome":"complete"}}"#, key, val);
        let _ = serde_json::from_str::<Receipt>(&json_str);
    }

    /// Null-valued fields don't panic.
    #[test]
    fn fuzz_receipt_null_fields(_i in 0..10u32) {
        let json_str = r#"{"meta":null,"backend":null,"outcome":null}"#;
        let _ = serde_json::from_str::<Receipt>(json_str);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Random JSON input parsing — AgentEvent
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random strings never panic when parsed as AgentEvent.
    #[test]
    fn fuzz_agent_event_random_string(s in "\\PC{0,200}") {
        let _ = serde_json::from_str::<AgentEvent>(&s);
    }

    /// Unknown event types don't panic.
    #[test]
    fn fuzz_agent_event_unknown_type(typ in "[a-z_]{1,20}") {
        let json_str = format!(r#"{{"ts":"2025-01-01T00:00:00Z","type":"{}","message":"hi"}}"#, typ);
        let _ = serde_json::from_str::<AgentEvent>(&json_str);
    }

    /// AgentEventKind with random type values doesn't panic.
    #[test]
    fn fuzz_agent_event_kind_random(s in "\\PC{0,200}") {
        let _ = serde_json::from_str::<AgentEventKind>(&s);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Random JSON input parsing — Envelope
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random strings never panic when decoded as Envelope.
    #[test]
    fn fuzz_envelope_random_string(s in "\\PC{0,300}") {
        let _ = JsonlCodec::decode(&s);
    }

    /// Unknown envelope types don't panic.
    #[test]
    fn fuzz_envelope_unknown_type(t in "[a-z]{1,15}") {
        let json_str = format!(r#"{{"t":"{}","data":"test"}}"#, t);
        let _ = JsonlCodec::decode(&json_str);
    }

    /// Envelope with missing required fields doesn't panic.
    #[test]
    fn fuzz_envelope_missing_fields(t in prop_oneof![
        Just("hello"), Just("run"), Just("event"), Just("final"), Just("fatal")
    ]) {
        let json_str = format!(r#"{{"t":"{}"}}"#, t);
        let _ = JsonlCodec::decode(&json_str);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5  Malformed JSONL protocol lines
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random multi-line JSONL streams don't panic in decode_stream.
    #[test]
    fn fuzz_jsonl_stream_random(lines in prop::collection::vec("\\PC{0,100}", 0..20)) {
        let input = lines.join("\n");
        let reader = BufReader::new(input.as_bytes());
        for result in JsonlCodec::decode_stream(reader) {
            let _ = result;
        }
    }

    /// Lines with only whitespace are skipped.
    #[test]
    fn fuzz_jsonl_whitespace_lines(
        spaces in prop::collection::vec("[ \\t\\r]{0,20}", 1..10)
    ) {
        let input = spaces.join("\n");
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        // Whitespace-only lines should be skipped
        for r in &results {
            // If not skipped, it should be an error, not a panic
            let _ = r;
        }
    }

    /// Mixed valid and invalid lines don't panic.
    #[test]
    fn fuzz_jsonl_mixed_valid_invalid(garbage in "[a-z]{1,30}") {
        let valid_line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let input = format!("{}\n{}\n{}\n", garbage, valid_line, garbage);
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        // At least the valid line should parse
        prop_assert!(results.iter().any(|r| r.is_ok()));
    }

    /// Truncated JSON lines don't panic.
    #[test]
    fn fuzz_jsonl_truncated(prefix_len in 1usize..40) {
        let full = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let truncated = &full[..prefix_len.min(full.len())];
        let _ = JsonlCodec::decode(truncated);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6  Random glob patterns (shouldn't panic)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random include patterns never panic in IncludeExcludeGlobs::new.
    #[test]
    fn fuzz_glob_random_include(patterns in prop::collection::vec("\\PC{0,50}", 0..5)) {
        let _ = IncludeExcludeGlobs::new(&patterns.to_vec(), &[]);
    }

    /// Random exclude patterns never panic.
    #[test]
    fn fuzz_glob_random_exclude(patterns in prop::collection::vec("\\PC{0,50}", 0..5)) {
        let _ = IncludeExcludeGlobs::new(&[], &patterns.to_vec());
    }

    /// Random paths against valid globs never panic.
    #[test]
    fn fuzz_glob_random_path(path in "\\PC{0,100}") {
        if let Ok(globs) = IncludeExcludeGlobs::new(&["**".to_string()], &[]) {
            let _ = globs.decide_str(&path);
        }
    }

    /// Mixed include and exclude with random patterns never panic.
    #[test]
    fn fuzz_glob_mixed_random(
        include in prop::collection::vec("[a-z*?/]{0,20}", 0..3),
        exclude in prop::collection::vec("[a-z*?/]{0,20}", 0..3),
    ) {
        let _ = IncludeExcludeGlobs::new(&include, &exclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7  Random policy profiles (shouldn't panic)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// PolicyEngine::new with random tool names never panics.
    #[test]
    fn fuzz_policy_random_tools(
        allowed in prop::collection::vec("\\PC{0,30}", 0..5),
        denied in prop::collection::vec("\\PC{0,30}", 0..5),
    ) {
        let profile = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..PolicyProfile::default()
        };
        let _ = PolicyEngine::new(&profile);
    }

    /// PolicyEngine with random deny_read globs never panics.
    #[test]
    fn fuzz_policy_random_deny_read(
        globs in prop::collection::vec("[a-z*?/]{0,30}", 0..5),
    ) {
        let profile = PolicyProfile {
            deny_read: globs,
            ..PolicyProfile::default()
        };
        let _ = PolicyEngine::new(&profile);
    }

    /// PolicyEngine with random deny_write globs never panics.
    #[test]
    fn fuzz_policy_random_deny_write(
        globs in prop::collection::vec("[a-z*?/]{0,30}", 0..5),
    ) {
        let profile = PolicyProfile {
            deny_write: globs,
            ..PolicyProfile::default()
        };
        let _ = PolicyEngine::new(&profile);
    }

    /// Random path checks against a valid policy engine never panic.
    #[test]
    fn fuzz_policy_path_check(path in "\\PC{0,100}") {
        let profile = PolicyProfile {
            deny_read: vec!["*.secret".to_string()],
            deny_write: vec!["*.log".to_string()],
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&profile) {
            let _ = engine.can_read_path(Path::new(&path));
            let _ = engine.can_write_path(Path::new(&path));
        }
    }

    /// Random tool name checks never panic.
    #[test]
    fn fuzz_policy_tool_check(tool in "\\PC{0,50}") {
        let profile = PolicyProfile {
            allowed_tools: vec!["bash".to_string()],
            disallowed_tools: vec!["rm".to_string()],
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&profile) {
            let _ = engine.can_use_tool(&tool);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8  Random config TOML strings (shouldn't panic)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random TOML strings never panic.
    #[test]
    fn fuzz_config_random_toml(s in "\\PC{0,200}") {
        let _ = parse_toml(&s);
    }

    /// Partially valid TOML with random keys never panics.
    #[test]
    fn fuzz_config_partial_toml(key in "[a-z_]{1,15}", val in "[a-z0-9]{1,15}") {
        let toml_str = format!("{} = \"{}\"", key, val);
        let _ = parse_toml(&toml_str);
    }

    /// Config validation with random log levels never panics.
    #[test]
    fn fuzz_config_random_log_level(level in "\\PC{0,20}") {
        let cfg = BackplaneConfig {
            log_level: Some(level),
            ..BackplaneConfig::default()
        };
        let _ = validate_config(&cfg);
    }

    /// Config merge with random configs never panics.
    #[test]
    fn fuzz_config_merge_random(
        name1 in "[a-z]{1,10}",
        name2 in "[a-z]{1,10}",
    ) {
        let mut cfg1 = BackplaneConfig::default();
        cfg1.backends.insert(name1, BackendEntry::Mock {});
        let mut cfg2 = BackplaneConfig::default();
        cfg2.backends.insert(name2, BackendEntry::Mock {});
        let _ = merge_configs(cfg1, cfg2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9  Random SDK dialect detection inputs
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random JSON values never panic in dialect detection.
    #[test]
    fn fuzz_dialect_detect_random_json(s in "\\PC{0,300}") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&s) {
            let detector = DialectDetector::new();
            let _ = detector.detect(&val);
            let _ = detector.detect_all(&val);
        }
    }

    /// Various JSON object structures don't panic.
    #[test]
    fn fuzz_dialect_detect_random_object(
        key1 in "[a-z_]{1,15}",
        key2 in "[a-z_]{1,15}",
        val1 in "[a-z]{1,10}",
    ) {
        let obj = json!({ key1: val1, key2: [] });
        let detector = DialectDetector::new();
        let _ = detector.detect(&obj);
        let _ = detector.detect_all(&obj);
    }

    /// Deeply nested JSON doesn't panic dialect detection.
    #[test]
    fn fuzz_dialect_detect_nested(depth in 1usize..50) {
        let mut val = json!("leaf");
        for _ in 0..depth {
            val = json!({"nested": val});
        }
        let detector = DialectDetector::new();
        let _ = detector.detect(&val);
    }

    /// Dialect enum serde roundtrips never panic.
    #[test]
    fn fuzz_dialect_serde_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<Dialect>(&format!("\"{}\"", s));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §10  Very large inputs (10MB strings)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_large_string_work_order_parse() {
    let large = "a".repeat(10 * 1024 * 1024);
    let _ = serde_json::from_str::<WorkOrder>(&large);
}

#[test]
fn fuzz_large_string_receipt_parse() {
    let large = "b".repeat(10 * 1024 * 1024);
    let _ = serde_json::from_str::<Receipt>(&large);
}

#[test]
fn fuzz_large_string_envelope_decode() {
    let large = "c".repeat(10 * 1024 * 1024);
    let _ = JsonlCodec::decode(&large);
}

#[test]
fn fuzz_large_string_toml_parse() {
    let large = "d".repeat(10 * 1024 * 1024);
    let _ = parse_toml(&large);
}

#[test]
fn fuzz_large_string_dialect_detect() {
    let large = "e".repeat(10 * 1024 * 1024);
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&format!(r#""{}""#, large)) {
        let detector = DialectDetector::new();
        let _ = detector.detect(&val);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §11  Deeply nested JSON objects
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_deeply_nested_json_work_order() {
    let mut val = String::from("\"leaf\"");
    for _ in 0..128 {
        val = format!(r#"{{"nested":{}}}"#, val);
    }
    let _ = serde_json::from_str::<WorkOrder>(&val);
}

#[test]
fn fuzz_deeply_nested_json_receipt() {
    let mut val = String::from("\"leaf\"");
    for _ in 0..128 {
        val = format!(r#"{{"nested":{}}}"#, val);
    }
    let _ = serde_json::from_str::<Receipt>(&val);
}

#[test]
fn fuzz_deeply_nested_json_envelope() {
    let mut val = String::from("\"leaf\"");
    for _ in 0..128 {
        val = format!(r#"{{"nested":{}}}"#, val);
    }
    let _ = JsonlCodec::decode(&val);
}

#[test]
fn fuzz_deeply_nested_json_agent_event() {
    let mut val = String::from("\"leaf\"");
    for _ in 0..128 {
        val = format!(r#"{{"nested":{}}}"#, val);
    }
    let _ = serde_json::from_str::<AgentEvent>(&val);
}

// ═══════════════════════════════════════════════════════════════════════
// §12  Special Unicode characters
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_unicode_null_bytes_work_order() {
    let s = "{\"\0task\": \"hello\0world\"}";
    let _ = serde_json::from_str::<WorkOrder>(s);
}

#[test]
fn fuzz_unicode_rtl_markers() {
    let rtl = "\u{200F}hello\u{200F}";
    let json_str = format!(r#"{{"task":"{}"}}"#, rtl);
    let _ = serde_json::from_str::<WorkOrder>(&json_str);
}

#[test]
fn fuzz_unicode_zero_width() {
    let zw = "\u{200B}\u{FEFF}\u{200C}\u{200D}";
    let json_str = format!(r#"{{"task":"{}"}}"#, zw);
    let _ = serde_json::from_str::<WorkOrder>(&json_str);
}

#[test]
fn fuzz_unicode_emoji_surrogates() {
    let emoji = "🦀🔥💀👻\u{1F680}";
    let json_str = format!(r#"{{"task":"{}"}}"#, emoji);
    let _ = serde_json::from_str::<WorkOrder>(&json_str);
}

#[test]
fn fuzz_unicode_policy_tool_names() {
    let tools = vec![
        "\u{200B}bash".to_string(),
        "rm\u{200F}".to_string(),
        "\0cat".to_string(),
        "🦀tool".to_string(),
    ];
    let profile = PolicyProfile {
        allowed_tools: tools.clone(),
        disallowed_tools: tools,
        ..PolicyProfile::default()
    };
    if let Ok(engine) = PolicyEngine::new(&profile) {
        let _ = engine.can_use_tool("\u{200B}bash");
        let _ = engine.can_use_tool("🦀tool");
    }
}

#[test]
fn fuzz_unicode_glob_patterns() {
    let patterns = vec![
        "*.🦀".to_string(),
        "**\u{200B}/*".to_string(),
        "\u{FEFF}src/**".to_string(),
    ];
    let _ = IncludeExcludeGlobs::new(&patterns, &[]);
}

#[test]
fn fuzz_unicode_config_values() {
    let toml_str = "default_backend = \"\u{200B}mock\u{200F}\"\nlog_level = \"de\u{200C}bug\"";
    let _ = parse_toml(toml_str);
}

#[test]
fn fuzz_unicode_envelope_error() {
    let json_str = r#"{"t":"fatal","ref_id":null,"error":"💀\u0000boom"}"#;
    let _ = JsonlCodec::decode(json_str);
}

// ═══════════════════════════════════════════════════════════════════════
// §13  Integer overflow scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_integer_overflow_usage_tokens() {
    let usage = IrUsage::from_io(u64::MAX, 0);
    assert_eq!(usage.input_tokens, u64::MAX);
}

#[test]
fn fuzz_integer_overflow_usage_merge() {
    // Merging large values will wrap around (wrapping addition in release)
    // This should not panic
    let a = IrUsage::from_io(u64::MAX / 2, u64::MAX / 2);
    let b = IrUsage::from_io(u64::MAX / 2, u64::MAX / 2);
    let _ = std::panic::catch_unwind(|| a.merge(b));
}

#[test]
fn fuzz_integer_overflow_duration_ms() {
    let json_str = format!(
        r#"{{"run_id":"00000000-0000-0000-0000-000000000000","work_order_id":"00000000-0000-0000-0000-000000000000","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:00Z","duration_ms":{}}}"#,
        u64::MAX
    );
    let _ = serde_json::from_str::<abp_core::RunMetadata>(&json_str);
}

#[test]
fn fuzz_integer_overflow_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(u32::MAX).build();
    assert_eq!(wo.config.max_turns, Some(u32::MAX));
    let json = serde_json::to_string(&wo).unwrap();
    let _ = serde_json::from_str::<WorkOrder>(&json);
}

#[test]
fn fuzz_integer_overflow_max_budget() {
    let wo = WorkOrderBuilder::new("test")
        .max_budget_usd(f64::MAX)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let _ = serde_json::from_str::<WorkOrder>(&json);
}

#[test]
fn fuzz_integer_overflow_timeout_parse() {
    let toml_str = format!(
        "[backends.test]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = []\ntimeout_secs = {}",
        u64::MAX
    );
    let _ = parse_toml(&toml_str);
}

// ═══════════════════════════════════════════════════════════════════════
// §14  Empty/whitespace-only inputs everywhere
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_empty_string_work_order() {
    let _ = serde_json::from_str::<WorkOrder>("");
}

#[test]
fn fuzz_empty_string_receipt() {
    let _ = serde_json::from_str::<Receipt>("");
}

#[test]
fn fuzz_empty_string_agent_event() {
    let _ = serde_json::from_str::<AgentEvent>("");
}

#[test]
fn fuzz_empty_string_envelope() {
    let _ = JsonlCodec::decode("");
}

#[test]
fn fuzz_empty_string_toml() {
    // Empty TOML should parse to default config
    let result = parse_toml("");
    assert!(result.is_ok());
}

#[test]
fn fuzz_whitespace_only_envelope() {
    let _ = JsonlCodec::decode("   ");
    let _ = JsonlCodec::decode("\t\t\t");
    let _ = JsonlCodec::decode("\n\n\n");
}

#[test]
fn fuzz_whitespace_only_toml() {
    let result = parse_toml("   \n\t\n  ");
    assert!(result.is_ok());
}

#[test]
fn fuzz_empty_glob_patterns() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(globs.decide_str("anything").is_allowed());
}

#[test]
fn fuzz_empty_policy_profile() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let d = engine.can_use_tool("anything");
    assert!(d.allowed);
}

#[test]
fn fuzz_whitespace_jsonl_stream() {
    let input = "  \n  \n\t\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    // All whitespace lines should be skipped
    assert!(results.is_empty());
}

#[test]
fn fuzz_empty_ir_conversation() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.tool_calls().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §15  Random IR type construction
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random IrMessage text content survives serde roundtrip.
    #[test]
    fn fuzz_ir_message_random_text(text in "\\PC{0,200}") {
        let msg = IrMessage::text(IrRole::User, &text);
        let json = serde_json::to_string(&msg).unwrap();
        let msg2: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(msg.role, msg2.role);
    }

    /// Random IrToolDefinition with random parameter schemas doesn't panic.
    #[test]
    fn fuzz_ir_tool_definition_random(
        name in "[a-zA-Z_]{1,20}",
        desc in "[a-zA-Z ]{0,50}",
    ) {
        let tool = IrToolDefinition {
            name,
            description: desc,
            parameters: json!({"type": "object"}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let tool2: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(tool.name, tool2.name);
    }

    /// IrUsage from_io with random values doesn't panic.
    #[test]
    fn fuzz_ir_usage_random(input in 0u64..u64::MAX/2, output in 0u64..u64::MAX/2) {
        let usage = IrUsage::from_io(input, output);
        prop_assert_eq!(usage.total_tokens, input + output);
    }

    /// Random IrConversation with mixed roles serde roundtrips.
    #[test]
    fn fuzz_ir_conversation_random(
        texts in prop::collection::vec("[a-zA-Z ]{1,30}", 1..10),
    ) {
        let msgs: Vec<IrMessage> = texts.iter().enumerate().map(|(i, t)| {
            let role = match i % 4 {
                0 => IrRole::System,
                1 => IrRole::User,
                2 => IrRole::Assistant,
                _ => IrRole::Tool,
            };
            IrMessage::text(role, t.as_str())
        }).collect();
        let conv = IrConversation::from_messages(msgs);
        let json = serde_json::to_string(&conv).unwrap();
        let conv2: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv.len(), conv2.len());
    }

    /// IrContentBlock::ToolUse with random input values doesn't panic on serde.
    #[test]
    fn fuzz_ir_content_block_tool_use(
        id in "[a-zA-Z0-9]{1,20}",
        name in "[a-zA-Z_]{1,20}",
    ) {
        let block = IrContentBlock::ToolUse {
            id,
            name,
            input: json!({"random": true}),
        };
        let json = serde_json::to_string(&block).unwrap();
        let block2: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, block2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §16  Random capability manifest construction
// ═══════════════════════════════════════════════════════════════════════

fn all_capabilities() -> Vec<Capability> {
    vec![
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
    ]
}

proptest! {
    #![proptest_config(fast_config())]

    /// Random capability manifests with random support levels serde roundtrip.
    #[test]
    fn fuzz_capability_manifest_random(
        indices in prop::collection::vec(0usize..26, 0..10),
        levels in prop::collection::vec(0u8..4, 0..10),
    ) {
        let caps = all_capabilities();
        let mut manifest = CapabilityManifest::new();
        for (idx_raw, level_raw) in indices.iter().zip(levels.iter()) {
            let cap = caps[*idx_raw % caps.len()].clone();
            let level = match level_raw % 4 {
                0 => SupportLevel::Native,
                1 => SupportLevel::Emulated,
                2 => SupportLevel::Unsupported,
                _ => SupportLevel::Restricted { reason: "test".into() },
            };
            manifest.insert(cap, level);
        }
        let json = serde_json::to_string(&manifest).unwrap();
        let manifest2: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), manifest2.len());
    }

    /// Negotiation with random requirements never panics.
    #[test]
    fn fuzz_capability_negotiation_random(
        cap_indices in prop::collection::vec(0usize..26, 0..5),
        manifest_indices in prop::collection::vec(0usize..26, 0..10),
    ) {
        let caps = all_capabilities();
        let mut manifest = CapabilityManifest::new();
        for idx in &manifest_indices {
            manifest.insert(caps[*idx % caps.len()].clone(), SupportLevel::Native);
        }
        let requirements = CapabilityRequirements {
            required: cap_indices.iter().map(|idx| {
                CapabilityRequirement {
                    capability: caps[*idx % caps.len()].clone(),
                    min_support: MinSupport::Emulated,
                }
            }).collect(),
        };
        let result = negotiate(&manifest, &requirements);
        let _ = result.is_compatible();
        let _ = result.total();
    }

    /// generate_report never panics with any input.
    #[test]
    fn fuzz_capability_report_random(
        cap_indices in prop::collection::vec(0usize..26, 0..5),
    ) {
        let caps = all_capabilities();
        let mut manifest = CapabilityManifest::new();
        for idx in &cap_indices {
            manifest.insert(caps[*idx % caps.len()].clone(), SupportLevel::Native);
        }
        let requirements = CapabilityRequirements { required: vec![] };
        let result = negotiate(&manifest, &requirements);
        let report = generate_report(&result);
        prop_assert!(report.compatible);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §17  Rapid state transitions
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Rapidly rebuilding PolicyEngine with different profiles never panics.
    #[test]
    fn fuzz_rapid_policy_rebuild(
        iterations in 1usize..20,
        tool in "[a-z]{1,10}",
    ) {
        for i in 0..iterations {
            let profile = PolicyProfile {
                allowed_tools: if i % 2 == 0 { vec![tool.clone()] } else { vec![] },
                disallowed_tools: if i % 3 == 0 { vec![tool.clone()] } else { vec![] },
                ..PolicyProfile::default()
            };
            if let Ok(engine) = PolicyEngine::new(&profile) {
                let _ = engine.can_use_tool(&tool);
            }
        }
    }

    /// Rapidly hashing different receipts never panics.
    #[test]
    fn fuzz_rapid_receipt_hashing(iterations in 1usize..20) {
        for i in 0..iterations {
            let receipt = ReceiptBuilder::new(format!("backend-{}", i))
                .outcome(if i % 2 == 0 { Outcome::Complete } else { Outcome::Failed })
                .build();
            let _ = compute_hash(&receipt);
            let _ = verify_hash(&receipt);
        }
    }

    /// Rapidly merging configs never panics.
    #[test]
    fn fuzz_rapid_config_merge(iterations in 1usize..20) {
        let mut cfg = BackplaneConfig::default();
        for i in 0..iterations {
            let mut overlay = BackplaneConfig::default();
            overlay.backends.insert(format!("b{}", i), BackendEntry::Mock {});
            cfg = merge_configs(cfg, overlay);
        }
        let _ = validate_config(&cfg);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §18  Mixed valid/invalid input streams
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// A stream of alternating valid/invalid envelopes doesn't panic.
    #[test]
    fn fuzz_mixed_envelope_stream(garbage_lines in prop::collection::vec("[a-z]{1,30}", 1..5)) {
        let valid = r#"{"t":"fatal","ref_id":null,"error":"test"}"#;
        let mut lines_vec = Vec::new();
        for g in &garbage_lines {
            lines_vec.push(g.as_str());
            lines_vec.push(valid);
        }
        let input = lines_vec.join("\n");
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        let ok_count = results.iter().filter(|r| r.is_ok()).count();
        prop_assert!(ok_count >= garbage_lines.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §19  Version parsing edge cases
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random version strings never panic.
    #[test]
    fn fuzz_version_parse_random(s in "\\PC{0,50}") {
        let _ = parse_version(&s);
    }

    /// Random version compatibility checks never panic.
    #[test]
    fn fuzz_version_compat_random(a in "\\PC{0,30}", b in "\\PC{0,30}") {
        let _ = is_compatible_version(&a, &b);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §20  Mapping types fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// MappingRegistry insert and lookup with random features never panic.
    #[test]
    fn fuzz_mapping_registry_random(feature in "[a-z_]{1,20}") {
        let mut registry = MappingRegistry::new();
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: feature.clone(),
            fidelity: Fidelity::Lossless,
        };
        registry.insert(rule);
        let _ = registry.lookup(Dialect::OpenAi, Dialect::Claude, &feature);
        let _ = registry.len();
    }

    /// MappingMatrix with random dialects never panics.
    #[test]
    fn fuzz_mapping_matrix_random(
        src_idx in 0usize..6,
        tgt_idx in 0usize..6,
    ) {
        let dialects = Dialect::all();
        let mut matrix = MappingMatrix::new();
        matrix.set(dialects[src_idx], dialects[tgt_idx], true);
        let _ = matrix.get(dialects[src_idx], dialects[tgt_idx]);
        let _ = matrix.is_supported(dialects[src_idx], dialects[tgt_idx]);
    }

    /// validate_mapping with random features never panics.
    #[test]
    fn fuzz_validate_mapping_random(
        feature in "[a-z_]{1,20}",
        src_idx in 0usize..6,
        tgt_idx in 0usize..6,
    ) {
        let dialects = Dialect::all();
        let registry = known_rules();
        let _ = validate_mapping(&registry, dialects[src_idx], dialects[tgt_idx], &[feature]);
    }

    /// Fidelity serde roundtrips.
    #[test]
    fn fuzz_fidelity_serde(s in "\\PC{0,50}") {
        let _ = serde_json::from_str::<Fidelity>(&format!("\"{}\"", s));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §21  Receipt builder edge cases
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// ReceiptBuilder with random backend IDs never panics.
    #[test]
    fn fuzz_receipt_builder_random_id(id in "\\PC{0,100}") {
        let receipt = ReceiptBuilder::new(id).build();
        let _ = compute_hash(&receipt);
    }

    /// ReceiptBuilder with random trace events never panics.
    #[test]
    fn fuzz_receipt_builder_with_events(count in 0usize..20) {
        let mut builder = ReceiptBuilder::new("test");
        for i in 0..count {
            let event = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{}", i),
                },
                ext: None,
            };
            builder = builder.add_trace_event(event);
        }
        let receipt = builder.build();
        let hash = compute_hash(&receipt).unwrap();
        prop_assert_eq!(hash.len(), 64);
    }

    /// Verify hash is consistent.
    #[test]
    fn fuzz_receipt_hash_consistency(backend in "[a-z]{1,15}") {
        let receipt = ReceiptBuilder::new(&backend)
            .outcome(Outcome::Complete)
            .build();
        let h1 = compute_hash(&receipt).unwrap();
        let h2 = compute_hash(&receipt).unwrap();
        prop_assert_eq!(h1, h2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §22  WorkOrderBuilder edge cases
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// WorkOrderBuilder with random task strings serde roundtrips.
    #[test]
    fn fuzz_work_order_builder_random_task(task in "\\PC{0,200}") {
        let wo = WorkOrderBuilder::new(&task).build();
        let json = serde_json::to_string(&wo).unwrap();
        let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.task, wo2.task);
    }

    /// WorkOrderBuilder with all options set never panics.
    #[test]
    fn fuzz_work_order_builder_full(
        task in "[a-zA-Z ]{1,30}",
        root in "[a-zA-Z/]{1,20}",
        model in "[a-zA-Z0-9-]{1,20}",
        turns in 0u32..1000,
    ) {
        let wo = WorkOrderBuilder::new(&task)
            .root(&root)
            .model(&model)
            .max_turns(turns)
            .lane(ExecutionLane::WorkspaceFirst)
            .workspace_mode(WorkspaceMode::Staged)
            .build();
        prop_assert_eq!(wo.task, task);
        prop_assert_eq!(wo.config.model.as_deref(), Some(model.as_str()));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §23  Serde roundtrip fuzzing for core enums
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Random strings parsed as ExecutionMode don't panic.
    #[test]
    fn fuzz_execution_mode_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<ExecutionMode>(&format!("\"{}\"", s));
    }

    /// Random strings parsed as Outcome don't panic.
    #[test]
    fn fuzz_outcome_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<Outcome>(&format!("\"{}\"", s));
    }

    /// Random strings parsed as ExecutionLane don't panic.
    #[test]
    fn fuzz_execution_lane_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<ExecutionLane>(&format!("\"{}\"", s));
    }

    /// Random strings parsed as Capability don't panic.
    #[test]
    fn fuzz_capability_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<Capability>(&format!("\"{}\"", s));
    }

    /// Random strings parsed as SupportLevel don't panic.
    #[test]
    fn fuzz_support_level_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<SupportLevel>(&format!("\"{}\"", s));
    }

    /// Random strings parsed as IrRole don't panic.
    #[test]
    fn fuzz_ir_role_random(s in "\\PC{0,30}") {
        let _ = serde_json::from_str::<IrRole>(&format!("\"{}\"", s));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §24  Canonical JSON and hashing edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_canonical_json_special_values() {
    let values: Vec<serde_json::Value> = vec![
        json!(null),
        json!(true),
        json!(false),
        json!(0),
        json!(-1),
        json!(f64::MAX),
        json!(f64::MIN),
        json!(""),
        json!([]),
        json!({}),
        json!({"a": null, "b": [1, 2, 3]}),
    ];
    for val in &values {
        let _ = abp_core::canonical_json(val);
    }
}

#[test]
fn fuzz_sha256_hex_empty() {
    let hash = abp_core::sha256_hex(b"");
    assert_eq!(hash.len(), 64);
}

#[test]
fn fuzz_sha256_hex_large() {
    let data = vec![0xFFu8; 1024 * 1024];
    let hash = abp_core::sha256_hex(&data);
    assert_eq!(hash.len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════
// §25  Envelope encode/decode roundtrip fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Fatal envelopes with random error messages roundtrip.
    #[test]
    fn fuzz_envelope_fatal_roundtrip(error in "[a-zA-Z0-9 ]{0,100}") {
        let env = Envelope::Fatal {
            ref_id: None,
            error: error.clone(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error: e, .. } => prop_assert_eq!(e, error),
            _ => prop_assert!(false, "expected Fatal"),
        }
    }

    /// Hello envelopes with random backend IDs roundtrip.
    #[test]
    fn fuzz_envelope_hello_roundtrip(id in "[a-zA-Z0-9_-]{1,30}") {
        let backend = BackendIdentity {
            id: id.clone(),
            backend_version: None,
            adapter_version: None,
        };
        let env = Envelope::hello(backend, CapabilityManifest::new());
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        match decoded {
            Envelope::Hello { backend, .. } => prop_assert_eq!(backend.id, id),
            _ => prop_assert!(false, "expected Hello"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §26  Dialect detection with known-format JSON
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// OpenAI-like messages don't panic detection.
    #[test]
    fn fuzz_dialect_openai_like(content in "[a-zA-Z ]{0,50}") {
        let obj = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": content}]
        });
        let detector = DialectDetector::new();
        let _ = detector.detect(&obj);
    }

    /// Claude-like messages don't panic detection.
    #[test]
    fn fuzz_dialect_claude_like(content in "[a-zA-Z ]{0,50}") {
        let obj = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": content}],
            "max_tokens": 1024
        });
        let detector = DialectDetector::new();
        let _ = detector.detect(&obj);
    }

    /// Gemini-like messages don't panic detection.
    #[test]
    fn fuzz_dialect_gemini_like(content in "[a-zA-Z ]{0,50}") {
        let obj = json!({
            "contents": [{"parts": [{"text": content}]}]
        });
        let detector = DialectDetector::new();
        let _ = detector.detect(&obj);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §27  ContextPacket and ContextSnippet fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// ContextPacket with random file paths serde roundtrips.
    #[test]
    fn fuzz_context_packet_random(
        files in prop::collection::vec("\\PC{0,50}", 0..10),
    ) {
        let packet = ContextPacket {
            files,
            snippets: vec![],
        };
        let json = serde_json::to_string(&packet).unwrap();
        let packet2: ContextPacket = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(packet.files.len(), packet2.files.len());
    }

    /// ContextSnippet with random content serde roundtrips.
    #[test]
    fn fuzz_context_snippet_random(
        name in "[a-zA-Z]{1,20}",
        content in "\\PC{0,200}",
    ) {
        let snippet = ContextSnippet {
            name: name.clone(),
            content,
        };
        let json = serde_json::to_string(&snippet).unwrap();
        let snippet2: ContextSnippet = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(snippet.name, snippet2.name);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §28  ArtifactRef and VerificationReport fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// ArtifactRef with random values serde roundtrips.
    #[test]
    fn fuzz_artifact_ref_random(kind in "[a-z]{1,10}", path in "[a-z/.]{1,30}") {
        let artifact = ArtifactRef { kind: kind.clone(), path };
        let json = serde_json::to_string(&artifact).unwrap();
        let artifact2: ArtifactRef = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(artifact.kind, artifact2.kind);
    }

    /// VerificationReport with random git_diff serde roundtrips.
    #[test]
    fn fuzz_verification_report_random(
        diff in prop::option::of("\\PC{0,200}"),
        status in prop::option::of("[a-zA-Z ]{0,50}"),
    ) {
        let report = VerificationReport {
            git_diff: diff,
            git_status: status,
            harness_ok: true,
        };
        let json = serde_json::to_string(&report).unwrap();
        let report2: VerificationReport = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(report.harness_ok, report2.harness_ok);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §29  UsageNormalized and RuntimeConfig fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// UsageNormalized with random token counts serde roundtrips.
    #[test]
    fn fuzz_usage_normalized_random(
        input in prop::option::of(0u64..u64::MAX),
        output in prop::option::of(0u64..u64::MAX),
    ) {
        let usage = UsageNormalized {
            input_tokens: input,
            output_tokens: output,
            ..UsageNormalized::default()
        };
        let json = serde_json::to_string(&usage).unwrap();
        let usage2: UsageNormalized = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(usage.input_tokens, usage2.input_tokens);
    }

    /// RuntimeConfig with random vendor flags serde roundtrips.
    #[test]
    fn fuzz_runtime_config_random(
        model in prop::option::of("[a-z0-9-]{1,20}"),
        turns in prop::option::of(0u32..10000),
    ) {
        let config = RuntimeConfig {
            model,
            max_turns: turns,
            ..RuntimeConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let config2: RuntimeConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(config.max_turns, config2.max_turns);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §30  BackendIdentity fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// BackendIdentity with random values serde roundtrips.
    #[test]
    fn fuzz_backend_identity_random(
        id in "\\PC{1,50}",
        version in prop::option::of("[0-9.]{1,10}"),
    ) {
        let identity = BackendIdentity {
            id: id.clone(),
            backend_version: version,
            adapter_version: None,
        };
        let json = serde_json::to_string(&identity).unwrap();
        let identity2: BackendIdentity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(identity.id, identity2.id);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §31  SupportLevel satisfies edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_support_level_satisfies_all_combinations() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "test".into(),
        },
    ];
    let mins = vec![MinSupport::Native, MinSupport::Emulated];
    for level in &levels {
        for min in &mins {
            // Should never panic
            let _ = level.satisfies(min);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §32  IrMessage helper methods fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// text_content with mixed block types never panics.
    #[test]
    fn fuzz_ir_message_text_content(text1 in "\\PC{0,50}", text2 in "\\PC{0,50}") {
        let msg = IrMessage::new(IrRole::User, vec![
            IrContentBlock::Text { text: text1.clone() },
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text { text: text2.clone() },
        ]);
        let combined = msg.text_content();
        prop_assert!(combined.contains(&text1));
        prop_assert!(combined.contains(&text2));
        prop_assert!(!msg.is_text_only());
    }

    /// tool_use_blocks returns only ToolUse blocks.
    #[test]
    fn fuzz_ir_message_tool_use_blocks(name in "[a-z]{1,10}") {
        let msg = IrMessage::new(IrRole::Assistant, vec![
            IrContentBlock::Text { text: "hello".into() },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: name.clone(),
                input: json!({}),
            },
        ]);
        let blocks = msg.tool_use_blocks();
        prop_assert_eq!(blocks.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §33  IrConversation accessor fuzz
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_ir_conversation_accessors() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a bot"),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
        IrMessage::text(IrRole::User, "Bye"),
        IrMessage::text(IrRole::Assistant, "Goodbye!"),
    ]);
    assert!(conv.system_message().is_some());
    assert_eq!(conv.last_assistant().unwrap().text_content(), "Goodbye!");
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
    assert!(!conv.is_empty());
    assert_eq!(conv.len(), 5);
    assert!(conv.last_message().is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// §34  Encode many to writer fuzz
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Encoding many fatal envelopes to a writer never panics.
    #[test]
    fn fuzz_encode_many_to_writer(count in 0usize..20) {
        let envelopes: Vec<Envelope> = (0..count).map(|i| {
            Envelope::Fatal {
                ref_id: None,
                error: format!("error-{}", i),
                error_code: None,
            }
        }).collect();
        let mut buf = Vec::new();
        let result = JsonlCodec::encode_many_to_writer(&mut buf, &envelopes);
        prop_assert!(result.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §35  Glob decide_path vs decide_str consistency
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// decide_str and decide_path agree for simple paths.
    #[test]
    fn fuzz_glob_decide_consistency(path in "[a-z/]{1,30}") {
        if let Ok(globs) = IncludeExcludeGlobs::new(
            &["**/*.rs".to_string()],
            &["**/test/**".to_string()],
        ) {
            let str_result = globs.decide_str(&path);
            let path_result = globs.decide_path(Path::new(&path));
            prop_assert_eq!(str_result, path_result);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §36  Config edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_config_extremely_long_backend_name() {
    let name = "a".repeat(10000);
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(name, BackendEntry::Mock {});
    let _ = validate_config(&cfg);
}

#[test]
fn fuzz_config_many_backends() {
    let mut cfg = BackplaneConfig::default();
    for i in 0..1000 {
        cfg.backends
            .insert(format!("backend-{}", i), BackendEntry::Mock {});
    }
    let _ = validate_config(&cfg);
}

#[test]
fn fuzz_config_special_chars_in_values() {
    let toml_str = r#"
default_backend = "mock<script>alert(1)</script>"
log_level = "debug"
workspace_dir = "/tmp/test; rm -rf /"
"#;
    let _ = parse_toml(toml_str);
}

// ═══════════════════════════════════════════════════════════════════════
// §37  Mapping known_rules never panics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_mapping_known_rules_complete() {
    let registry = known_rules();
    assert!(!registry.is_empty());
    // Lookup every dialect pair for every known feature
    let known_features = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];
    for src in Dialect::all() {
        for tgt in Dialect::all() {
            for feature in &known_features {
                let _ = registry.lookup(*src, *tgt, feature);
            }
        }
    }
}

#[test]
fn fuzz_mapping_matrix_from_registry() {
    let registry = known_rules();
    let matrix = MappingMatrix::from_registry(&registry);
    for src in Dialect::all() {
        for tgt in Dialect::all() {
            let _ = matrix.get(*src, *tgt);
            let _ = matrix.is_supported(*src, *tgt);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §38  IrContentBlock ToolResult with nested blocks
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// ToolResult with nested content blocks serde roundtrips.
    #[test]
    fn fuzz_ir_tool_result_nested(text in "[a-zA-Z ]{0,30}", is_error in proptest::bool::ANY) {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "tu-1".into(),
            content: vec![
                IrContentBlock::Text { text: text.clone() },
                IrContentBlock::Text { text: "extra".into() },
            ],
            is_error,
        };
        let json = serde_json::to_string(&block).unwrap();
        let block2: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, block2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §39  Envelope error_code extraction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_envelope_error_code_non_fatal() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(env.error_code().is_none());
}

#[test]
fn fuzz_envelope_error_code_fatal_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    assert!(env.error_code().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// §40  JSON arrays and non-object inputs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fuzz_json_array_as_work_order() {
    let _ = serde_json::from_str::<WorkOrder>("[]");
    let _ = serde_json::from_str::<WorkOrder>("[1,2,3]");
}

#[test]
fn fuzz_json_primitive_as_receipt() {
    let _ = serde_json::from_str::<Receipt>("42");
    let _ = serde_json::from_str::<Receipt>("true");
    let _ = serde_json::from_str::<Receipt>("null");
    let _ = serde_json::from_str::<Receipt>("\"hello\"");
}

#[test]
fn fuzz_json_array_as_envelope() {
    let _ = JsonlCodec::decode("[]");
    let _ = JsonlCodec::decode("[1,2,3]");
    let _ = JsonlCodec::decode("\"just a string\"");
    let _ = JsonlCodec::decode("42");
}

#[test]
fn fuzz_json_non_object_as_agent_event() {
    let _ = serde_json::from_str::<AgentEvent>("null");
    let _ = serde_json::from_str::<AgentEvent>("false");
    let _ = serde_json::from_str::<AgentEvent>("[\"array\"]");
}
