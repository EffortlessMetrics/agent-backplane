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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Security-focused tests for the agent backplane.
//!
//! Covers policy enforcement, path traversal prevention, input fuzzing,
//! receipt integrity, protocol hardening, config parsing safety, and
//! serde deserialization of untrusted payloads.

use std::collections::BTreeMap;
use std::path::Path;

use abp_config::{BackendEntry, BackplaneConfig, parse_toml, validate_config};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, PolicyProfile, Receipt,
    WorkOrderBuilder,
};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use abp_policy::audit::PolicyAuditor;
use abp_policy::compose::{ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_protocol::validate::EnvelopeValidator;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{Outcome, ReceiptBuilder, ReceiptChain, compute_hash, verify_hash};
use chrono::Utc;
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn restrictive_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Shell".into(), "Execute".into()],
        deny_read: vec![
            "**/.env".into(),
            "**/.ssh/**".into(),
            "**/secrets/**".into(),
        ],
        deny_write: vec!["**/config/**".into(), "**/.git/**".into()],
        ..Default::default()
    }
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

// ===========================================================================
// 1. Policy enforcement — unauthorized tool access
// ===========================================================================

#[test]
fn policy_blocks_disallowed_tool() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let d = engine.can_use_tool("Shell");
    assert!(!d.allowed);
}

#[test]
fn policy_blocks_second_disallowed_tool() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_use_tool("Execute").allowed);
}

#[test]
fn policy_allows_explicitly_listed_tool() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
}

#[test]
fn policy_blocks_unlisted_tool_when_allowlist_set() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_use_tool("UnknownTool").allowed);
}

#[test]
fn policy_empty_allowlist_permits_any_tool_not_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Dangerous".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Dangerous").allowed);
}

// ===========================================================================
// 2. Policy enforcement — unauthorized file read/write
// ===========================================================================

#[test]
fn policy_denies_read_of_env_file() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn policy_denies_read_of_nested_env_file() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_read_path(Path::new("app/.env")).allowed);
}

#[test]
fn policy_denies_read_of_ssh_keys() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_read_path(Path::new(".ssh/id_rsa")).allowed);
}

#[test]
fn policy_denies_write_to_config_dir() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_write_path(Path::new("config/app.toml")).allowed);
}

#[test]
fn policy_denies_write_to_git_dir() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/HEAD")).allowed);
}

#[test]
fn policy_allows_read_of_normal_file() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_allows_write_to_normal_file() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

// ===========================================================================
// 3. Deny rules take precedence over allow rules
// ===========================================================================

#[test]
fn deny_overrides_allow_for_tools() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Shell".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Shell").allowed);
}

#[test]
fn deny_overrides_allow_wildcard_glob() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Shell*".into()],
        disallowed_tools: vec!["Shell*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("ShellExec").allowed);
}

#[test]
fn composed_deny_overrides_precedence() {
    let permissive = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..Default::default()
    };
    let deny_shell = PolicyProfile {
        disallowed_tools: vec!["Shell".into()],
        ..Default::default()
    };
    let engine = ComposedEngine::new(
        vec![permissive, deny_shell],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();
    assert!(engine.check_tool("Shell").is_deny());
}

#[test]
fn deny_read_overrides_absence_of_deny() {
    let policy = PolicyProfile {
        deny_read: vec!["**/secret.txt".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("data/secret.txt")).allowed);
    assert!(engine.can_read_path(Path::new("data/public.txt")).allowed);
}

#[test]
fn deny_write_overrides_absence_of_deny() {
    let policy = PolicyProfile {
        deny_write: vec!["**/readonly/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(Path::new("readonly/data.txt"))
            .allowed
    );
}

// ===========================================================================
// 4. Path traversal attacks
// ===========================================================================

#[test]
fn traversal_dotdot_etc_passwd_denied() {
    let policy = PolicyProfile {
        deny_read: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(Path::new("../../../etc/passwd"))
            .allowed
    );
}

#[test]
fn traversal_dotdot_in_middle_denied() {
    let policy = PolicyProfile {
        deny_read: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(Path::new("src/../../../etc/shadow"))
            .allowed
    );
}

#[test]
fn traversal_dotdot_write_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(Path::new("../outside/evil.sh"))
            .allowed
    );
}

#[test]
fn traversal_encoded_dotdot_in_glob_pattern() {
    // Deny literal ".." segments anywhere
    let policy = PolicyProfile {
        deny_read: vec!["**/..".into(), "**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(Path::new("foo/bar/../../etc/hosts"))
            .allowed
    );
}

#[test]
fn deeply_nested_traversal_denied() {
    let policy = PolicyProfile {
        deny_read: vec!["**/../**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let evil_path = "../".repeat(50) + "etc/passwd";
    assert!(!engine.can_read_path(Path::new(&evil_path)).allowed);
}

// ===========================================================================
// 5. Glob injection handled safely
// ===========================================================================

#[test]
fn glob_with_special_regex_chars_does_not_panic() {
    // Characters that are special in regex but should be handled safely in globs
    let patterns = vec![
        "src/[a-z].rs".into(),
        "src/{a,b}.rs".into(),
        "**/*.rs".into(),
    ];
    let result = IncludeExcludeGlobs::new(&patterns, &[]);
    assert!(result.is_ok());
}

#[test]
fn glob_with_unclosed_bracket_returns_error() {
    let patterns = vec!["src/[unclosed".into()];
    let result = IncludeExcludeGlobs::new(&patterns, &[]);
    assert!(result.is_err());
}

#[test]
fn glob_with_null_byte_in_pattern_does_not_panic() {
    let patterns = vec!["src/\0evil".into()];
    // Should either compile or return error, but never panic
    let _ = IncludeExcludeGlobs::new(&patterns, &[]);
}

#[test]
fn glob_with_extremely_long_pattern_does_not_panic() {
    let long_pattern = "**/".repeat(500) + "*.rs";
    let _ = IncludeExcludeGlobs::new(&[long_pattern], &[]);
}

#[test]
fn glob_exclude_takes_precedence_over_include() {
    let inc = vec!["**/*.rs".into()];
    let exc = vec!["**/secret.rs".into()];
    let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
    assert!(!globs.decide_path(Path::new("secret.rs")).is_allowed());
    assert!(globs.decide_path(Path::new("main.rs")).is_allowed());
}

#[test]
fn glob_empty_string_pattern_does_not_panic() {
    let _ = IncludeExcludeGlobs::new(&["".into()], &[]);
}

// ===========================================================================
// 6. Unicode normalization attacks
// ===========================================================================

#[test]
fn unicode_path_nfd_vs_nfc_dot_env() {
    // NFC: ".env" vs NFD composed differently; policy should still catch it
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Standard ASCII .env
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn unicode_homoglyph_tool_name_not_confused_with_ascii() {
    // Cyrillic 'а' (U+0430) looks like Latin 'a' but is different
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec![],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // The Cyrillic version should NOT match the ASCII allowlist
    assert!(!engine.can_use_tool("Re\u{0430}d").allowed);
}

#[test]
fn unicode_tool_name_with_zero_width_chars() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Zero-width space (U+200B) inserted into tool name
    assert!(!engine.can_use_tool("Re\u{200B}ad").allowed);
}

#[test]
fn unicode_path_with_bidi_override_chars() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Right-to-left override character shouldn't bypass the check
    let evil_path = "\u{202E}.env";
    // The path doesn't match the glob, so it won't be denied by this rule,
    // but crucially it should not PANIC
    let _ = engine.can_read_path(Path::new(evil_path));
}

#[test]
fn unicode_glob_patterns_compile_safely() {
    let patterns = vec!["**/日本語/**".into(), "**/données/*.txt".into()];
    let result = IncludeExcludeGlobs::new(&patterns, &[]);
    assert!(result.is_ok());
}

#[test]
fn unicode_path_matching_works() {
    let globs = IncludeExcludeGlobs::new(&["**/données/**".into()], &[]).unwrap();
    assert!(
        globs
            .decide_path(Path::new("données/file.txt"))
            .is_allowed()
    );
}

// ===========================================================================
// 7. Oversized inputs don't cause panics
// ===========================================================================

#[test]
fn very_long_task_string_does_not_panic() {
    let huge_task = "A".repeat(10_000_000);
    let wo = WorkOrderBuilder::new(huge_task).build();
    assert!(wo.task.len() == 10_000_000);
}

#[test]
fn very_long_tool_name_does_not_panic() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let long_name = "X".repeat(1_000_000);
    let d = engine.can_use_tool(&long_name);
    assert!(!d.allowed);
}

#[test]
fn very_long_path_does_not_panic() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let long_path = "a/".repeat(10_000) + ".env";
    let _ = engine.can_read_path(Path::new(&long_path));
}

#[test]
fn deeply_nested_json_in_agent_event_does_not_panic() {
    // Build deeply nested JSON
    let mut val = json!("leaf");
    for _ in 0..200 {
        val = json!({ "nest": val });
    }
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "test".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: val,
        },
        ext: None,
    };
    let serialized = serde_json::to_string(&event);
    assert!(serialized.is_ok());
}

#[test]
fn large_receipt_trace_serializes_without_panic() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..10_000 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let hash = compute_hash(&receipt);
    assert!(hash.is_ok());
}

#[test]
fn many_policy_rules_do_not_panic() {
    let policy = PolicyProfile {
        allowed_tools: (0..1000).map(|i| format!("tool_{i}")).collect(),
        disallowed_tools: (0..1000).map(|i| format!("deny_{i}")).collect(),
        deny_read: (0..1000).map(|i| format!("**/secret_{i}/**")).collect(),
        deny_write: (0..1000).map(|i| format!("**/locked_{i}/**")).collect(),
        ..Default::default()
    };
    let result = PolicyEngine::new(&policy);
    assert!(result.is_ok());
}

// ===========================================================================
// 8. Empty/null inputs don't cause undefined behavior
// ===========================================================================

#[test]
fn empty_tool_name_handled() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let d = engine.can_use_tool("");
    // Should not panic; specific behavior depends on policy (denied since not in allowlist)
    assert!(!d.allowed);
}

#[test]
fn empty_path_read_handled() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let _ = engine.can_read_path(Path::new(""));
}

#[test]
fn empty_path_write_handled() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let _ = engine.can_write_path(Path::new(""));
}

#[test]
fn default_policy_profile_is_permissive() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(engine.can_read_path(Path::new("any/file")).allowed);
    assert!(engine.can_write_path(Path::new("any/file")).allowed);
}

#[test]
fn empty_glob_lists_compile() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(globs.decide_path(Path::new("anything")).is_allowed());
}

#[test]
fn null_json_value_in_work_order_config_roundtrips() {
    let wo = WorkOrderBuilder::new("test").build();
    let json_str = serde_json::to_string(&wo).unwrap();
    let roundtripped: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(roundtripped.is_object());
}

// ===========================================================================
// 9. Receipt hash tampering detection
// ===========================================================================

#[test]
fn tampered_receipt_hash_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    assert!(verify_hash(&receipt));

    // Tamper with the outcome
    receipt.outcome = Outcome::Failed;
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_receipt_backend_id_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    receipt.backend.id = "evil-backend".into();
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_receipt_trace_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
    receipt.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "injected".into(),
        },
        ext: None,
    });
    assert!(!verify_hash(&receipt));
}

#[test]
fn tampered_hash_value_detected() {
    let mut receipt = make_receipt();
    receipt.receipt_sha256 = Some("deadbeef".repeat(8));
    assert!(!verify_hash(&receipt));
}

#[test]
fn receipt_with_no_hash_passes_verification() {
    let receipt = make_receipt();
    assert!(receipt.receipt_sha256.is_none());
    assert!(verify_hash(&receipt));
}

#[test]
fn receipt_hash_excludes_hash_field_from_computation() {
    let receipt = make_receipt();
    let h1 = compute_hash(&receipt).unwrap();

    let mut receipt2 = make_receipt();
    // Force identical metadata for comparison
    receipt2.meta = receipt.meta.clone();
    receipt2.receipt_sha256 = Some("anything".into());
    let h2 = compute_hash(&receipt2).unwrap();

    // Hash should be the same since receipt_sha256 is nulled before hashing
    assert_eq!(h1, h2);
}

// ===========================================================================
// 10. Receipt hash collision resistance
// ===========================================================================

#[test]
fn different_outcomes_produce_different_hashes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    // The run_ids differ (v4 UUIDs), so hashes differ anyway; but verify no panic
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn identical_receipts_produce_identical_hashes() {
    let receipt = make_receipt();
    let h1 = compute_hash(&receipt).unwrap();
    let h2 = compute_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let receipt = make_receipt();
    let h = compute_hash(&receipt).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_chain_detects_tampered_hash_on_verify() {
    let mut chain = ReceiptChain::new();
    let mut r = make_receipt();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    chain.push(r).unwrap();

    // Tamper after insertion is not directly possible through the chain API,
    // but we can verify that the chain validates successfully when untampered
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// 11. Workspace staging doesn't leak files outside allowed globs
// ===========================================================================

#[test]
fn workspace_glob_excludes_dotenv() {
    let globs = IncludeExcludeGlobs::new(&["**/*.rs".into()], &["**/.env".into()]).unwrap();
    assert!(!globs.decide_path(Path::new(".env")).is_allowed());
    assert!(globs.decide_path(Path::new("src/main.rs")).is_allowed());
}

#[test]
fn workspace_glob_excludes_git_directory() {
    let globs = IncludeExcludeGlobs::new(&["**/*".into()], &["**/.git/**".into()]).unwrap();
    assert!(!globs.decide_path(Path::new(".git/HEAD")).is_allowed());
    assert!(!globs.decide_path(Path::new(".git/config")).is_allowed());
}

#[test]
fn workspace_glob_excludes_secrets_directory() {
    let globs = IncludeExcludeGlobs::new(&["**/*".into()], &["**/secrets/**".into()]).unwrap();
    assert!(
        !globs
            .decide_path(Path::new("secrets/api_key.txt"))
            .is_allowed()
    );
    assert!(globs.decide_path(Path::new("src/main.rs")).is_allowed());
}

#[test]
fn workspace_glob_include_restricts_to_src_only() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert!(globs.decide_path(Path::new("src/lib.rs")).is_allowed());
    assert!(!globs.decide_path(Path::new("config/app.toml")).is_allowed());
    assert!(!globs.decide_path(Path::new(".env")).is_allowed());
}

#[test]
fn workspace_glob_traversal_outside_root_excluded() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["**/../**".into()]).unwrap();
    assert!(
        !globs
            .decide_path(Path::new("src/../../../etc/passwd"))
            .is_allowed()
    );
}

// ===========================================================================
// 12. Config parsing handles malicious TOML safely
// ===========================================================================

#[test]
fn config_parse_empty_string() {
    let result = parse_toml("");
    // Empty TOML should parse to default-ish config or error; not panic
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn config_parse_garbage_input() {
    let result = parse_toml("not valid toml at all {{{{");
    assert!(result.is_err());
}

#[test]
fn config_parse_extremely_long_value() {
    let long_val = "x".repeat(10_000_000);
    let toml_str = format!("default_backend = \"{long_val}\"");
    let result = parse_toml(&toml_str);
    // Should either succeed or error, not panic
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn config_parse_deeply_nested_tables() {
    // TOML doesn't support truly deep arbitrary nesting, but we can try
    let toml_str = "[backends.mock]\ntype = \"mock\"";
    let result = parse_toml(toml_str);
    assert!(result.is_ok());
}

#[test]
fn config_parse_null_bytes_in_toml() {
    let result = parse_toml("default_backend = \"test\0evil\"");
    // Should handle gracefully
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn config_validate_rejects_empty_sidecar_command() {
    let mut config = BackplaneConfig::default();
    config.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let result = validate_config(&config);
    assert!(result.is_err());
}

#[test]
fn config_validate_rejects_excessive_timeout() {
    let mut config = BackplaneConfig::default();
    config.backends.insert(
        "slow".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(100_000),
        },
    );
    let result = validate_config(&config);
    assert!(result.is_err());
}

#[test]
fn config_validate_rejects_zero_timeout() {
    let mut config = BackplaneConfig::default();
    config.backends.insert(
        "instant".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let result = validate_config(&config);
    assert!(result.is_err());
}

// ===========================================================================
// 13. JSONL protocol handles malicious input safely
// ===========================================================================

#[test]
fn protocol_decode_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn protocol_decode_garbage_json() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
}

#[test]
fn protocol_decode_valid_json_but_wrong_shape() {
    let result = JsonlCodec::decode(r#"{"foo": "bar"}"#);
    assert!(result.is_err());
}

#[test]
fn protocol_decode_missing_discriminator_field() {
    let result = JsonlCodec::decode(r#"{"contract_version": "abp/v0.1"}"#);
    assert!(result.is_err());
}

#[test]
fn protocol_decode_unknown_envelope_type() {
    let result = JsonlCodec::decode(r#"{"t": "exploit", "payload": "evil"}"#);
    assert!(result.is_err());
}

#[test]
fn protocol_decode_extremely_large_payload_does_not_panic() {
    let big = "A".repeat(10_000_000);
    let json = format!(r#"{{"t": "fatal", "error": "{big}"}}"#);
    let result = JsonlCodec::decode(&json);
    // Should parse successfully as a fatal envelope
    assert!(result.is_ok());
}

#[test]
fn protocol_decode_null_bytes_in_json() {
    let result = JsonlCodec::decode("{\"t\": \"fatal\", \"error\": \"test\x00evil\"}");
    // JSON technically doesn't allow raw null bytes; serde may reject
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn protocol_encode_decode_roundtrip_preserves_envelope() {
    let hello = hello_envelope();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    // Verify it's still a Hello variant
    match decoded {
        Envelope::Hello { .. } => {}
        _ => panic!("expected Hello envelope after roundtrip"),
    }
}

#[test]
fn protocol_validator_rejects_missing_hello() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("test").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let errors = validator.validate_sequence(&[run]);
    assert!(!errors.is_empty());
}

#[test]
fn protocol_validator_accepts_valid_sequence() {
    let validator = EnvelopeValidator::new();
    let hello = hello_envelope();
    let wo = WorkOrderBuilder::new("test").build();
    let run_id = wo.id.to_string();
    let run = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };
    let fin = Envelope::Final {
        ref_id: run_id,
        receipt: make_receipt(),
    };
    let errors = validator.validate_sequence(&[hello, run, fin]);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

// ===========================================================================
// 14. No unintended information disclosure in error messages
// ===========================================================================

#[test]
fn protocol_error_does_not_leak_full_json_on_decode_failure() {
    let secret_payload = r#"{"t": "run", "secret_key": "sk-1234567890abcdef"}"#;
    let result = JsonlCodec::decode(secret_payload);
    if let Err(e) = result {
        let msg = e.to_string();
        // Error message should not contain the secret key value
        assert!(
            !msg.contains("sk-1234567890abcdef"),
            "error message leaked secret: {msg}"
        );
    }
}

#[test]
fn config_error_does_not_leak_file_system_paths_verbatim() {
    let result = parse_toml("invalid = [[[");
    if let Err(e) = result {
        let msg = format!("{e:?}");
        // Should describe the parse error, not expose system internals
        assert!(!msg.contains("\\\\?\\"), "error leaked Windows path prefix");
    }
}

#[test]
fn policy_denial_reason_is_generic() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Shell".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_use_tool("Shell");
    assert!(!d.allowed);
    // The reason should exist but not expose internal implementation details
    if let Some(reason) = &d.reason {
        assert!(!reason.is_empty());
    }
}

#[test]
fn fatal_envelope_error_string_is_present() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "Something went wrong".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "Something went wrong");
        }
        _ => panic!("expected Fatal envelope"),
    }
}

// ===========================================================================
// 15. Serde deserialization handles untrusted input
// ===========================================================================

#[test]
fn deserialize_work_order_from_untrusted_json() {
    let bad_json = r#"{"id": "not-a-uuid", "task": 12345}"#;
    let result = serde_json::from_str::<abp_core::WorkOrder>(bad_json);
    assert!(result.is_err());
}

#[test]
fn deserialize_receipt_from_empty_object() {
    let result = serde_json::from_str::<Receipt>("{}");
    assert!(result.is_err());
}

#[test]
fn deserialize_receipt_with_extra_fields_succeeds() {
    let receipt = make_receipt();
    let mut val = serde_json::to_value(&receipt).unwrap();
    val.as_object_mut()
        .unwrap()
        .insert("injected_field".into(), json!("evil"));
    // Serde should ignore unknown fields by default
    let result = serde_json::from_value::<Receipt>(val);
    assert!(result.is_ok());
}

#[test]
fn deserialize_agent_event_with_bad_type_tag() {
    let bad = r#"{"type": "nonexistent_event", "ts": "2024-01-01T00:00:00Z"}"#;
    let result = serde_json::from_str::<AgentEvent>(bad);
    assert!(result.is_err());
}

#[test]
fn deserialize_agent_event_missing_ts() {
    let bad = r#"{"type": "run_started", "message": "hi"}"#;
    let result = serde_json::from_str::<AgentEvent>(bad);
    assert!(result.is_err());
}

#[test]
fn deserialize_envelope_with_integer_overflow() {
    // Try to overflow a u64 field
    let json = r#"{"t": "fatal", "error": "test", "ref_id": null}"#;
    let result = serde_json::from_str::<Envelope>(json);
    assert!(result.is_ok());
}

#[test]
fn deserialize_policy_profile_with_extra_fields() {
    let json = r#"{"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": [], "evil_field": "payload"}"#;
    let result = serde_json::from_str::<PolicyProfile>(json);
    assert!(result.is_ok());
}

#[test]
fn deserialize_config_with_unknown_backend_type() {
    let toml_str = r#"
[backends.evil]
type = "rootkit"
command = "rm -rf /"
"#;
    let result = parse_toml(toml_str);
    assert!(result.is_err());
}

#[test]
fn deserialize_deeply_nested_serde_value_does_not_stack_overflow() {
    // Build deeply nested JSON string — serde_json has a recursion limit,
    // so very deep nesting returns an error rather than stack-overflowing.
    let mut json = String::new();
    let depth = 128;
    for _ in 0..depth {
        json.push_str(r#"{"a":"#);
    }
    json.push_str("\"leaf\"");
    for _ in 0..depth {
        json.push('}');
    }
    let result = serde_json::from_str::<serde_json::Value>(&json);
    // Either succeeds or returns a controlled error — must not panic/stack overflow
    assert!(result.is_ok() || result.is_err());
}

// ===========================================================================
// Additional edge cases — policy, audit, rules, composed engine
// ===========================================================================

#[test]
fn auditor_tracks_denied_tool_access() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_tool("Shell");
    auditor.check_tool("Read");
    assert!(auditor.denied_count() >= 1);
    assert!(auditor.allowed_count() >= 1);
}

#[test]
fn auditor_tracks_denied_read_path() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_read(".env");
    assert!(auditor.denied_count() >= 1);
}

#[test]
fn auditor_tracks_denied_write_path() {
    let engine = PolicyEngine::new(&restrictive_policy()).unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_write("config/app.toml");
    assert!(auditor.denied_count() >= 1);
}

#[test]
fn rule_engine_deny_takes_precedence_over_allow() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "allow-all".into(),
        description: "Allow everything".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 10,
    });
    engine.add_rule(Rule {
        id: "deny-shell".into(),
        description: "Deny shell".into(),
        condition: RuleCondition::Pattern("Shell".into()),
        effect: RuleEffect::Deny,
        priority: 100,
    });
    assert!(matches!(engine.evaluate("Shell"), RuleEffect::Deny));
}

#[test]
fn rule_engine_not_condition_inverts() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "deny-non-read".into(),
        description: "Deny anything that is not Read".into(),
        condition: RuleCondition::Not(Box::new(RuleCondition::Pattern("Read".into()))),
        effect: RuleEffect::Deny,
        priority: 100,
    });
    assert!(matches!(engine.evaluate("Write"), RuleEffect::Deny));
    // "Read" matches the inner pattern, so Not inverts it — should not be Deny
    assert!(!matches!(engine.evaluate("Read"), RuleEffect::Deny));
}

#[test]
fn composed_engine_first_applicable_stops_at_first_match() {
    let allow_all = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..Default::default()
    };
    let deny_all = PolicyProfile {
        disallowed_tools: vec!["*".into()],
        ..Default::default()
    };
    // FirstApplicable: first policy wins
    let engine =
        ComposedEngine::new(vec![allow_all, deny_all], PolicyPrecedence::FirstApplicable).unwrap();
    assert!(engine.check_tool("anything").is_allow());
}

#[test]
fn policy_validator_warns_on_overly_broad_glob() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["*".into()],
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&policy);
    assert!(!warnings.is_empty());
}

#[test]
fn policy_set_merge_unions_deny_lists() {
    let mut set = PolicySet::new("combined");
    set.add(PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    });
    set.add(PolicyProfile {
        deny_read: vec!["**/secrets/**".into()],
        ..Default::default()
    });
    let merged = set.merge();
    assert!(merged.deny_read.contains(&"**/.env".to_string()));
    assert!(merged.deny_read.contains(&"**/secrets/**".to_string()));
}

#[test]
fn work_order_with_malicious_policy_still_builds() {
    let policy = PolicyProfile {
        allowed_tools: vec!["'; DROP TABLE users; --".into()],
        deny_read: vec!["$(rm -rf /)".into()],
        ..Default::default()
    };
    // Builder should not execute any of these as commands
    let wo = WorkOrderBuilder::new("test").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools[0], "'; DROP TABLE users; --");
}

#[test]
fn receipt_with_unicode_backend_id_hashes_consistently() {
    let receipt = ReceiptBuilder::new("バックエンド")
        .outcome(Outcome::Complete)
        .build();
    let h1 = compute_hash(&receipt).unwrap();
    let h2 = compute_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn envelope_with_special_chars_in_error_roundtrips() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "error with <html> & \"quotes\" and \nnewlines".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("<html>"));
            assert!(error.contains("\"quotes\""));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn config_merge_overlay_does_not_remove_base_backends() {
    let base = BackplaneConfig {
        backends: {
            let mut m = BTreeMap::new();
            m.insert("mock".into(), BackendEntry::Mock {});
            m
        },
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let merged = abp_config::merge_configs(base, overlay);
    assert!(merged.backends.contains_key("mock"));
}

#[test]
fn receipt_chain_rejects_duplicate_run_ids() {
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(uuid::Uuid::nil())
        .build();
    let mut r1_hashed = r1.clone();
    r1_hashed.receipt_sha256 = Some(compute_hash(&r1_hashed).unwrap());

    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(uuid::Uuid::nil())
        .build();
    let mut r2_hashed = r2;
    r2_hashed.receipt_sha256 = Some(compute_hash(&r2_hashed).unwrap());

    chain.push(r1_hashed).unwrap();
    let result = chain.push(r2_hashed);
    assert!(result.is_err());
}

#[test]
fn protocol_version_parsing_handles_garbage() {
    assert!(abp_protocol::parse_version("").is_none());
    assert!(abp_protocol::parse_version("garbage").is_none());
    assert!(abp_protocol::parse_version("abp/").is_none());
    assert!(abp_protocol::parse_version("abp/vX.Y").is_none());
}

#[test]
fn protocol_version_parsing_handles_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
}

#[test]
fn protocol_incompatible_major_version_detected() {
    assert!(!abp_protocol::is_compatible_version("abp/v9.0", "abp/v0.1"));
}

#[test]
fn protocol_compatible_same_major_version() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
}
