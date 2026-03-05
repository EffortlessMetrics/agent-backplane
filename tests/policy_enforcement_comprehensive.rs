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
//! Comprehensive tests for the policy engine enforcement system.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::audit::{AuditSummary, PolicyAuditor, PolicyDecision as AuditDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision as ComposeDecision, PolicyPrecedence, PolicySet,
    PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn s(v: &str) -> String {
    v.to_string()
}

fn sv(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| s(x)).collect()
}

fn engine(p: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(p).expect("compile policy")
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. PolicyProfile construction
// ═══════════════════════════════════════════════════════════════════════════

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
fn profile_with_all_fields() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.example.com"]),
        require_approval_for: sv(&["DeleteFile"]),
    };
    assert_eq!(p.allowed_tools.len(), 2);
    assert_eq!(p.disallowed_tools, vec!["Bash"]);
    assert_eq!(p.deny_read, vec!["**/.env"]);
    assert_eq!(p.deny_write, vec!["**/.git/**"]);
    assert_eq!(p.allow_network, vec!["*.example.com"]);
    assert_eq!(p.deny_network, vec!["evil.example.com"]);
    assert_eq!(p.require_approval_for, vec!["DeleteFile"]);
}

#[test]
fn profile_clone_is_independent() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    };
    let mut q = p.clone();
    q.allowed_tools.push(s("Write"));
    assert_eq!(p.allowed_tools.len(), 1);
    assert_eq!(q.allowed_tools.len(), 2);
}

#[test]
fn profile_debug_impl() {
    let p = PolicyProfile::default();
    let dbg = format!("{p:?}");
    assert!(dbg.contains("PolicyProfile"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. PolicyEngine::new() — compilation success and failure
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn compile_empty_profile_succeeds() {
    PolicyEngine::new(&PolicyProfile::default()).unwrap();
}

#[test]
fn compile_with_valid_globs_succeeds() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read*", "Write?"]),
        disallowed_tools: sv(&["Bash*"]),
        deny_read: sv(&["**/.env", "*.secret"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    PolicyEngine::new(&p).unwrap();
}

#[test]
fn compile_invalid_tool_glob_fails() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["["]),
        ..PolicyProfile::default()
    };
    let err = PolicyEngine::new(&p);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("compile tool policy"));
}

#[test]
fn compile_invalid_deny_read_glob_fails() {
    let p = PolicyProfile {
        deny_read: sv(&["["]),
        ..PolicyProfile::default()
    };
    let err = PolicyEngine::new(&p);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("compile deny_read"));
}

#[test]
fn compile_invalid_deny_write_glob_fails() {
    let p = PolicyProfile {
        deny_write: sv(&["["]),
        ..PolicyProfile::default()
    };
    let err = PolicyEngine::new(&p);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("compile deny_write"));
}

#[test]
fn compile_invalid_allowed_tools_glob_fails() {
    let p = PolicyProfile {
        allowed_tools: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn compile_multiple_valid_patterns() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash*", "Shell*", "Exec*"]),
        deny_read: sv(&["**/.env", "**/.secret", "**/id_rsa"]),
        deny_write: sv(&["**/.git/**", "**/node_modules/**"]),
        ..PolicyProfile::default()
    };
    PolicyEngine::new(&p).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. can_use_tool() — tool checking with glob patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_no_rules_allows_all() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("AnyTool").allowed);
}

#[test]
fn tool_exact_disallow() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_glob_star_disallow() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_glob_question_mark_disallow() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bas?"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Bass").allowed);
    // "Ba" doesn't match "Bas?" — too short
    assert!(e.can_use_tool("Ba").allowed);
}

#[test]
fn tool_allowlist_only() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_allowlist_with_glob() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_deny_overrides_allow() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn tool_deny_overrides_specific_allow() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Bash", "Read"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_denied_has_reason() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
}

#[test]
fn tool_not_in_allowlist_has_reason() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_use_tool("Write");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Write' not in allowlist"));
}

#[test]
fn tool_allowed_has_no_reason() {
    let e = engine(&PolicyProfile::default());
    let d = e.can_use_tool("Read");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn tool_multiple_disallow_patterns() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash*", "Shell*", "Exec*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCmd").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
}

#[test]
fn tool_case_sensitivity() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("bash").allowed);
    // Glob matching is case-sensitive by default
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("BASH").allowed);
}

#[test]
fn tool_empty_string_name() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn tool_special_characters_in_name() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["my-tool"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("my-tool").allowed);
    assert!(e.can_use_tool("my_tool").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. can_read_path() — read path matching
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn read_no_deny_allows_all() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn read_deny_exact_file() {
    let p = PolicyProfile {
        deny_read: sv(&["secret.txt"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(e.can_read_path(Path::new("public.txt")).allowed);
}

#[test]
fn read_deny_glob_star() {
    let p = PolicyProfile {
        deny_read: sv(&["*.secret"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("db.secret")).allowed);
    assert!(!e.can_read_path(Path::new("api.secret")).allowed);
    assert!(e.can_read_path(Path::new("api.txt")).allowed);
}

#[test]
fn read_deny_double_star_recursive() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/.env")).allowed);
    assert!(e.can_read_path(Path::new(".env.local")).allowed);
}

#[test]
fn read_deny_directory_recursive() {
    let p = PolicyProfile {
        deny_read: sv(&["secret/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secret/file.txt")).allowed);
    assert!(!e.can_read_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/file.txt")).allowed);
}

#[test]
fn read_deny_multiple_patterns() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*", "**/id_rsa"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_denied_has_reason() {
    let p = PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("read denied"));
}

#[test]
fn read_allowed_has_no_reason() {
    let p = PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_read_path(Path::new("public.txt"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn read_path_traversal() {
    let p = PolicyProfile {
        deny_read: sv(&["**/etc/passwd"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("../../etc/passwd")).allowed);
}

#[test]
fn read_empty_path() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. can_write_path() — write path matching
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn write_no_deny_allows_all() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn write_deny_exact_file() {
    let p = PolicyProfile {
        deny_write: sv(&["locked.txt"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("locked.txt")).allowed);
    assert!(e.can_write_path(Path::new("unlocked.txt")).allowed);
}

#[test]
fn write_deny_glob_star() {
    let p = PolicyProfile {
        deny_write: sv(&["*.lock"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("package.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn write_deny_git_directory() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/config")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_deny_deep_nested() {
    let p = PolicyProfile {
        deny_write: sv(&["secret/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(!e.can_write_path(Path::new("secret/x.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn write_deny_multiple_patterns() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**", "**/node_modules/**", "*.lock"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("yarn.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_denied_has_reason() {
    let p = PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_write_path(Path::new("locked.md"));
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("write denied"));
}

#[test]
fn write_path_traversal() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("../.git/config")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Combined policy checks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn combined_tool_and_path_restrictions() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        disallowed_tools: sv(&["Write"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/locked/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);

    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn combined_read_write_different_globs() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.secret"]),
        deny_write: sv(&["**/.readonly"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);

    // Read deny doesn't affect write
    assert!(e.can_write_path(Path::new("data/.secret")).allowed);
    // Write deny doesn't affect read
    assert!(e.can_read_path(Path::new("data/.readonly")).allowed);
    // Both denied for their respective ops
    assert!(!e.can_read_path(Path::new("data/.secret")).allowed);
    assert!(!e.can_write_path(Path::new("data/.readonly")).allowed);
}

#[test]
fn combined_allowlist_with_deny_paths() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        deny_read: sv(&["private/**"]),
        deny_write: sv(&["**/*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);

    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_read_path(Path::new("private/data.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
    assert!(!e.can_write_path(Path::new("any.txt")).allowed);
}

#[test]
fn combined_full_policy() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read*", "Grep*", "List*"]),
        disallowed_tools: sv(&["ReadSecret*"]),
        deny_read: sv(&["**/.env", "**/.env.*", "**/id_rsa", "private/**"]),
        deny_write: sv(&["**/.git/**", "**/node_modules/**", "dist/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.example.com"]),
        require_approval_for: sv(&["DeleteFile", "ExecCommand"]),
    };
    let e = engine(&p);

    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(e.can_use_tool("GrepCode").allowed);
    assert!(!e.can_use_tool("ReadSecretFile").allowed);
    assert!(!e.can_use_tool("Bash").allowed);

    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("private/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);

    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_default_profile() {
    let p = PolicyProfile::default();
    let json = serde_json::to_string(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert!(p2.allowed_tools.is_empty());
    assert!(p2.disallowed_tools.is_empty());
    assert!(p2.deny_read.is_empty());
    assert!(p2.deny_write.is_empty());
}

#[test]
fn serde_roundtrip_full_profile() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.example.com"]),
        require_approval_for: sv(&["DeleteFile"]),
    };
    let json = serde_json::to_string_pretty(&p).unwrap();
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
fn serde_roundtrip_decision_allow() {
    let d = Decision::allow();
    let json = serde_json::to_string(&d).unwrap();
    let d2: Decision = serde_json::from_str(&json).unwrap();
    assert!(d2.allowed);
    assert!(d2.reason.is_none());
}

#[test]
fn serde_roundtrip_decision_deny() {
    let d = Decision::deny("blocked");
    let json = serde_json::to_string(&d).unwrap();
    let d2: Decision = serde_json::from_str(&json).unwrap();
    assert!(!d2.allowed);
    assert_eq!(d2.reason.as_deref(), Some("blocked"));
}

#[test]
fn serde_profile_json_fields() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    };
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"allowed_tools\""));
    assert!(json.contains("\"disallowed_tools\""));
    assert!(json.contains("\"deny_read\""));
    assert!(json.contains("\"deny_write\""));
}

#[test]
fn serde_profile_from_json_string() {
    let json = r#"{
        "allowed_tools": ["Read"],
        "disallowed_tools": ["Bash"],
        "deny_read": ["**/.env"],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    }"#;
    let p: PolicyProfile = serde_json::from_str(json).unwrap();
    assert_eq!(p.allowed_tools, vec!["Read"]);
    assert_eq!(p.disallowed_tools, vec!["Bash"]);
    assert_eq!(p.deny_read, vec!["**/.env"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_wildcard_only_allowlist() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("AnyToolName").allowed);
}

#[test]
fn edge_wildcard_only_denylist() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("").allowed);
}

#[test]
fn edge_deny_all_reads() {
    let p = PolicyProfile {
        deny_read: sv(&["**/*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("any.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c.txt")).allowed);
}

#[test]
fn edge_deny_all_writes() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("any.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/c.txt")).allowed);
}

#[test]
fn edge_allow_all_policy() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("AnyTool").allowed);
    assert!(e.can_read_path(Path::new("any/path.txt")).allowed);
    assert!(e.can_write_path(Path::new("any/path.txt")).allowed);
}

#[test]
fn edge_deny_everything() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["*"]),
        deny_read: sv(&["**/*"]),
        deny_write: sv(&["**/*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new("a.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a.txt")).allowed);
}

#[test]
fn edge_very_long_tool_name() {
    let long_name = "A".repeat(1000);
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool(&long_name).allowed);
}

#[test]
fn edge_very_deep_path() {
    let deep_path = "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q.txt";
    let p = PolicyProfile {
        deny_read: sv(&["a/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(deep_path)).allowed);
}

#[test]
fn edge_tool_with_spaces() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["My Tool"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("My Tool").allowed);
    assert!(e.can_use_tool("MyTool").allowed);
}

#[test]
fn edge_unicode_in_path() {
    let p = PolicyProfile {
        deny_read: sv(&["données/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("données/fichier.rs")).allowed);
    assert!(e.can_read_path(Path::new("data/file.rs")).allowed);
}

#[test]
fn edge_extension_glob() {
    let p = PolicyProfile {
        deny_write: sv(&["*.{lock,bak,tmp}"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("data.bak")).allowed);
    assert!(!e.can_write_path(Path::new("scratch.tmp")).allowed);
    assert!(e.can_write_path(Path::new("main.rs")).allowed);
}

#[test]
fn edge_dot_files_deny() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("sub/.hidden")).allowed);
    assert!(e.can_read_path(Path::new("visible.txt")).allowed);
}

#[test]
fn edge_duplicate_patterns() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash", "Bash", "Bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn edge_overlapping_allow_deny_same_tool() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    // Deny always wins
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Decision constructors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decision_allow_constructor() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_constructor() {
    let d = Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

#[test]
fn decision_deny_from_string() {
    let d = Decision::deny(String::from("not allowed"));
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("not allowed"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. PolicyEngine clone
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn engine_clone_works_independently() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e1 = engine(&p);
    let e2 = e1.clone();
    assert!(!e1.can_use_tool("Bash").allowed);
    assert!(!e2.can_use_tool("Bash").allowed);
    assert!(e1.can_use_tool("Read").allowed);
    assert!(e2.can_use_tool("Read").allowed);
}

#[test]
fn engine_debug_impl() {
    let e = engine(&PolicyProfile::default());
    let dbg = format!("{e:?}");
    assert!(dbg.contains("PolicyEngine"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. PolicyAuditor
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auditor_records_tool_decisions() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

    let d1 = auditor.check_tool("Read");
    assert!(matches!(d1, AuditDecision::Allow));
    let d2 = auditor.check_tool("Bash");
    assert!(matches!(d2, AuditDecision::Deny { .. }));

    assert_eq!(auditor.entries().len(), 2);
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_read_decisions() {
    let p = PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

    let d1 = auditor.check_read("public.txt");
    assert!(matches!(d1, AuditDecision::Allow));
    let d2 = auditor.check_read("secret.key");
    assert!(matches!(d2, AuditDecision::Deny { .. }));

    assert_eq!(auditor.entries().len(), 2);
}

#[test]
fn auditor_records_write_decisions() {
    let p = PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

    auditor.check_write("open.txt");
    auditor.check_write("locked.md");

    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_summary() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_read("public.txt");
    auditor.check_read("secret.txt");

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

#[test]
fn auditor_empty_summary() {
    let e = engine(&PolicyProfile::default());
    let auditor = PolicyAuditor::new(e);
    let summary = auditor.summary();
    assert_eq!(
        summary,
        AuditSummary {
            allowed: 0,
            denied: 0,
            warned: 0,
        }
    );
}

#[test]
fn auditor_entry_has_timestamp() {
    let e = engine(&PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    let entry = &auditor.entries()[0];
    assert_eq!(entry.action, "tool");
    assert_eq!(entry.resource, "Read");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. ComposedEngine & PolicySet
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_set_merge_unions_all_fields() {
    let mut set = PolicySet::new("test");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret/**"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Shell"]),
        deny_write: sv(&["locked/**"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Shell")));
    assert!(merged.deny_read.contains(&s("secret/**")));
    assert!(merged.deny_write.contains(&s("locked/**")));
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
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
    let set = PolicySet::new("my-policies");
    assert_eq!(set.name(), "my-policies");
}

#[test]
fn composed_engine_deny_overrides() {
    let policies = vec![
        PolicyProfile {
            allowed_tools: sv(&["*"]),
            ..PolicyProfile::default()
        },
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        },
    ];
    let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
    let d = ce.check_tool("Bash");
    assert!(d.is_deny());
}

#[test]
fn composed_engine_allow_overrides() {
    let policies = vec![
        PolicyProfile {
            allowed_tools: sv(&["*"]),
            ..PolicyProfile::default()
        },
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        },
    ];
    let ce = ComposedEngine::new(policies, PolicyPrecedence::AllowOverrides).unwrap();
    let d = ce.check_tool("Bash");
    assert!(d.is_allow());
}

#[test]
fn composed_engine_first_applicable() {
    let policies = vec![
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        },
        PolicyProfile {
            allowed_tools: sv(&["*"]),
            ..PolicyProfile::default()
        },
    ];
    let ce = ComposedEngine::new(policies, PolicyPrecedence::FirstApplicable).unwrap();
    let d = ce.check_tool("Bash");
    assert!(d.is_deny());
}

#[test]
fn composed_engine_empty_policies_abstains() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    let d = ce.check_tool("Bash");
    assert!(d.is_abstain());
}

#[test]
fn composed_engine_check_read() {
    let policies = vec![PolicyProfile {
        deny_read: sv(&["secret/**"]),
        ..PolicyProfile::default()
    }];
    let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read("secret/key.pem").is_deny());
    assert!(ce.check_read("public/data.txt").is_allow());
}

#[test]
fn composed_engine_check_write() {
    let policies = vec![PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    }];
    let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/lib.rs").is_allow());
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. PolicyValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validator_no_warnings_for_clean_policy() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

#[test]
fn validator_detects_empty_glob() {
    let p = PolicyProfile {
        allowed_tools: sv(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_overlapping_tool_allow_deny() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Bash"]),
        disallowed_tools: sv(&["Bash"]),
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
fn validator_detects_wildcard_deny_unreachable() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        disallowed_tools: sv(&["*"]),
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
fn validator_detects_deny_read_catch_all() {
    let p = PolicyProfile {
        deny_read: sv(&["**"]),
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
fn validator_detects_deny_write_catch_all() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*"]),
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
fn validator_warns_empty_in_multiple_lists() {
    let p = PolicyProfile {
        allowed_tools: sv(&[""]),
        disallowed_tools: sv(&[""]),
        deny_read: sv(&[""]),
        deny_write: sv(&[""]),
        allow_network: sv(&[""]),
        deny_network: sv(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    let empty_count = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::EmptyGlob)
        .count();
    assert_eq!(empty_count, 6);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. RuleEngine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rule_engine_empty_allows_all() {
    let eng = RuleEngine::new();
    assert_eq!(eng.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_single_deny() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny bash"),
        condition: RuleCondition::Pattern(s("Bash*")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(eng.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_priority_wins() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("allow-all"),
        description: s("Allow everything"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    eng.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
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
fn rule_condition_and() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Always,
        RuleCondition::Pattern(s("*.rs")),
    ]);
    assert!(cond.matches("main.rs"));
    assert!(!cond.matches("main.py"));
}

#[test]
fn rule_condition_or() {
    let cond = RuleCondition::Or(vec![
        RuleCondition::Pattern(s("*.rs")),
        RuleCondition::Pattern(s("*.py")),
    ]);
    assert!(cond.matches("main.rs"));
    assert!(cond.matches("main.py"));
    assert!(!cond.matches("main.js"));
}

#[test]
fn rule_condition_not() {
    let cond = RuleCondition::Not(Box::new(RuleCondition::Pattern(s("*.rs"))));
    assert!(!cond.matches("main.rs"));
    assert!(cond.matches("main.py"));
}

#[test]
fn rule_engine_evaluate_all() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("r1"),
        description: s("rule 1"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    eng.add_rule(Rule {
        id: s("r2"),
        description: s("rule 2"),
        condition: RuleCondition::Never,
        effect: RuleEffect::Deny,
        priority: 2,
    });
    let results = eng.evaluate_all("test");
    assert_eq!(results.len(), 2);
    assert!(results[0].matched);
    assert!(!results[1].matched);
}

#[test]
fn rule_engine_remove_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("r1"),
        description: s("temp"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 1,
    });
    assert_eq!(eng.rule_count(), 1);
    eng.remove_rule("r1");
    assert_eq!(eng.rule_count(), 0);
}

#[test]
fn rule_engine_throttle_effect() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("throttle"),
        description: s("Rate limit"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Throttle { max: 5 },
        priority: 1,
    });
    assert_eq!(eng.evaluate("anything"), RuleEffect::Throttle { max: 5 });
}

#[test]
fn rule_engine_log_effect() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("log"),
        description: s("Log usage"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Log,
        priority: 1,
    });
    assert_eq!(eng.evaluate("anything"), RuleEffect::Log);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Additional edge cases and pattern variations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_network_fields_preserved() {
    let p = PolicyProfile {
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.example.com"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&p);
    assert_eq!(p.allow_network, vec!["*.example.com"]);
    assert_eq!(p.deny_network, vec!["evil.example.com"]);
}

#[test]
fn edge_require_approval_preserved() {
    let p = PolicyProfile {
        require_approval_for: sv(&["Bash", "DeleteFile"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&p);
    assert_eq!(p.require_approval_for, vec!["Bash", "DeleteFile"]);
}

#[test]
fn edge_many_deny_patterns() {
    let patterns: Vec<String> = (0..50).map(|i| format!("pattern{i}*")).collect();
    let p = PolicyProfile {
        disallowed_tools: patterns,
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("pattern0tool").allowed);
    assert!(!e.can_use_tool("pattern49tool").allowed);
    assert!(e.can_use_tool("other").allowed);
}

#[test]
fn edge_read_write_independent() {
    let p = PolicyProfile {
        deny_read: sv(&["a.txt"]),
        deny_write: sv(&["b.txt"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("a.txt")).allowed);
    assert!(e.can_write_path(Path::new("a.txt")).allowed);
    assert!(e.can_read_path(Path::new("b.txt")).allowed);
    assert!(!e.can_write_path(Path::new("b.txt")).allowed);
}

#[test]
fn edge_single_char_tool_name() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["X"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("X").allowed);
    assert!(e.can_use_tool("Y").allowed);
}

#[test]
fn edge_path_with_dots() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.hidden/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".hidden/file.txt")).allowed);
    assert!(e.can_read_path(Path::new("visible/file.txt")).allowed);
}

#[test]
fn edge_composed_serde() {
    let d = ComposeDecision::Allow { reason: s("ok") };
    let json = serde_json::to_string(&d).unwrap();
    let d2: ComposeDecision = serde_json::from_str(&json).unwrap();
    assert!(d2.is_allow());
}

#[test]
fn edge_audit_decision_serde() {
    let d = AuditDecision::Deny {
        reason: s("blocked"),
    };
    let json = serde_json::to_string(&d).unwrap();
    let d2: AuditDecision = serde_json::from_str(&json).unwrap();
    assert!(matches!(d2, AuditDecision::Deny { .. }));
}

#[test]
fn edge_rule_condition_nested_and_or() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Or(vec![
            RuleCondition::Pattern(s("*.rs")),
            RuleCondition::Pattern(s("*.py")),
        ]),
        RuleCondition::Not(Box::new(RuleCondition::Pattern(s("test*")))),
    ]);
    assert!(cond.matches("main.rs"));
    assert!(!cond.matches("test_main.rs"));
    assert!(cond.matches("app.py"));
}
