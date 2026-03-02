// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the ABP policy engine.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::audit::{PolicyAuditor, PolicyDecision as AuditDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

// ───────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────

fn s(v: &str) -> String {
    v.to_string()
}

fn sv(vs: &[&str]) -> Vec<String> {
    vs.iter().map(|v| s(v)).collect()
}

fn engine(p: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(p).expect("compile policy")
}

// ===================================================================
// 1. Default policy allows everything
// ===================================================================

#[test]
fn default_policy_allows_any_tool() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Execute").allowed);
    assert!(e.can_use_tool("DeleteFile").allowed);
}

#[test]
fn default_policy_allows_any_read_path() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("/etc/passwd")).allowed);
    assert!(
        e.can_read_path(Path::new("deeply/nested/dir/file.txt"))
            .allowed
    );
}

#[test]
fn default_policy_allows_any_write_path() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_write_path(Path::new("a/b/c/d/e.txt")).allowed);
}

#[test]
fn default_policy_decision_has_no_reason() {
    let e = engine(&PolicyProfile::default());
    let d = e.can_use_tool("Anything");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

// ===================================================================
// 2. Tool allow/deny patterns
// ===================================================================

#[test]
fn disallowed_tool_is_denied() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn multiple_disallowed_tools() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash", "Execute", "DeleteFile"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
    assert!(!e.can_use_tool("DeleteFile").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn allowed_tools_whitelist_blocks_unlisted() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_deny_reason_message() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&p);
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
}

#[test]
fn tool_missing_from_allowlist_reason_message() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..Default::default()
    };
    let e = engine(&p);
    let d = e.can_use_tool("Write");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Write' not in allowlist"));
}

#[test]
fn wildcard_tool_allowlist_permits_all() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("AnyRandomTool").allowed);
}

#[test]
fn glob_pattern_in_tool_denylist() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash*"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// ===================================================================
// 3. Read path allow/deny patterns
// ===================================================================

#[test]
fn deny_read_blocks_matching_paths() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_read_multiple_patterns() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env", "**/id_rsa", "**/*.key"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".ssh/id_rsa")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.key")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_read_decision_contains_path() {
    let p = PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..Default::default()
    };
    let e = engine(&p);
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("secret.txt"));
}

#[test]
fn deny_read_directory_glob() {
    let p = PolicyProfile {
        deny_read: sv(&["private/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("private/data.txt")).allowed);
    assert!(
        !e.can_read_path(Path::new("private/deep/nested/file.rs"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
}

// ===================================================================
// 4. Write path allow/deny patterns
// ===================================================================

#[test]
fn deny_write_blocks_matching_paths() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/hooks/pre-commit")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_write_multiple_patterns() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**", "**/node_modules/**", "dist/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(e.can_write_path(Path::new("src/app.ts")).allowed);
}

#[test]
fn deny_write_decision_contains_path() {
    let p = PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..Default::default()
    };
    let e = engine(&p);
    let d = e.can_write_path(Path::new("locked.md"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("locked.md"));
}

#[test]
fn deny_write_deep_nested() {
    let p = PolicyProfile {
        deny_write: sv(&["vault/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("vault/a/b/c/d/e.txt")).allowed);
    assert!(e.can_write_path(Path::new("data/file.txt")).allowed);
}

// ===================================================================
// 5. Combined policies (tool + read + write)
// ===================================================================

#[test]
fn combined_tool_read_write_restrictions() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        disallowed_tools: sv(&["Write"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/locked/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed); // deny overrides allow
    assert!(!e.can_use_tool("Bash").allowed); // not in allowlist
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn combined_all_restrictions_active() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash", "Execute"]),
        deny_read: sv(&["**/.env", "**/secret/**"]),
        deny_write: sv(&["**/.git/**", "**/dist/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(!e.can_write_path(Path::new("dist/output.js")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn combined_independent_dimensions() {
    // Tool denied doesn't affect path decisions
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret.txt"]),
        deny_write: sv(&["locked.txt"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_read_path(Path::new("public.txt")).allowed);
    assert!(e.can_write_path(Path::new("public.txt")).allowed);
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_write_path(Path::new("locked.txt")).allowed);
}

// ===================================================================
// 6. Policy precedence (deny overrides allow)
// ===================================================================

#[test]
fn deny_overrides_allow_in_tools() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn deny_overrides_allow_specific_tool_in_both_lists() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Bash", "Read"]),
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn deny_overrides_allow_with_glob_patterns() {
    let p = PolicyProfile {
        allowed_tools: sv(&["File*"]),
        disallowed_tools: sv(&["FileDelete"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileDelete").allowed);
}

// ===================================================================
// 7. Glob pattern matching edge cases
// ===================================================================

#[test]
fn double_star_matches_any_depth() {
    let p = PolicyProfile {
        deny_read: sv(&["**/*.secret"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("a.secret")).allowed);
    assert!(!e.can_read_path(Path::new("a/b.secret")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/d.secret")).allowed);
    assert!(e.can_read_path(Path::new("a.txt")).allowed);
}

#[test]
fn single_star_matches_within_segment() {
    let p = PolicyProfile {
        deny_write: sv(&["*.log"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("app.log")).allowed);
    // globset default: * crosses path separators (literal_separator = false)
    assert!(!e.can_write_path(Path::new("logs/app.log")).allowed);
    assert!(e.can_write_path(Path::new("app.txt")).allowed);
}

#[test]
fn question_mark_matches_single_char() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Tool?"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("ToolA").allowed);
    assert!(!e.can_use_tool("ToolX").allowed);
    // Two chars after "Tool" won't match single ?
    assert!(e.can_use_tool("ToolAB").allowed);
    assert!(e.can_use_tool("Tool").allowed);
}

#[test]
fn braces_glob_matching() {
    let p = PolicyProfile {
        deny_read: sv(&["*.{key,pem,crt}"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
    assert!(!e.can_read_path(Path::new("ca.crt")).allowed);
    assert!(e.can_read_path(Path::new("readme.txt")).allowed);
}

#[test]
fn double_star_directory_prefix() {
    let p = PolicyProfile {
        deny_write: sv(&["**/build/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("build/out.js")).allowed);
    assert!(!e.can_write_path(Path::new("project/build/out.js")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/build/c/d.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_star_only_pattern() {
    let p = PolicyProfile {
        deny_read: sv(&["*"]),
        ..Default::default()
    };
    let e = engine(&p);
    // * in globset matches everything (literal_separator = false)
    assert!(!e.can_read_path(Path::new("anything.txt")).allowed);
    assert!(!e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn glob_double_star_only_pattern() {
    let p = PolicyProfile {
        deny_write: sv(&["**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("any/path/at/all.txt")).allowed);
    assert!(!e.can_write_path(Path::new("file.txt")).allowed);
}

// ===================================================================
// 8. Policy serialization/deserialization
// ===================================================================

#[test]
fn policy_profile_serde_roundtrip_json() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.com"]),
        require_approval_for: sv(&["DeleteFile"]),
    };
    let json = serde_json::to_string(&p).expect("serialize");
    let deserialized: PolicyProfile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.allowed_tools, p.allowed_tools);
    assert_eq!(deserialized.disallowed_tools, p.disallowed_tools);
    assert_eq!(deserialized.deny_read, p.deny_read);
    assert_eq!(deserialized.deny_write, p.deny_write);
    assert_eq!(deserialized.allow_network, p.allow_network);
    assert_eq!(deserialized.deny_network, p.deny_network);
    assert_eq!(deserialized.require_approval_for, p.require_approval_for);
}

#[test]
fn policy_profile_serde_default_fields() {
    let json = r#"{
        "allowed_tools": [],
        "disallowed_tools": [],
        "deny_read": [],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    }"#;
    let p: PolicyProfile = serde_json::from_str(json).expect("deserialize with empty arrays");
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn deserialized_policy_compiles_correctly() {
    let json = r#"{
        "allowed_tools": ["Read"],
        "disallowed_tools": ["Bash"],
        "deny_read": ["**/.env"],
        "deny_write": ["**/.git/**"],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    }"#;
    let p: PolicyProfile = serde_json::from_str(json).expect("deserialize");
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn policy_profile_json_schema_exists() {
    use schemars::schema_for;
    let schema = schema_for!(PolicyProfile);
    let json = serde_json::to_string(&schema).expect("schema to json");
    assert!(json.contains("allowed_tools"));
    assert!(json.contains("disallowed_tools"));
    assert!(json.contains("deny_read"));
    assert!(json.contains("deny_write"));
}

// ===================================================================
// 9. Empty policy profiles
// ===================================================================

#[test]
fn empty_profile_compiles_without_error() {
    let _ = engine(&PolicyProfile::default());
}

#[test]
fn empty_vecs_in_profile_allows_all() {
    let p = PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Any").allowed);
    assert!(e.can_read_path(Path::new("any/path")).allowed);
    assert!(e.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn empty_allow_network_and_deny_network_stored() {
    let p = PolicyProfile::default();
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
}

// ===================================================================
// 10. Very restrictive policies (deny all)
// ===================================================================

#[test]
fn deny_all_tools_via_wildcard() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Anything").allowed);
}

#[test]
fn deny_all_reads() {
    let p = PolicyProfile {
        deny_read: sv(&["**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("any.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c.rs")).allowed);
    // Write is still allowed
    assert!(e.can_write_path(Path::new("any.txt")).allowed);
}

#[test]
fn deny_all_writes() {
    let p = PolicyProfile {
        deny_write: sv(&["**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("any.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/c.rs")).allowed);
    // Read is still allowed
    assert!(e.can_read_path(Path::new("any.txt")).allowed);
}

#[test]
fn deny_everything_combined() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["*"]),
        deny_read: sv(&["**"]),
        deny_write: sv(&["**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new("x")).allowed);
    assert!(!e.can_write_path(Path::new("x")).allowed);
}

#[test]
fn allowlist_with_no_entries_denies_all_tools() {
    // When allowed_tools has entries but none match, tool is denied.
    let p = PolicyProfile {
        allowed_tools: sv(&["NonExistentTool"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("NonExistentTool").allowed);
}

// ===================================================================
// 11. Policy with many rules (performance)
// ===================================================================

#[test]
fn many_deny_read_patterns_compile_and_evaluate() {
    let patterns: Vec<String> = (0..200).map(|i| format!("**/secret_{i}/**")).collect();
    let p = PolicyProfile {
        deny_read: patterns,
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secret_42/data.txt")).allowed);
    assert!(
        !e.can_read_path(Path::new("secret_199/deep/file.rs"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn many_deny_write_patterns_compile_and_evaluate() {
    let patterns: Vec<String> = (0..200).map(|i| format!("**/locked_{i}/**")).collect();
    let p = PolicyProfile {
        deny_write: patterns,
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("locked_0/file.txt")).allowed);
    assert!(
        !e.can_write_path(Path::new("locked_100/deep/file.txt"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("open/file.txt")).allowed);
}

#[test]
fn many_disallowed_tools_compile_and_evaluate() {
    let tools: Vec<String> = (0..100).map(|i| format!("Tool_{i}")).collect();
    let p = PolicyProfile {
        disallowed_tools: tools,
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Tool_0").allowed);
    assert!(!e.can_use_tool("Tool_50").allowed);
    assert!(!e.can_use_tool("Tool_99").allowed);
    assert!(e.can_use_tool("SafeTool").allowed);
}

#[test]
fn many_allowed_tools_compile_and_evaluate() {
    let tools: Vec<String> = (0..100).map(|i| format!("Tool_{i}")).collect();
    let p = PolicyProfile {
        allowed_tools: tools,
        ..Default::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Tool_0").allowed);
    assert!(e.can_use_tool("Tool_99").allowed);
    assert!(!e.can_use_tool("UnlistedTool").allowed);
}

// ===================================================================
// 12. Case sensitivity in patterns
// ===================================================================

#[test]
fn tool_names_are_case_sensitive() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    // globset Glob::new is case-sensitive on all platforms
    assert!(e.can_use_tool("BASH").allowed);
    assert!(e.can_use_tool("bash").allowed);
}

#[test]
fn path_case_sensitivity_in_deny_read() {
    let p = PolicyProfile {
        deny_read: sv(&["SECRET.txt"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("SECRET.txt")).allowed);
    // globset Glob::new is case-sensitive on all platforms
    assert!(e.can_read_path(Path::new("secret.txt")).allowed);
}

#[test]
fn path_case_sensitivity_in_deny_write() {
    let p = PolicyProfile {
        deny_write: sv(&["LOCKED.txt"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("LOCKED.txt")).allowed);
    // globset Glob::new is case-sensitive on all platforms
    assert!(e.can_write_path(Path::new("locked.txt")).allowed);
}

// ===================================================================
// 13. Path normalization in policy checks
// ===================================================================

#[test]
fn path_traversal_read_denied() {
    let p = PolicyProfile {
        deny_read: sv(&["**/etc/passwd"]),
        ..Default::default()
    };
    let e = engine(&p);
    let d = e.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed);
}

#[test]
fn path_traversal_write_denied() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    let d = e.can_write_path(Path::new("../.git/config"));
    assert!(!d.allowed);
}

#[test]
fn relative_path_with_dot_segments() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("./.env")).allowed);
    assert!(!e.can_read_path(Path::new("dir/../.env")).allowed);
}

#[test]
fn forward_slash_paths_in_deny_write() {
    let p = PolicyProfile {
        deny_write: sv(&["src/generated/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("src/generated/out.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ===================================================================
// 14. Policy composition (multiple profiles)
// ===================================================================

#[test]
fn policy_set_merge_unions_deny_lists() {
    let mut set = PolicySet::new("test");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        ..Default::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Execute"]),
        deny_write: sv(&["**/.git/**"]),
        ..Default::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Execute")));
    assert!(merged.deny_read.contains(&s("**/.env")));
    assert!(merged.deny_write.contains(&s("**/.git/**")));
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash", "Read"]),
        ..Default::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash", "Write"]),
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
fn policy_set_merge_compiles() {
    let mut set = PolicySet::new("compile-test");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        ..Default::default()
    });
    set.add(PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..Default::default()
    });
    let merged = set.merge();
    let e = engine(&merged);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn policy_set_name() {
    let set = PolicySet::new("my-policy");
    assert_eq!(set.name(), "my-policy");
}

#[test]
fn composed_engine_deny_overrides() {
    let profiles = vec![
        PolicyProfile {
            allowed_tools: sv(&["*"]),
            ..Default::default()
        },
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..Default::default()
        },
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_allow_overrides() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..Default::default()
        },
        PolicyProfile {
            allowed_tools: sv(&["*"]),
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
            disallowed_tools: sv(&["Bash"]),
            ..Default::default()
        },
        PolicyProfile {
            allowed_tools: sv(&["*"]),
            ..Default::default()
        },
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::FirstApplicable).unwrap();
    // First profile denies Bash, first applicable returns that
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn composed_engine_empty_profiles_abstains() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_abstain());
    assert!(ce.check_read("any.txt").is_abstain());
    assert!(ce.check_write("any.txt").is_abstain());
}

#[test]
fn composed_engine_read_write_checks() {
    let profiles = vec![
        PolicyProfile {
            deny_read: sv(&["**/.env"]),
            ..Default::default()
        },
        PolicyProfile {
            deny_write: sv(&["**/.git/**"]),
            ..Default::default()
        },
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read(".env").is_deny());
    assert!(ce.check_read("src/lib.rs").is_allow());
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/lib.rs").is_allow());
}

// ===================================================================
// 15. Policy validation errors
// ===================================================================

#[test]
fn validator_detects_empty_globs() {
    let p = PolicyProfile {
        allowed_tools: sv(&[""]),
        ..Default::default()
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
        deny_read: sv(&["**/.env", ""]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::EmptyGlob && w.message.contains("deny_read"))
    );
}

#[test]
fn validator_detects_overlapping_allow_deny_tools() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Bash"]),
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
    );
}

#[test]
fn validator_detects_overlapping_allow_deny_network() {
    let p = PolicyProfile {
        allow_network: sv(&["evil.com"]),
        deny_network: sv(&["evil.com"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny && w.message.contains("network"))
    );
}

#[test]
fn validator_detects_unreachable_rules_wildcard_deny() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["*"]),
        ..Default::default()
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
        deny_read: sv(&["**"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_read"))
    );
}

#[test]
fn validator_detects_catch_all_deny_write() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_write"))
    );
}

#[test]
fn validator_no_warnings_for_clean_profile() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

#[test]
fn invalid_glob_pattern_returns_error() {
    let p = PolicyProfile {
        deny_read: sv(&["["]),
        ..Default::default()
    };
    let result = PolicyEngine::new(&p);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("deny_read"));
}

#[test]
fn invalid_glob_in_disallowed_tools_returns_error() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["["]),
        ..Default::default()
    };
    let result = PolicyEngine::new(&p);
    assert!(result.is_err());
}

// ===================================================================
// Bonus: Audit trail
// ===================================================================

#[test]
fn auditor_records_tool_decisions() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..Default::default()
    };
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

    assert!(matches!(auditor.check_tool("Read"), AuditDecision::Allow));
    assert!(matches!(
        auditor.check_tool("Bash"),
        AuditDecision::Deny { .. }
    ));

    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 1);
    assert_eq!(auditor.entries().len(), 2);
}

#[test]
fn auditor_records_read_write_decisions() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        ..Default::default()
    };
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

    auditor.check_read("src/lib.rs");
    auditor.check_read(".env");
    auditor.check_write("src/lib.rs");
    auditor.check_write(".git/config");

    let summary = auditor.summary();
    assert_eq!(summary.allowed, 2);
    assert_eq!(summary.denied, 2);
    assert_eq!(summary.warned, 0);
}

#[test]
fn auditor_entries_have_action_and_resource() {
    let p = PolicyProfile::default();
    let e = engine(&p);
    let mut auditor = PolicyAuditor::new(e);

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

// ===================================================================
// Bonus: Rule engine
// ===================================================================

#[test]
fn rule_engine_default_allows() {
    let engine = RuleEngine::new();
    assert_eq!(engine.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_pattern_deny() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash*")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(engine.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_priority_ordering() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("allow-all"),
        description: s("Allow everything"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(engine.evaluate("Bash"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_condition_and_combinator() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Pattern(s("File*")),
        RuleCondition::Not(Box::new(RuleCondition::Pattern(s("FileDelete")))),
    ]);
    assert!(cond.matches("FileRead"));
    assert!(!cond.matches("FileDelete"));
    assert!(!cond.matches("Bash"));
}

#[test]
fn rule_condition_or_combinator() {
    let cond = RuleCondition::Or(vec![
        RuleCondition::Pattern(s("Bash")),
        RuleCondition::Pattern(s("Execute")),
    ]);
    assert!(cond.matches("Bash"));
    assert!(cond.matches("Execute"));
    assert!(!cond.matches("Read"));
}

#[test]
fn rule_condition_never() {
    assert!(!RuleCondition::Never.matches("anything"));
}

#[test]
fn rule_condition_always() {
    assert!(RuleCondition::Always.matches("anything"));
}

#[test]
fn rule_engine_evaluate_all() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("r1"),
        description: s("Rule 1"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: s("r2"),
        description: s("Rule 2"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    let results = engine.evaluate_all("Bash");
    assert_eq!(results.len(), 2);
    assert!(results[0].matched); // Always matches
    assert!(results[1].matched); // Pattern matches Bash
}

#[test]
fn rule_engine_remove_rule() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("r1"),
        description: s("test"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 1,
    });
    assert_eq!(engine.rule_count(), 1);
    engine.remove_rule("r1");
    assert_eq!(engine.rule_count(), 0);
    assert_eq!(engine.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_throttle_effect() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("throttle"),
        description: s("Throttle Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Throttle { max: 5 },
        priority: 10,
    });
    assert_eq!(engine.evaluate("Bash"), RuleEffect::Throttle { max: 5 });
    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}
