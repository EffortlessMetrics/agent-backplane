// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the compose module.

use abp_core::PolicyProfile;
use abp_policy::compose::{
    ComposedEngine, PolicyDecision, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};

// ── helpers ──────────────────────────────────────────────────────────────

fn deny_bash() -> PolicyProfile {
    PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    }
}

fn allow_read_grep() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    }
}

fn deny_write_git() -> PolicyProfile {
    PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    }
}

fn deny_read_env() -> PolicyProfile {
    PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    }
}

// ── PolicySet tests ──────────────────────────────────────────────────────

#[test]
fn policy_set_name() {
    let ps = PolicySet::new("org-defaults");
    assert_eq!(ps.name(), "org-defaults");
}

#[test]
fn policy_set_merge_unions_deny_lists() {
    let mut ps = PolicySet::new("test");
    ps.add(deny_bash());
    ps.add(deny_write_git());

    let merged = ps.merge();
    assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
    assert!(merged.deny_write.contains(&"**/.git/**".to_string()));
}

#[test]
fn policy_set_merge_unions_allow_lists() {
    let mut ps = PolicySet::new("test");
    ps.add(allow_read_grep());
    ps.add(PolicyProfile {
        allowed_tools: vec!["Write".into()],
        ..PolicyProfile::default()
    });

    let merged = ps.merge();
    assert!(merged.allowed_tools.contains(&"Read".to_string()));
    assert!(merged.allowed_tools.contains(&"Grep".to_string()));
    assert!(merged.allowed_tools.contains(&"Write".to_string()));
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut ps = PolicySet::new("test");
    ps.add(deny_bash());
    ps.add(deny_bash());

    let merged = ps.merge();
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
fn policy_set_merge_empty_set() {
    let ps = PolicySet::new("empty");
    let merged = ps.merge();
    assert!(merged.allowed_tools.is_empty());
    assert!(merged.disallowed_tools.is_empty());
}

// ── PolicyDecision helpers ───────────────────────────────────────────────

#[test]
fn decision_is_helpers() {
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

    assert!(deny.is_deny());
    assert!(!deny.is_allow());
    assert!(!deny.is_abstain());

    assert!(abstain.is_abstain());
    assert!(!abstain.is_allow());
    assert!(!abstain.is_deny());
}

// ── ComposedEngine – DenyOverrides ───────────────────────────────────────

#[test]
fn deny_overrides_any_deny_wins() {
    let permissive = PolicyProfile::default(); // allows everything
    let engine = ComposedEngine::new(
        vec![permissive, deny_bash()],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    let d = engine.check_tool("Bash");
    assert!(d.is_deny());
}

#[test]
fn deny_overrides_allows_when_no_deny() {
    let engine = ComposedEngine::new(
        vec![PolicyProfile::default(), PolicyProfile::default()],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    assert!(engine.check_tool("Anything").is_allow());
}

#[test]
fn deny_overrides_read_denied() {
    let engine =
        ComposedEngine::new(vec![deny_read_env()], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_read(".env").is_deny());
    assert!(engine.check_read("src/main.rs").is_allow());
}

#[test]
fn deny_overrides_write_denied() {
    let engine =
        ComposedEngine::new(vec![deny_write_git()], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_write(".git/config").is_deny());
    assert!(engine.check_write("src/main.rs").is_allow());
}

// ── ComposedEngine – AllowOverrides ──────────────────────────────────────

#[test]
fn allow_overrides_any_allow_wins() {
    let engine = ComposedEngine::new(
        vec![deny_bash(), PolicyProfile::default()],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();

    // The default profile doesn't disallow Bash → produces Allow → AllowOverrides picks it.
    assert!(engine.check_tool("Bash").is_allow());
}

#[test]
fn allow_overrides_all_deny_gives_deny() {
    let engine = ComposedEngine::new(
        vec![deny_bash(), deny_bash()],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();

    assert!(engine.check_tool("Bash").is_deny());
}

// ── ComposedEngine – FirstApplicable ─────────────────────────────────────

#[test]
fn first_applicable_uses_first_policy() {
    // First profile denies Bash; second allows everything.
    let engine = ComposedEngine::new(
        vec![deny_bash(), PolicyProfile::default()],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();

    assert!(engine.check_tool("Bash").is_deny());
}

#[test]
fn first_applicable_order_matters() {
    // Reverse order: permissive first.
    let engine = ComposedEngine::new(
        vec![PolicyProfile::default(), deny_bash()],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();

    // Default allows everything → first applicable → Allow.
    assert!(engine.check_tool("Bash").is_allow());
}

// ── ComposedEngine – empty ───────────────────────────────────────────────

#[test]
fn empty_engine_abstains() {
    let engine = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Anything").is_abstain());
    assert!(engine.check_read("any.txt").is_abstain());
    assert!(engine.check_write("any.txt").is_abstain());
}

// ── PolicyValidator – empty globs ────────────────────────────────────────

#[test]
fn validator_detects_empty_glob_in_allowed_tools() {
    let p = PolicyProfile {
        allowed_tools: vec!["".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::EmptyGlob && w.message.contains("allowed_tools"))
    );
}

#[test]
fn validator_detects_empty_glob_in_deny_read() {
    let p = PolicyProfile {
        deny_read: vec!["valid/*".into(), "".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::EmptyGlob && w.message.contains("deny_read"))
    );
}

// ── PolicyValidator – overlapping allow/deny ─────────────────────────────

#[test]
fn validator_detects_tool_overlap() {
    let p = PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny && w.message.contains("tool"))
    );
}

#[test]
fn validator_detects_network_overlap() {
    let p = PolicyProfile {
        allow_network: vec!["example.com".into()],
        deny_network: vec!["example.com".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny && w.message.contains("network"))
    );
}

// ── PolicyValidator – unreachable rules ──────────────────────────────────

#[test]
fn validator_detects_unreachable_allowed_tool() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("Read"))
    );
}

#[test]
fn validator_detects_catchall_deny_read() {
    let p = PolicyProfile {
        deny_read: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_read"))
    );
}

#[test]
fn validator_detects_catchall_deny_write() {
    let p = PolicyProfile {
        deny_write: vec!["**/*".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_write"))
    );
}

// ── PolicyValidator – clean profile ──────────────────────────────────────

#[test]
fn validator_clean_profile_no_warnings() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty(), "expected no warnings: {warnings:?}");
}

// ── ComposedEngine – complex multi-profile ───────────────────────────────

#[test]
fn deny_overrides_multi_profile_read_write() {
    let engine = ComposedEngine::new(
        vec![deny_read_env(), deny_write_git()],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    assert!(engine.check_read(".env").is_deny());
    assert!(engine.check_write(".git/config").is_deny());
    assert!(engine.check_read("src/lib.rs").is_allow());
    assert!(engine.check_write("src/lib.rs").is_allow());
}

// ── PolicySet + ComposedEngine round-trip ────────────────────────────────

#[test]
fn policy_set_merge_into_composed_engine() {
    let mut ps = PolicySet::new("combined");
    ps.add(deny_bash());
    ps.add(deny_write_git());

    let merged = ps.merge();
    let engine = ComposedEngine::new(vec![merged], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_write(".git/config").is_deny());
    assert!(engine.check_tool("Read").is_allow());
}
