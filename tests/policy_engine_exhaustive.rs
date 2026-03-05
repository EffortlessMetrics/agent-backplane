#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::audit::{AuditAction, AuditLog, AuditSummary, PolicyAuditor, PolicyDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision as ComposePolicyDecision, PolicyPrecedence, PolicySet,
    PolicyValidator, WarningKind,
};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// ============================================================================
// Helpers
// ============================================================================

fn p(s: &str) -> String {
    s.to_string()
}

fn ps(ss: &[&str]) -> Vec<String> {
    ss.iter().map(|s| s.to_string()).collect()
}

fn engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("compile policy")
}

fn default_engine() -> PolicyEngine {
    engine(&PolicyProfile::default())
}

// ============================================================================
// 1. PolicyProfile default / construction
// ============================================================================

#[test]
fn profile_default_has_empty_vecs() {
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
fn profile_with_all_fields_populated() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read", "Write"]),
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env"]),
        deny_write: ps(&["**/.git/**"]),
        allow_network: ps(&["*.example.com"]),
        deny_network: ps(&["evil.com"]),
        require_approval_for: ps(&["DeleteFile"]),
    };
    assert_eq!(profile.allowed_tools.len(), 2);
    assert_eq!(profile.disallowed_tools.len(), 1);
    assert_eq!(profile.deny_read.len(), 1);
    assert_eq!(profile.deny_write.len(), 1);
    assert_eq!(profile.allow_network.len(), 1);
    assert_eq!(profile.deny_network.len(), 1);
    assert_eq!(profile.require_approval_for.len(), 1);
}

// ============================================================================
// 2. PolicyEngine compilation
// ============================================================================

#[test]
fn engine_compiles_default_profile() {
    let _e = default_engine();
}

#[test]
fn engine_compiles_with_allowed_tools() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read", "Write", "Grep"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&profile);
}

#[test]
fn engine_compiles_with_disallowed_tools() {
    let profile = PolicyProfile {
        disallowed_tools: ps(&["Bash*", "Shell*"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&profile);
}

#[test]
fn engine_compiles_with_deny_read_patterns() {
    let profile = PolicyProfile {
        deny_read: ps(&["**/.env", "**/.env.*", "**/id_rsa"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&profile);
}

#[test]
fn engine_compiles_with_deny_write_patterns() {
    let profile = PolicyProfile {
        deny_write: ps(&["**/.git/**", "**/node_modules/**"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&profile);
}

#[test]
fn engine_fails_on_invalid_glob_in_allowed_tools() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&profile).is_err());
}

#[test]
fn engine_fails_on_invalid_glob_in_disallowed_tools() {
    let profile = PolicyProfile {
        disallowed_tools: ps(&["[unclosed"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&profile).is_err());
}

#[test]
fn engine_fails_on_invalid_glob_in_deny_read() {
    let profile = PolicyProfile {
        deny_read: ps(&["[bad"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&profile).is_err());
}

#[test]
fn engine_fails_on_invalid_glob_in_deny_write() {
    let profile = PolicyProfile {
        deny_write: ps(&["[bad"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&profile).is_err());
}

// ============================================================================
// 3. Default policy — empty profile allows everything
// ============================================================================

#[test]
fn default_policy_allows_all_tools() {
    let e = default_engine();
    for tool in &["Bash", "Read", "Write", "Grep", "DeleteFile", "Anything"] {
        assert!(e.can_use_tool(tool).allowed, "should allow tool {tool}");
    }
}

#[test]
fn default_policy_allows_all_reads() {
    let e = default_engine();
    for path in &["src/lib.rs", ".env", ".git/config", "secret.key"] {
        assert!(
            e.can_read_path(Path::new(path)).allowed,
            "should allow read {path}"
        );
    }
}

#[test]
fn default_policy_allows_all_writes() {
    let e = default_engine();
    for path in &["src/lib.rs", ".env", ".git/config", "secret.key"] {
        assert!(
            e.can_write_path(Path::new(path)).allowed,
            "should allow write {path}"
        );
    }
}

// ============================================================================
// 4. Tool allow/deny with glob patterns
// ============================================================================

#[test]
fn allowlist_permits_listed_tools() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Read", "Write"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn allowlist_blocks_unlisted_tools() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Read"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("not in allowlist"));
}

#[test]
fn denylist_blocks_listed_tools() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("disallowed"));
}

#[test]
fn denylist_allows_unlisted_tools() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn glob_star_in_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("AnythingGoes").allowed);
}

#[test]
fn glob_prefix_in_denylist() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_suffix_in_denylist() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["*Exec"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("RemoteExec").allowed);
    assert!(e.can_use_tool("Bash").allowed);
}

#[test]
fn glob_question_mark_in_denylist() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bas?"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Base").allowed);
    assert!(e.can_use_tool("Ba").allowed);
    assert!(e.can_use_tool("Basket").allowed);
}

#[test]
fn multiple_denylist_patterns() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash", "Shell", "Exec*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(!e.can_use_tool("ExecRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn multiple_allowlist_patterns() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Read*", "Write*", "Grep"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("WriteFile").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

// ============================================================================
// 5. Priority: deny beats allow (tool rules)
// ============================================================================

#[test]
fn deny_overrides_allow_exact_match() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Bash"]),
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("disallowed"));
}

#[test]
fn deny_star_overrides_allow_star() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["*"]),
        disallowed_tools: ps(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Read").allowed);
}

#[test]
fn deny_specific_overrides_allow_wildcard() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["*"]),
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn deny_glob_overrides_allow_glob() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Shell*"]),
        disallowed_tools: ps(&["Shell*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("ShellExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
}

// ============================================================================
// 6. Read path allow/deny with glob patterns
// ============================================================================

#[test]
fn deny_read_blocks_matching_paths() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/.env")).allowed);
}

#[test]
fn deny_read_allows_non_matching_paths() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_read_path(Path::new("README.md")).allowed);
    assert!(e.can_read_path(Path::new(".env.bak")).allowed);
}

#[test]
fn deny_read_with_extension_glob() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env.*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env.local")).allowed);
}

#[test]
fn deny_read_multiple_patterns() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env", "**/id_rsa", "**/*.key"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.key")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_read_directory_glob() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("secret/file.txt")).allowed);
    assert!(!e.can_read_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/file.txt")).allowed);
}

#[test]
fn deny_read_reason_contains_path() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("config/.env"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

// ============================================================================
// 7. Write path allow/deny with glob patterns
// ============================================================================

#[test]
fn deny_write_blocks_matching_paths() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/config")).allowed);
}

#[test]
fn deny_write_allows_non_matching_paths() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_write_path(Path::new("README.md")).allowed);
}

#[test]
fn deny_write_with_extension_glob() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/*.lock"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("yarn.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn deny_write_multiple_patterns() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**", "**/node_modules/**", "**/*.lock"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/foo/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_write_deep_nested() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["locked/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("locked/a/b/c/d/e.txt")).allowed);
    assert!(e.can_write_path(Path::new("unlocked/file.txt")).allowed);
}

#[test]
fn deny_write_reason_contains_path() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new(".git/config"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

// ============================================================================
// 8. Path traversal in deny rules
// ============================================================================

#[test]
fn deny_read_catches_path_traversal() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/etc/passwd"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed);
}

#[test]
fn deny_write_catches_path_traversal() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("../.git/config"));
    assert!(!d.allowed);
}

// ============================================================================
// 9. Edge cases: empty patterns, wildcard only
// ============================================================================

#[test]
fn wildcard_allowlist_permits_everything() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Anything").allowed);
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn wildcard_denylist_blocks_everything() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn empty_tool_name() {
    let e = default_engine();
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn empty_tool_name_with_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Read"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("").allowed);
}

#[test]
fn empty_path_for_read() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["secret*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("")).allowed);
}

#[test]
fn empty_path_for_write() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["secret*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_write_path(Path::new("")).allowed);
}

#[test]
fn deny_read_catch_all_glob() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("anything.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c.txt")).allowed);
}

#[test]
fn deny_write_catch_all_glob() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("anything.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/c.txt")).allowed);
}

// ============================================================================
// 10. Complex combined policies
// ============================================================================

#[test]
fn combined_tool_and_path_policy() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Read", "Write", "Grep"]),
        disallowed_tools: ps(&["Write"]),
        deny_read: ps(&["**/.env"]),
        deny_write: ps(&["**/locked/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn restrictive_policy_only_read_and_grep() {
    let e = engine(&PolicyProfile {
        allowed_tools: ps(&["Read", "Grep"]),
        deny_write: ps(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_write_path(Path::new("anything.txt")).allowed);
    assert!(e.can_read_path(Path::new("anything.txt")).allowed);
}

#[test]
fn permissive_policy_with_targeted_deny() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env", "**/*.key"]),
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ============================================================================
// 11. Decision type
// ============================================================================

#[test]
fn decision_allow_fields() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_fields() {
    let d = Decision::deny("test reason");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("test reason"));
}

#[test]
fn decision_deny_with_string() {
    let d = Decision::deny(String::from("owned reason"));
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("owned reason"));
}

// ============================================================================
// 12. Serde roundtrip for PolicyProfile
// ============================================================================

#[test]
fn serde_roundtrip_default_profile() {
    let profile = PolicyProfile::default();
    let json = serde_json::to_string(&profile).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert!(back.allowed_tools.is_empty());
    assert!(back.disallowed_tools.is_empty());
    assert!(back.deny_read.is_empty());
    assert!(back.deny_write.is_empty());
}

#[test]
fn serde_roundtrip_populated_profile() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read", "Write"]),
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env"]),
        deny_write: ps(&["**/.git/**"]),
        allow_network: ps(&["*.example.com"]),
        deny_network: ps(&["evil.com"]),
        require_approval_for: ps(&["DeleteFile"]),
    };
    let json = serde_json::to_string_pretty(&profile).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.allowed_tools, profile.allowed_tools);
    assert_eq!(back.disallowed_tools, profile.disallowed_tools);
    assert_eq!(back.deny_read, profile.deny_read);
    assert_eq!(back.deny_write, profile.deny_write);
    assert_eq!(back.allow_network, profile.allow_network);
    assert_eq!(back.deny_network, profile.deny_network);
    assert_eq!(back.require_approval_for, profile.require_approval_for);
}

#[test]
fn serde_roundtrip_engine_still_works() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read"]),
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env"]),
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let json = serde_json::to_string(&profile).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    let e = engine(&back);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
}

#[test]
fn serde_deserialize_from_json_object() {
    let json = r#"{
        "allowed_tools": ["Read"],
        "disallowed_tools": ["Bash"],
        "deny_read": [],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    }"#;
    let profile: PolicyProfile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.allowed_tools, vec!["Read"]);
    assert_eq!(profile.disallowed_tools, vec!["Bash"]);
}

// ============================================================================
// 13. PolicySet merging / inheritance
// ============================================================================

#[test]
fn policy_set_empty() {
    let set = PolicySet::new("test");
    assert_eq!(set.name(), "test");
    let merged = set.merge();
    assert!(merged.allowed_tools.is_empty());
    assert!(merged.disallowed_tools.is_empty());
}

#[test]
fn policy_set_single_profile() {
    let mut set = PolicySet::new("single");
    set.add(PolicyProfile {
        allowed_tools: ps(&["Read"]),
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(merged.allowed_tools, vec!["Read"]);
    assert_eq!(merged.disallowed_tools, vec!["Bash"]);
}

#[test]
fn policy_set_merges_two_profiles() {
    let mut set = PolicySet::new("duo");
    set.add(PolicyProfile {
        allowed_tools: ps(&["Read"]),
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        allowed_tools: ps(&["Write"]),
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(merged.allowed_tools, vec!["Read", "Write"]);
    assert_eq!(merged.deny_read, vec!["**/.env"]);
    assert_eq!(merged.deny_write, vec!["**/.git/**"]);
}

#[test]
fn policy_set_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: ps(&["Bash", "Shell"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: ps(&["Bash", "Exec"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(merged.disallowed_tools, vec!["Bash", "Exec", "Shell"]);
}

#[test]
fn policy_set_merged_engine_works() {
    let mut set = PolicySet::new("engine_test");
    set.add(PolicyProfile {
        allowed_tools: ps(&["Read", "Grep"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: ps(&["Grep"]),
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    let e = engine(&merged);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Grep").allowed); // deny wins
    assert!(!e.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn policy_set_merges_network_rules() {
    let mut set = PolicySet::new("net");
    set.add(PolicyProfile {
        allow_network: ps(&["*.example.com"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        deny_network: ps(&["evil.com"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(merged.allow_network, vec!["*.example.com"]);
    assert_eq!(merged.deny_network, vec!["evil.com"]);
}

#[test]
fn policy_set_merges_require_approval() {
    let mut set = PolicySet::new("approval");
    set.add(PolicyProfile {
        require_approval_for: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        require_approval_for: ps(&["DeleteFile", "Bash"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(merged.require_approval_for, vec!["Bash", "DeleteFile"]);
}

#[test]
fn policy_set_three_profiles_merged() {
    let mut set = PolicySet::new("triple");
    set.add(PolicyProfile {
        allowed_tools: ps(&["A"]),
        deny_read: ps(&["r1"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        allowed_tools: ps(&["B"]),
        deny_read: ps(&["r2"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        allowed_tools: ps(&["C"]),
        deny_write: ps(&["w1"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert_eq!(merged.allowed_tools, vec!["A", "B", "C"]);
    assert_eq!(merged.deny_read, vec!["r1", "r2"]);
    assert_eq!(merged.deny_write, vec!["w1"]);
}

// ============================================================================
// 14. PolicyValidator
// ============================================================================

#[test]
fn validator_no_warnings_for_default() {
    let warnings = PolicyValidator::validate(&PolicyProfile::default());
    assert!(warnings.is_empty());
}

#[test]
fn validator_detects_empty_glob_in_allowed_tools() {
    let profile = PolicyProfile {
        allowed_tools: ps(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_disallowed_tools() {
    let profile = PolicyProfile {
        disallowed_tools: ps(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_deny_read() {
    let profile = PolicyProfile {
        deny_read: ps(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_deny_write() {
    let profile = PolicyProfile {
        deny_write: ps(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_allow_network() {
    let profile = PolicyProfile {
        allow_network: ps(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_deny_network() {
    let profile = PolicyProfile {
        deny_network: ps(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_overlapping_tool_allow_deny() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Bash"]),
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::OverlappingAllowDeny));
}

#[test]
fn validator_detects_overlapping_network_allow_deny() {
    let profile = PolicyProfile {
        allow_network: ps(&["example.com"]),
        deny_network: ps(&["example.com"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::OverlappingAllowDeny));
}

#[test]
fn validator_detects_unreachable_tools_with_wildcard_deny() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read", "Write"]),
        disallowed_tools: ps(&["*"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule));
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.kind == WarningKind::UnreachableRule)
            .count(),
        2
    );
}

#[test]
fn validator_detects_catch_all_deny_read() {
    let profile = PolicyProfile {
        deny_read: ps(&["**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_read")));
}

#[test]
fn validator_detects_catch_all_deny_read_star_slash() {
    let profile = PolicyProfile {
        deny_read: ps(&["**/*"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_read")));
}

#[test]
fn validator_detects_catch_all_deny_write() {
    let profile = PolicyProfile {
        deny_write: ps(&["**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings
        .iter()
        .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_write")));
}

#[test]
fn validator_no_false_positives_for_clean_profile() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read", "Grep"]),
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env"]),
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&profile);
    assert!(warnings.is_empty());
}

// ============================================================================
// 15. ComposedEngine (compose module)
// ============================================================================

#[test]
fn composed_engine_empty_returns_abstain() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_abstain());
    assert!(ce.check_read("file.txt").is_abstain());
    assert!(ce.check_write("file.txt").is_abstain());
}

#[test]
fn composed_engine_deny_overrides_single_deny() {
    let p1 = PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(vec![p1], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_deny_overrides_mixed() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
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
        allowed_tools: ps(&["Read"]),
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile::default();
    let ce = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();
    // permissive allows Bash, so AllowOverrides lets it through
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_first_applicable() {
    let p1 = PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    };
    let p2 = PolicyProfile::default();
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::FirstApplicable).unwrap();
    // First profile denies Bash
    assert!(ce.check_tool("Bash").is_deny());
    // First profile allows Read
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_deny_overrides_read_path() {
    let p1 = PolicyProfile::default();
    let p2 = PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read(".env").is_deny());
    assert!(ce.check_read("src/lib.rs").is_allow());
}

#[test]
fn composed_engine_deny_overrides_write_path() {
    let p1 = PolicyProfile::default();
    let p2 = PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/lib.rs").is_allow());
}

// ============================================================================
// 16. ComposedPolicy (composed module)
// ============================================================================

#[test]
fn composed_policy_empty_allows() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_policy_all_must_allow_single_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    cp.add_policy("restrictive", e);
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_policy_all_must_allow_all_permit() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("p1", default_engine());
    cp.add_policy("p2", default_engine());
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_policy_any_must_allow_one_permits() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    let restrictive = engine(&PolicyProfile {
        allowed_tools: ps(&["Read"]),
        ..PolicyProfile::default()
    });
    cp.add_policy("restrictive", restrictive);
    cp.add_policy("permissive", default_engine());
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_policy_any_must_allow_all_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    let r1 = engine(&PolicyProfile {
        allowed_tools: ps(&["Read"]),
        ..PolicyProfile::default()
    });
    let r2 = engine(&PolicyProfile {
        allowed_tools: ps(&["Write"]),
        ..PolicyProfile::default()
    });
    cp.add_policy("r1", r1);
    cp.add_policy("r2", r2);
    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_policy_first_match() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    let first = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    cp.add_policy("first", first);
    cp.add_policy("second", default_engine());
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_policy_evaluate_read() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    cp.add_policy("secure", e);
    assert!(cp.evaluate_read(".env").is_denied());
    assert!(cp.evaluate_read("src/lib.rs").is_allowed());
}

#[test]
fn composed_policy_evaluate_write() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    cp.add_policy("secure", e);
    assert!(cp.evaluate_write(".git/config").is_denied());
    assert!(cp.evaluate_write("src/lib.rs").is_allowed());
}

#[test]
fn composed_policy_strategy_accessor() {
    let cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    assert_eq!(cp.strategy(), CompositionStrategy::AnyMustAllow);
}

#[test]
fn composed_result_is_allowed_and_denied() {
    let allowed = ComposedResult::Allowed { by: "test".into() };
    let denied = ComposedResult::Denied {
        by: "test".into(),
        reason: "nope".into(),
    };
    assert!(allowed.is_allowed());
    assert!(!allowed.is_denied());
    assert!(denied.is_denied());
    assert!(!denied.is_allowed());
}

// ============================================================================
// 17. PolicyAuditor
// ============================================================================

#[test]
fn auditor_records_tool_allow() {
    let e = default_engine();
    let mut auditor = PolicyAuditor::new(e);
    let d = auditor.check_tool("Read");
    assert!(matches!(d, PolicyDecision::Allow));
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 0);
}

#[test]
fn auditor_records_tool_deny() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    let d = auditor.check_tool("Bash");
    assert!(matches!(d, PolicyDecision::Deny { .. }));
    assert_eq!(auditor.denied_count(), 1);
    assert_eq!(auditor.allowed_count(), 0);
}

#[test]
fn auditor_records_read_allow() {
    let mut auditor = PolicyAuditor::new(default_engine());
    auditor.check_read("src/lib.rs");
    assert_eq!(auditor.allowed_count(), 1);
}

#[test]
fn auditor_records_read_deny() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_read(".env");
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_write_allow() {
    let mut auditor = PolicyAuditor::new(default_engine());
    auditor.check_write("src/lib.rs");
    assert_eq!(auditor.allowed_count(), 1);
}

#[test]
fn auditor_records_write_deny() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_write(".git/config");
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_entries_chronological() {
    let mut auditor = PolicyAuditor::new(default_engine());
    auditor.check_tool("Read");
    auditor.check_read("file.txt");
    auditor.check_write("file.txt");
    let entries = auditor.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].action, "tool");
    assert_eq!(entries[1].action, "read");
    assert_eq!(entries[2].action, "write");
}

#[test]
fn auditor_summary() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_read(".env");
    auditor.check_read("src/lib.rs");
    let summary = auditor.summary();
    assert_eq!(
        summary,
        AuditSummary {
            allowed: 2,
            denied: 2,
            warned: 0,
        }
    );
}

// ============================================================================
// 18. AuditLog
// ============================================================================

#[test]
fn audit_log_new_is_empty() {
    let log = AuditLog::new();
    assert!(log.is_empty());
    assert_eq!(log.len(), 0);
    assert_eq!(log.denied_count(), 0);
}

#[test]
fn audit_log_record_and_entries() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", Some("p1"), None);
    log.record(
        AuditAction::ToolDenied,
        "Bash",
        Some("p1"),
        Some("disallowed"),
    );
    assert_eq!(log.len(), 2);
    assert_eq!(log.denied_count(), 1);
    let entries = log.entries();
    assert_eq!(entries[0].resource, "Read");
    assert_eq!(entries[1].resource, "Bash");
}

#[test]
fn audit_log_filter_by_action() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    log.record(AuditAction::ToolDenied, "Bash", None, None);
    log.record(AuditAction::ReadAllowed, "file.txt", None, None);
    let allowed = log.filter_by_action(&AuditAction::ToolAllowed);
    assert_eq!(allowed.len(), 1);
    assert_eq!(allowed[0].resource, "Read");
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

// ============================================================================
// 19. RuleEngine / rules
// ============================================================================

#[test]
fn rule_engine_empty_allows_everything() {
    let eng = RuleEngine::new();
    assert_eq!(eng.evaluate("anything"), RuleEffect::Allow);
    assert_eq!(eng.rule_count(), 0);
}

#[test]
fn rule_engine_single_deny_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "r1".into(),
        description: "deny bash".into(),
        condition: RuleCondition::Pattern("Bash".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
    assert_eq!(eng.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_priority_wins() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "allow".into(),
        description: "allow all".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    eng.add_rule(Rule {
        id: "deny".into(),
        description: "deny bash".into(),
        condition: RuleCondition::Pattern("Bash".into()),
        effect: RuleEffect::Deny,
        priority: 100,
    });
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
    assert_eq!(eng.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_remove_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "r1".into(),
        description: "deny".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.evaluate("x"), RuleEffect::Deny);
    eng.remove_rule("r1");
    assert_eq!(eng.evaluate("x"), RuleEffect::Allow);
    assert_eq!(eng.rule_count(), 0);
}

#[test]
fn rule_engine_evaluate_all() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "r1".into(),
        description: "always deny".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 1,
    });
    eng.add_rule(Rule {
        id: "r2".into(),
        description: "never matches".into(),
        condition: RuleCondition::Never,
        effect: RuleEffect::Allow,
        priority: 100,
    });
    let results = eng.evaluate_all("test");
    assert_eq!(results.len(), 2);
    assert!(results[0].matched);
    assert!(!results[1].matched);
}

#[test]
fn rule_condition_always() {
    assert!(RuleCondition::Always.matches("anything"));
}

#[test]
fn rule_condition_never() {
    assert!(!RuleCondition::Never.matches("anything"));
}

#[test]
fn rule_condition_pattern() {
    let cond = RuleCondition::Pattern("Bash*".into());
    assert!(cond.matches("Bash"));
    assert!(cond.matches("BashExec"));
    assert!(!cond.matches("Read"));
}

#[test]
fn rule_condition_and() {
    let cond = RuleCondition::And(vec![RuleCondition::Always, RuleCondition::Always]);
    assert!(cond.matches("x"));
    let cond2 = RuleCondition::And(vec![RuleCondition::Always, RuleCondition::Never]);
    assert!(!cond2.matches("x"));
}

#[test]
fn rule_condition_or() {
    let cond = RuleCondition::Or(vec![RuleCondition::Never, RuleCondition::Always]);
    assert!(cond.matches("x"));
    let cond2 = RuleCondition::Or(vec![RuleCondition::Never, RuleCondition::Never]);
    assert!(!cond2.matches("x"));
}

#[test]
fn rule_condition_not() {
    let cond = RuleCondition::Not(Box::new(RuleCondition::Never));
    assert!(cond.matches("x"));
    let cond2 = RuleCondition::Not(Box::new(RuleCondition::Always));
    assert!(!cond2.matches("x"));
}

#[test]
fn rule_condition_nested_complex() {
    // (Pattern("Bash*") AND NOT Never) OR Never => true for "Bash"
    let cond = RuleCondition::Or(vec![
        RuleCondition::And(vec![
            RuleCondition::Pattern("Bash*".into()),
            RuleCondition::Not(Box::new(RuleCondition::Never)),
        ]),
        RuleCondition::Never,
    ]);
    assert!(cond.matches("Bash"));
    assert!(!cond.matches("Read"));
}

#[test]
fn rule_effect_throttle() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "throttle".into(),
        description: "throttle heavy ops".into(),
        condition: RuleCondition::Pattern("Heavy*".into()),
        effect: RuleEffect::Throttle { max: 10 },
        priority: 50,
    });
    assert_eq!(
        eng.evaluate("HeavyCompute"),
        RuleEffect::Throttle { max: 10 }
    );
    assert_eq!(eng.evaluate("LightRead"), RuleEffect::Allow);
}

#[test]
fn rule_effect_log() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "log".into(),
        description: "log file ops".into(),
        condition: RuleCondition::Pattern("File*".into()),
        effect: RuleEffect::Log,
        priority: 5,
    });
    assert_eq!(eng.evaluate("FileRead"), RuleEffect::Log);
}

// ============================================================================
// 20. RateLimitPolicy
// ============================================================================

#[test]
fn rate_limit_unlimited_always_allows() {
    let policy = RateLimitPolicy::unlimited();
    assert!(policy.check_rate_limit(1000, 1_000_000, 100).is_allowed());
}

#[test]
fn rate_limit_rpm_throttles() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        ..RateLimitPolicy::default()
    };
    assert!(policy.check_rate_limit(5, 0, 0).is_allowed());
    assert!(policy.check_rate_limit(10, 0, 0).is_throttled());
    assert!(policy.check_rate_limit(100, 0, 0).is_throttled());
}

#[test]
fn rate_limit_tpm_throttles() {
    let policy = RateLimitPolicy {
        max_tokens_per_minute: Some(1000),
        ..RateLimitPolicy::default()
    };
    assert!(policy.check_rate_limit(0, 500, 0).is_allowed());
    assert!(policy.check_rate_limit(0, 1000, 0).is_throttled());
}

#[test]
fn rate_limit_concurrent_denies() {
    let policy = RateLimitPolicy {
        max_concurrent: Some(5),
        ..RateLimitPolicy::default()
    };
    assert!(policy.check_rate_limit(0, 0, 3).is_allowed());
    assert!(policy.check_rate_limit(0, 0, 5).is_denied());
    assert!(policy.check_rate_limit(0, 0, 10).is_denied());
}

#[test]
fn rate_limit_concurrent_takes_precedence_over_rpm() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(100),
        max_concurrent: Some(2),
        ..RateLimitPolicy::default()
    };
    // concurrent exceeded → denied (not throttled)
    assert!(policy.check_rate_limit(200, 0, 5).is_denied());
}

#[test]
fn rate_limit_result_helpers() {
    assert!(RateLimitResult::Allowed.is_allowed());
    assert!(!RateLimitResult::Allowed.is_throttled());
    assert!(!RateLimitResult::Allowed.is_denied());

    let throttled = RateLimitResult::Throttled {
        retry_after_ms: 100,
    };
    assert!(!throttled.is_allowed());
    assert!(throttled.is_throttled());
    assert!(!throttled.is_denied());

    let denied = RateLimitResult::Denied {
        reason: "nope".into(),
    };
    assert!(!denied.is_allowed());
    assert!(!denied.is_throttled());
    assert!(denied.is_denied());
}

// ============================================================================
// 21. Serde roundtrip for compose types
// ============================================================================

#[test]
fn serde_roundtrip_policy_precedence() {
    for prec in &[
        PolicyPrecedence::DenyOverrides,
        PolicyPrecedence::AllowOverrides,
        PolicyPrecedence::FirstApplicable,
    ] {
        let json = serde_json::to_string(prec).unwrap();
        let back: PolicyPrecedence = serde_json::from_str(&json).unwrap();
        assert_eq!(*prec, back);
    }
}

#[test]
fn serde_roundtrip_compose_policy_decision() {
    let allow = ComposePolicyDecision::Allow {
        reason: "ok".into(),
    };
    let json = serde_json::to_string(&allow).unwrap();
    let back: ComposePolicyDecision = serde_json::from_str(&json).unwrap();
    assert!(back.is_allow());

    let deny = ComposePolicyDecision::Deny {
        reason: "no".into(),
    };
    let json = serde_json::to_string(&deny).unwrap();
    let back: ComposePolicyDecision = serde_json::from_str(&json).unwrap();
    assert!(back.is_deny());

    let abstain = ComposePolicyDecision::Abstain;
    let json = serde_json::to_string(&abstain).unwrap();
    let back: ComposePolicyDecision = serde_json::from_str(&json).unwrap();
    assert!(back.is_abstain());
}

#[test]
fn serde_roundtrip_composition_strategy() {
    for strat in &[
        CompositionStrategy::AllMustAllow,
        CompositionStrategy::AnyMustAllow,
        CompositionStrategy::FirstMatch,
    ] {
        let json = serde_json::to_string(strat).unwrap();
        let back: CompositionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*strat, back);
    }
}

#[test]
fn serde_roundtrip_composed_result() {
    let allowed = ComposedResult::Allowed {
        by: "engine1".into(),
    };
    let json = serde_json::to_string(&allowed).unwrap();
    let back: ComposedResult = serde_json::from_str(&json).unwrap();
    assert!(back.is_allowed());

    let denied = ComposedResult::Denied {
        by: "engine2".into(),
        reason: "blocked".into(),
    };
    let json = serde_json::to_string(&denied).unwrap();
    let back: ComposedResult = serde_json::from_str(&json).unwrap();
    assert!(back.is_denied());
}

#[test]
fn serde_roundtrip_rate_limit_policy() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        max_tokens_per_minute: Some(100_000),
        max_concurrent: Some(10),
    };
    let json = serde_json::to_string(&policy).unwrap();
    let back: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_requests_per_minute, Some(60));
    assert_eq!(back.max_tokens_per_minute, Some(100_000));
    assert_eq!(back.max_concurrent, Some(10));
}

#[test]
fn serde_roundtrip_rate_limit_result() {
    let allowed = RateLimitResult::Allowed;
    let json = serde_json::to_string(&allowed).unwrap();
    let back: RateLimitResult = serde_json::from_str(&json).unwrap();
    assert!(back.is_allowed());

    let throttled = RateLimitResult::Throttled {
        retry_after_ms: 500,
    };
    let json = serde_json::to_string(&throttled).unwrap();
    let back: RateLimitResult = serde_json::from_str(&json).unwrap();
    assert!(back.is_throttled());
}

#[test]
fn serde_roundtrip_rule_condition() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Pattern("Bash*".into()),
        RuleCondition::Not(Box::new(RuleCondition::Never)),
    ]);
    let json = serde_json::to_string(&cond).unwrap();
    let back: RuleCondition = serde_json::from_str(&json).unwrap();
    assert!(back.matches("BashExec"));
    assert!(!back.matches("Read"));
}

#[test]
fn serde_roundtrip_rule_effect() {
    for effect in &[
        RuleEffect::Allow,
        RuleEffect::Deny,
        RuleEffect::Log,
        RuleEffect::Throttle { max: 42 },
    ] {
        let json = serde_json::to_string(effect).unwrap();
        let back: RuleEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(*effect, back);
    }
}

#[test]
fn serde_roundtrip_audit_action() {
    for action in &[
        AuditAction::ToolAllowed,
        AuditAction::ToolDenied,
        AuditAction::ReadAllowed,
        AuditAction::ReadDenied,
        AuditAction::WriteAllowed,
        AuditAction::WriteDenied,
        AuditAction::RateLimited,
    ] {
        let json = serde_json::to_string(action).unwrap();
        let back: AuditAction = serde_json::from_str(&json).unwrap();
        assert_eq!(*action, back);
    }
}

// ============================================================================
// 22. Additional edge cases
// ============================================================================

#[test]
fn unicode_tool_name() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["日本語*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("日本語ツール").allowed);
    assert!(e.can_use_tool("EnglishTool").allowed);
}

#[test]
fn unicode_path_deny_read() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["données/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("données/secret.txt")).allowed);
    assert!(e.can_read_path(Path::new("data/secret.txt")).allowed);
}

#[test]
fn very_long_tool_name() {
    let long_name = "A".repeat(1000);
    let e = default_engine();
    assert!(e.can_use_tool(&long_name).allowed);
}

#[test]
fn very_long_path() {
    let long_path = format!("{}/file.txt", "a/b/c/d/e".repeat(100));
    let e = default_engine();
    assert!(e.can_read_path(Path::new(&long_path)).allowed);
    assert!(e.can_write_path(Path::new(&long_path)).allowed);
}

#[test]
fn tool_name_with_special_characters() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["tool-with-dashes"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("tool-with-dashes").allowed);
    assert!(e.can_use_tool("tool_with_underscores").allowed);
}

#[test]
fn path_with_spaces() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/My Documents/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("My Documents/file.txt")).allowed);
    assert!(e.can_read_path(Path::new("MyDocuments/file.txt")).allowed);
}

#[test]
fn multiple_engines_different_configs() {
    let e1 = engine(&PolicyProfile {
        allowed_tools: ps(&["Read"]),
        ..PolicyProfile::default()
    });
    let e2 = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    // e1 only allows Read
    assert!(e1.can_use_tool("Read").allowed);
    assert!(!e1.can_use_tool("Bash").allowed);
    // e2 denies Bash but allows everything else
    assert!(e2.can_use_tool("Read").allowed);
    assert!(!e2.can_use_tool("Bash").allowed);
    assert!(e2.can_use_tool("Write").allowed);
}

#[test]
fn deny_read_and_write_same_pattern() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.secret/**"]),
        deny_write: ps(&["**/.secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".secret/key")).allowed);
    assert!(!e.can_write_path(Path::new(".secret/key")).allowed);
    assert!(e.can_read_path(Path::new("public/file")).allowed);
    assert!(e.can_write_path(Path::new("public/file")).allowed);
}

#[test]
fn deny_read_does_not_affect_write() {
    let e = engine(&PolicyProfile {
        deny_read: ps(&["**/.env"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_write_path(Path::new(".env")).allowed);
}

#[test]
fn deny_write_does_not_affect_read() {
    let e = engine(&PolicyProfile {
        deny_write: ps(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_read_path(Path::new(".git/config")).allowed);
}

#[test]
fn policy_clone() {
    let profile = PolicyProfile {
        allowed_tools: ps(&["Read"]),
        disallowed_tools: ps(&["Bash"]),
        deny_read: ps(&["**/.env"]),
        deny_write: ps(&["**/.git/**"]),
        allow_network: ps(&["*.example.com"]),
        deny_network: ps(&["evil.com"]),
        require_approval_for: ps(&["DeleteFile"]),
    };
    let cloned = profile.clone();
    assert_eq!(cloned.allowed_tools, profile.allowed_tools);
    assert_eq!(cloned.disallowed_tools, profile.disallowed_tools);
}

#[test]
fn engine_clone() {
    let e = engine(&PolicyProfile {
        disallowed_tools: ps(&["Bash"]),
        ..PolicyProfile::default()
    });
    let cloned = e.clone();
    assert!(!cloned.can_use_tool("Bash").allowed);
    assert!(cloned.can_use_tool("Read").allowed);
}

#[test]
fn decision_debug_format() {
    let d = Decision::allow();
    let debug = format!("{:?}", d);
    assert!(debug.contains("allowed"));
}

#[test]
fn policy_engine_debug_format() {
    let e = default_engine();
    let debug = format!("{:?}", e);
    assert!(debug.contains("PolicyEngine"));
}
