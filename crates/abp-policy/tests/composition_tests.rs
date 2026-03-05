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
//! Comprehensive policy composition, precedence, and enforcement tests.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::audit::{AuditSummary, PolicyAuditor};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::{Decision, PolicyEngine};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Single policy (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn single_empty_policy_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();

    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn single_tool_deny_blocks_specific_tool() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let denied = engine.can_use_tool("Bash");
    assert!(!denied.allowed);
    assert!(denied.reason.as_deref().unwrap().contains("Bash"));

    // Other tools remain allowed
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
}

#[test]
fn single_read_deny_blocks_specific_path() {
    let policy = PolicyProfile {
        deny_read: vec!["secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(
        !engine
            .can_read_path(Path::new("secrets/api_key.txt"))
            .allowed
    );
    assert!(
        !engine
            .can_read_path(Path::new("secrets/nested/deep.txt"))
            .allowed
    );
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    // Write is unaffected
    assert!(
        engine
            .can_write_path(Path::new("secrets/api_key.txt"))
            .allowed
    );
}

#[test]
fn single_write_deny_blocks_specific_path() {
    let policy = PolicyProfile {
        deny_write: vec!["protected/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(
        !engine
            .can_write_path(Path::new("protected/data.db"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new("protected/a/b/c.txt"))
            .allowed
    );
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
    // Read is unaffected
    assert!(engine.can_read_path(Path::new("protected/data.db")).allowed);
}

#[test]
fn single_allow_list_restricts_to_only_listed_items() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);

    let denied = engine.can_use_tool("Bash");
    assert!(!denied.allowed);
    assert!(
        denied
            .reason
            .as_deref()
            .unwrap()
            .contains("not in allowlist")
    );

    let denied2 = engine.can_use_tool("Write");
    assert!(!denied2.allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Composed policies (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn composed_deny_overrides_allow() {
    // One profile allows everything, the other denies Bash
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_tool("Read").is_allow());
}

#[test]
fn composed_multiple_deny_patterns_combine() {
    let deny_env = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let deny_secrets = PolicyProfile {
        deny_read: vec!["**/secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(
        vec![deny_env, deny_secrets],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    assert!(engine.check_read(".env").is_deny());
    assert!(engine.check_read("secrets/key.txt").is_deny());
    assert!(engine.check_read("src/main.rs").is_allow());
}

#[test]
fn composed_glob_patterns_in_deny_rules() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Shell*".into(), "Exec*".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("ShellExec").is_deny());
    assert!(engine.check_tool("ShellRun").is_deny());
    assert!(engine.check_tool("ExecBash").is_deny());
    assert!(engine.check_tool("Read").is_allow());
}

#[test]
fn composed_glob_patterns_in_allow_rules() {
    let policy = PolicyProfile {
        allowed_tools: vec!["File*".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("FileRead").is_allow());
    assert!(engine.check_tool("FileWrite").is_allow());
    assert!(engine.check_tool("Bash").is_deny());
}

#[test]
fn composed_mixed_tool_read_write_in_single_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Read").is_allow());
    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_tool("Write").is_deny()); // not in allowlist
    assert!(engine.check_read(".env").is_deny());
    assert!(engine.check_read("src/lib.rs").is_allow());
    assert!(engine.check_write(".git/config").is_deny());
    assert!(engine.check_write("src/lib.rs").is_allow());
}

#[test]
fn composed_policy_with_all_denies() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["*".into()],
        deny_read: vec!["**".into()],
        deny_write: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Anything").is_deny());
    assert!(engine.check_read("any/file.rs").is_deny());
    assert!(engine.check_write("any/file.rs").is_deny());
}

#[test]
fn composed_policy_with_all_allows() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Bash").is_allow());
    assert!(engine.check_tool("Read").is_allow());
    assert!(engine.check_read("any/path.txt").is_allow());
    assert!(engine.check_write("any/path.txt").is_allow());
}

#[test]
fn composed_precedence_deny_wins_over_allow() {
    // Both in allow and deny — deny must win under DenyOverrides
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["DangerTool".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("DangerTool").is_deny());
    assert!(engine.check_tool("SafeTool").is_allow());
}

#[test]
fn composed_nested_glob_patterns() {
    let policy = PolicyProfile {
        deny_read: vec!["**/*.rs".into()],
        deny_write: vec!["src/**/*.toml".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_read("src/main.rs").is_deny());
    assert!(engine.check_read("crates/core/lib.rs").is_deny());
    assert!(engine.check_read("README.md").is_allow());
    assert!(engine.check_write("src/config/app.toml").is_deny());
    assert!(engine.check_write("Cargo.toml").is_allow());
}

#[test]
fn composed_case_sensitivity_in_tool_names() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = ComposedEngine::new(vec![policy], PolicyPrecedence::DenyOverrides).unwrap();

    // Exact match should deny
    assert!(engine.check_tool("Bash").is_deny());
    // Different case — glob matching is case-sensitive by default
    assert!(engine.check_tool("bash").is_allow());
    assert!(engine.check_tool("BASH").is_allow());
    assert!(engine.check_tool("bAsH").is_allow());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Edge cases (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_tool_name() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    // Empty tool name on a default policy should be allowed
    let d = engine.can_use_tool("");
    assert!(d.allowed);

    // With an allowlist, empty name should be denied (not in list)
    let restricted = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..PolicyProfile::default()
    };
    let engine2 = PolicyEngine::new(&restricted).unwrap();
    assert!(!engine2.can_use_tool("").allowed);
}

#[test]
fn edge_very_long_path() {
    let long_segment = "a".repeat(200);
    let long_path = format!("src/{long_segment}/{long_segment}/{long_segment}.rs");

    let policy = PolicyProfile {
        deny_read: vec!["**/*.rs".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_read_path(Path::new(&long_path)).allowed);

    // A long path that doesn't match should be allowed
    let long_txt = format!("src/{long_segment}.txt");
    assert!(engine.can_read_path(Path::new(&long_txt)).allowed);
}

#[test]
fn edge_path_with_special_characters() {
    let policy = PolicyProfile {
        deny_write: vec!["**/*.log".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Spaces in path
    assert!(
        !engine
            .can_write_path(Path::new("my project/output.log"))
            .allowed
    );
    // Hyphens and underscores
    assert!(
        !engine
            .can_write_path(Path::new("my-project_v2/debug.log"))
            .allowed
    );
    // Allowed extension
    assert!(
        engine
            .can_write_path(Path::new("my project/output.txt"))
            .allowed
    );
}

#[test]
fn edge_unicode_in_tool_names_and_paths() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["ツール".into()],
        deny_read: vec!["日本語/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("ツール").allowed);
    assert!(engine.can_use_tool("Tool").allowed);
    assert!(!engine.can_read_path(Path::new("日本語/file.txt")).allowed);
    assert!(engine.can_read_path(Path::new("english/file.txt")).allowed);
}

#[test]
fn edge_policy_roundtrip_through_serde() {
    let original = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["DeleteFile".into()],
    };

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: PolicyProfile = serde_json::from_str(&json).unwrap();

    // Build engines from both and verify identical behavior
    let engine_orig = PolicyEngine::new(&original).unwrap();
    let engine_deser = PolicyEngine::new(&deserialized).unwrap();

    assert_eq!(
        engine_orig.can_use_tool("Bash").allowed,
        engine_deser.can_use_tool("Bash").allowed
    );
    assert_eq!(
        engine_orig.can_use_tool("Read").allowed,
        engine_deser.can_use_tool("Read").allowed
    );
    assert_eq!(
        engine_orig.can_read_path(Path::new(".env")).allowed,
        engine_deser.can_read_path(Path::new(".env")).allowed,
    );
    assert_eq!(
        engine_orig.can_write_path(Path::new(".git/config")).allowed,
        engine_deser
            .can_write_path(Path::new(".git/config"))
            .allowed,
    );
}

#[test]
fn edge_policy_set_builder_api() {
    let mut set = PolicySet::new("layered");
    assert_eq!(set.name(), "layered");

    // Build up layers
    set.add(PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    });

    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
    assert!(merged.deny_write.contains(&"**/.git/**".to_string()));
    assert!(merged.deny_read.contains(&"**/.env".to_string()));

    // Compile the merged profile into a working engine
    let engine = PolicyEngine::new(&merged).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn edge_decision_with_reason_metadata() {
    let allow = Decision::allow();
    assert!(allow.allowed);
    assert!(allow.reason.is_none());

    let deny = Decision::deny("blocked by admin policy");
    assert!(!deny.allowed);
    assert_eq!(deny.reason.as_deref(), Some("blocked by admin policy"));

    // Verify engine provides meaningful reasons
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["secret*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let tool_deny = engine.can_use_tool("Bash");
    assert!(tool_deny.reason.as_deref().unwrap().contains("disallowed"));

    let read_deny = engine.can_read_path(Path::new("secret.txt"));
    assert!(read_deny.reason.as_deref().unwrap().contains("denied"));
}

#[test]
fn edge_concurrent_policy_evaluation_thread_safety() {
    use std::sync::Arc;
    use std::thread;

    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                for _ in 0..100 {
                    assert!(engine.can_use_tool("Read").allowed);
                    assert!(!engine.can_use_tool("Bash").allowed);
                    assert!(!engine.can_read_path(Path::new(".env")).allowed);
                    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
                    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
                }
                i
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn edge_default_policy_behavior() {
    // Default PolicyProfile should produce a maximally permissive engine
    let default_profile = PolicyProfile::default();
    assert!(default_profile.allowed_tools.is_empty());
    assert!(default_profile.disallowed_tools.is_empty());
    assert!(default_profile.deny_read.is_empty());
    assert!(default_profile.deny_write.is_empty());

    let engine = PolicyEngine::new(&default_profile).unwrap();

    // When no allowlist is set, all tools are allowed
    assert!(engine.can_use_tool("Anything").allowed);
    assert!(engine.can_use_tool("").allowed);

    // When no deny paths, everything is readable/writable
    assert!(engine.can_read_path(Path::new("anything")).allowed);
    assert!(engine.can_write_path(Path::new("anything")).allowed);
}

#[test]
fn edge_policy_validation_comprehensive() {
    // Clean policy produces no warnings
    let clean = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    assert!(PolicyValidator::validate(&clean).is_empty());

    // Empty globs are detected in all fields
    let with_empty = PolicyProfile {
        allowed_tools: vec!["".into()],
        disallowed_tools: vec!["".into()],
        deny_read: vec!["".into()],
        deny_write: vec!["".into()],
        allow_network: vec!["".into()],
        deny_network: vec!["".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&with_empty);
    assert!(
        warnings
            .iter()
            .filter(|w| w.kind == WarningKind::EmptyGlob)
            .count()
            >= 6
    );

    // Overlapping tool allow/deny detected
    let overlapping = PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&overlapping);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
    );

    // Wildcard deny makes specific allows unreachable
    let unreachable = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&unreachable);
    assert!(
        warnings
            .iter()
            .filter(|w| w.kind == WarningKind::UnreachableRule)
            .count()
            >= 2
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Bonus: Auditor, ComposedEngine precedence, and PolicySet merge tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auditor_tracks_allow_and_deny_counts() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let mut auditor = PolicyAuditor::new(engine);

    let _ = auditor.check_tool("Read"); // allowed
    let _ = auditor.check_tool("Bash"); // denied
    let _ = auditor.check_read("src/main.rs"); // allowed
    let _ = auditor.check_read(".env"); // denied
    let _ = auditor.check_write("output.txt"); // allowed

    assert_eq!(auditor.allowed_count(), 3);
    assert_eq!(auditor.denied_count(), 2);
    assert_eq!(auditor.entries().len(), 5);
    assert_eq!(
        auditor.summary(),
        AuditSummary {
            allowed: 3,
            denied: 2,
            warned: 0,
        }
    );
}

#[test]
fn auditor_entries_have_correct_actions() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let mut auditor = PolicyAuditor::new(engine);

    auditor.check_tool("Read");
    auditor.check_read("file.txt");
    auditor.check_write("out.txt");

    let entries = auditor.entries();
    assert_eq!(entries[0].action, "tool");
    assert_eq!(entries[0].resource, "Read");
    assert_eq!(entries[1].action, "read");
    assert_eq!(entries[1].resource, "file.txt");
    assert_eq!(entries[2].action, "write");
    assert_eq!(entries[2].resource, "out.txt");
}

#[test]
fn composed_allow_overrides_path_denial() {
    // One policy denies reading .env, the other allows everything
    let deny_env = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile::default();

    let engine =
        ComposedEngine::new(vec![deny_env, permissive], PolicyPrecedence::AllowOverrides).unwrap();

    // Under AllowOverrides, any allow wins — second policy allows .env
    assert!(engine.check_read(".env").is_allow());
}

#[test]
fn composed_first_applicable_respects_order() {
    let deny_all_tools = PolicyProfile {
        disallowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let allow_all = PolicyProfile::default();

    // Deny-all first
    let engine1 = ComposedEngine::new(
        vec![deny_all_tools.clone(), allow_all.clone()],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();
    assert!(engine1.check_tool("Read").is_deny());

    // Allow-all first
    let engine2 = ComposedEngine::new(
        vec![allow_all, deny_all_tools],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();
    assert!(engine2.check_tool("Read").is_allow());
}

#[test]
fn policy_set_merge_preserves_network_and_approval() {
    let mut set = PolicySet::new("full");
    set.add(PolicyProfile {
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["Delete".into()],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        allow_network: vec!["*.trusted.org".into()],
        require_approval_for: vec!["Exec".into()],
        ..PolicyProfile::default()
    });

    let merged = set.merge();
    assert!(merged.allow_network.contains(&"*.example.com".to_string()));
    assert!(merged.allow_network.contains(&"*.trusted.org".to_string()));
    assert!(merged.deny_network.contains(&"evil.com".to_string()));
    assert!(merged.require_approval_for.contains(&"Delete".to_string()));
    assert!(merged.require_approval_for.contains(&"Exec".to_string()));
}

#[test]
fn composed_three_policies_deny_overrides() {
    let p1 = PolicyProfile {
        allowed_tools: vec!["*".into()],
        ..PolicyProfile::default()
    };
    let p2 = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let p3 = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(vec![p1, p2, p3], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_tool("Read").is_allow());
    assert!(engine.check_write(".git/HEAD").is_deny());
    assert!(engine.check_write("src/main.rs").is_allow());
}

#[test]
fn composed_serde_roundtrip_of_policy_decision() {
    let allow = PolicyDecision::Allow {
        reason: "permitted".into(),
    };
    let deny = PolicyDecision::Deny {
        reason: "blocked".into(),
    };
    let abstain = PolicyDecision::Abstain;

    let json_allow = serde_json::to_string(&allow).unwrap();
    let json_deny = serde_json::to_string(&deny).unwrap();
    let json_abstain = serde_json::to_string(&abstain).unwrap();

    let rt_allow: PolicyDecision = serde_json::from_str(&json_allow).unwrap();
    let rt_deny: PolicyDecision = serde_json::from_str(&json_deny).unwrap();
    let rt_abstain: PolicyDecision = serde_json::from_str(&json_abstain).unwrap();

    assert!(rt_allow.is_allow());
    assert!(rt_deny.is_deny());
    assert!(rt_abstain.is_abstain());
}

#[test]
fn composed_precedence_serde_roundtrip() {
    for prec in [
        PolicyPrecedence::DenyOverrides,
        PolicyPrecedence::AllowOverrides,
        PolicyPrecedence::FirstApplicable,
    ] {
        let json = serde_json::to_string(&prec).unwrap();
        let rt: PolicyPrecedence = serde_json::from_str(&json).unwrap();
        assert_eq!(prec, rt);
    }
}
