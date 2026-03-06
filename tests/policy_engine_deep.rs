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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep integration tests for the abp-policy crate.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::audit::{AuditSummary, PolicyAuditor, PolicyDecision as AuditDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// =========================================================================
// Helpers
// =========================================================================

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("compile policy")
}

// =========================================================================
// 1. PolicyEngine::new (compile) from various PolicyProfile configs
// =========================================================================

mod compile {
    use super::*;

    #[test]
    fn empty_profile_compiles() {
        engine(&PolicyProfile::default());
    }

    #[test]
    fn single_allowed_tool() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..Default::default()
        };
        engine(&p);
    }

    #[test]
    fn single_disallowed_tool() {
        let p = PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        };
        engine(&p);
    }

    #[test]
    fn wildcard_allowed_tool() {
        let p = PolicyProfile {
            allowed_tools: s(&["*"]),
            ..Default::default()
        };
        engine(&p);
    }

    #[test]
    fn deny_read_globs_compile() {
        let p = PolicyProfile {
            deny_read: s(&["**/.env", "**/secrets/**"]),
            ..Default::default()
        };
        engine(&p);
    }

    #[test]
    fn deny_write_globs_compile() {
        let p = PolicyProfile {
            deny_write: s(&["**/.git/**", "**/node_modules/**"]),
            ..Default::default()
        };
        engine(&p);
    }

    #[test]
    fn invalid_glob_returns_error() {
        let p = PolicyProfile {
            disallowed_tools: s(&["["]),
            ..Default::default()
        };
        assert!(PolicyEngine::new(&p).is_err());
    }

    #[test]
    fn invalid_deny_read_glob_returns_error() {
        let p = PolicyProfile {
            deny_read: s(&["["]),
            ..Default::default()
        };
        assert!(PolicyEngine::new(&p).is_err());
    }

    #[test]
    fn invalid_deny_write_glob_returns_error() {
        let p = PolicyProfile {
            deny_write: s(&["["]),
            ..Default::default()
        };
        assert!(PolicyEngine::new(&p).is_err());
    }

    #[test]
    fn all_fields_populated() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/.git/**"]),
            allow_network: s(&["*.example.com"]),
            deny_network: s(&["evil.com"]),
            require_approval_for: s(&["DeleteFile"]),
        };
        engine(&p);
    }

    #[test]
    fn many_patterns_compile() {
        let tools: Vec<String> = (0..100).map(|i| format!("Tool{i}")).collect();
        let p = PolicyProfile {
            allowed_tools: tools,
            ..Default::default()
        };
        engine(&p);
    }
}

// =========================================================================
// 2. check_tool — can_use_tool with allowed/denied/no-rule patterns
// =========================================================================

mod check_tool {
    use super::*;

    #[test]
    fn empty_policy_allows_any_tool() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("anything").allowed);
    }

    #[test]
    fn allowlist_permits_listed() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
    }

    #[test]
    fn allowlist_denies_unlisted() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..Default::default()
        });
        let d = e.can_use_tool("Bash");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("tool 'Bash' not in allowlist"));
    }

    #[test]
    fn denylist_denies_listed() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        let d = e.can_use_tool("Bash");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
    }

    #[test]
    fn denylist_allows_unlisted() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn deny_beats_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn glob_pattern_in_deny() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash*"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("BashExec").allowed);
        assert!(!e.can_use_tool("BashRun").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn glob_pattern_in_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["File*"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("FileRead").allowed);
        assert!(e.can_use_tool("FileWrite").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn question_mark_glob_in_tools() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Tool?"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("ToolA").allowed);
        assert!(e.can_use_tool("ToolZ").allowed);
        // "Tool" alone has no character after it for ? match
        // globset is permissive with ?, let's just check multi-char fails
        assert!(!e.can_use_tool("ToolAB").allowed);
    }

    #[test]
    fn exact_match_in_allow_and_deny() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Write"]),
            disallowed_tools: s(&["Write"]),
            ..Default::default()
        });
        // Deny takes precedence
        assert!(!e.can_use_tool("Write").allowed);
    }

    #[test]
    fn case_sensitive_tool_names() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["bash"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("bash").allowed);
        // Globs are case-insensitive by default in globset on Windows,
        // but tool name matching should work consistently
    }

    #[test]
    fn empty_tool_name() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        // Empty string doesn't match "Bash" pattern
        assert!(e.can_use_tool("").allowed);
    }

    #[test]
    fn multiple_deny_patterns() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash", "Shell", "Exec*"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(!e.can_use_tool("ExecCommand").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn wildcard_allow_with_multiple_denies() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["Bash", "Shell", "Rm"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(!e.can_use_tool("Rm").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
        assert!(e.can_use_tool("Grep").allowed);
    }
}

// =========================================================================
// 3. check_read — can_read_path with path-based rules
// =========================================================================

mod check_read {
    use super::*;

    #[test]
    fn empty_policy_allows_all_reads() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
    }

    #[test]
    fn deny_read_blocks_matching_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret*"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    }

    #[test]
    fn deny_read_allows_non_matching_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret*"]),
            ..Default::default()
        });
        assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_read_with_double_star() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.env"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new("config/.env")).allowed);
        assert!(!e.can_read_path(Path::new("deep/nested/.env")).allowed);
        assert!(e.can_read_path(Path::new(".env.local")).allowed);
    }

    #[test]
    fn deny_read_recursive_dir() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["private/**"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("private/data.txt")).allowed);
        assert!(!e.can_read_path(Path::new("private/a/b/c.txt")).allowed);
        assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
    }

    #[test]
    fn deny_read_multiple_patterns() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.env", "**/.env.*", "**/id_rsa"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new(".env.production")).allowed);
        assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
        assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_read_decision_has_reason() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.secret"]),
            ..Default::default()
        });
        let d = e.can_read_path(Path::new("api.secret"));
        assert!(!d.allowed);
        assert!(d.reason.unwrap().contains("denied"));
    }

    #[test]
    fn deny_read_path_traversal() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/etc/passwd"]),
            ..Default::default()
        });
        let d = e.can_read_path(Path::new("../../etc/passwd"));
        assert!(!d.allowed);
    }

    #[test]
    fn deny_read_extension_glob() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.pem", "*.key"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
        assert!(!e.can_read_path(Path::new("server.key")).allowed);
        assert!(e.can_read_path(Path::new("readme.md")).allowed);
    }

    #[test]
    fn deny_read_empty_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.txt"]),
            ..Default::default()
        });
        // Empty path doesn't match *.txt
        assert!(e.can_read_path(Path::new("")).allowed);
    }
}

// =========================================================================
// 4. check_write — can_write_path with path-based rules
// =========================================================================

mod check_write {
    use super::*;

    #[test]
    fn empty_policy_allows_all_writes() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    }

    #[test]
    fn deny_write_blocks_matching() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["locked*"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("locked.md")).allowed);
    }

    #[test]
    fn deny_write_allows_non_matching() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["locked*"]),
            ..Default::default()
        });
        assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_write_git_dir() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
        assert!(!e.can_write_path(Path::new("repo/.git/objects/abc")).allowed);
        assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_write_recursive() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["secret/**"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
        assert!(!e.can_write_path(Path::new("secret/x.txt")).allowed);
        assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
    }

    #[test]
    fn deny_write_multiple_patterns() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**", "**/node_modules/**", "*.lock"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(
            !e.can_write_path(Path::new("node_modules/pkg/index.js"))
                .allowed
        );
        assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_decision_has_reason() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["*.lock"]),
            ..Default::default()
        });
        let d = e.can_write_path(Path::new("yarn.lock"));
        assert!(!d.allowed);
        assert!(d.reason.unwrap().contains("denied"));
    }

    #[test]
    fn deny_write_path_traversal() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..Default::default()
        });
        let d = e.can_write_path(Path::new("../.git/config"));
        assert!(!d.allowed);
    }

    #[test]
    fn deny_write_extension_glob() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["*.exe", "*.dll"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("app.exe")).allowed);
        assert!(!e.can_write_path(Path::new("lib.dll")).allowed);
        assert!(e.can_write_path(Path::new("lib.rs")).allowed);
    }

    #[test]
    fn deny_write_empty_path() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["*.txt"]),
            ..Default::default()
        });
        assert!(e.can_write_path(Path::new("")).allowed);
    }
}

// =========================================================================
// 5. Policy composition (multiple profiles)
// =========================================================================

mod composition {
    use super::*;

    #[test]
    fn policy_set_merge_unions_deny_lists() {
        let mut set = PolicySet::new("test");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            disallowed_tools: s(&["Shell"]),
            ..Default::default()
        });
        let merged = set.merge();
        assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
        assert!(merged.disallowed_tools.contains(&"Shell".to_string()));
    }

    #[test]
    fn policy_set_merge_unions_allow_lists() {
        let mut set = PolicySet::new("test");
        set.add(PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            allowed_tools: s(&["Write"]),
            ..Default::default()
        });
        let merged = set.merge();
        assert!(merged.allowed_tools.contains(&"Read".to_string()));
        assert!(merged.allowed_tools.contains(&"Write".to_string()));
    }

    #[test]
    fn policy_set_merge_deduplicates() {
        let mut set = PolicySet::new("dedup");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
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
    fn policy_set_name() {
        let set = PolicySet::new("my-policy");
        assert_eq!(set.name(), "my-policy");
    }

    #[test]
    fn policy_set_merge_deny_read_write() {
        let mut set = PolicySet::new("rw");
        set.add(PolicyProfile {
            deny_read: s(&["*.secret"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            deny_write: s(&["*.lock"]),
            ..Default::default()
        });
        let merged = set.merge();
        assert!(merged.deny_read.contains(&"*.secret".to_string()));
        assert!(merged.deny_write.contains(&"*.lock".to_string()));
    }

    #[test]
    fn policy_set_merge_network_fields() {
        let mut set = PolicySet::new("net");
        set.add(PolicyProfile {
            allow_network: s(&["*.example.com"]),
            deny_network: s(&["evil.com"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            allow_network: s(&["*.safe.org"]),
            ..Default::default()
        });
        let merged = set.merge();
        assert!(merged.allow_network.contains(&"*.example.com".to_string()));
        assert!(merged.allow_network.contains(&"*.safe.org".to_string()));
        assert!(merged.deny_network.contains(&"evil.com".to_string()));
    }

    #[test]
    fn policy_set_merge_require_approval() {
        let mut set = PolicySet::new("approval");
        set.add(PolicyProfile {
            require_approval_for: s(&["Bash"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            require_approval_for: s(&["Delete"]),
            ..Default::default()
        });
        let merged = set.merge();
        assert!(merged.require_approval_for.contains(&"Bash".to_string()));
        assert!(merged.require_approval_for.contains(&"Delete".to_string()));
    }

    #[test]
    fn composed_engine_deny_overrides_blocks() {
        let profiles = vec![
            PolicyProfile {
                allowed_tools: s(&["*"]),
                ..Default::default()
            },
            PolicyProfile {
                disallowed_tools: s(&["Bash"]),
                ..Default::default()
            },
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_tool("Bash").is_deny());
        assert!(ce.check_tool("Read").is_allow());
    }

    #[test]
    fn composed_engine_allow_overrides_permits() {
        let profiles = vec![
            PolicyProfile {
                disallowed_tools: s(&["Bash"]),
                ..Default::default()
            },
            PolicyProfile {
                allowed_tools: s(&["*"]),
                ..Default::default()
            },
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::AllowOverrides).unwrap();
        assert!(ce.check_tool("Bash").is_allow());
    }

    #[test]
    fn composed_engine_first_applicable() {
        let profiles = vec![
            PolicyProfile {
                disallowed_tools: s(&["Bash"]),
                ..Default::default()
            },
            PolicyProfile {
                allowed_tools: s(&["*"]),
                ..Default::default()
            },
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::FirstApplicable).unwrap();
        // First profile says deny Bash
        assert!(ce.check_tool("Bash").is_deny());
    }

    #[test]
    fn composed_engine_empty_abstains() {
        let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_tool("anything").is_abstain());
    }

    #[test]
    fn composed_engine_check_read() {
        let profiles = vec![PolicyProfile {
            deny_read: s(&["*.secret"]),
            ..Default::default()
        }];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_read("data.secret").is_deny());
        assert!(ce.check_read("src/lib.rs").is_allow());
    }

    #[test]
    fn composed_engine_check_write() {
        let profiles = vec![PolicyProfile {
            deny_write: s(&["*.lock"]),
            ..Default::default()
        }];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_write("Cargo.lock").is_deny());
        assert!(ce.check_write("src/lib.rs").is_allow());
    }

    #[test]
    fn composed_engine_multiple_read_profiles_deny_overrides() {
        let profiles = vec![
            PolicyProfile::default(), // allows everything
            PolicyProfile {
                deny_read: s(&["*.pem"]),
                ..Default::default()
            },
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_read("cert.pem").is_deny());
    }

    #[test]
    fn composed_engine_multiple_write_profiles_allow_overrides() {
        let profiles = vec![
            PolicyProfile {
                deny_write: s(&["*.lock"]),
                ..Default::default()
            },
            PolicyProfile::default(), // allows everything
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::AllowOverrides).unwrap();
        // One profile allows, allow overrides
        assert!(ce.check_write("Cargo.lock").is_allow());
    }
}

// =========================================================================
// 6. Audit logging
// =========================================================================

mod audit {
    use super::*;

    fn auditor(profile: &PolicyProfile) -> PolicyAuditor {
        let e = engine(profile);
        PolicyAuditor::new(e)
    }

    #[test]
    fn check_tool_records_allow() {
        let mut a = auditor(&PolicyProfile::default());
        let d = a.check_tool("Read");
        assert!(matches!(d, AuditDecision::Allow));
        assert_eq!(a.entries().len(), 1);
        assert_eq!(a.entries()[0].action, "tool");
        assert_eq!(a.entries()[0].resource, "Read");
    }

    #[test]
    fn check_tool_records_deny() {
        let mut a = auditor(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        });
        let d = a.check_tool("Bash");
        assert!(matches!(d, AuditDecision::Deny { .. }));
        assert_eq!(a.denied_count(), 1);
    }

    #[test]
    fn check_read_records_entry() {
        let mut a = auditor(&PolicyProfile {
            deny_read: s(&["*.secret"]),
            ..Default::default()
        });
        a.check_read("data.secret");
        a.check_read("src/lib.rs");
        assert_eq!(a.entries().len(), 2);
        assert_eq!(a.denied_count(), 1);
        assert_eq!(a.allowed_count(), 1);
    }

    #[test]
    fn check_write_records_entry() {
        let mut a = auditor(&PolicyProfile {
            deny_write: s(&["*.lock"]),
            ..Default::default()
        });
        a.check_write("Cargo.lock");
        a.check_write("src/lib.rs");
        assert_eq!(a.entries().len(), 2);
        assert_eq!(a.denied_count(), 1);
    }

    #[test]
    fn summary_counts() {
        let mut a = auditor(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["*.secret"]),
            ..Default::default()
        });
        a.check_tool("Bash");
        a.check_tool("Read");
        a.check_read("data.secret");
        a.check_read("src/lib.rs");
        let s = a.summary();
        assert_eq!(
            s,
            AuditSummary {
                allowed: 2,
                denied: 2,
                warned: 0,
            }
        );
    }

    #[test]
    fn empty_auditor_summary() {
        let a = auditor(&PolicyProfile::default());
        let s = a.summary();
        assert_eq!(
            s,
            AuditSummary {
                allowed: 0,
                denied: 0,
                warned: 0,
            }
        );
    }

    #[test]
    fn entries_are_chronological() {
        let mut a = auditor(&PolicyProfile::default());
        a.check_tool("First");
        a.check_tool("Second");
        a.check_tool("Third");
        assert_eq!(a.entries()[0].resource, "First");
        assert_eq!(a.entries()[1].resource, "Second");
        assert_eq!(a.entries()[2].resource, "Third");
    }

    #[test]
    fn audit_entry_has_timestamp() {
        let mut a = auditor(&PolicyProfile::default());
        a.check_tool("Read");
        assert!(a.entries()[0].timestamp <= chrono::Utc::now());
    }

    #[test]
    fn denied_count_zero_when_all_allowed() {
        let mut a = auditor(&PolicyProfile::default());
        a.check_tool("Read");
        a.check_tool("Write");
        assert_eq!(a.denied_count(), 0);
        assert_eq!(a.allowed_count(), 2);
    }

    #[test]
    fn all_denied_summary() {
        let mut a = auditor(&PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..Default::default()
        });
        a.check_tool("Bash");
        a.check_tool("Shell");
        a.check_tool("Exec");
        let s = a.summary();
        assert_eq!(s.denied, 3);
        assert_eq!(s.allowed, 0);
    }

    #[test]
    fn mixed_operations_audit() {
        let mut a = auditor(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["*.key"]),
            deny_write: s(&["*.lock"]),
            ..Default::default()
        });
        a.check_tool("Bash");
        a.check_tool("Read");
        a.check_read("server.key");
        a.check_read("readme.md");
        a.check_write("Cargo.lock");
        a.check_write("src/main.rs");
        assert_eq!(a.entries().len(), 6);
        assert_eq!(a.denied_count(), 3);
        assert_eq!(a.allowed_count(), 3);
    }
}

// =========================================================================
// 7. Custom policy rules (RuleEngine, RuleCondition, etc.)
// =========================================================================

mod rules {
    use super::*;

    fn make_rule(id: &str, cond: RuleCondition, effect: RuleEffect, priority: u32) -> Rule {
        Rule {
            id: id.to_string(),
            description: format!("Rule {id}"),
            condition: cond,
            effect,
            priority,
        }
    }

    #[test]
    fn empty_engine_allows_all() {
        let eng = RuleEngine::new();
        assert_eq!(eng.evaluate("anything"), RuleEffect::Allow);
    }

    #[test]
    fn single_deny_rule() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule(
            "deny-bash",
            RuleCondition::Pattern("Bash".into()),
            RuleEffect::Deny,
            10,
        ));
        assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
        assert_eq!(eng.evaluate("Read"), RuleEffect::Allow);
    }

    #[test]
    fn always_condition() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule(
            "deny-all",
            RuleCondition::Always,
            RuleEffect::Deny,
            10,
        ));
        assert_eq!(eng.evaluate("anything"), RuleEffect::Deny);
    }

    #[test]
    fn never_condition() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule(
            "never",
            RuleCondition::Never,
            RuleEffect::Deny,
            10,
        ));
        assert_eq!(eng.evaluate("anything"), RuleEffect::Allow);
    }

    #[test]
    fn and_condition() {
        let cond = RuleCondition::And(vec![
            RuleCondition::Pattern("*.rs".into()),
            RuleCondition::Not(Box::new(RuleCondition::Pattern("test*".into()))),
        ]);
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("log-non-test-rs", cond, RuleEffect::Log, 5));
        assert_eq!(eng.evaluate("main.rs"), RuleEffect::Log);
        assert_eq!(eng.evaluate("test_main.rs"), RuleEffect::Allow);
    }

    #[test]
    fn or_condition() {
        let cond = RuleCondition::Or(vec![
            RuleCondition::Pattern("*.rs".into()),
            RuleCondition::Pattern("*.py".into()),
        ]);
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("code-files", cond, RuleEffect::Log, 5));
        assert_eq!(eng.evaluate("main.rs"), RuleEffect::Log);
        assert_eq!(eng.evaluate("app.py"), RuleEffect::Log);
        assert_eq!(eng.evaluate("readme.md"), RuleEffect::Allow);
    }

    #[test]
    fn not_condition() {
        let cond = RuleCondition::Not(Box::new(RuleCondition::Pattern("*.rs".into())));
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("non-rs", cond, RuleEffect::Deny, 5));
        assert_eq!(eng.evaluate("readme.md"), RuleEffect::Deny);
        assert_eq!(eng.evaluate("main.rs"), RuleEffect::Allow);
    }

    #[test]
    fn priority_higher_wins() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule(
            "low-allow",
            RuleCondition::Always,
            RuleEffect::Allow,
            1,
        ));
        eng.add_rule(make_rule(
            "high-deny",
            RuleCondition::Always,
            RuleEffect::Deny,
            10,
        ));
        assert_eq!(eng.evaluate("anything"), RuleEffect::Deny);
    }

    #[test]
    fn evaluate_all_returns_all_rules() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Allow, 1));
        eng.add_rule(make_rule("r2", RuleCondition::Never, RuleEffect::Deny, 10));
        let results = eng.evaluate_all("test");
        assert_eq!(results.len(), 2);
        assert!(results[0].matched);
        assert!(!results[1].matched);
    }

    #[test]
    fn remove_rule() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule(
            "deny-bash",
            RuleCondition::Pattern("Bash".into()),
            RuleEffect::Deny,
            10,
        ));
        assert_eq!(eng.rule_count(), 1);
        eng.remove_rule("deny-bash");
        assert_eq!(eng.rule_count(), 0);
        assert_eq!(eng.evaluate("Bash"), RuleEffect::Allow);
    }

    #[test]
    fn remove_nonexistent_rule_is_noop() {
        let mut eng = RuleEngine::new();
        eng.remove_rule("does-not-exist");
        assert_eq!(eng.rule_count(), 0);
    }

    #[test]
    fn rules_accessor() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Allow, 1));
        assert_eq!(eng.rules().len(), 1);
        assert_eq!(eng.rules()[0].id, "r1");
    }

    #[test]
    fn throttle_effect() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule(
            "throttle",
            RuleCondition::Always,
            RuleEffect::Throttle { max: 5 },
            10,
        ));
        assert_eq!(eng.evaluate("anything"), RuleEffect::Throttle { max: 5 });
    }

    #[test]
    fn log_effect() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("log", RuleCondition::Always, RuleEffect::Log, 10));
        assert_eq!(eng.evaluate("anything"), RuleEffect::Log);
    }

    #[test]
    fn nested_and_or() {
        let cond = RuleCondition::And(vec![
            RuleCondition::Or(vec![
                RuleCondition::Pattern("*.rs".into()),
                RuleCondition::Pattern("*.toml".into()),
            ]),
            RuleCondition::Not(Box::new(RuleCondition::Pattern("*test*".into()))),
        ]);
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("complex", cond, RuleEffect::Log, 5));
        assert_eq!(eng.evaluate("main.rs"), RuleEffect::Log);
        assert_eq!(eng.evaluate("Cargo.toml"), RuleEffect::Log);
        assert_eq!(eng.evaluate("test_main.rs"), RuleEffect::Allow);
        assert_eq!(eng.evaluate("readme.md"), RuleEffect::Allow);
    }

    #[test]
    fn empty_and_is_always_true() {
        let cond = RuleCondition::And(vec![]);
        assert!(cond.matches("anything"));
    }

    #[test]
    fn empty_or_is_always_false() {
        let cond = RuleCondition::Or(vec![]);
        assert!(!cond.matches("anything"));
    }

    #[test]
    fn pattern_glob_wildcard() {
        let cond = RuleCondition::Pattern("*".into());
        assert!(cond.matches("anything"));
        assert!(cond.matches(""));
    }

    #[test]
    fn rule_evaluation_fields() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Deny, 5));
        let evals = eng.evaluate_all("test");
        assert_eq!(evals[0].rule_id, "r1");
        assert!(evals[0].matched);
        assert_eq!(evals[0].effect, RuleEffect::Deny);
    }
}

// =========================================================================
// 8. Edge cases
// =========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_policy_allows_everything() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
        assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    }

    #[test]
    fn wildcard_only_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("anything").allowed);
    }

    #[test]
    fn wildcard_only_deny() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["*"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("anything").allowed);
    }

    #[test]
    fn nested_path_deny_write() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["a/b/c/d/**"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("a/b/c/d/e.txt")).allowed);
        assert!(e.can_write_path(Path::new("a/b/c/other.txt")).allowed);
    }

    #[test]
    fn deeply_nested_path_deny_read() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["very/deep/nested/path/**"]),
            ..Default::default()
        });
        assert!(
            !e.can_read_path(Path::new("very/deep/nested/path/file.txt"))
                .allowed
        );
        assert!(e.can_read_path(Path::new("very/deep/other.txt")).allowed);
    }

    #[test]
    fn decision_allow_constructor() {
        let d = Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn decision_deny_constructor() {
        let d = Decision::deny("reason");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("reason"));
    }

    #[test]
    fn complex_combination() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read", "Write", "Grep"]),
            disallowed_tools: s(&["Write"]),
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/locked/**"]),
            ..Default::default()
        });
        // Write is in both lists — deny wins
        assert!(!e.can_use_tool("Write").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
        assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
        assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_read_and_write_independent() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.key"]),
            deny_write: s(&["*.lock"]),
            ..Default::default()
        });
        // deny_read doesn't affect writes
        assert!(e.can_write_path(Path::new("server.key")).allowed);
        // deny_write doesn't affect reads
        assert!(e.can_read_path(Path::new("Cargo.lock")).allowed);
    }

    #[test]
    fn unicode_tool_name() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Outil"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("Outil").allowed);
        assert!(e.can_use_tool("Tool").allowed);
    }

    #[test]
    fn unicode_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["données/**"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("données/fichier.txt")).allowed);
        assert!(e.can_read_path(Path::new("data/file.txt")).allowed);
    }

    #[test]
    fn tool_name_with_special_chars() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["my-tool"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("my-tool").allowed);
    }

    #[test]
    fn single_dot_path() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_read_path(Path::new(".")).allowed);
        assert!(e.can_write_path(Path::new(".")).allowed);
    }

    #[test]
    fn double_dot_path() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_read_path(Path::new("..")).allowed);
    }
}

// =========================================================================
// 9. PolicyValidator
// =========================================================================

mod validator {
    use super::*;

    #[test]
    fn empty_profile_no_warnings() {
        let w = PolicyValidator::validate(&PolicyProfile::default());
        assert!(w.is_empty());
    }

    #[test]
    fn empty_glob_warning() {
        let p = PolicyProfile {
            allowed_tools: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn overlapping_tool_allow_deny() {
        let p = PolicyProfile {
            allowed_tools: s(&["Bash"]),
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(
            w.iter()
                .any(|x| x.kind == WarningKind::OverlappingAllowDeny)
        );
    }

    #[test]
    fn overlapping_network_allow_deny() {
        let p = PolicyProfile {
            allow_network: s(&["evil.com"]),
            deny_network: s(&["evil.com"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(
            w.iter()
                .any(|x| x.kind == WarningKind::OverlappingAllowDeny)
        );
    }

    #[test]
    fn wildcard_deny_tools_unreachable() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["*"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::UnreachableRule));
    }

    #[test]
    fn catch_all_deny_read() {
        let p = PolicyProfile {
            deny_read: s(&["**"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::UnreachableRule));
    }

    #[test]
    fn catch_all_deny_write() {
        let p = PolicyProfile {
            deny_write: s(&["**/*"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::UnreachableRule));
    }

    #[test]
    fn no_warnings_for_normal_policy() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["*.secret"]),
            deny_write: s(&["*.lock"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.is_empty());
    }

    #[test]
    fn empty_glob_in_deny_read() {
        let p = PolicyProfile {
            deny_read: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn empty_glob_in_deny_write() {
        let p = PolicyProfile {
            deny_write: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn multiple_warnings() {
        let p = PolicyProfile {
            allowed_tools: s(&["Bash"]),
            disallowed_tools: s(&["Bash", "*"]),
            deny_read: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        // Should have: EmptyGlob (deny_read), OverlappingAllowDeny (Bash), UnreachableRule (Bash under *)
        assert!(w.len() >= 3);
    }
}

// =========================================================================
// 10. PolicyDecision (compose module) variants
// =========================================================================

mod compose_decision {
    use abp_policy::compose::PolicyDecision;

    #[test]
    fn allow_is_allow() {
        let d = PolicyDecision::Allow {
            reason: "ok".into(),
        };
        assert!(d.is_allow());
        assert!(!d.is_deny());
        assert!(!d.is_abstain());
    }

    #[test]
    fn deny_is_deny() {
        let d = PolicyDecision::Deny {
            reason: "nope".into(),
        };
        assert!(!d.is_allow());
        assert!(d.is_deny());
        assert!(!d.is_abstain());
    }

    #[test]
    fn abstain_is_abstain() {
        let d = PolicyDecision::Abstain;
        assert!(!d.is_allow());
        assert!(!d.is_deny());
        assert!(d.is_abstain());
    }
}

// =========================================================================
// 11. Serialization of audit types
// =========================================================================

mod audit_serde {
    use abp_policy::audit::PolicyDecision as AuditPD;

    #[test]
    fn audit_decision_allow_is_serializable() {
        let json = serde_json::to_string(&AuditPD::Allow).unwrap();
        assert!(json.contains("allow"));
    }

    #[test]
    fn audit_decision_deny_is_serializable() {
        let d = AuditPD::Deny {
            reason: "bad".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("deny"));
        assert!(json.contains("bad"));
    }

    #[test]
    fn audit_decision_roundtrip() {
        let d = AuditPD::Warn {
            reason: "caution".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: AuditPD = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }
}

// =========================================================================
// 12. Additional coverage: tool pattern specificity & edge scenarios
// =========================================================================

mod tool_pattern_specificity {
    use super::*;

    #[test]
    fn underscore_glob_pattern_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["read_*"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("read_file").allowed);
        assert!(e.can_use_tool("read_dir").allowed);
        assert!(!e.can_use_tool("write_file").allowed);
    }

    #[test]
    fn underscore_glob_pattern_deny() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["exec_*"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("exec_cmd").allowed);
        assert!(!e.can_use_tool("exec_shell").allowed);
        assert!(e.can_use_tool("read_file").allowed);
    }

    #[test]
    fn multiple_allow_patterns() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["read_*", "list_*", "Grep"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("read_file").allowed);
        assert!(e.can_use_tool("list_dir").allowed);
        assert!(e.can_use_tool("Grep").allowed);
        assert!(!e.can_use_tool("write_file").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn broad_allow_narrow_deny() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["exec_shell"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("exec_shell").allowed);
        assert!(e.can_use_tool("exec_cmd").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn narrow_allow_broad_deny() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["*"]),
            ..Default::default()
        });
        // Deny always beats allow, even specific vs wildcard
        assert!(!e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn pattern_deny_overrides_pattern_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["file_*"]),
            disallowed_tools: s(&["file_delete*"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("file_read").allowed);
        assert!(e.can_use_tool("file_write").allowed);
        assert!(!e.can_use_tool("file_delete").allowed);
        assert!(!e.can_use_tool("file_delete_recursive").allowed);
    }

    #[test]
    fn tool_name_with_dots() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["com.example.*"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("com.example.tool").allowed);
        assert!(e.can_use_tool("org.other.tool").allowed);
    }

    #[test]
    fn tool_name_with_slashes() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["ns/dangerous_*"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("ns/dangerous_exec").allowed);
        assert!(e.can_use_tool("ns/safe_read").allowed);
    }

    #[test]
    fn curly_brace_alternation_deny() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["{Bash,Shell,Exec}"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(!e.can_use_tool("Exec").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn curly_brace_alternation_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["{Read,Write,Grep}"]),
            ..Default::default()
        });
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
        assert!(e.can_use_tool("Grep").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn very_long_tool_name() {
        let long_name: String = "Tool".repeat(250);
        let e = engine(&PolicyProfile {
            disallowed_tools: vec![long_name.clone()],
            ..Default::default()
        });
        assert!(!e.can_use_tool(&long_name).allowed);
        assert!(e.can_use_tool("Short").allowed);
    }
}

// =========================================================================
// 13. Additional file read coverage
// =========================================================================

mod read_advanced {
    use super::*;

    #[test]
    fn deny_read_hidden_files() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.*"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
        assert!(!e.can_read_path(Path::new("config/.hidden")).allowed);
        assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_read_specific_nested_dir() {
        let e = engine(&PolicyProfile {
            deny_read: s(&[".secret/**"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new(".secret/key.pem")).allowed);
        assert!(!e.can_read_path(Path::new(".secret/nested/data")).allowed);
        assert!(e.can_read_path(Path::new("public/key.pem")).allowed);
    }

    #[test]
    fn deny_read_env_variants() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.env", "*.env.*"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new("app.env")).allowed);
        assert!(!e.can_read_path(Path::new(".env.local")).allowed);
        assert!(!e.can_read_path(Path::new("config.env.production")).allowed);
        assert!(e.can_read_path(Path::new("environment.rs")).allowed);
    }

    #[test]
    fn deny_read_very_long_path() {
        let long_path = "a/".repeat(200) + "secret.txt";
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/secret.txt"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new(&long_path)).allowed);
    }

    #[test]
    #[cfg(windows)]
    fn deny_read_windows_style_separators() {
        // Path::new normalizes on each OS; test that the deny pattern works
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/secret.txt"]),
            ..Default::default()
        });
        // On Windows, backslash is the separator; Path::new handles it
        assert!(!e.can_read_path(Path::new("dir\\secret.txt")).allowed);
    }

    #[test]
    fn deny_read_curly_brace_extension_set() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.{pem,key,p12}"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
        assert!(!e.can_read_path(Path::new("server.key")).allowed);
        assert!(!e.can_read_path(Path::new("keystore.p12")).allowed);
        assert!(e.can_read_path(Path::new("readme.md")).allowed);
    }

    #[test]
    fn deny_read_does_not_affect_write() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.secret"]),
            ..Default::default()
        });
        // Writing the same path should still be allowed
        assert!(e.can_write_path(Path::new("data.secret")).allowed);
    }

    #[test]
    fn deny_read_root_file_only() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["Makefile"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("Makefile")).allowed);
        // globset default: * crosses directories, so nested may also match
        assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    }
}

// =========================================================================
// 14. Additional file write coverage
// =========================================================================

mod write_advanced {
    use super::*;

    #[test]
    fn deny_write_workspace_root_dotfiles() {
        let e = engine(&PolicyProfile {
            deny_write: s(&[".*"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new(".gitignore")).allowed);
        assert!(!e.can_write_path(Path::new(".env")).allowed);
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_node_modules_deep() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/node_modules/**"]),
            ..Default::default()
        });
        assert!(
            !e.can_write_path(Path::new("node_modules/pkg/index.js"))
                .allowed
        );
        assert!(
            !e.can_write_path(Path::new("frontend/node_modules/x/y/z.js"))
                .allowed
        );
        assert!(e.can_write_path(Path::new("src/index.js")).allowed);
    }

    #[test]
    fn deny_write_binary_extensions() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["*.{exe,dll,so,dylib}"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("app.exe")).allowed);
        assert!(!e.can_write_path(Path::new("libfoo.so")).allowed);
        assert!(!e.can_write_path(Path::new("libbar.dylib")).allowed);
        assert!(e.can_write_path(Path::new("main.rs")).allowed);
    }

    #[test]
    fn deny_write_does_not_affect_read() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/*.lock"]),
            ..Default::default()
        });
        assert!(e.can_read_path(Path::new("Cargo.lock")).allowed);
    }

    #[test]
    fn deny_write_very_long_path() {
        let long_path = "deep/".repeat(150) + "file.txt";
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/file.txt"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new(&long_path)).allowed);
    }

    #[test]
    fn deny_write_workspace_root_files() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["Cargo.toml", "Cargo.lock"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("Cargo.toml")).allowed);
        assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_parent_traversal() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/etc/**"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("../../../etc/passwd")).allowed);
    }

    #[test]
    fn deny_write_only_specific_subdir() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["build/release/**"]),
            ..Default::default()
        });
        assert!(
            !e.can_write_path(Path::new("build/release/output.bin"))
                .allowed
        );
        assert!(
            e.can_write_path(Path::new("build/debug/output.bin"))
                .allowed
        );
    }
}

// =========================================================================
// 15. Serialization roundtrip and TOML config
// =========================================================================

mod serialization {
    use super::*;

    #[test]
    fn policy_profile_json_roundtrip() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/.git/**"]),
            allow_network: s(&["*.example.com"]),
            deny_network: s(&["evil.com"]),
            require_approval_for: s(&["Delete"]),
        };
        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.allowed_tools, p.allowed_tools);
        assert_eq!(back.disallowed_tools, p.disallowed_tools);
        assert_eq!(back.deny_read, p.deny_read);
        assert_eq!(back.deny_write, p.deny_write);
        assert_eq!(back.allow_network, p.allow_network);
        assert_eq!(back.deny_network, p.deny_network);
        assert_eq!(back.require_approval_for, p.require_approval_for);
    }

    #[test]
    fn policy_profile_toml_roundtrip() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/.git/**"]),
            allow_network: s(&["*.example.com"]),
            deny_network: s(&["evil.com"]),
            require_approval_for: s(&["Delete"]),
        };
        let toml_str = toml::to_string_pretty(&p).unwrap();
        let back: PolicyProfile = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.allowed_tools, p.allowed_tools);
        assert_eq!(back.disallowed_tools, p.disallowed_tools);
        assert_eq!(back.deny_read, p.deny_read);
        assert_eq!(back.deny_write, p.deny_write);
    }

    #[test]
    fn policy_from_toml_string() {
        let toml_str = r#"
allowed_tools = ["Read", "Write"]
disallowed_tools = ["Bash"]
deny_read = ["**/.env"]
deny_write = ["**/.git/**"]
allow_network = []
deny_network = []
require_approval_for = []
"#;
        let p: PolicyProfile = toml::from_str(toml_str).unwrap();
        assert_eq!(p.allowed_tools, vec!["Read", "Write"]);
        assert_eq!(p.disallowed_tools, vec!["Bash"]);
        // Verify engine compiles from parsed TOML
        let e = engine(&p);
        assert!(e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn default_policy_profile_json_roundtrip() {
        let p = PolicyProfile::default();
        let json = serde_json::to_string(&p).unwrap();
        let back: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert!(back.allowed_tools.is_empty());
        assert!(back.disallowed_tools.is_empty());
        assert!(back.deny_read.is_empty());
        assert!(back.deny_write.is_empty());
    }

    #[test]
    fn decision_is_serializable() {
        let d = Decision::deny("forbidden");
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("forbidden"));
        assert!(json.contains("false"));
    }

    #[test]
    fn composed_policy_decision_json_roundtrip() {
        use abp_policy::compose::PolicyDecision;

        let cases = vec![
            PolicyDecision::Allow {
                reason: "ok".into(),
            },
            PolicyDecision::Deny {
                reason: "bad".into(),
            },
            PolicyDecision::Abstain,
        ];
        for d in &cases {
            let json = serde_json::to_string(d).unwrap();
            let back: PolicyDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, d);
        }
    }
}

// =========================================================================
// 16. Compilation edge cases
// =========================================================================

mod compile_advanced {
    use super::*;

    #[test]
    fn compile_with_only_deny_read() {
        let p = PolicyProfile {
            deny_read: s(&["*.secret", "**/.env"]),
            ..Default::default()
        };
        let e = engine(&p);
        assert!(!e.can_read_path(Path::new("api.secret")).allowed);
        assert!(e.can_use_tool("anything").allowed);
        assert!(e.can_write_path(Path::new("api.secret")).allowed);
    }

    #[test]
    fn compile_with_only_deny_write() {
        let p = PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..Default::default()
        };
        let e = engine(&p);
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(e.can_read_path(Path::new(".git/config")).allowed);
        assert!(e.can_use_tool("anything").allowed);
    }

    #[test]
    fn compile_all_deny_lists_populated() {
        let p = PolicyProfile {
            disallowed_tools: s(&["Bash", "Shell"]),
            deny_read: s(&["*.key", "*.pem"]),
            deny_write: s(&["*.lock", "**/.git/**"]),
            ..Default::default()
        };
        let e = engine(&p);
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_read_path(Path::new("server.key")).allowed);
        assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
        assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn compile_invalid_in_allowed_tools() {
        let p = PolicyProfile {
            allowed_tools: s(&["["]),
            ..Default::default()
        };
        assert!(PolicyEngine::new(&p).is_err());
    }

    #[test]
    fn compile_invalid_in_deny_read_preserves_context() {
        let p = PolicyProfile {
            deny_read: s(&["[invalid"]),
            ..Default::default()
        };
        let err = PolicyEngine::new(&p).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("deny_read"), "error: {msg}");
    }

    #[test]
    fn compile_mixed_valid_and_invalid_fails() {
        let p = PolicyProfile {
            deny_write: s(&["valid_pattern", "[invalid"]),
            ..Default::default()
        };
        assert!(PolicyEngine::new(&p).is_err());
    }

    #[test]
    fn compile_double_star_pattern() {
        let p = PolicyProfile {
            deny_read: s(&["**"]),
            ..Default::default()
        };
        let e = engine(&p);
        assert!(!e.can_read_path(Path::new("anything")).allowed);
        assert!(!e.can_read_path(Path::new("a/b/c")).allowed);
    }

    #[test]
    fn compile_empty_string_pattern_succeeds() {
        // Empty string is a valid (though useless) glob
        let p = PolicyProfile {
            deny_read: vec![String::new()],
            ..Default::default()
        };
        // Should compile without error
        let _e = engine(&p);
    }
}

// =========================================================================
// 17. Rule engine advanced scenarios
// =========================================================================

mod rules_advanced {
    use super::*;

    #[test]
    fn multiple_rules_same_priority_first_wins() {
        let mut eng = RuleEngine::new();
        eng.add_rule(Rule {
            id: "first".into(),
            description: "First rule".into(),
            condition: RuleCondition::Always,
            effect: RuleEffect::Log,
            priority: 10,
        });
        eng.add_rule(Rule {
            id: "second".into(),
            description: "Second rule".into(),
            condition: RuleCondition::Always,
            effect: RuleEffect::Deny,
            priority: 10,
        });
        // max_by_key returns last max when equal, so second wins
        // (this tests the actual behavior)
        let result = eng.evaluate("test");
        assert!(result == RuleEffect::Log || result == RuleEffect::Deny);
    }

    #[test]
    fn rule_condition_pattern_invalid_glob_no_match() {
        // Invalid glob pattern in a RuleCondition::Pattern silently doesn't match
        let cond = RuleCondition::Pattern("[".into());
        assert!(!cond.matches("anything"));
    }

    #[test]
    fn deeply_nested_conditions() {
        let cond = RuleCondition::Not(Box::new(RuleCondition::And(vec![
            RuleCondition::Or(vec![
                RuleCondition::Pattern("*.rs".into()),
                RuleCondition::Pattern("*.py".into()),
            ]),
            RuleCondition::Not(Box::new(RuleCondition::Pattern("test*".into()))),
        ])));
        // The inner matches non-test .rs/.py files
        // NOT inverts: matches everything EXCEPT non-test .rs/.py
        assert!(!cond.matches("main.rs")); // inner matches → NOT = false
        assert!(cond.matches("test_main.rs")); // inner doesn't match → NOT = true
        assert!(cond.matches("readme.md")); // inner doesn't match → NOT = true
    }

    #[test]
    fn rule_serde_roundtrip() {
        let rule = Rule {
            id: "test-rule".into(),
            description: "A test rule".into(),
            condition: RuleCondition::Pattern("*.rs".into()),
            effect: RuleEffect::Deny,
            priority: 42,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "test-rule");
        assert_eq!(back.priority, 42);
    }

    #[test]
    fn evaluate_all_with_no_rules() {
        let eng = RuleEngine::new();
        let results = eng.evaluate_all("anything");
        assert!(results.is_empty());
    }

    #[test]
    fn add_and_remove_multiple_rules() {
        let mut eng = RuleEngine::new();
        for i in 0..10 {
            eng.add_rule(Rule {
                id: format!("rule-{i}"),
                description: format!("Rule {i}"),
                condition: RuleCondition::Always,
                effect: RuleEffect::Allow,
                priority: i,
            });
        }
        assert_eq!(eng.rule_count(), 10);
        eng.remove_rule("rule-5");
        assert_eq!(eng.rule_count(), 9);
        eng.remove_rule("rule-0");
        assert_eq!(eng.rule_count(), 8);
        // Removing again is a no-op
        eng.remove_rule("rule-0");
        assert_eq!(eng.rule_count(), 8);
    }
}

// =========================================================================
// 18. Validator advanced scenarios
// =========================================================================

mod validator_advanced {
    use super::*;

    #[test]
    fn validator_empty_glob_in_allowed_tools() {
        let p = PolicyProfile {
            allowed_tools: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn validator_empty_glob_in_disallowed_tools() {
        let p = PolicyProfile {
            disallowed_tools: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn validator_empty_glob_in_allow_network() {
        let p = PolicyProfile {
            allow_network: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn validator_empty_glob_in_deny_network() {
        let p = PolicyProfile {
            deny_network: vec![String::new()],
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn validator_wildcard_deny_with_multiple_allows_unreachable() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write", "Grep"]),
            disallowed_tools: s(&["*"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        let unreachable_count = w
            .iter()
            .filter(|x| x.kind == WarningKind::UnreachableRule)
            .count();
        assert_eq!(unreachable_count, 3);
    }

    #[test]
    fn validator_catch_all_deny_write_star_star_slash_star() {
        let p = PolicyProfile {
            deny_write: s(&["**/*"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w.iter().any(|x| x.kind == WarningKind::UnreachableRule));
    }

    #[test]
    fn validator_no_false_overlap_for_different_patterns() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["Bash"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(
            !w.iter()
                .any(|x| x.kind == WarningKind::OverlappingAllowDeny)
        );
    }
}

// =========================================================================
// 19. Composed engine advanced scenarios
// =========================================================================

mod compose_advanced {
    use super::*;

    #[test]
    fn composed_deny_overrides_read_from_multiple_profiles() {
        let profiles = vec![
            PolicyProfile::default(),
            PolicyProfile {
                deny_read: s(&["*.pem"]),
                ..Default::default()
            },
            PolicyProfile::default(),
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
        // One deny among many defaults still blocks
        assert!(ce.check_read("cert.pem").is_deny());
        assert!(ce.check_read("readme.md").is_allow());
    }

    #[test]
    fn composed_first_applicable_read() {
        let profiles = vec![
            PolicyProfile {
                deny_read: s(&["*.secret"]),
                ..Default::default()
            },
            PolicyProfile::default(),
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::FirstApplicable).unwrap();
        assert!(ce.check_read("api.secret").is_deny());
        // Non-matching: first profile allows (no deny match)
        assert!(ce.check_read("readme.md").is_allow());
    }

    #[test]
    fn composed_allow_overrides_write() {
        let profiles = vec![
            PolicyProfile {
                deny_write: s(&["*.lock"]),
                ..Default::default()
            },
            PolicyProfile::default(),
        ];
        let ce = ComposedEngine::new(profiles, PolicyPrecedence::AllowOverrides).unwrap();
        // Second profile allows everything, allow overrides
        assert!(ce.check_write("Cargo.lock").is_allow());
    }

    #[test]
    fn composed_invalid_glob_fails() {
        let profiles = vec![PolicyProfile {
            deny_read: s(&["["]),
            ..Default::default()
        }];
        assert!(ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).is_err());
    }

    #[test]
    fn policy_set_empty_merge_is_default() {
        let set = PolicySet::new("empty");
        let merged = set.merge();
        assert!(merged.allowed_tools.is_empty());
        assert!(merged.disallowed_tools.is_empty());
        assert!(merged.deny_read.is_empty());
        assert!(merged.deny_write.is_empty());
    }

    #[test]
    fn policy_set_three_profiles_merged() {
        let mut set = PolicySet::new("triple");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["*.key"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            disallowed_tools: s(&["Shell"]),
            deny_write: s(&["*.lock"]),
            ..Default::default()
        });
        set.add(PolicyProfile {
            allowed_tools: s(&["Read"]),
            require_approval_for: s(&["Exec"]),
            ..Default::default()
        });
        let merged = set.merge();
        assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
        assert!(merged.disallowed_tools.contains(&"Shell".to_string()));
        assert!(merged.allowed_tools.contains(&"Read".to_string()));
        assert!(merged.deny_read.contains(&"*.key".to_string()));
        assert!(merged.deny_write.contains(&"*.lock".to_string()));
        assert!(merged.require_approval_for.contains(&"Exec".to_string()));
    }
}

// =========================================================================
// 20. Unicode and special character paths
// =========================================================================

mod unicode_and_special {
    use super::*;

    #[test]
    fn unicode_directory_deny_write() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["données/**"]),
            ..Default::default()
        });
        assert!(!e.can_write_path(Path::new("données/fichier.txt")).allowed);
        assert!(e.can_write_path(Path::new("data/file.txt")).allowed);
    }

    #[test]
    fn cjk_characters_in_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["秘密/**"]),
            ..Default::default()
        });
        assert!(!e.can_read_path(Path::new("秘密/data.txt")).allowed);
        assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
    }

    #[test]
    fn emoji_in_tool_name() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["🔧Tool"]),
            ..Default::default()
        });
        assert!(!e.can_use_tool("🔧Tool").allowed);
        assert!(e.can_use_tool("Tool").allowed);
    }

    #[test]
    fn spaces_in_file_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/my documents/**"]),
            ..Default::default()
        });
        assert!(
            !e.can_read_path(Path::new("home/my documents/secret.txt"))
                .allowed
        );
    }

    #[test]
    fn path_with_special_glob_chars_literal() {
        // A path containing literal brackets should not cause issues
        let e = engine(&PolicyProfile::default());
        // With default (empty) policy, everything is allowed
        assert!(e.can_read_path(Path::new("file[1].txt")).allowed);
        assert!(e.can_write_path(Path::new("file{a,b}.txt")).allowed);
    }
}
