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
//! Deep end-to-end tests for the policy engine covering all policy composition
//! patterns, glob matching edge cases, validation, auditing, rule engine, and
//! integration with work orders.

use std::path::Path;

use abp_core::{PolicyProfile, WorkOrderBuilder};
use abp_policy::audit::{AuditAction, AuditLog, AuditSummary, PolicyAuditor};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};
use abp_policy::rate_limit::RateLimitPolicy;
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// ===================================================================
// Helpers
// ===================================================================

fn policy(
    allowed: &[&str],
    disallowed: &[&str],
    deny_read: &[&str],
    deny_write: &[&str],
) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: allowed.iter().map(|s| s.to_string()).collect(),
        disallowed_tools: disallowed.iter().map(|s| s.to_string()).collect(),
        deny_read: deny_read.iter().map(|s| s.to_string()).collect(),
        deny_write: deny_write.iter().map(|s| s.to_string()).collect(),
        ..PolicyProfile::default()
    }
}

fn engine(p: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(p).expect("compile policy")
}

// ===================================================================
// 1. Single policy evaluation (15+ tests)
// ===================================================================

#[test]
fn single_allow_specific_tool_by_name() {
    let e = engine(&policy(&["Read"], &[], &[], &[]));
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn single_allow_multiple_tools_by_name() {
    let e = engine(&policy(&["Read", "Write", "Grep"], &[], &[], &[]));
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
}

#[test]
fn single_deny_specific_tool_by_name() {
    let e = engine(&policy(&[], &["Bash"], &[], &[]));
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("disallowed"));
}

#[test]
fn single_deny_tools_by_glob_pattern() {
    let e = engine(&policy(&[], &["Bash*"], &[], &[]));
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn single_allow_read_path_with_glob() {
    // No deny_read → everything is readable
    let e = engine(&policy(&[], &[], &[], &[]));
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("deep/nested/path.txt")).allowed);
}

#[test]
fn single_deny_read_path_with_glob() {
    let e = engine(&policy(&[], &[], &["**/.env*"], &[]));
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env.local")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn single_deny_write_path_with_glob() {
    let e = engine(&policy(&[], &[], &[], &["**/.git/**"]));
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn single_mixed_allow_deny_tool_rules() {
    let e = engine(&policy(&["*"], &["Bash"], &[], &[]));
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn single_default_allow_when_no_rules() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("AnyTool").allowed);
    assert!(e.can_read_path(Path::new("any/path")).allowed);
    assert!(e.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn single_default_deny_when_allowlist_present() {
    let e = engine(&policy(&["Read"], &[], &[], &[]));
    let d = e.can_use_tool("Write");
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("not in allowlist"));
}

#[test]
fn single_deny_beats_allow_for_same_tool() {
    let e = engine(&policy(&["Bash"], &["Bash"], &[], &[]));
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn single_deny_read_multiple_patterns() {
    let e = engine(&policy(
        &[],
        &[],
        &["**/.env", "**/.env.*", "**/id_rsa", "**/id_ed25519"],
        &[],
    ));
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env.local")).allowed);
    assert!(!e.can_read_path(Path::new(".ssh/id_rsa")).allowed);
    assert!(!e.can_read_path(Path::new(".ssh/id_ed25519")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn single_deny_write_multiple_patterns() {
    let e = engine(&policy(
        &[],
        &[],
        &[],
        &["**/.git/**", "**/node_modules/**", "target/**"],
    ));
    assert!(!e.can_write_path(Path::new(".git/objects/ab")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/foo/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("target/debug/build")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn single_deny_reason_contains_path() {
    let e = engine(&policy(&[], &[], &["secret*"], &[]));
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(!d.allowed);
    assert!(d.reason.as_ref().unwrap().contains("secret.txt"));
}

#[test]
fn single_tool_decision_reason_format() {
    let e = engine(&policy(&["Read"], &["Bash"], &[], &[]));

    let deny_explicit = e.can_use_tool("Bash");
    assert!(deny_explicit.reason.unwrap().contains("disallowed"));

    let deny_missing = e.can_use_tool("Unknown");
    assert!(deny_missing.reason.unwrap().contains("not in allowlist"));

    let allow = e.can_use_tool("Read");
    assert!(allow.reason.is_none());
}

#[test]
fn single_wildcard_deny_all_tools() {
    let e = engine(&policy(&[], &["*"], &[], &[]));
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

// ===================================================================
// 2. Policy composition — compose::ComposedEngine (15+ tests)
// ===================================================================

#[test]
fn compose_deny_overrides_any_deny_wins() {
    let p1 = policy(&[], &[], &[], &[]);
    let p2 = policy(&[], &["Bash"], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn compose_deny_overrides_all_allow() {
    let p1 = policy(&[], &[], &[], &[]);
    let p2 = policy(&[], &[], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn compose_allow_overrides_any_allow_wins() {
    let p1 = policy(&[], &["Bash"], &[], &[]);
    let p2 = policy(&[], &[], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn compose_allow_overrides_all_deny() {
    let p1 = policy(&["Read"], &["Bash"], &[], &[]);
    let p2 = policy(&["Read"], &["Bash"], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn compose_first_applicable_first_wins() {
    let p1 = policy(&[], &["Bash"], &[], &[]);
    let p2 = policy(&[], &[], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::FirstApplicable).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn compose_first_applicable_second_wins_when_first_allows() {
    let p1 = policy(&[], &[], &[], &[]);
    let p2 = policy(&[], &["Bash"], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::FirstApplicable).unwrap();
    // First profile has no deny, so it returns allow → first applicable returns allow.
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn compose_empty_policy_set_returns_abstain() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("anything").is_abstain());
}

#[test]
fn compose_multiple_policies_combined_deny_overrides() {
    let p1 = policy(&[], &[], &["secret/**"], &[]);
    let p2 = policy(&[], &[], &[], &["locked/**"]);
    let p3 = policy(&[], &["Bash"], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2, p3], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_read("secret/key.pem").is_deny());
    assert!(ce.check_write("locked/data.db").is_deny());
    assert!(ce.check_tool("Read").is_allow());
    assert!(ce.check_read("src/lib.rs").is_allow());
    assert!(ce.check_write("src/lib.rs").is_allow());
}

#[test]
fn compose_contradicting_policies_deny_overrides() {
    // One policy allows Bash (no deny), another denies it.
    let p1 = policy(&[], &[], &[], &[]);
    let p2 = policy(&[], &["Bash"], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn compose_contradicting_policies_allow_overrides() {
    let p1 = policy(&[], &[], &[], &[]);
    let p2 = policy(&[], &["Bash"], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn compose_read_deny_overrides() {
    let p1 = policy(&[], &[], &["**/.env"], &[]);
    let p2 = policy(&[], &[], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read(".env").is_deny());
    assert!(ce.check_read("src/main.rs").is_allow());
}

#[test]
fn compose_write_deny_overrides() {
    let p1 = policy(&[], &[], &[], &["**/.git/**"]);
    let p2 = policy(&[], &[], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/main.rs").is_allow());
}

#[test]
fn compose_write_allow_overrides() {
    let p1 = policy(&[], &[], &[], &["**/.git/**"]);
    let p2 = policy(&[], &[], &[], &[]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_write(".git/config").is_allow());
}

#[test]
fn compose_single_policy_behaves_like_raw_engine() {
    let p = policy(&["Read"], &["Bash"], &["secret*"], &["locked*"]);
    let ce = ComposedEngine::new(vec![p.clone()], PolicyPrecedence::DenyOverrides).unwrap();
    let raw = engine(&p);

    assert_eq!(
        ce.check_tool("Read").is_allow(),
        raw.can_use_tool("Read").allowed
    );
    assert_eq!(
        ce.check_tool("Bash").is_deny(),
        !raw.can_use_tool("Bash").allowed
    );
    assert_eq!(
        ce.check_read("secret.txt").is_deny(),
        !raw.can_read_path(Path::new("secret.txt")).allowed
    );
}

#[test]
fn compose_precedence_default_is_deny_overrides() {
    assert_eq!(PolicyPrecedence::default(), PolicyPrecedence::DenyOverrides);
}

// ===================================================================
// 2b. Policy composition — composed::ComposedPolicy (additional)
// ===================================================================

#[test]
fn composed_all_must_allow_single_deny_vetoes() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", engine(&PolicyProfile::default()));
    cp.add_policy("restrictive", engine(&policy(&[], &["Bash"], &[], &[])));
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_any_must_allow_single_allow_wins() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy(
        "restrictive",
        engine(&policy(&["Read"], &["Bash"], &[], &[])),
    );
    cp.add_policy("permissive", engine(&PolicyProfile::default()));
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_first_match_uses_first_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy("deny-bash", engine(&policy(&[], &["Bash"], &[], &[])));
    cp.add_policy("allow-all", engine(&PolicyProfile::default()));
    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_empty_returns_allowed() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("anything").is_allowed());
}

#[test]
fn composed_policy_count() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    cp.add_policy("a", engine(&PolicyProfile::default()));
    cp.add_policy("b", engine(&PolicyProfile::default()));
    assert_eq!(cp.policy_count(), 2);
}

#[test]
fn composed_strategy_accessor() {
    let cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    assert_eq!(cp.strategy(), CompositionStrategy::AnyMustAllow);
}

#[test]
fn composed_read_all_must_allow() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("open", engine(&PolicyProfile::default()));
    cp.add_policy("deny-env", engine(&policy(&[], &[], &["**/.env"], &[])));
    assert!(cp.evaluate_read(".env").is_denied());
    assert!(cp.evaluate_read("src/main.rs").is_allowed());
}

#[test]
fn composed_write_any_must_allow() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("deny-git", engine(&policy(&[], &[], &[], &["**/.git/**"])));
    cp.add_policy("open", engine(&PolicyProfile::default()));
    assert!(cp.evaluate_write(".git/config").is_allowed());
}

#[test]
fn composed_result_denied_includes_engine_name() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("security", engine(&policy(&[], &["Bash"], &[], &[])));
    let result = cp.evaluate_tool("Bash");
    match result {
        ComposedResult::Denied { by, .. } => assert_eq!(by, "security"),
        _ => panic!("expected denied"),
    }
}

// ===================================================================
// 3. Glob pattern matching (10+ tests)
// ===================================================================

#[test]
fn glob_exact_path_match() {
    let e = engine(&policy(&[], &[], &["Cargo.toml"], &[]));
    assert!(!e.can_read_path(Path::new("Cargo.toml")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.lock")).allowed);
}

#[test]
fn glob_wildcard_star_rs() {
    let e = engine(&policy(&[], &[], &["*.rs"], &[]));
    assert!(!e.can_read_path(Path::new("main.rs")).allowed);
    // globset with literal_separator=false: *.rs also matches nested paths
    assert!(!e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn glob_double_star_recursive() {
    let e = engine(&policy(&[], &[], &["src/**"], &[]));
    assert!(!e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_read_path(Path::new("src/a/b/c.rs")).allowed);
    assert!(e.can_read_path(Path::new("tests/test.rs")).allowed);
}

#[test]
fn glob_complex_nested_pattern() {
    let e = engine(&policy(&[], &[], &["**/secrets/**/*.pem"], &[]));
    assert!(
        !e.can_read_path(Path::new("secrets/certs/server.pem"))
            .allowed
    );
    assert!(
        !e.can_read_path(Path::new("config/secrets/tls/ca.pem"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("secrets/readme.txt")).allowed);
}

#[test]
fn glob_dot_files() {
    let e = engine(&policy(&[], &[], &["**/.*"], &[]));
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new("config/.hidden")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_brace_expansion_in_tool_names() {
    let e = engine(&policy(&[], &["{Bash,Shell,Exec}*"], &[], &[]));
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCommand").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_question_mark_single_char() {
    let e = engine(&policy(&[], &[], &["?.txt"], &[]));
    assert!(!e.can_read_path(Path::new("a.txt")).allowed);
    // globset: ? matches one char
    assert!(e.can_read_path(Path::new("ab.txt")).allowed);
}

#[test]
fn glob_extension_deny_write() {
    let e = engine(&policy(&[], &[], &[], &["*.lock"]));
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(
        !e.can_write_path(Path::new("package-lock.json.lock"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn glob_multiple_extensions() {
    let e = engine(&policy(&[], &[], &["*.{pem,key,crt}"], &[]));
    assert!(!e.can_read_path(Path::new("server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("private.key")).allowed);
    assert!(!e.can_read_path(Path::new("cert.crt")).allowed);
    assert!(e.can_read_path(Path::new("config.toml")).allowed);
}

#[test]
fn glob_deeply_nested_deny() {
    let e = engine(&policy(&[], &[], &[], &["a/b/c/d/e/**"]));
    assert!(!e.can_write_path(Path::new("a/b/c/d/e/f.txt")).allowed);
    assert!(e.can_write_path(Path::new("a/b/c/d/f.txt")).allowed);
}

#[test]
fn glob_star_matches_across_separators_in_globset() {
    // globset default has literal_separator = false, so * crosses /
    let e = engine(&policy(&[], &[], &["*.toml"], &[]));
    assert!(!e.can_read_path(Path::new("Cargo.toml")).allowed);
    assert!(!e.can_read_path(Path::new("config/settings.toml")).allowed);
}

// ===================================================================
// 4. Policy profiles (10+ tests)
// ===================================================================

#[test]
fn profile_default_is_empty() {
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
fn profile_serde_roundtrip() {
    let p = policy(&["Read", "Write"], &["Bash"], &["**/.env"], &["**/.git/**"]);
    let json = serde_json::to_string(&p).unwrap();
    let deserialized: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.allowed_tools, p.allowed_tools);
    assert_eq!(deserialized.disallowed_tools, p.disallowed_tools);
    assert_eq!(deserialized.deny_read, p.deny_read);
    assert_eq!(deserialized.deny_write, p.deny_write);
}

#[test]
fn profile_network_fields() {
    let p = PolicyProfile {
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.example.com".into()],
        ..PolicyProfile::default()
    };
    assert_eq!(p.allow_network, vec!["*.example.com"]);
    assert_eq!(p.deny_network, vec!["evil.example.com"]);
}

#[test]
fn profile_require_approval_for() {
    let p = PolicyProfile {
        require_approval_for: vec!["Bash".into(), "Delete".into()],
        ..PolicyProfile::default()
    };
    assert_eq!(p.require_approval_for.len(), 2);
}

#[test]
fn policy_set_create_and_name() {
    let ps = PolicySet::new("test-set");
    assert_eq!(ps.name(), "test-set");
}

#[test]
fn policy_set_merge_unions_all_lists() {
    let mut ps = PolicySet::new("merged");
    ps.add(policy(&["Read"], &["Bash"], &["secret*"], &["locked*"]));
    ps.add(policy(&["Write"], &["Exec"], &["*.pem"], &["*.lock"]));
    let merged = ps.merge();
    assert!(merged.allowed_tools.contains(&"Read".to_string()));
    assert!(merged.allowed_tools.contains(&"Write".to_string()));
    assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
    assert!(merged.disallowed_tools.contains(&"Exec".to_string()));
    assert!(merged.deny_read.contains(&"secret*".to_string()));
    assert!(merged.deny_read.contains(&"*.pem".to_string()));
    assert!(merged.deny_write.contains(&"locked*".to_string()));
    assert!(merged.deny_write.contains(&"*.lock".to_string()));
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut ps = PolicySet::new("dedup");
    ps.add(policy(&["Read"], &["Bash"], &[], &[]));
    ps.add(policy(&["Read"], &["Bash"], &[], &[]));
    let merged = ps.merge();
    assert_eq!(
        merged.allowed_tools.iter().filter(|t| *t == "Read").count(),
        1
    );
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
fn policy_set_empty_merge_is_default() {
    let ps = PolicySet::new("empty");
    let merged = ps.merge();
    assert!(merged.allowed_tools.is_empty());
    assert!(merged.disallowed_tools.is_empty());
}

#[test]
fn policy_set_merge_network_and_approval() {
    let mut ps = PolicySet::new("network");
    let p1 = PolicyProfile {
        allow_network: vec!["api.example.com".into()],
        require_approval_for: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let p2 = PolicyProfile {
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["Delete".into()],
        ..PolicyProfile::default()
    };
    ps.add(p1);
    ps.add(p2);
    let merged = ps.merge();
    assert!(merged
        .allow_network
        .contains(&"api.example.com".to_string()));
    assert!(merged.deny_network.contains(&"evil.com".to_string()));
    assert!(merged.require_approval_for.contains(&"Bash".to_string()));
    assert!(merged.require_approval_for.contains(&"Delete".to_string()));
}

#[test]
fn policy_set_merged_profile_compiles() {
    let mut ps = PolicySet::new("compile-check");
    ps.add(policy(&["Read"], &["Bash"], &["secret*"], &["locked*"]));
    ps.add(policy(&["Write"], &[], &[], &["**/.git/**"]));
    let merged = ps.merge();
    let e = engine(&merged);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
}

// ===================================================================
// 4b. Policy validation
// ===================================================================

#[test]
fn validator_detects_empty_glob() {
    let p = PolicyProfile {
        allowed_tools: vec!["".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_overlapping_allow_deny_tools() {
    let p = PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::OverlappingAllowDeny));
}

#[test]
fn validator_detects_unreachable_rule_wildcard_deny() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule));
}

#[test]
fn validator_detects_catchall_deny_read() {
    let p = PolicyProfile {
        deny_read: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_read")));
}

#[test]
fn validator_detects_catchall_deny_write() {
    let p = PolicyProfile {
        deny_write: vec!["**/*".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_write")));
}

#[test]
fn validator_clean_profile_no_warnings() {
    let p = policy(&["Read", "Write"], &["Bash"], &["**/.env"], &["**/.git/**"]);
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}

// ===================================================================
// 5. Integration with work orders (10+ tests)
// ===================================================================

#[test]
fn work_order_default_policy_allows_all() {
    let wo = WorkOrderBuilder::new("test task").build();
    let e = engine(&wo.policy);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn work_order_restrictive_policy_denies_tools() {
    let wo = WorkOrderBuilder::new("secure task")
        .policy(policy(&["Read", "Grep"], &["Bash", "Exec*"], &[], &[]))
        .build();
    let e = engine(&wo.policy);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("ExecShell").allowed);
}

#[test]
fn work_order_policy_denies_read_sensitive() {
    let wo = WorkOrderBuilder::new("safe read")
        .policy(policy(&[], &[], &["**/.env*", "**/id_rsa"], &[]))
        .build();
    let e = engine(&wo.policy);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn work_order_policy_denies_write_protected_dirs() {
    let wo = WorkOrderBuilder::new("write guard")
        .policy(policy(&[], &[], &[], &["**/.git/**", "dist/**"]))
        .build();
    let e = engine(&wo.policy);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(e.can_write_path(Path::new("src/app.rs")).allowed);
}

#[test]
fn work_order_policy_rejection_has_reason() {
    let wo = WorkOrderBuilder::new("denied task")
        .policy(policy(&["Read"], &[], &[], &[]))
        .build();
    let e = engine(&wo.policy);
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.is_some());
}

#[test]
fn work_order_policy_allows_permitted_tools() {
    let wo = WorkOrderBuilder::new("allowed task")
        .policy(policy(&["*"], &[], &[], &[]))
        .build();
    let e = engine(&wo.policy);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Bash").allowed);
}

#[test]
fn work_order_policy_restricts_capabilities() {
    let wo = WorkOrderBuilder::new("restricted")
        .policy(policy(
            &["Read"],
            &[],
            &["secret/**"],
            &["**/.git/**", "node_modules/**"],
        ))
        .build();
    let e = engine(&wo.policy);

    // Only Read is allowed
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);

    // Read restrictions
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);

    // Write restrictions
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn work_order_policy_compose_over_work_order() {
    let wo = WorkOrderBuilder::new("composed")
        .policy(policy(&[], &["Bash"], &[], &[]))
        .build();
    let org_policy = policy(&[], &[], &["**/.env"], &["**/.git/**"]);

    let mut ps = PolicySet::new("combined");
    ps.add(wo.policy.clone());
    ps.add(org_policy);
    let merged = ps.merge();
    let e = engine(&merged);

    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn work_order_serializes_policy() {
    let wo = WorkOrderBuilder::new("serde test")
        .policy(policy(&["Read"], &["Bash"], &["*.env"], &["*.lock"]))
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("Read"));
    assert!(json.contains("Bash"));
    assert!(json.contains("*.env"));
    assert!(json.contains("*.lock"));
}

#[test]
fn work_order_multiple_policies_via_composed_engine() {
    let wo = WorkOrderBuilder::new("multi-policy")
        .policy(policy(&["Read", "Write"], &[], &[], &[]))
        .build();
    let extra = policy(&[], &["Write"], &[], &[]);
    let ce = ComposedEngine::new(vec![wo.policy, extra], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Read").is_allow());
    assert!(ce.check_tool("Write").is_deny());
}

// ===================================================================
// 6. Rule engine tests
// ===================================================================

#[test]
fn rule_engine_always_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "deny-all".into(),
        description: "deny everything".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("anything"), RuleEffect::Deny);
}

#[test]
fn rule_engine_never_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "never-fires".into(),
        description: "never matches".into(),
        condition: RuleCondition::Never,
        effect: RuleEffect::Deny,
        priority: 10,
    });
    // No rule matches → default is Allow
    assert_eq!(re.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_pattern_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "deny-bash".into(),
        description: "deny bash".into(),
        condition: RuleCondition::Pattern("Bash*".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(re.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_priority_highest_wins() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "low-allow".into(),
        description: "low prio allow".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    re.add_rule(Rule {
        id: "high-deny".into(),
        description: "high prio deny".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 100,
    });
    assert_eq!(re.evaluate("resource"), RuleEffect::Deny);
}

#[test]
fn rule_engine_and_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "and-rule".into(),
        description: "match both".into(),
        condition: RuleCondition::And(vec![
            RuleCondition::Pattern("Bash*".into()),
            RuleCondition::Pattern("*Exec".into()),
        ]),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(re.evaluate("BashRun"), RuleEffect::Allow);
    assert_eq!(re.evaluate("ShellExec"), RuleEffect::Allow);
}

#[test]
fn rule_engine_or_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "or-rule".into(),
        description: "match either".into(),
        condition: RuleCondition::Or(vec![
            RuleCondition::Pattern("Bash*".into()),
            RuleCondition::Pattern("Shell*".into()),
        ]),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(re.evaluate("ShellRun"), RuleEffect::Deny);
    assert_eq!(re.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_not_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "not-rule".into(),
        description: "deny everything except Read*".into(),
        condition: RuleCondition::Not(Box::new(RuleCondition::Pattern("Read*".into()))),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("ReadFile"), RuleEffect::Allow);
    assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
}

#[test]
fn rule_engine_evaluate_all() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "r1".into(),
        description: "matches all".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    re.add_rule(Rule {
        id: "r2".into(),
        description: "never matches".into(),
        condition: RuleCondition::Never,
        effect: RuleEffect::Deny,
        priority: 100,
    });
    let results = re.evaluate_all("x");
    assert_eq!(results.len(), 2);
    assert!(results[0].matched);
    assert!(!results[1].matched);
}

#[test]
fn rule_engine_remove_rule() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "r1".into(),
        description: "".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.rule_count(), 1);
    re.remove_rule("r1");
    assert_eq!(re.rule_count(), 0);
    assert_eq!(re.evaluate("x"), RuleEffect::Allow);
}

#[test]
fn rule_engine_throttle_effect() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "throttle".into(),
        description: "throttle everything".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Throttle { max: 100 },
        priority: 10,
    });
    assert_eq!(re.evaluate("any"), RuleEffect::Throttle { max: 100 });
}

#[test]
fn rule_engine_log_effect() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "log".into(),
        description: "log everything".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Log,
        priority: 10,
    });
    assert_eq!(re.evaluate("any"), RuleEffect::Log);
}

// ===================================================================
// 7. Audit trail tests
// ===================================================================

#[test]
fn auditor_records_tool_allow() {
    let e = engine(&PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(e);
    let decision = auditor.check_tool("Read");
    assert!(matches!(decision, abp_policy::audit::PolicyDecision::Allow));
    assert_eq!(auditor.entries().len(), 1);
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 0);
}

#[test]
fn auditor_records_tool_deny() {
    let e = engine(&policy(&[], &["Bash"], &[], &[]));
    let mut auditor = PolicyAuditor::new(e);
    let decision = auditor.check_tool("Bash");
    assert!(matches!(
        decision,
        abp_policy::audit::PolicyDecision::Deny { .. }
    ));
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_read_and_write() {
    let e = engine(&policy(&[], &[], &["secret*"], &["locked*"]));
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_read("secret.txt");
    auditor.check_write("locked.db");
    auditor.check_read("src/main.rs");
    assert_eq!(auditor.entries().len(), 3);
    assert_eq!(auditor.denied_count(), 2);
    assert_eq!(auditor.allowed_count(), 1);
}

#[test]
fn auditor_summary() {
    let e = engine(&policy(&["Read"], &["Bash"], &[], &[]));
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_tool("Write");
    let s = auditor.summary();
    assert_eq!(
        s,
        AuditSummary {
            allowed: 1,
            denied: 2,
            warned: 0,
        }
    );
}

#[test]
fn audit_log_record_and_filter() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", Some("default"), None);
    log.record(
        AuditAction::ToolDenied,
        "Bash",
        Some("security"),
        Some("disallowed"),
    );
    log.record(
        AuditAction::ReadDenied,
        ".env",
        Some("security"),
        Some("sensitive"),
    );
    assert_eq!(log.len(), 3);
    assert!(!log.is_empty());
    assert_eq!(log.denied_count(), 2);
    assert_eq!(log.filter_by_action(&AuditAction::ToolAllowed).len(), 1);
    assert_eq!(log.filter_by_action(&AuditAction::ToolDenied).len(), 1);
    assert_eq!(log.filter_by_action(&AuditAction::ReadDenied).len(), 1);
}

#[test]
fn audit_action_is_denied() {
    assert!(!AuditAction::ToolAllowed.is_denied());
    assert!(AuditAction::ToolDenied.is_denied());
    assert!(!AuditAction::ReadAllowed.is_denied());
    assert!(AuditAction::ReadDenied.is_denied());
    assert!(!AuditAction::WriteAllowed.is_denied());
    assert!(AuditAction::WriteDenied.is_denied());
    assert!(AuditAction::RateLimited.is_denied());
}

// ===================================================================
// 8. Rate limiting tests
// ===================================================================

#[test]
fn rate_limit_unlimited_allows() {
    let rl = RateLimitPolicy::unlimited();
    assert!(rl.check_rate_limit(999, 999_999, 999).is_allowed());
}

#[test]
fn rate_limit_rpm_throttle() {
    let rl = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        ..Default::default()
    };
    assert!(rl.check_rate_limit(9, 0, 0).is_allowed());
    assert!(rl.check_rate_limit(10, 0, 0).is_throttled());
}

#[test]
fn rate_limit_concurrent_deny() {
    let rl = RateLimitPolicy {
        max_concurrent: Some(5),
        ..Default::default()
    };
    assert!(rl.check_rate_limit(0, 0, 4).is_allowed());
    assert!(rl.check_rate_limit(0, 0, 5).is_denied());
}

#[test]
fn rate_limit_tpm_throttle() {
    let rl = RateLimitPolicy {
        max_tokens_per_minute: Some(1000),
        ..Default::default()
    };
    assert!(rl.check_rate_limit(0, 999, 0).is_allowed());
    assert!(rl.check_rate_limit(0, 1000, 0).is_throttled());
}

// ===================================================================
// 9. Decision type tests
// ===================================================================

#[test]
fn decision_allow_has_no_reason() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_has_reason() {
    let d = Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

#[test]
fn policy_decision_enum_variants() {
    let allow = PolicyDecision::Allow {
        reason: "ok".into(),
    };
    let deny = PolicyDecision::Deny {
        reason: "no".into(),
    };
    let abstain = PolicyDecision::Abstain;

    assert!(allow.is_allow());
    assert!(!allow.is_deny());
    assert!(!allow.is_abstain());

    assert!(!deny.is_allow());
    assert!(deny.is_deny());
    assert!(!deny.is_abstain());

    assert!(!abstain.is_allow());
    assert!(!abstain.is_deny());
    assert!(abstain.is_abstain());
}

#[test]
fn composed_result_helpers() {
    let allowed = ComposedResult::Allowed { by: "test".into() };
    let denied = ComposedResult::Denied {
        by: "test".into(),
        reason: "no".into(),
    };
    assert!(allowed.is_allowed());
    assert!(!allowed.is_denied());
    assert!(!denied.is_allowed());
    assert!(denied.is_denied());
}
