// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-policy engine — PolicyProfile construction,
//! PolicyEngine compilation, tool/read/write checks, rate limiting, composed
//! policies, audit logging, edge cases, and serde roundtrips.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::audit::{AuditAction, AuditLog, PolicyAuditor, PolicyDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine(p: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(p).expect("compile policy")
}

fn s(v: &str) -> String {
    v.to_string()
}

// ===========================================================================
// 1. PolicyProfile construction
// ===========================================================================

#[test]
fn profile_default_has_empty_fields() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn profile_with_tools_allow_deny() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    assert_eq!(p.allowed_tools.len(), 2);
    assert_eq!(p.disallowed_tools.len(), 1);
}

#[test]
fn profile_with_read_write_patterns() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.env"), s("**/secrets/**")],
        deny_write: vec![s("**/.git/**"), s("**/node_modules/**")],
        ..PolicyProfile::default()
    };
    assert_eq!(p.deny_read.len(), 2);
    assert_eq!(p.deny_write.len(), 2);
}

// ===========================================================================
// 2. PolicyEngine compilation
// ===========================================================================

#[test]
fn engine_compiles_from_default_profile() {
    let _e = engine(&PolicyProfile::default());
}

#[test]
fn engine_compiles_with_complex_globs() {
    let p = PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash*"), s("Shell*")],
        deny_read: vec![s("**/.env"), s("**/.env.*"), s("**/id_rsa")],
        deny_write: vec![s("**/.git/**"), s("**/locked/**")],
        ..PolicyProfile::default()
    };
    let _e = engine(&p);
}

#[test]
fn engine_rejects_invalid_glob() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("[invalid")],
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

// ===========================================================================
// 3. Tool allow/deny checks
// ===========================================================================

#[test]
fn tool_deny_beats_allow_wildcard() {
    let p = PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_allowlist_blocks_unlisted() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_glob_pattern_matching() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bash*"), s("Shell*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(!e.can_use_tool("ShellCmd").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
}

#[test]
fn tool_empty_allowlist_permits_all() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Anything").allowed);
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn tool_deny_reason_contains_tool_name() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Dangerous")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_use_tool("Dangerous");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("Dangerous"));
}

#[test]
fn tool_missing_allowlist_reason() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_use_tool("Write");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("not in allowlist"));
}

// ===========================================================================
// 4. Read path checks
// ===========================================================================

#[test]
fn read_deny_blocks_matching_paths() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.env"), s("**/.env.*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_deny_with_double_star() {
    let p = PolicyProfile {
        deny_read: vec![s("**/secrets/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secrets/key.pem")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/secrets/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_no_deny_allows_everything() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/path/here.txt")).allowed);
    assert!(e.can_read_path(Path::new(".git/config")).allowed);
}

// ===========================================================================
// 5. Write path checks
// ===========================================================================

#[test]
fn write_deny_blocks_git_directory() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/HEAD")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_multiple_patterns() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**"), s("**/node_modules/**"), s("*.lock")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_deny_deeply_nested() {
    let p = PolicyProfile {
        deny_write: vec![s("locked/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("locked/a/b/c/d.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn write_deny_reason_contains_path() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_write_path(Path::new(".git/config"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

// ===========================================================================
// 6. Rate limiting
// ===========================================================================

#[test]
fn rate_limit_unlimited_allows_all() {
    let rl = RateLimitPolicy::unlimited();
    assert!(rl.check_rate_limit(1000, 1_000_000, 100).is_allowed());
}

#[test]
fn rate_limit_default_is_unlimited() {
    let rl = RateLimitPolicy::default();
    assert!(rl.max_requests_per_minute.is_none());
    assert!(rl.max_tokens_per_minute.is_none());
    assert!(rl.max_concurrent.is_none());
}

#[test]
fn rate_limit_rpm_throttles() {
    let rl = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        ..RateLimitPolicy::default()
    };
    assert!(rl.check_rate_limit(5, 0, 0).is_allowed());
    let result = rl.check_rate_limit(10, 0, 0);
    assert!(result.is_throttled());
    if let RateLimitResult::Throttled { retry_after_ms } = result {
        assert!(retry_after_ms > 0);
    }
}

#[test]
fn rate_limit_tpm_throttles() {
    let rl = RateLimitPolicy {
        max_tokens_per_minute: Some(1000),
        ..RateLimitPolicy::default()
    };
    assert!(rl.check_rate_limit(0, 500, 0).is_allowed());
    assert!(rl.check_rate_limit(0, 1000, 0).is_throttled());
}

#[test]
fn rate_limit_concurrent_denies() {
    let rl = RateLimitPolicy {
        max_concurrent: Some(5),
        ..RateLimitPolicy::default()
    };
    assert!(rl.check_rate_limit(0, 0, 3).is_allowed());
    let result = rl.check_rate_limit(0, 0, 5);
    assert!(result.is_denied());
    if let RateLimitResult::Denied { reason } = result {
        assert!(reason.contains("concurrent"));
    }
}

#[test]
fn rate_limit_concurrent_takes_precedence_over_rpm() {
    let rl = RateLimitPolicy {
        max_requests_per_minute: Some(100),
        max_concurrent: Some(2),
        ..RateLimitPolicy::default()
    };
    // Both exceeded — concurrent check runs first and produces Denied
    let result = rl.check_rate_limit(200, 0, 5);
    assert!(result.is_denied());
}

#[test]
fn rate_limit_rpm_zero_gives_60s_retry() {
    let rl = RateLimitPolicy {
        max_requests_per_minute: Some(0),
        ..RateLimitPolicy::default()
    };
    let result = rl.check_rate_limit(0, 0, 0);
    if let RateLimitResult::Throttled { retry_after_ms } = result {
        assert_eq!(retry_after_ms, 60_000);
    } else {
        panic!("expected throttled");
    }
}

#[test]
fn rate_limit_tpm_zero_gives_60s_retry() {
    let rl = RateLimitPolicy {
        max_tokens_per_minute: Some(0),
        ..RateLimitPolicy::default()
    };
    let result = rl.check_rate_limit(0, 0, 0);
    if let RateLimitResult::Throttled { retry_after_ms } = result {
        assert_eq!(retry_after_ms, 60_000);
    } else {
        panic!("expected throttled");
    }
}

// ===========================================================================
// 7. Composed policies (ComposedPolicy — composed.rs)
// ===========================================================================

fn make_permissive_engine() -> PolicyEngine {
    engine(&PolicyProfile::default())
}

fn make_restrictive_engine() -> PolicyEngine {
    engine(&PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    })
}

#[test]
fn composed_all_must_allow_denies_on_single_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", make_permissive_engine());
    cp.add_policy("restrictive", make_restrictive_engine());

    let result = cp.evaluate_tool("Bash");
    assert!(result.is_denied());
}

#[test]
fn composed_all_must_allow_allows_when_all_agree() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("a", make_permissive_engine());
    cp.add_policy("b", make_permissive_engine());

    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_any_must_allow_permits_on_single_allow() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("restrictive", make_restrictive_engine());
    cp.add_policy("permissive", make_permissive_engine());

    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_any_must_allow_denies_when_all_deny() {
    let restrictive = engine(&PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..PolicyProfile::default()
    });
    let also_restrictive = engine(&PolicyProfile {
        allowed_tools: vec![s("Grep")],
        ..PolicyProfile::default()
    });
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("a", restrictive);
    cp.add_policy("b", also_restrictive);

    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_first_match_uses_first_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy("restrictive", make_restrictive_engine());
    cp.add_policy("permissive", make_permissive_engine());

    // First engine denies Bash
    assert!(cp.evaluate_tool("Bash").is_denied());
    // First engine allows Read
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_empty_returns_allowed() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("Anything").is_allowed());
}

#[test]
fn composed_policy_count() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    cp.add_policy("a", make_permissive_engine());
    assert_eq!(cp.policy_count(), 1);
    cp.add_policy("b", make_restrictive_engine());
    assert_eq!(cp.policy_count(), 2);
}

#[test]
fn composed_evaluate_read() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("restrictive", make_restrictive_engine());
    // restrictive doesn't block reads, so this should be allowed
    assert!(cp.evaluate_read("src/main.rs").is_allowed());
}

#[test]
fn composed_evaluate_write_denied() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("restrictive", make_restrictive_engine());
    assert!(cp.evaluate_write(".git/config").is_denied());
}

#[test]
fn composed_result_attributes_deny_to_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("strict-policy", make_restrictive_engine());

    if let ComposedResult::Denied { by, .. } = cp.evaluate_tool("Bash") {
        assert_eq!(by, "strict-policy");
    } else {
        panic!("expected denial");
    }
}

// ===========================================================================
// 7b. ComposedEngine (compose.rs) with PolicyPrecedence
// ===========================================================================

#[test]
fn composed_engine_deny_overrides() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_allow_overrides() {
    let restrictive = PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile::default();
    let ce = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();
    // permissive allows Bash, so AllowOverrides means Bash is allowed
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_first_applicable() {
    let restrictive = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile::default();
    let ce = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();
    // First engine denies Bash
    assert!(ce.check_tool("Bash").is_deny());
    // First engine allows Read
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_empty_returns_abstain() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Anything").is_abstain());
}

#[test]
fn composed_engine_read_write() {
    let p = PolicyProfile {
        deny_read: vec![s("**/secret*")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(vec![p], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read("secret.txt").is_deny());
    assert!(ce.check_read("src/main.rs").is_allow());
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/main.rs").is_allow());
}

// ===========================================================================
// 7c. PolicySet merge
// ===========================================================================

#[test]
fn policy_set_merge_unions_deny_lists() {
    let mut set = PolicySet::new("merged");
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Shell")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Shell")));
    assert!(merged.deny_read.contains(&s("**/.env")));
    assert!(merged.deny_write.contains(&s("**/.git/**")));
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(
        merged
            .disallowed_tools
            .iter()
            .filter(|t| *t == "Bash")
            .count(),
        1
    );
}

#[test]
fn policy_set_name() {
    let set = PolicySet::new("test-set");
    assert_eq!(set.name(), "test-set");
}

// ===========================================================================
// 7d. PolicyValidator
// ===========================================================================

#[test]
fn validator_detects_empty_globs() {
    let p = PolicyProfile {
        allowed_tools: vec![s("")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_overlapping_allow_deny() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Bash")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
    );
}

#[test]
fn validator_detects_unreachable_rules() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![s("*")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule)
    );
}

#[test]
fn validator_detects_catch_all_deny_read() {
    let p = PolicyProfile {
        deny_read: vec![s("**")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule)
    );
}

#[test]
fn validator_clean_profile_has_no_warnings() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

// ===========================================================================
// 8. Audit logging
// ===========================================================================

#[test]
fn auditor_records_tool_allow() {
    let e = engine(&PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(e);
    let decision = auditor.check_tool("Read");
    assert_eq!(decision, PolicyDecision::Allow);
    assert_eq!(auditor.entries().len(), 1);
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 0);
}

#[test]
fn auditor_records_tool_deny() {
    let e = engine(&PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    let decision = auditor.check_tool("Bash");
    assert!(matches!(decision, PolicyDecision::Deny { .. }));
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_read_and_write() {
    let e = engine(&PolicyProfile {
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_read(".env");
    auditor.check_read("src/main.rs");
    auditor.check_write(".git/config");
    auditor.check_write("src/lib.rs");

    assert_eq!(auditor.entries().len(), 4);
    assert_eq!(auditor.denied_count(), 2);
    assert_eq!(auditor.allowed_count(), 2);
}

#[test]
fn auditor_summary_counts() {
    let e = engine(&PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_tool("Grep");

    let summary = auditor.summary();
    assert_eq!(summary.allowed, 2);
    assert_eq!(summary.denied, 1);
    assert_eq!(summary.warned, 0);
}

#[test]
fn auditor_entries_have_correct_action_field() {
    let e = engine(&PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_read("file.txt");
    auditor.check_write("file.txt");

    assert_eq!(auditor.entries()[0].action, "tool");
    assert_eq!(auditor.entries()[1].action, "read");
    assert_eq!(auditor.entries()[2].action, "write");
}

// ---------------------------------------------------------------------------
// AuditLog (extended audit trail)
// ---------------------------------------------------------------------------

#[test]
fn audit_log_record_and_query() {
    let mut log = AuditLog::new();
    assert!(log.is_empty());

    log.record(AuditAction::ToolAllowed, "Read", Some("default"), None);
    log.record(
        AuditAction::ToolDenied,
        "Bash",
        Some("strict"),
        Some("disallowed"),
    );
    log.record(AuditAction::ReadAllowed, "src/main.rs", None, None);

    assert_eq!(log.len(), 3);
    assert!(!log.is_empty());
    assert_eq!(log.denied_count(), 1);
}

#[test]
fn audit_log_filter_by_action() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    log.record(AuditAction::ToolDenied, "Bash", None, None);
    log.record(AuditAction::ToolAllowed, "Grep", None, None);

    let allowed = log.filter_by_action(&AuditAction::ToolAllowed);
    assert_eq!(allowed.len(), 2);
    let denied = log.filter_by_action(&AuditAction::ToolDenied);
    assert_eq!(denied.len(), 1);
}

#[test]
fn audit_action_is_denied_variants() {
    assert!(AuditAction::ToolDenied.is_denied());
    assert!(AuditAction::ReadDenied.is_denied());
    assert!(AuditAction::WriteDenied.is_denied());
    assert!(AuditAction::RateLimited.is_denied());
    assert!(!AuditAction::ToolAllowed.is_denied());
    assert!(!AuditAction::ReadAllowed.is_denied());
    assert!(!AuditAction::WriteAllowed.is_denied());
}

// ===========================================================================
// 8b. RuleEngine (rules.rs)
// ===========================================================================

#[test]
fn rule_engine_no_rules_allows() {
    let re = RuleEngine::new();
    assert_eq!(re.evaluate("anything"), RuleEffect::Allow);
    assert_eq!(re.rule_count(), 0);
}

#[test]
fn rule_engine_deny_rule_matches() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash*")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(re.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_higher_priority_wins() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: s("deny-all"),
        description: s("Deny all"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 1,
    });
    re.add_rule(Rule {
        id: s("allow-all"),
        description: s("Allow all"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 10,
    });
    assert_eq!(re.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_evaluate_all() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: s("r1"),
        description: s("Rule 1"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    re.add_rule(Rule {
        id: s("r2"),
        description: s("Rule 2"),
        condition: RuleCondition::Never,
        effect: RuleEffect::Deny,
        priority: 2,
    });
    let evals = re.evaluate_all("test");
    assert_eq!(evals.len(), 2);
    assert!(evals[0].matched);
    assert!(!evals[1].matched);
}

#[test]
fn rule_engine_remove_rule() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: s("r1"),
        description: s("Rule 1"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 1,
    });
    assert_eq!(re.rule_count(), 1);
    re.remove_rule("r1");
    assert_eq!(re.rule_count(), 0);
}

#[test]
fn rule_condition_and_or_not() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Pattern(s("Bash*")),
        RuleCondition::Not(Box::new(RuleCondition::Pattern(s("BashSafe")))),
    ]);
    assert!(cond.matches("BashExec"));
    assert!(!cond.matches("BashSafe"));
    assert!(!cond.matches("Read"));

    let or_cond = RuleCondition::Or(vec![
        RuleCondition::Pattern(s("Read")),
        RuleCondition::Pattern(s("Grep")),
    ]);
    assert!(or_cond.matches("Read"));
    assert!(or_cond.matches("Grep"));
    assert!(!or_cond.matches("Write"));
}

#[test]
fn rule_condition_always_never() {
    assert!(RuleCondition::Always.matches("anything"));
    assert!(!RuleCondition::Never.matches("anything"));
}

#[test]
fn rule_effect_throttle() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: s("throttle-api"),
        description: s("Throttle API"),
        condition: RuleCondition::Pattern(s("api*")),
        effect: RuleEffect::Throttle { max: 100 },
        priority: 5,
    });
    assert_eq!(re.evaluate("api_call"), RuleEffect::Throttle { max: 100 });
    assert_eq!(re.evaluate("read_file"), RuleEffect::Allow);
}

// ===========================================================================
// 9. Edge cases
// ===========================================================================

#[test]
fn edge_empty_tool_name() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn edge_empty_path() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("")).allowed);
    assert!(e.can_write_path(Path::new("")).allowed);
}

#[test]
fn edge_wildcard_only_deny_tools() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("").allowed);
}

#[test]
fn edge_deny_and_allow_same_tool() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Bash")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    // Deny always wins
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn edge_path_traversal() {
    let p = PolicyProfile {
        deny_read: vec![s("**/etc/passwd")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("../../etc/passwd")).allowed);
    assert!(!e.can_write_path(Path::new("../.git/config")).allowed);
}

#[test]
fn edge_unicode_tool_name() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("outil*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("outil_spécial").allowed);
    assert!(e.can_use_tool("normal").allowed);
}

#[test]
fn edge_unicode_path() {
    let p = PolicyProfile {
        deny_read: vec![s("données/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("données/fichier.txt")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn edge_question_mark_glob() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bas?")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Bass").allowed);
    assert!(e.can_use_tool("Ba").allowed);
    assert!(e.can_use_tool("Basher").allowed);
}

#[test]
fn edge_brace_expansion_glob() {
    let p = PolicyProfile {
        deny_write: vec![s("*.{lock,bak}")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("data.bak")).allowed);
    assert!(e.can_write_path(Path::new("main.rs")).allowed);
}

// ===========================================================================
// 10. Serde roundtrip
// ===========================================================================

#[test]
fn serde_policy_profile_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.com")],
        require_approval_for: vec![s("DeleteFile")],
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(p.allowed_tools, p2.allowed_tools);
    assert_eq!(p.disallowed_tools, p2.disallowed_tools);
    assert_eq!(p.deny_read, p2.deny_read);
    assert_eq!(p.deny_write, p2.deny_write);
    assert_eq!(p.allow_network, p2.allow_network);
    assert_eq!(p.deny_network, p2.deny_network);
    assert_eq!(p.require_approval_for, p2.require_approval_for);
}

#[test]
fn serde_rate_limit_policy_roundtrip() {
    let rl = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        max_tokens_per_minute: Some(10_000),
        max_concurrent: Some(5),
    };
    let json = serde_json::to_string(&rl).unwrap();
    let rl2: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(rl.max_requests_per_minute, rl2.max_requests_per_minute);
    assert_eq!(rl.max_tokens_per_minute, rl2.max_tokens_per_minute);
    assert_eq!(rl.max_concurrent, rl2.max_concurrent);
}

#[test]
fn serde_rate_limit_result_roundtrip() {
    let cases = vec![
        RateLimitResult::Allowed,
        RateLimitResult::Throttled {
            retry_after_ms: 5000,
        },
        RateLimitResult::Denied {
            reason: s("too many"),
        },
    ];
    for r in &cases {
        let json = serde_json::to_string(r).unwrap();
        let r2: RateLimitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(*r, r2);
    }
}

#[test]
fn serde_composition_strategy_roundtrip() {
    let strategies = vec![
        CompositionStrategy::AllMustAllow,
        CompositionStrategy::AnyMustAllow,
        CompositionStrategy::FirstMatch,
    ];
    for st in &strategies {
        let json = serde_json::to_string(st).unwrap();
        let st2: CompositionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*st, st2);
    }
}

#[test]
fn serde_composed_result_roundtrip() {
    let cases = vec![
        ComposedResult::Allowed { by: s("engine-a") },
        ComposedResult::Denied {
            by: s("engine-b"),
            reason: s("not permitted"),
        },
    ];
    for c in &cases {
        let json = serde_json::to_string(c).unwrap();
        let c2: ComposedResult = serde_json::from_str(&json).unwrap();
        assert_eq!(*c, c2);
    }
}

#[test]
fn serde_audit_action_roundtrip() {
    let actions = vec![
        AuditAction::ToolAllowed,
        AuditAction::ToolDenied,
        AuditAction::ReadAllowed,
        AuditAction::ReadDenied,
        AuditAction::WriteAllowed,
        AuditAction::WriteDenied,
        AuditAction::RateLimited,
    ];
    for a in &actions {
        let json = serde_json::to_string(a).unwrap();
        let a2: AuditAction = serde_json::from_str(&json).unwrap();
        assert_eq!(*a, a2);
    }
}

#[test]
fn serde_rule_condition_roundtrip() {
    let conditions = vec![
        RuleCondition::Always,
        RuleCondition::Never,
        RuleCondition::Pattern(s("Bash*")),
        RuleCondition::And(vec![RuleCondition::Always, RuleCondition::Never]),
        RuleCondition::Or(vec![
            RuleCondition::Pattern(s("A")),
            RuleCondition::Pattern(s("B")),
        ]),
        RuleCondition::Not(Box::new(RuleCondition::Always)),
    ];
    for c in &conditions {
        let json = serde_json::to_string(c).unwrap();
        let c2: RuleCondition = serde_json::from_str(&json).unwrap();
        // Verify roundtrip by re-serializing
        let json2 = serde_json::to_string(&c2).unwrap();
        assert_eq!(json, json2);
    }
}

#[test]
fn serde_rule_effect_roundtrip() {
    let effects = vec![
        RuleEffect::Allow,
        RuleEffect::Deny,
        RuleEffect::Log,
        RuleEffect::Throttle { max: 42 },
    ];
    for e in &effects {
        let json = serde_json::to_string(e).unwrap();
        let e2: RuleEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(*e, e2);
    }
}

#[test]
fn serde_policy_precedence_roundtrip() {
    let precs = vec![
        PolicyPrecedence::DenyOverrides,
        PolicyPrecedence::AllowOverrides,
        PolicyPrecedence::FirstApplicable,
    ];
    for p in &precs {
        let json = serde_json::to_string(p).unwrap();
        let p2: PolicyPrecedence = serde_json::from_str(&json).unwrap();
        assert_eq!(*p, p2);
    }
}

#[test]
fn serde_audit_log_roundtrip() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", Some("default"), None);
    log.record(
        AuditAction::WriteDenied,
        ".git/config",
        Some("strict"),
        Some("git protected"),
    );

    let json = serde_json::to_string(&log).unwrap();
    let log2: AuditLog = serde_json::from_str(&json).unwrap();
    assert_eq!(log.len(), log2.len());
    assert_eq!(log.denied_count(), log2.denied_count());
}

#[test]
fn serde_decision_roundtrip() {
    let d = abp_policy::Decision::allow();
    let json = serde_json::to_string(&d).unwrap();
    let d2: abp_policy::Decision = serde_json::from_str(&json).unwrap();
    assert!(d2.allowed);

    let d = abp_policy::Decision::deny("reason");
    let json = serde_json::to_string(&d).unwrap();
    let d2: abp_policy::Decision = serde_json::from_str(&json).unwrap();
    assert!(!d2.allowed);
    assert_eq!(d2.reason.as_deref(), Some("reason"));
}
