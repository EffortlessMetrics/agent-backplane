#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::needless_update)]
#![allow(clippy::field_reassign_with_default)]
#![allow(unknown_lints)]
//! Deep exhaustive mutation-guard tests.
//!
//! Each test is designed so that a single mutation (flipping a boolean,
//! changing `<` to `<=`, swapping `+` and `-`, etc.) in the source
//! would cause exactly that test to fail.
//!
//! ## Categories
//! 1. Boundary guards (off-by-one, edge cases)
//! 2. Logic inversion guards (allow/deny flipped, Some/None)
//! 3. Arithmetic guards (score calculations, ratios, token math)
//! 4. String guards (case sensitivity, trim, prefix/suffix)
//! 5. Collection guards (empty, single, ordering, dedup, contains)

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use abp_core::aggregate::{EventAggregator, RunAnalytics};
use abp_core::chain::{ChainError, ReceiptChain};
use abp_core::ext::{AgentEventExt, ReceiptExt, WorkOrderExt};
use abp_core::filter::EventFilter;
use abp_core::ir::IrUsage;
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::{Decision, PolicyEngine};
use chrono::Utc;
use serde_json::json;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_tool_call(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    })
}

fn make_assistant_msg(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage { text: text.into() })
}

fn make_error(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.into(),
        error_code: None,
    })
}

fn baseline_receipt() -> Receipt {
    ReceiptBuilder::new("deep-test-backend").build()
}

fn policy_engine(profile: PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&profile).expect("compile policy")
}

fn hashed_receipt() -> Receipt {
    ReceiptBuilder::new("deep-test-backend")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. BOUNDARY GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

/// IrUsage::from_io must compute total_tokens = input + output exactly.
/// Mutation: changing `+` to `-` or off-by-one would break this.
#[test]
fn boundary_ir_usage_total_equals_sum() {
    let usage = IrUsage::from_io(100, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_ne!(usage.total_tokens, 149); // off-by-one guard
    assert_ne!(usage.total_tokens, 151);
}

/// IrUsage with zero input tokens must still compute correctly.
#[test]
fn boundary_ir_usage_zero_input() {
    let usage = IrUsage::from_io(0, 42);
    assert_eq!(usage.total_tokens, 42);
    assert_eq!(usage.input_tokens, 0);
}

/// IrUsage with zero output tokens must still compute correctly.
#[test]
fn boundary_ir_usage_zero_output() {
    let usage = IrUsage::from_io(42, 0);
    assert_eq!(usage.total_tokens, 42);
    assert_eq!(usage.output_tokens, 0);
}

/// IrUsage with both zero must yield zero total.
#[test]
fn boundary_ir_usage_both_zero() {
    let usage = IrUsage::from_io(0, 0);
    assert_eq!(usage.total_tokens, 0);
}

/// max_turns boundary: setting to 1 must be exactly 1, not 0 or 2.
#[test]
fn boundary_max_turns_exactly_one() {
    let wo = WorkOrderBuilder::new("task").max_turns(1).build();
    assert_eq!(wo.config.max_turns, Some(1));
    assert_ne!(wo.config.max_turns, Some(0));
    assert_ne!(wo.config.max_turns, Some(2));
    assert_ne!(wo.config.max_turns, None);
}

/// max_turns boundary: setting to 0 must be exactly 0.
#[test]
fn boundary_max_turns_zero() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    assert_eq!(wo.config.max_turns, Some(0));
}

/// task_summary at exact boundary should not truncate.
/// Mutation: `<=` changed to `<` would truncate at exactly max_len.
#[test]
fn boundary_task_summary_exact_length() {
    let wo = WorkOrderBuilder::new("hello").build();
    let summary = wo.task_summary(5);
    assert_eq!(summary, "hello");
    assert!(!summary.contains('…'));
}

/// task_summary one char over boundary must truncate.
#[test]
fn boundary_task_summary_one_over() {
    let wo = WorkOrderBuilder::new("hello!").build();
    let summary = wo.task_summary(5);
    assert!(summary.contains('…'));
    assert_ne!(summary, "hello!");
}

/// Empty task string edge case.
#[test]
fn boundary_empty_task_summary() {
    let wo = WorkOrderBuilder::new("").build();
    let summary = wo.task_summary(10);
    assert_eq!(summary, "");
}

/// Single-character task at boundary.
#[test]
fn boundary_single_char_task_summary() {
    let wo = WorkOrderBuilder::new("X").build();
    assert_eq!(wo.task_summary(1), "X");
    assert_eq!(wo.task_summary(0), "…"); // 0-length must truncate
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. LOGIC INVERSION GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

/// Deny rule must actually deny. Mutation: flipping the boolean would allow.
#[test]
fn logic_deny_tool_is_denied() {
    let engine = policy_engine(PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    });
    let d = engine.can_use_tool("Bash");
    assert_eq!(d.allowed, false);
    assert!(d.reason.is_some());
}

/// Allow rule must actually allow. Mutation: flipping boolean would deny.
#[test]
fn logic_allowed_tool_is_allowed() {
    let engine = policy_engine(PolicyProfile::default());
    let d = engine.can_use_tool("Read");
    assert_eq!(d.allowed, true);
    assert!(d.reason.is_none());
}

/// Deny-read path must deny. Inversion would allow.
#[test]
fn logic_deny_read_actually_denies() {
    let engine = policy_engine(PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..PolicyProfile::default()
    });
    let d = engine.can_read_path(Path::new("keys.secret"));
    assert_eq!(d.allowed, false);
}

/// Non-denied read path must allow.
#[test]
fn logic_non_denied_read_allows() {
    let engine = policy_engine(PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..PolicyProfile::default()
    });
    let d = engine.can_read_path(Path::new("readme.md"));
    assert_eq!(d.allowed, true);
}

/// Deny-write path must deny. Inversion would allow.
#[test]
fn logic_deny_write_actually_denies() {
    let engine = policy_engine(PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    });
    let d = engine.can_write_path(Path::new(".git/config"));
    assert_eq!(d.allowed, false);
}

/// is_success must be true only for Complete, not Partial or Failed.
#[test]
fn logic_is_success_only_complete() {
    let complete = ReceiptBuilder::new("b").outcome(Outcome::Complete).build();
    let partial = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    let failed = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(complete.is_success(), true);
    assert_eq!(partial.is_success(), false);
    assert_eq!(failed.is_success(), false);
}

/// is_failure must be true only for Failed, not Complete or Partial.
#[test]
fn logic_is_failure_only_failed() {
    let complete = ReceiptBuilder::new("b").outcome(Outcome::Complete).build();
    let partial = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    let failed = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(failed.is_failure(), true);
    assert_eq!(complete.is_failure(), false);
    assert_eq!(partial.is_failure(), false);
}

/// SupportLevel::Native satisfies MinSupport::Native. Swapping would break.
#[test]
fn logic_native_satisfies_native() {
    assert_eq!(SupportLevel::Native.satisfies(&MinSupport::Native), true);
}

/// SupportLevel::Emulated does NOT satisfy MinSupport::Native.
#[test]
fn logic_emulated_does_not_satisfy_native() {
    assert_eq!(SupportLevel::Emulated.satisfies(&MinSupport::Native), false);
}

/// SupportLevel::Unsupported does NOT satisfy MinSupport::Emulated.
#[test]
fn logic_unsupported_does_not_satisfy_emulated() {
    assert_eq!(
        SupportLevel::Unsupported.satisfies(&MinSupport::Emulated),
        false
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. ARITHMETIC GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

/// IrUsage::merge must sum all fields correctly (including total_tokens).
#[test]
fn arithmetic_ir_usage_merge_sums() {
    let a = IrUsage::from_io(100, 50); // total=150
    let b = IrUsage::from_io(200, 75); // total=275
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 125);
    assert_eq!(merged.total_tokens, 425); // 150 + 275
}

/// IrUsage::with_cache must include cache tokens but total = input + output only.
#[test]
fn arithmetic_ir_usage_with_cache_total() {
    let usage = IrUsage::with_cache(100, 50, 20, 10);
    assert_eq!(usage.total_tokens, 150); // NOT 180
    assert_eq!(usage.cache_read_tokens, 20);
    assert_eq!(usage.cache_write_tokens, 10);
}

/// duration_secs must divide by 1000.0 exactly.
/// Mutation: dividing by 100 or 10000 would break.
#[test]
fn arithmetic_duration_secs_conversion() {
    let mut r = baseline_receipt();
    r.meta.duration_ms = 1500;
    assert!((r.duration_secs() - 1.5).abs() < f64::EPSILON);
}

/// duration_secs for 0ms must be exactly 0.0.
#[test]
fn arithmetic_duration_secs_zero() {
    let mut r = baseline_receipt();
    r.meta.duration_ms = 0;
    assert!((r.duration_secs() - 0.0).abs() < f64::EPSILON);
}

/// duration_secs for 1ms must be exactly 0.001.
#[test]
fn arithmetic_duration_secs_one_ms() {
    let mut r = baseline_receipt();
    r.meta.duration_ms = 1;
    assert!((r.duration_secs() - 0.001).abs() < f64::EPSILON);
}

/// ReceiptChain success_rate with all Complete must be 1.0.
#[test]
fn arithmetic_chain_success_rate_all_complete() {
    let mut chain = ReceiptChain::new();
    for _ in 0..3 {
        chain.push(hashed_receipt()).unwrap();
    }
    assert!((chain.success_rate() - 1.0).abs() < f64::EPSILON);
}

/// ReceiptChain success_rate with all Failed must be 0.0.
#[test]
fn arithmetic_chain_success_rate_all_failed() {
    let mut chain = ReceiptChain::new();
    for _ in 0..3 {
        let r = ReceiptBuilder::new("b")
            .outcome(Outcome::Failed)
            .build()
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert!((chain.success_rate() - 0.0).abs() < f64::EPSILON);
}

/// ReceiptChain success_rate with empty chain must be 0.0.
#[test]
fn arithmetic_chain_success_rate_empty() {
    let chain = ReceiptChain::new();
    assert!((chain.success_rate() - 0.0).abs() < f64::EPSILON);
}

/// tool_usage_ratio with no events must be 0.0, not NaN or panic.
#[test]
fn arithmetic_tool_usage_ratio_no_events() {
    let analytics = RunAnalytics::from_events(&[]);
    assert!((analytics.tool_usage_ratio() - 0.0).abs() < f64::EPSILON);
}

/// tool_usage_ratio with 1 tool call out of 2 events must be 0.5.
#[test]
fn arithmetic_tool_usage_ratio_half() {
    let events = vec![make_tool_call("Read"), make_assistant_msg("done")];
    let analytics = RunAnalytics::from_events(&events);
    assert!((analytics.tool_usage_ratio() - 0.5).abs() < f64::EPSILON);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. STRING GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

/// CONTRACT_VERSION must be exactly "abp/v0.1" — case-sensitive.
#[test]
fn string_contract_version_exact() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    assert_ne!(CONTRACT_VERSION, "ABP/V0.1");
    assert_ne!(CONTRACT_VERSION, "abp/v0.2");
    assert_ne!(CONTRACT_VERSION, "abp/v0.1 ");
}

/// Outcome serialization must use exact snake_case strings.
#[test]
fn string_outcome_serde_case_sensitive() {
    let s = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(s, r#""complete""#);
    assert_ne!(s, r#""Complete""#);
    assert_ne!(s, r#""COMPLETE""#);
}

/// ExecutionMode serialization must use snake_case.
#[test]
fn string_execution_mode_serde_case() {
    let s = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(s, r#""passthrough""#);
    let s2 = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(s2, r#""mapped""#);
}

/// ExecutionLane serialization must use snake_case.
#[test]
fn string_execution_lane_serde_case() {
    let patch = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(patch, r#""patch_first""#);
    let ws = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(ws, r#""workspace_first""#);
}

/// SupportLevel variant names are case-sensitive in serialization.
#[test]
fn string_support_level_serde_case() {
    let s = serde_json::to_string(&SupportLevel::Native).unwrap();
    assert_eq!(s, r#""native""#);
    assert_ne!(s, r#""Native""#);
    let s2 = serde_json::to_string(&SupportLevel::Emulated).unwrap();
    assert_eq!(s2, r#""emulated""#);
}

/// Backend identity preserves exact string, no trimming.
#[test]
fn string_backend_id_no_trim() {
    let r = ReceiptBuilder::new(" mock ").build();
    assert_eq!(r.backend.id, " mock ");
    assert_ne!(r.backend.id, "mock");
}

/// sha256_hex output must be exactly 64 lowercase hex chars.
#[test]
fn string_sha256_hex_format() {
    let h = sha256_hex(b"test");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(h.chars().all(|c| !c.is_ascii_uppercase()));
}

/// Empty string hashes to a known SHA-256 value.
#[test]
fn string_sha256_empty_input() {
    let h = sha256_hex(b"");
    assert_eq!(h.len(), 64);
    // SHA-256 of empty input is a known constant
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// is_code_task must detect keywords case-insensitively.
#[test]
fn string_is_code_task_case_insensitive() {
    let wo1 = WorkOrderBuilder::new("Fix the bug").build();
    assert!(wo1.is_code_task());
    let wo2 = WorkOrderBuilder::new("FIX THE BUG").build();
    assert!(wo2.is_code_task());
    let wo3 = WorkOrderBuilder::new("write a poem").build();
    assert!(!wo3.is_code_task());
}

/// Empty task is not a code task.
#[test]
fn string_empty_task_not_code_task() {
    let wo = WorkOrderBuilder::new("").build();
    assert!(!wo.is_code_task());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. COLLECTION GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

/// Empty ReceiptChain must report len=0 and is_empty=true.
#[test]
fn collection_empty_chain() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.len(), 0);
    assert_eq!(chain.is_empty(), true);
    assert!(chain.last().is_none());
}

/// Single-item chain must report len=1 and is_empty=false.
#[test]
fn collection_single_item_chain() {
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt()).unwrap();
    assert_eq!(chain.len(), 1);
    assert_eq!(chain.is_empty(), false);
    assert!(chain.last().is_some());
}

/// Chain with multiple items must track count correctly.
#[test]
fn collection_chain_count_matches_pushes() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        chain.push(hashed_receipt()).unwrap();
    }
    assert_eq!(chain.len(), 5);
}

/// ReceiptChain.total_events sums trace lengths across all receipts.
#[test]
fn collection_chain_total_events() {
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("b")
        .add_trace_event(make_tool_call("Read"))
        .add_trace_event(make_assistant_msg("hi"))
        .build()
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .add_trace_event(make_tool_call("Write"))
        .build()
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.total_events(), 3);
}

/// find_by_backend must return only matching receipts.
#[test]
fn collection_find_by_backend_filters() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("alpha").build().with_hash().unwrap())
        .unwrap();
    chain
        .push(ReceiptBuilder::new("beta").build().with_hash().unwrap())
        .unwrap();
    chain
        .push(ReceiptBuilder::new("alpha").build().with_hash().unwrap())
        .unwrap();
    assert_eq!(chain.find_by_backend("alpha").len(), 2);
    assert_eq!(chain.find_by_backend("beta").len(), 1);
    assert_eq!(chain.find_by_backend("gamma").len(), 0);
}

/// EventAggregator unique_tool_count deduplicates.
#[test]
fn collection_unique_tool_count_dedup() {
    let mut agg = EventAggregator::new();
    agg.add(&make_tool_call("Read"));
    agg.add(&make_tool_call("Read"));
    agg.add(&make_tool_call("Write"));
    assert_eq!(agg.unique_tool_count(), 2); // not 3
}

/// EventAggregator tool_calls preserves order.
#[test]
fn collection_tool_calls_ordering() {
    let mut agg = EventAggregator::new();
    agg.add(&make_tool_call("Bash"));
    agg.add(&make_tool_call("Read"));
    agg.add(&make_tool_call("Write"));
    let names: Vec<&str> = agg.tool_calls();
    assert_eq!(names, vec!["Bash", "Read", "Write"]);
}

/// Empty aggregator must report zero for everything.
#[test]
fn collection_empty_aggregator() {
    let agg = EventAggregator::new();
    assert_eq!(agg.event_count(), 0);
    assert_eq!(agg.unique_tool_count(), 0);
    assert!(!agg.has_errors());
    assert_eq!(agg.text_length(), 0);
    assert!(agg.first_timestamp().is_none());
    assert!(agg.last_timestamp().is_none());
    assert!(agg.duration_ms().is_none());
    let summary = agg.summary();
    assert_eq!(summary.total_events, 0);
    assert_eq!(summary.tool_calls, 0);
    assert_eq!(summary.errors, 0);
}

/// CapabilityManifest (BTreeMap) preserves ordering by key.
#[test]
fn collection_capability_manifest_ordered() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    let keys: Vec<&Capability> = manifest.keys().collect();
    // BTreeMap sorts by Ord; verify at least that order is consistent
    let keys2: Vec<&Capability> = manifest.keys().collect();
    assert_eq!(keys, keys2);
    assert_eq!(manifest.len(), 3);
    assert!(manifest.contains_key(&Capability::Streaming));
    assert!(!manifest.contains_key(&Capability::ToolBash));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. ADDITIONAL CROSS-CUTTING GUARDS (bonus tests beyond 50)
// ═══════════════════════════════════════════════════════════════════════════════

/// Glob: empty include/exclude allows everything.
#[test]
fn glob_empty_rules_allow_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("any/file.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

/// Glob: exclude overrides include.
#[test]
fn glob_exclude_overrides_include() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

/// Glob: missing include denies non-matching.
#[test]
fn glob_include_denies_non_matching() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(
        g.decide_str("tests/foo.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

/// MatchDecision::is_allowed only for Allowed variant.
#[test]
fn glob_is_allowed_only_for_allowed() {
    assert_eq!(MatchDecision::Allowed.is_allowed(), true);
    assert_eq!(MatchDecision::DeniedByExclude.is_allowed(), false);
    assert_eq!(MatchDecision::DeniedByMissingInclude.is_allowed(), false);
}

/// EventFilter include must not match excluded kinds.
#[test]
fn filter_include_rejects_unlisted() {
    let filter = EventFilter::include_kinds(&["error"]);
    let msg = make_assistant_msg("hi");
    assert!(!filter.matches(&msg));
    let err = make_error("boom");
    assert!(filter.matches(&err));
}

/// EventFilter exclude must pass non-excluded kinds.
#[test]
fn filter_exclude_passes_non_excluded() {
    let filter = EventFilter::exclude_kinds(&["assistant_delta"]);
    let msg = make_assistant_msg("hi");
    assert!(filter.matches(&msg));
    let delta = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
    assert!(!filter.matches(&delta));
}

/// AgentEventExt: is_tool_call only for ToolCall variant.
#[test]
fn ext_is_tool_call_correct_variant() {
    let tc = make_tool_call("Read");
    assert!(tc.is_tool_call());
    let msg = make_assistant_msg("hi");
    assert!(!msg.is_tool_call());
}

/// AgentEventExt: is_terminal only for RunCompleted.
#[test]
fn ext_is_terminal_correct_variant() {
    let completed = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(completed.is_terminal());
    let started = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(!started.is_terminal());
}

/// AgentEventExt: text_content extracts from message/delta, None for others.
#[test]
fn ext_text_content_some_and_none() {
    let msg = make_assistant_msg("hello");
    assert_eq!(msg.text_content(), Some("hello"));
    let delta = make_event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    assert_eq!(delta.text_content(), Some("chunk"));
    let tc = make_tool_call("Read");
    assert_eq!(tc.text_content(), None);
}

/// has_errors must detect Error events in trace.
#[test]
fn ext_has_errors_true_when_present() {
    let r = ReceiptBuilder::new("b")
        .add_trace_event(make_error("boom"))
        .build();
    assert!(r.has_errors());
}

/// has_errors must be false when no Error events exist.
#[test]
fn ext_has_errors_false_when_absent() {
    let r = ReceiptBuilder::new("b")
        .add_trace_event(make_assistant_msg("ok"))
        .build();
    assert!(!r.has_errors());
}

/// ReceiptChain: duplicate run_id is rejected.
#[test]
fn chain_rejects_duplicate_run_id() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt();
    let r2 = r.clone();
    chain.push(r).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

/// ReceiptChain: verify on empty chain returns EmptyChain error.
#[test]
fn chain_verify_empty_is_error() {
    let chain = ReceiptChain::new();
    let err = chain.verify().unwrap_err();
    assert!(matches!(err, ChainError::EmptyChain));
}

/// Receipt with_hash produces stable hash for same content.
#[test]
fn hash_stability_same_content() {
    let r1 = ReceiptBuilder::new("stable-test").build();
    let r2 = r1.clone();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

/// canonical_json produces deterministic key ordering.
#[test]
fn canonical_json_key_order() {
    let j1 = canonical_json(&json!({"z": 1, "a": 2})).unwrap();
    let j2 = canonical_json(&json!({"a": 2, "z": 1})).unwrap();
    assert_eq!(j1, j2);
    assert!(j1.starts_with(r#"{"a":2"#));
}

/// MinSupport::Any must accept all SupportLevel variants.
#[test]
fn logic_any_min_support_accepts_all() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Any));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Any));
    assert!(SupportLevel::Unsupported.satisfies(&MinSupport::Any));
    assert!(SupportLevel::Restricted {
        reason: "test".into()
    }
    .satisfies(&MinSupport::Any));
}

/// WorkOrderBuilder default lane is PatchFirst.
#[test]
fn builder_default_lane_is_patch_first() {
    let wo = WorkOrderBuilder::new("test").build();
    assert_eq!(wo.lane, ExecutionLane::PatchFirst);
    assert_ne!(wo.lane, ExecutionLane::WorkspaceFirst);
}

/// ExecutionMode default is Mapped.
#[test]
fn builder_default_mode_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    assert_ne!(ExecutionMode::default(), ExecutionMode::Passthrough);
}

/// ReceiptBuilder default outcome is Complete.
#[test]
fn builder_default_outcome_is_complete() {
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.outcome, Outcome::Complete);
}

/// aggregation text_length sums across multiple events.
#[test]
fn aggregator_text_length_sums() {
    let mut agg = EventAggregator::new();
    agg.add(&make_assistant_msg("abc")); // 3
    agg.add(&make_assistant_msg("de")); // 2
    agg.add(&make_tool_call("Read")); // 0
    assert_eq!(agg.text_length(), 5);
}

/// average_text_per_event divides correctly.
#[test]
fn arithmetic_average_text_per_event() {
    let events = vec![
        make_assistant_msg("abcdef"), // 6 chars
        make_tool_call("Read"),       // 0 chars
    ];
    let analytics = RunAnalytics::from_events(&events);
    assert!((analytics.average_text_per_event() - 3.0).abs() < f64::EPSILON);
}

/// average_text_per_event returns 0.0 for empty events.
#[test]
fn arithmetic_average_text_per_event_empty() {
    let analytics = RunAnalytics::from_events(&[]);
    assert!((analytics.average_text_per_event() - 0.0).abs() < f64::EPSILON);
}
