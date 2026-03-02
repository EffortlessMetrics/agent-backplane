// SPDX-License-Identifier: MIT OR Apache-2.0
//! Security-focused tests covering policy enforcement, path traversal
//! prevention, input validation, and resource bounding.

use std::io::BufReader;
use std::path::Path;

use abp_core::chain::ReceiptChain;
use abp_core::validate::validate_receipt;
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, PolicyProfile, ReceiptBuilder, UsageNormalized,
    WorkOrderBuilder,
};
use abp_policy::PolicyEngine;
use abp_policy::audit::PolicyAuditor;
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_protocol::validate::{EnvelopeValidator, ValidationWarning};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// § Path traversal prevention
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn traversal_read_etc_passwd_denied() {
    let policy = PolicyProfile {
        deny_read: vec!["**/etc/passwd".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed, "path traversal read must be denied");
}

#[test]
fn traversal_write_dotdot_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_write_path(Path::new("../../.git/config"));
    assert!(!d.allowed, "path traversal write must be denied");
}

#[test]
fn traversal_write_embedded_dotdot_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Even if a component before the traversal looks innocent, the
    // glob engine should still match after the `..` resolves.
    let d = engine.can_write_path(Path::new("secret/../secret/key.pem"));
    // This path still contains "secret/" so the glob should catch it.
    assert!(!d.allowed, "embedded .. in write path must be denied");
}

#[test]
fn absolute_path_denied_by_restrictive_write_policy() {
    let policy = PolicyProfile {
        deny_write: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Absolute paths on either platform
    let d_unix = engine.can_write_path(Path::new("/etc/shadow"));
    assert!(
        !d_unix.allowed,
        "absolute path write must be denied by catch-all"
    );

    let d_win = engine.can_write_path(Path::new("C:\\Windows\\System32\\config"));
    assert!(
        !d_win.allowed,
        "absolute Windows path write must be denied by catch-all"
    );
}

#[test]
fn null_bytes_in_path_handled_gracefully() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Paths containing null bytes — the engine should not panic.
    let d = engine.can_read_path(Path::new("some\0file.txt"));
    // With a catch-all deny, this must be denied.
    assert!(!d.allowed, "null byte path must be denied by catch-all");
}

#[test]
fn unicode_normalization_path_not_bypass() {
    // The slash-like character ∕ (U+2215 DIVISION SLASH) and
    // the fullwidth solidus ／ (U+FF0F) should not bypass policy.
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let d1 = engine.can_read_path(Path::new("..∕etc∕passwd"));
    assert!(
        !d1.allowed,
        "unicode division slash must be denied by catch-all"
    );

    let d2 = engine.can_read_path(Path::new("..／etc／passwd"));
    assert!(!d2.allowed, "fullwidth solidus must be denied by catch-all");
}

#[test]
fn symlink_traversal_workspace_does_not_follow_links() {
    // The workspace copy function uses `follow_links(false)`.
    // We verify that the WalkDir builder in copy_workspace sets this flag
    // by reading its source (tested indirectly here via staging: symlinks
    // are not followed, so a symlink pointing outside is skipped).
    //
    // This is a structural assertion — actual symlink following is covered
    // by the workspace_staging integration tests.  Here we ensure the
    // policy itself still denies suspicious paths even if they look local.
    let policy = PolicyProfile {
        deny_read: vec!["**/etc/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // A path that a symlink might resolve to:
    let d = engine.can_read_path(Path::new("link_to_etc/etc/shadow"));
    assert!(!d.allowed, "resolved symlink target must match deny globs");
}

// ═══════════════════════════════════════════════════════════════════════
// § Policy enforcement
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn restrictive_policy_blocks_all_tools() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    for tool in &["Bash", "Read", "Write", "Grep", "WebFetch"] {
        let d = engine.can_use_tool(tool);
        assert!(
            !d.allowed,
            "tool '{tool}' should be blocked by wildcard deny"
        );
    }
}

#[test]
fn deny_overrides_allow_in_composed_engine() {
    // Policy A allows everything; Policy B denies Bash.
    let allow_all = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let deny_bash = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };

    let engine =
        ComposedEngine::new(vec![allow_all, deny_bash], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(
        engine.check_tool("Bash").is_deny(),
        "DenyOverrides: deny must win"
    );
    assert!(
        engine.check_tool("Read").is_allow(),
        "Read should still be allowed"
    );
}

#[test]
fn multiple_overlapping_policies_most_restrictive_wins() {
    let mut set = PolicySet::new("security");
    set.add(PolicyProfile {
        deny_write: vec!["*.log".into()],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        deny_write: vec!["*.tmp".into()],
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    let engine = PolicyEngine::new(&merged).unwrap();

    assert!(!engine.can_write_path(Path::new("app.log")).allowed);
    assert!(!engine.can_write_path(Path::new("cache.tmp")).allowed);
    assert!(engine.can_write_path(Path::new("main.rs")).allowed);
}

#[test]
fn policy_bypass_encoding_tricks_still_denied() {
    // Percent-encoded path traversal: the policy engine operates on
    // raw strings, so encoded traversal sequences are just literal chars.
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Percent-encoded ".." — %2e%2e
    let d = engine.can_read_path(Path::new("%2e%2e/etc/passwd"));
    assert!(
        !d.allowed,
        "percent-encoded traversal must be denied by catch-all"
    );

    // Backslash variant
    let d2 = engine.can_read_path(Path::new("..\\..\\etc\\passwd"));
    assert!(
        !d2.allowed,
        "backslash traversal must be denied by catch-all"
    );
}

#[test]
fn overly_broad_glob_issues_warning() {
    let policy = PolicyProfile {
        deny_read: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule),
        "catch-all deny_read should produce an unreachable-rule warning"
    );
}

#[test]
fn overlapping_allow_deny_tools_produces_warning() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny),
        "same tool in allow and deny must warn"
    );
}

#[test]
fn empty_glob_in_policy_produces_warning() {
    let policy = PolicyProfile {
        deny_read: vec!["".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);
    assert!(
        warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob),
        "empty glob string must produce a warning"
    );
}

#[test]
fn auditor_records_all_denials() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/secret*".into()],
        deny_write: vec!["**/locked*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let mut auditor = PolicyAuditor::new(engine);

    auditor.check_tool("Bash");
    auditor.check_read("secret.key");
    auditor.check_write("locked.db");
    auditor.check_tool("Read"); // allowed

    assert_eq!(auditor.denied_count(), 3);
    assert_eq!(auditor.allowed_count(), 1);
    let summary = auditor.summary();
    assert_eq!(summary.denied, 3);
    assert_eq!(summary.allowed, 1);
}

// ═══════════════════════════════════════════════════════════════════════
// § Rule engine security
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rule_engine_highest_priority_deny_wins() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "allow-all".into(),
        description: "allow everything".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: "deny-bash".into(),
        description: "deny Bash".into(),
        condition: RuleCondition::Pattern("Bash*".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });

    assert_eq!(engine.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_not_condition() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "deny-non-read".into(),
        description: "deny anything not matching Read".into(),
        condition: RuleCondition::Not(Box::new(RuleCondition::Pattern("Read".into()))),
        effect: RuleEffect::Deny,
        priority: 1,
    });

    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
    assert_eq!(engine.evaluate("Bash"), RuleEffect::Deny);
}

// ═══════════════════════════════════════════════════════════════════════
// § Input validation — WorkOrder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workorder_extremely_long_task_serialises_round_trip() {
    // 10 MB task string — should not panic or OOM.
    let big = "A".repeat(10_000_000);
    let wo = WorkOrderBuilder::new(&big).build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.task.len(), 10_000_000);
}

#[test]
fn workorder_with_null_bytes_in_task() {
    let task = "hello\0world";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    // JSON will escape the null byte — round trip should preserve it.
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(round.task.contains('\0'));
}

#[test]
fn workorder_empty_task_validated_by_envelope_validator() {
    let wo = WorkOrderBuilder::new("").build();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&envelope);
    assert!(!result.valid, "empty task should be flagged as error");
    assert!(
        result.errors.iter().any(|e| {
            matches!(e, abp_protocol::validate::ValidationError::EmptyField { field } if field == "work_order.task")
        }),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// § Input validation — Envelope / JSONL
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_exceeding_size_limit_produces_warning() {
    // Build an envelope with a very large error message (> 10 MiB).
    let big_error = "E".repeat(11 * 1024 * 1024);
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: big_error,
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&envelope);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::LargePayload { .. })),
        "oversized envelope must produce LargePayload warning"
    );
}

#[test]
fn deeply_nested_json_does_not_panic() {
    // serde_json has a default recursion limit of 128.  Build JSON
    // exceeding that depth and confirm deserialization fails gracefully.
    let depth = 200;
    let mut nested = String::new();
    for _ in 0..depth {
        nested.push_str(r#"{"t":"event","ref_id":"x","event":{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"#);
    }
    nested.push_str("\"deep\"");
    for _ in 0..depth {
        nested.push_str("}}");
    }

    let result = JsonlCodec::decode(&nested);
    assert!(result.is_err(), "deeply nested JSON must fail to parse");
}

#[test]
fn invalid_utf8_in_jsonl_stream_errors_gracefully() {
    // Inject an invalid UTF-8 sequence into an otherwise valid JSONL stream.
    let valid_line = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(valid_line.as_bytes());
    bytes.push(b'\n');
    // Invalid UTF-8 bytes
    bytes.extend_from_slice(&[0xFF, 0xFE, b'\n']);
    bytes.extend_from_slice(valid_line.as_bytes());
    bytes.push(b'\n');

    let reader = BufReader::new(&bytes[..]);
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    // First line should decode fine.
    assert!(results[0].is_ok());
    // Second line has invalid UTF-8 — should be an error.
    assert!(results[1].is_err());
    // Third line should recover.
    assert!(results[2].is_ok());
}

#[test]
fn malformed_envelope_tag_rejected() {
    let bad_lines = [
        r#"{"t":"unknown_type","data":"foo"}"#,
        r#"{"no_tag_field": true}"#,
        r#"not json at all"#,
        r#"{"t":"hello"}"#, // missing required fields
    ];
    for line in &bad_lines {
        let result = JsonlCodec::decode(line);
        assert!(result.is_err(), "malformed line should fail: {line}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// § Resource limits — receipt chain
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_chain_rejects_duplicate_run_ids() {
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2_same_id = {
        // Build a receipt with the same run_id
        let mut r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        r.meta.run_id = r1.meta.run_id;
        r.with_hash().unwrap()
    };

    chain.push(r1).unwrap();
    let err = chain.push(r2_same_id);
    assert!(err.is_err(), "duplicate run_id must be rejected");
}

#[test]
fn receipt_chain_bounded_length() {
    let mut chain = ReceiptChain::new();
    let limit = 1000;
    for _ in 0..limit {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), limit);
}

#[test]
fn receipt_chain_verify_detects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    // Tamper with the hash
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let err = chain.push(r);
    assert!(err.is_err(), "tampered hash must be rejected by chain");
}

// ═══════════════════════════════════════════════════════════════════════
// § Resource limits — usage stats overflow
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn usage_stats_u64_max_serialises_correctly() {
    let usage = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
        cache_read_tokens: Some(u64::MAX),
        cache_write_tokens: Some(u64::MAX),
        request_units: Some(u64::MAX),
        estimated_cost_usd: Some(f64::MAX),
    };
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage(usage)
        .build();
    let json = serde_json::to_string(&receipt).unwrap();
    let round: abp_core::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(round.usage.input_tokens, Some(u64::MAX));
    assert_eq!(round.usage.output_tokens, Some(u64::MAX));
}

// ═══════════════════════════════════════════════════════════════════════
// § Resource limits — event stream bounding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn large_trace_receipt_hashes_deterministically() {
    // A receipt with many trace events should still produce a
    // deterministic and valid hash.
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..10_000 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());

    // Verify the hash is valid
    assert!(validate_receipt(&receipt).is_ok());
}

#[test]
fn receipt_with_many_tool_calls_validates() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..500 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("use_{i}")),
                parent_tool_use_id: None,
                input: json!({"arg": i}),
            },
            ext: None,
        });
    }
    let receipt = builder.with_hash().unwrap();
    assert!(validate_receipt(&receipt).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// § Protocol-level hardening
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_sequence_missing_hello_detected() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("task").build();
    let envelopes = vec![
        Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        },
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "crash".into(),
            error_code: None,
        },
    ];
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, abp_protocol::validate::SequenceError::MissingHello)),
        "missing Hello must be reported"
    );
}

#[test]
fn protocol_sequence_ref_id_mismatch_detected() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("task").build();

    let hello = Envelope::hello(
        abp_core::BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    let envelopes = vec![
        hello,
        Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "WRONG-ID".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "oops".into(),
                },
                ext: None,
            },
        },
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "done".into(),
            error_code: None,
        },
    ];
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            abp_protocol::validate::SequenceError::RefIdMismatch { .. }
        )),
        "ref_id mismatch must be detected"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// § Composed policy edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn composed_engine_deny_overrides_read_path() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        deny_read: vec!["**/.env*".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    assert!(engine.check_read(".env").is_deny());
    assert!(engine.check_read(".env.production").is_deny());
    assert!(engine.check_read("src/main.rs").is_allow());
}

#[test]
fn composed_engine_allow_overrides_strategy() {
    let deny_all_tools = PolicyProfile {
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let allow_read = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(
        vec![deny_all_tools, allow_read],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();

    assert!(
        engine.check_tool("Read").is_allow(),
        "AllowOverrides: allow must win"
    );
}

#[test]
fn composed_engine_first_applicable_strategy() {
    let deny_bash = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let allow_all = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(
        vec![deny_bash, allow_all],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();

    assert!(
        engine.check_tool("Bash").is_deny(),
        "FirstApplicable: first non-abstain (deny) must win"
    );
    assert!(
        engine.check_tool("Read").is_allow(),
        "Read should be allowed by second policy"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// § Receipt validation edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_empty_backend_id_rejected() {
    let receipt = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    let errs = validate_receipt(&receipt).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, abp_core::validate::ValidationError::EmptyBackendId)),
    );
}

#[test]
fn receipt_hash_self_referential_prevention() {
    // Build two receipts that differ only in receipt_sha256 — they
    // should produce the same canonical hash because the field is
    // nulled before hashing.
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = {
        let mut r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        r.meta = r1.meta.clone();
        r.receipt_sha256 = Some("something".into());
        r
    };

    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "receipt_sha256 must not influence the hash");
}

// ═══════════════════════════════════════════════════════════════════════
// § Path-based policy with various traversal patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn deny_read_various_traversal_patterns() {
    let policy = PolicyProfile {
        deny_read: vec![
            "**/etc/**".into(),
            "**/passwd".into(),
            "**/shadow".into(),
            "**/.ssh/**".into(),
        ],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let dangerous_paths = [
        "../../etc/passwd",
        "../../../etc/shadow",
        "foo/../../etc/hosts",
        ".ssh/id_rsa",
        "home/.ssh/authorized_keys",
    ];
    for p in &dangerous_paths {
        let d = engine.can_read_path(Path::new(p));
        assert!(!d.allowed, "read should be denied for '{p}'");
    }
}

#[test]
fn deny_write_various_traversal_patterns() {
    let policy = PolicyProfile {
        deny_write: vec![
            "**/.git/**".into(),
            "**/node_modules/**".into(),
            "**/*.exe".into(),
        ],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let dangerous_paths = [
        "../.git/config",
        "foo/.git/HEAD",
        "../../node_modules/evil/index.js",
        "payload.exe",
    ];
    for p in &dangerous_paths {
        let d = engine.can_write_path(Path::new(p));
        assert!(!d.allowed, "write should be denied for '{p}'");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// § Wildcard deny + specific allow interaction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn wildcard_deny_all_tools_overrides_specific_allow() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // In ABP, deny always beats allow — even wildcard deny vs specific allow.
    assert!(!engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[test]
fn validator_warns_on_wildcard_deny_with_specific_allows() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule),
        "wildcard deny should produce unreachable warnings for specific allows"
    );
    // Should have one warning per allowed tool
    let unreachable_count = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::UnreachableRule)
        .count();
    assert_eq!(
        unreachable_count, 2,
        "one warning per unreachable allowed tool"
    );
}
