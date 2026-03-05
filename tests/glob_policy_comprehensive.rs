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
//! Comprehensive tests for glob matching, policy enforcement, composition,
//! workspace integration, and edge cases.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::audit::{AuditSummary, PolicyAuditor, PolicyDecision as AuditDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn p(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("compile policy")
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Glob matching (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn glob_basic_star_rs() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("lib.py"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_double_star_ts() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.ts"]), &[]).unwrap();
    assert_eq!(g.decide_str("index.ts"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/components/App.ts"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_src_double_star() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/t.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_include_plus_exclude_combined() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "lib/**"]), &p(&["src/gen/**"])).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/gen/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("lib/utils.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/guide.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_complex_nested_directory_patterns() {
    let g = IncludeExcludeGlobs::new(
        &p(&["project/**/src/**/*.rs"]),
        &p(&["project/**/src/**/test_*"]),
    )
    .unwrap();
    assert_eq!(
        g.decide_str("project/core/src/lib.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("project/core/src/tests/test_main"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("project/README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_case_sensitivity() {
    // globset is case-insensitive on Windows by default, case-sensitive on Unix.
    // We test that at least the exact-case match works everywhere.
    let g = IncludeExcludeGlobs::new(&p(&["*.RS"]), &[]).unwrap();
    assert_eq!(g.decide_str("MAIN.RS"), MatchDecision::Allowed);
}

#[test]
fn glob_dotfile_handling() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["**/.*"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("home/.bashrc"), MatchDecision::DeniedByExclude);
}

#[test]
fn glob_dotfile_include_only() {
    let g = IncludeExcludeGlobs::new(&p(&[".*"]), &[]).unwrap();
    assert_eq!(g.decide_str(".env"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_empty_patterns_allow_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("deeply/nested/path.txt"),
        MatchDecision::Allowed
    );
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn glob_unicode_filenames() {
    let g = IncludeExcludeGlobs::new(&p(&["docs/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("docs/日本語.md"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("docs/über/straße.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("other/中文.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_multiple_extensions() {
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml,md}"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_question_mark_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    // '?' matches a single char; "ab.txt" has two chars before the dot
    // globset with literal_separator=false: '?' matches anything but /
    assert_eq!(
        g.decide_str("ab.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_exclude_only_denies_matches() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.log", "*.tmp"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("session.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn glob_decide_path_consistency() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/secret/**"])).unwrap();
    let cases = ["src/lib.rs", "src/secret/key.pem", "README.md"];
    for c in &cases {
        assert_eq!(
            g.decide_str(c),
            g.decide_path(Path::new(c)),
            "mismatch: {c}"
        );
    }
}

#[test]
fn glob_invalid_pattern_error() {
    let err = IncludeExcludeGlobs::new(&p(&["[unclosed"]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Policy enforcement (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_tool_allow_specific() {
    let e = engine(&PolicyProfile {
        allowed_tools: p(&["Read", "Grep"]),
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn policy_tool_deny_specific() {
    let e = engine(&PolicyProfile {
        disallowed_tools: p(&["Bash", "Exec"]),
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn policy_read_path_allow() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn policy_read_path_deny() {
    let e = engine(&PolicyProfile {
        deny_read: p(&["**/.env", "**/secrets/**"]),
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/secrets/api.key")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_write_path_allow() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_write_path_deny() {
    let e = engine(&PolicyProfile {
        deny_write: p(&["**/.git/**", "**/node_modules/**"]),
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("app/node_modules/pkg/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_deny_overrides_allow() {
    let e = engine(&PolicyProfile {
        allowed_tools: p(&["*"]),
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn policy_wildcard_patterns() {
    let e = engine(&PolicyProfile {
        disallowed_tools: p(&["Bash*"]),
        ..Default::default()
    });
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn policy_empty_allows_all() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Anything").allowed);
    assert!(e.can_read_path(Path::new("any/path")).allowed);
    assert!(e.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn policy_complex_profile() {
    let e = engine(&PolicyProfile {
        allowed_tools: p(&["Read", "Write", "Grep"]),
        disallowed_tools: p(&["Write"]),
        deny_read: p(&["**/.env"]),
        deny_write: p(&["**/locked/**"]),
        ..Default::default()
    });
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_decision_reason_text_tool_disallowed() {
    let e = engine(&PolicyProfile {
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
}

#[test]
fn policy_decision_reason_text_not_in_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: p(&["Read"]),
        ..Default::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' not in allowlist"));
}

#[test]
fn policy_deny_read_path_traversal() {
    let e = engine(&PolicyProfile {
        deny_read: p(&["**/etc/passwd"]),
        ..Default::default()
    });
    let d = e.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("denied"));
}

#[test]
fn policy_deny_write_deep_nesting() {
    let e = engine(&PolicyProfile {
        deny_write: p(&["vault/**"]),
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("vault/a/b/c/d/e.key")).allowed);
    assert!(e.can_write_path(Path::new("public/index.html")).allowed);
}

#[test]
fn policy_multiple_deny_read_patterns() {
    let e = engine(&PolicyProfile {
        deny_read: p(&["**/.env", "**/.env.*", "**/id_rsa", "**/*.pem"]),
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("ssh/id_rsa")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Policy + workspace integration (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn integration_policy_restricts_read_in_workspace() {
    let profile = PolicyProfile {
        deny_read: p(&["**/.env", "**/secrets/**"]),
        ..Default::default()
    };
    let e = engine(&profile);

    let workspace_files = [
        "src/main.rs",
        "src/config.rs",
        ".env",
        "secrets/api.key",
        "README.md",
    ];
    let readable: Vec<_> = workspace_files
        .iter()
        .filter(|f| e.can_read_path(Path::new(f)).allowed)
        .copied()
        .collect();
    assert_eq!(readable, vec!["src/main.rs", "src/config.rs", "README.md"]);
}

#[test]
fn integration_policy_restricts_write_in_workspace() {
    let profile = PolicyProfile {
        deny_write: p(&["**/.git/**", "Cargo.lock"]),
        ..Default::default()
    };
    let e = engine(&profile);

    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn integration_policy_restricts_tools() {
    let profile = PolicyProfile {
        allowed_tools: p(&["Read", "Grep", "ListDir"]),
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&profile);

    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
}

#[test]
fn integration_policy_serde_roundtrip() {
    let profile = PolicyProfile {
        allowed_tools: p(&["Read", "Write"]),
        disallowed_tools: p(&["Bash"]),
        deny_read: p(&["**/.env"]),
        deny_write: p(&["**/.git/**"]),
        allow_network: p(&["*.example.com"]),
        deny_network: p(&["evil.com"]),
        require_approval_for: p(&["DeleteFile"]),
    };
    let json = serde_json::to_string(&profile).unwrap();
    let restored: PolicyProfile = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.allowed_tools, profile.allowed_tools);
    assert_eq!(restored.disallowed_tools, profile.disallowed_tools);
    assert_eq!(restored.deny_read, profile.deny_read);
    assert_eq!(restored.deny_write, profile.deny_write);
    assert_eq!(restored.allow_network, profile.allow_network);
    assert_eq!(restored.deny_network, profile.deny_network);
    assert_eq!(restored.require_approval_for, profile.require_approval_for);

    // Restored policy compiles identically
    let e = engine(&restored);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn integration_policy_composition_merge() {
    let mut set = PolicySet::new("test-set");
    set.add(PolicyProfile {
        disallowed_tools: p(&["Bash"]),
        deny_read: p(&["**/.env"]),
        ..Default::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: p(&["Exec"]),
        deny_write: p(&["**/.git/**"]),
        ..Default::default()
    });

    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
    assert!(merged.disallowed_tools.contains(&"Exec".to_string()));
    assert!(merged.deny_read.contains(&"**/.env".to_string()));
    assert!(merged.deny_write.contains(&"**/.git/**".to_string()));

    let e = engine(&merged);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn integration_policy_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
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
fn integration_audit_trail_records_decisions() {
    let e = engine(&PolicyProfile {
        disallowed_tools: p(&["Bash"]),
        deny_read: p(&["**/.env"]),
        ..Default::default()
    });
    let mut auditor = PolicyAuditor::new(e);

    assert!(matches!(auditor.check_tool("Read"), AuditDecision::Allow));
    assert!(matches!(
        auditor.check_tool("Bash"),
        AuditDecision::Deny { .. }
    ));
    assert!(matches!(
        auditor.check_read("src/lib.rs"),
        AuditDecision::Allow
    ));
    assert!(matches!(
        auditor.check_read(".env"),
        AuditDecision::Deny { .. }
    ));
    assert!(matches!(
        auditor.check_write("src/lib.rs"),
        AuditDecision::Allow
    ));

    assert_eq!(auditor.entries().len(), 5);
    assert_eq!(auditor.allowed_count(), 3);
    assert_eq!(auditor.denied_count(), 2);
}

#[test]
fn integration_audit_summary() {
    let e = engine(&PolicyProfile {
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_tool("Grep");

    let s = auditor.summary();
    assert_eq!(
        s,
        AuditSummary {
            allowed: 2,
            denied: 1,
            warned: 0
        }
    );
}

#[test]
fn integration_composed_engine_deny_overrides() {
    let profiles = vec![
        PolicyProfile {
            allowed_tools: p(&["*"]),
            ..Default::default()
        },
        PolicyProfile {
            disallowed_tools: p(&["Bash"]),
            ..Default::default()
        },
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn integration_composed_engine_allow_overrides() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: p(&["Bash"]),
            ..Default::default()
        },
        PolicyProfile {
            allowed_tools: p(&["*"]),
            ..Default::default()
        },
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn integration_composed_engine_first_applicable() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: p(&["Bash"]),
            ..Default::default()
        },
        PolicyProfile {
            allowed_tools: p(&["*"]),
            ..Default::default()
        },
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::FirstApplicable).unwrap();
    // First profile denies Bash, so first-applicable = deny
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn integration_composed_engine_empty_abstains() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Anything").is_abstain());
}

#[test]
fn integration_validator_warns_overlapping() {
    let profile = PolicyProfile {
        allowed_tools: p(&["Bash"]),
        disallowed_tools: p(&["Bash"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
    );
}

#[test]
fn integration_validator_warns_empty_glob() {
    let profile = PolicyProfile {
        disallowed_tools: p(&[""]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn integration_validator_warns_unreachable_wildcard_deny() {
    let profile = PolicyProfile {
        allowed_tools: p(&["Read"]),
        disallowed_tools: p(&["*"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Edge cases (15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_very_long_path() {
    let segment = "a".repeat(50);
    let long_path = format!("src/{segment}/{segment}/{segment}/{segment}/{segment}/file.rs");
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str(&long_path), MatchDecision::Allowed);
}

#[test]
fn edge_path_with_spaces() {
    let g = IncludeExcludeGlobs::new(&p(&["my project/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("my project/src/lib.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("other/file.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_path_with_special_chars() {
    let g = IncludeExcludeGlobs::new(&p(&["data/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("data/file(1).txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("data/file #2.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("data/$var.sh"), MatchDecision::Allowed);
}

#[test]
fn edge_empty_glob_lists_allow_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/d/e"), MatchDecision::Allowed);
}

#[test]
fn edge_conflicting_include_exclude_exclude_wins() {
    // The same pattern in both include and exclude: exclude takes precedence
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn edge_policy_no_rules_allows_everything() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("AnyTool").allowed);
    assert!(e.can_read_path(Path::new("any/path.txt")).allowed);
    assert!(e.can_write_path(Path::new("any/path.txt")).allowed);
}

#[test]
fn edge_policy_all_deny_write() {
    let e = engine(&PolicyProfile {
        deny_write: p(&["**"]),
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("anything")).allowed);
    assert!(!e.can_write_path(Path::new("src/lib.rs")).allowed);
    // Read should still be allowed
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn edge_policy_all_deny_read() {
    let e = engine(&PolicyProfile {
        deny_read: p(&["**"]),
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("anything")).allowed);
    // Write should still be allowed
    assert!(e.can_write_path(Path::new("anything")).allowed);
}

#[test]
fn edge_many_patterns_in_policy() {
    let mut deny_read: Vec<String> = (0..100).map(|i| format!("dir{i}/**")).collect();
    deny_read.push("**/.secret".to_string());
    let e = engine(&PolicyProfile {
        deny_read,
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("dir0/file.txt")).allowed);
    assert!(!e.can_read_path(Path::new("dir99/deep/file.txt")).allowed);
    assert!(!e.can_read_path(Path::new(".secret")).allowed);
    assert!(e.can_read_path(Path::new("allowed/file.txt")).allowed);
}

#[test]
fn edge_decision_helpers() {
    let allow = Decision::allow();
    assert!(allow.allowed);
    assert!(allow.reason.is_none());

    let deny = Decision::deny("forbidden");
    assert!(!deny.allowed);
    assert_eq!(deny.reason.as_deref(), Some("forbidden"));
}

#[test]
fn edge_rule_engine_priority_ordering() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "low".into(),
        description: "low priority allow".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    re.add_rule(Rule {
        id: "high".into(),
        description: "high priority deny".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(re.evaluate("anything"), RuleEffect::Deny);
}

#[test]
fn edge_rule_engine_pattern_condition() {
    let mut re = RuleEngine::new();
    re.add_rule(Rule {
        id: "deny-bash".into(),
        description: "deny bash".into(),
        condition: RuleCondition::Pattern("Bash*".into()),
        effect: RuleEffect::Deny,
        priority: 1,
    });
    assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(re.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn edge_rule_condition_combinators() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Pattern("*.rs".into()),
        RuleCondition::Not(Box::new(RuleCondition::Pattern("test_*".into()))),
    ]);
    assert!(cond.matches("main.rs"));
    assert!(!cond.matches("test_main.rs"));
    assert!(!cond.matches("main.py"));
}

#[test]
fn edge_rule_condition_or() {
    let cond = RuleCondition::Or(vec![
        RuleCondition::Pattern("*.rs".into()),
        RuleCondition::Pattern("*.toml".into()),
    ]);
    assert!(cond.matches("lib.rs"));
    assert!(cond.matches("Cargo.toml"));
    assert!(!cond.matches("README.md"));
}

#[test]
fn edge_policy_set_name() {
    let set = PolicySet::new("security");
    assert_eq!(set.name(), "security");
}
