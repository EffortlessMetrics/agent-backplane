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
//! Tests for rate_limit, audit (AuditLog), and composed policy extensions.

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::audit::{AuditAction, AuditLog};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};

// =========================================================================
// Rate-limit tests
// =========================================================================

#[test]
fn rate_limit_unlimited_always_allows() {
    let policy = RateLimitPolicy::unlimited();
    assert!(policy.check_rate_limit(1000, 1_000_000, 500).is_allowed());
}

#[test]
fn rate_limit_rpm_throttle() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        ..Default::default()
    };
    let result = policy.check_rate_limit(60, 0, 0);
    assert!(result.is_throttled());
    match result {
        RateLimitResult::Throttled { retry_after_ms } => assert_eq!(retry_after_ms, 1000),
        _ => panic!("expected throttled"),
    }
}

#[test]
fn rate_limit_rpm_below_limit_allowed() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        ..Default::default()
    };
    assert!(policy.check_rate_limit(59, 0, 0).is_allowed());
}

#[test]
fn rate_limit_tpm_throttle() {
    let policy = RateLimitPolicy {
        max_tokens_per_minute: Some(10_000),
        ..Default::default()
    };
    let result = policy.check_rate_limit(0, 10_000, 0);
    assert!(result.is_throttled());
}

#[test]
fn rate_limit_tpm_below_limit_allowed() {
    let policy = RateLimitPolicy {
        max_tokens_per_minute: Some(10_000),
        ..Default::default()
    };
    assert!(policy.check_rate_limit(0, 9_999, 0).is_allowed());
}

#[test]
fn rate_limit_concurrent_denied() {
    let policy = RateLimitPolicy {
        max_concurrent: Some(5),
        ..Default::default()
    };
    let result = policy.check_rate_limit(0, 0, 5);
    assert!(result.is_denied());
    match result {
        RateLimitResult::Denied { reason } => {
            assert!(reason.contains("concurrent"));
        }
        _ => panic!("expected denied"),
    }
}

#[test]
fn rate_limit_concurrent_below_limit_allowed() {
    let policy = RateLimitPolicy {
        max_concurrent: Some(5),
        ..Default::default()
    };
    assert!(policy.check_rate_limit(0, 0, 4).is_allowed());
}

#[test]
fn rate_limit_concurrent_takes_precedence_over_rpm() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        max_concurrent: Some(2),
        ..Default::default()
    };
    // Both exceeded — concurrent should win (hard deny).
    let result = policy.check_rate_limit(10, 0, 2);
    assert!(result.is_denied());
}

#[test]
fn rate_limit_all_limits_set_within_bounds() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(100),
        max_tokens_per_minute: Some(50_000),
        max_concurrent: Some(10),
    };
    assert!(policy.check_rate_limit(50, 25_000, 5).is_allowed());
}

#[test]
fn rate_limit_default_is_unlimited() {
    let policy = RateLimitPolicy::default();
    assert!(policy.max_requests_per_minute.is_none());
    assert!(policy.max_tokens_per_minute.is_none());
    assert!(policy.max_concurrent.is_none());
}

#[test]
fn rate_limit_serialization_roundtrip() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(120),
        max_tokens_per_minute: Some(100_000),
        max_concurrent: Some(8),
    };
    let json = serde_json::to_string(&policy).unwrap();
    let deserialized: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.max_requests_per_minute, Some(120));
    assert_eq!(deserialized.max_tokens_per_minute, Some(100_000));
    assert_eq!(deserialized.max_concurrent, Some(8));
}

#[test]
fn rate_limit_result_serialization() {
    let allowed = RateLimitResult::Allowed;
    let json = serde_json::to_string(&allowed).unwrap();
    assert!(json.contains("allowed"));

    let throttled = RateLimitResult::Throttled {
        retry_after_ms: 500,
    };
    let json = serde_json::to_string(&throttled).unwrap();
    assert!(json.contains("throttled"));
    assert!(json.contains("500"));
}

// =========================================================================
// Audit log tests
// =========================================================================

#[test]
fn audit_log_starts_empty() {
    let log = AuditLog::new();
    assert!(log.is_empty());
    assert_eq!(log.len(), 0);
    assert_eq!(log.denied_count(), 0);
}

#[test]
fn audit_log_record_and_entries() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", Some("default"), None);
    log.record(
        AuditAction::ToolDenied,
        "Bash",
        Some("strict"),
        Some("disallowed"),
    );

    assert_eq!(log.len(), 2);
    assert_eq!(log.entries()[0].resource, "Read");
    assert_eq!(log.entries()[1].resource, "Bash");
}

#[test]
fn audit_log_denied_count() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    log.record(AuditAction::ToolDenied, "Bash", None, None);
    log.record(AuditAction::ReadDenied, ".env", None, None);
    log.record(AuditAction::WriteAllowed, "src/lib.rs", None, None);
    log.record(AuditAction::RateLimited, "api-call", None, None);

    assert_eq!(log.denied_count(), 3); // ToolDenied + ReadDenied + RateLimited
}

#[test]
fn audit_log_filter_by_action() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    log.record(AuditAction::ToolAllowed, "Grep", None, None);
    log.record(AuditAction::ToolDenied, "Bash", None, None);

    let allowed = log.filter_by_action(&AuditAction::ToolAllowed);
    assert_eq!(allowed.len(), 2);

    let denied = log.filter_by_action(&AuditAction::ToolDenied);
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].resource, "Bash");
}

#[test]
fn audit_log_filter_no_matches() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    assert!(log.filter_by_action(&AuditAction::WriteDenied).is_empty());
}

#[test]
fn audit_action_is_denied_variants() {
    assert!(!AuditAction::ToolAllowed.is_denied());
    assert!(AuditAction::ToolDenied.is_denied());
    assert!(!AuditAction::ReadAllowed.is_denied());
    assert!(AuditAction::ReadDenied.is_denied());
    assert!(!AuditAction::WriteAllowed.is_denied());
    assert!(AuditAction::WriteDenied.is_denied());
    assert!(AuditAction::RateLimited.is_denied());
}

#[test]
fn audit_log_records_policy_name_and_reason() {
    let mut log = AuditLog::new();
    log.record(
        AuditAction::ReadDenied,
        ".env",
        Some("security-policy"),
        Some("secrets not readable"),
    );
    let entry = &log.entries()[0];
    assert_eq!(entry.policy_name.as_deref(), Some("security-policy"));
    assert_eq!(entry.reason.as_deref(), Some("secrets not readable"));
}

#[test]
fn audit_log_timestamp_is_populated() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    assert!(!log.entries()[0].timestamp.is_empty());
}

#[test]
fn audit_log_serialization_roundtrip() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolDenied, "Bash", Some("p1"), Some("no"));
    let json = serde_json::to_string(&log).unwrap();
    let deserialized: AuditLog = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.len(), 1);
    assert_eq!(deserialized.entries()[0].resource, "Bash");
}

// =========================================================================
// Composed policy tests
// =========================================================================

fn permissive_engine() -> PolicyEngine {
    PolicyEngine::new(&PolicyProfile::default()).unwrap()
}

fn restrictive_engine() -> PolicyEngine {
    PolicyEngine::new(&PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/locked/**".into()],
        ..PolicyProfile::default()
    })
    .unwrap()
}

#[test]
fn composed_empty_allows_everything() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("Bash").is_allowed());
    assert!(cp.evaluate_read(".env").is_allowed());
    assert!(cp.evaluate_write("locked/x").is_allowed());
}

#[test]
fn composed_all_must_allow_single_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", permissive_engine());
    cp.add_policy("restrictive", restrictive_engine());

    let result = cp.evaluate_tool("Bash");
    assert!(result.is_denied());
    match result {
        ComposedResult::Denied { by, .. } => assert_eq!(by, "restrictive"),
        _ => panic!("expected denied"),
    }
}

#[test]
fn composed_all_must_allow_all_permit() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("a", permissive_engine());
    cp.add_policy("b", permissive_engine());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_any_must_allow_one_permits() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("restrictive", restrictive_engine());
    cp.add_policy("permissive", permissive_engine());

    // Bash is denied by restrictive but allowed by permissive → allowed
    let result = cp.evaluate_tool("Bash");
    assert!(result.is_allowed());
    match result {
        ComposedResult::Allowed { by } => assert_eq!(by, "permissive"),
        _ => panic!("expected allowed"),
    }
}

#[test]
fn composed_any_must_allow_all_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("r1", restrictive_engine());
    cp.add_policy("r2", restrictive_engine());

    let result = cp.evaluate_tool("Bash");
    assert!(result.is_denied());
}

#[test]
fn composed_first_match_uses_first_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy("permissive", permissive_engine());
    cp.add_policy("restrictive", restrictive_engine());

    // First engine allows Bash → allowed
    let result = cp.evaluate_tool("Bash");
    assert!(result.is_allowed());
    match result {
        ComposedResult::Allowed { by } => assert_eq!(by, "permissive"),
        _ => panic!("expected allowed"),
    }
}

#[test]
fn composed_first_match_first_denies() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy("restrictive", restrictive_engine());
    cp.add_policy("permissive", permissive_engine());

    let result = cp.evaluate_tool("Bash");
    assert!(result.is_denied());
}

#[test]
fn composed_evaluate_read() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("r", restrictive_engine());

    assert!(cp.evaluate_read(".env").is_denied());
    assert!(cp.evaluate_read("src/lib.rs").is_allowed());
}

#[test]
fn composed_evaluate_write() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("r", restrictive_engine());

    assert!(cp.evaluate_write("locked/data.txt").is_denied());
    assert!(cp.evaluate_write("src/lib.rs").is_allowed());
}

#[test]
fn composed_policy_count() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    cp.add_policy("a", permissive_engine());
    cp.add_policy("b", restrictive_engine());
    assert_eq!(cp.policy_count(), 2);
}

#[test]
fn composed_strategy_accessor() {
    let cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    assert_eq!(cp.strategy(), CompositionStrategy::AnyMustAllow);
}

#[test]
fn composed_result_serialization() {
    let allowed = ComposedResult::Allowed { by: "test".into() };
    let json = serde_json::to_string(&allowed).unwrap();
    assert!(json.contains("allowed"));

    let denied = ComposedResult::Denied {
        by: "strict".into(),
        reason: "nope".into(),
    };
    let json = serde_json::to_string(&denied).unwrap();
    assert!(json.contains("denied"));
}

#[test]
fn composition_strategy_default_is_all_must_allow() {
    assert_eq!(
        CompositionStrategy::default(),
        CompositionStrategy::AllMustAllow
    );
}
