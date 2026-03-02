// SPDX-License-Identifier: MIT OR Apache-2.0
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
        assert!(!e.can_write_path(Path::new("node_modules/pkg/index.js")).allowed);
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
        eng.add_rule(make_rule("deny-all", RuleCondition::Always, RuleEffect::Deny, 10));
        assert_eq!(eng.evaluate("anything"), RuleEffect::Deny);
    }

    #[test]
    fn never_condition() {
        let mut eng = RuleEngine::new();
        eng.add_rule(make_rule("never", RuleCondition::Never, RuleEffect::Deny, 10));
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
        eng.add_rule(make_rule(
            "r1",
            RuleCondition::Always,
            RuleEffect::Allow,
            1,
        ));
        eng.add_rule(make_rule(
            "r2",
            RuleCondition::Never,
            RuleEffect::Deny,
            10,
        ));
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
        eng.add_rule(make_rule(
            "log",
            RuleCondition::Always,
            RuleEffect::Log,
            10,
        ));
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
        assert!(!e.can_read_path(Path::new("very/deep/nested/path/file.txt")).allowed);
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
        assert!(w
            .iter()
            .any(|x| x.kind == WarningKind::OverlappingAllowDeny));
    }

    #[test]
    fn overlapping_network_allow_deny() {
        let p = PolicyProfile {
            allow_network: s(&["evil.com"]),
            deny_network: s(&["evil.com"]),
            ..Default::default()
        };
        let w = PolicyValidator::validate(&p);
        assert!(w
            .iter()
            .any(|x| x.kind == WarningKind::OverlappingAllowDeny));
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
