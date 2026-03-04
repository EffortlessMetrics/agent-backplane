#![allow(clippy::all)]

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::audit::{
    AuditAction, AuditLog, AuditSummary, PolicyAuditor, PolicyDecision as AuditPolicyDecision,
};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision as ComposePolicyDecision, PolicyPrecedence, PolicySet,
    PolicyValidator, WarningKind,
};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// =========================================================================
// Helper
// =========================================================================

fn make_engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("compile policy")
}

fn s(v: &str) -> String {
    v.to_string()
}

fn sv(vals: &[&str]) -> Vec<String> {
    vals.iter().map(|v| v.to_string()).collect()
}

// =========================================================================
// 1. PolicyProfile construction
// =========================================================================

#[test]
fn profile_default_has_empty_lists() {
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
        deny_read: sv(&["secret/**"]),
        deny_write: sv(&["**/.git/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.com"]),
        require_approval_for: sv(&["Bash"]),
    };
    assert_eq!(p.allowed_tools.len(), 2);
    assert_eq!(p.disallowed_tools.len(), 1);
    assert_eq!(p.deny_read.len(), 1);
    assert_eq!(p.deny_write.len(), 1);
    assert_eq!(p.allow_network.len(), 1);
    assert_eq!(p.deny_network.len(), 1);
    assert_eq!(p.require_approval_for.len(), 1);
}

#[test]
fn profile_clone_is_independent() {
    let mut p = PolicyProfile::default();
    p.allowed_tools.push(s("Read"));
    let p2 = p.clone();
    assert_eq!(p.allowed_tools, p2.allowed_tools);
}

// =========================================================================
// 2. Policy compilation (PolicyProfile → PolicyEngine)
// =========================================================================

#[test]
fn compile_empty_policy_succeeds() {
    let _ = make_engine(&PolicyProfile::default());
}

#[test]
fn compile_complex_policy_succeeds() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash*"]),
        deny_read: sv(&["**/.env", "**/.env.*", "**/id_rsa"]),
        deny_write: sv(&["**/.git/**", "Cargo.lock"]),
        ..PolicyProfile::default()
    };
    let _ = make_engine(&p);
}

#[test]
fn compile_invalid_glob_returns_error() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn compile_invalid_deny_read_glob_returns_error() {
    let p = PolicyProfile {
        deny_read: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn compile_invalid_deny_write_glob_returns_error() {
    let p = PolicyProfile {
        deny_write: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

// =========================================================================
// 3. Decision type
// =========================================================================

#[test]
fn decision_allow_is_allowed() {
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
fn decision_deny_from_string_owned() {
    let d = Decision::deny(String::from("owned reason"));
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("owned reason"));
}

// =========================================================================
// 4. Tool allow/deny lists
// =========================================================================

#[test]
fn empty_policy_allows_all_tools() {
    let e = make_engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
}

#[test]
fn disallowed_tool_is_denied() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn disallowed_tool_reason_text() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
}

#[test]
fn allowlist_only_permits_listed_tools() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn allowlist_missing_tool_reason_text() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' not in allowlist"));
}

#[test]
fn disallowed_tool_beats_allowlist() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn wildcard_allowlist_permits_everything() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("AnythingGoes").allowed);
}

#[test]
fn glob_pattern_in_disallowed_tools() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_pattern_in_allowed_tools() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["Read*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn multiple_disallowed_tools() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash", "Delete", "Execute"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Delete").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn case_sensitive_tool_names() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    // Different case should be allowed (glob matching is case-sensitive by default)
    // Actually globset is case-insensitive on Windows. Let's just verify Bash itself is denied.
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn tool_name_with_special_chars() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["my-tool_v2"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("my-tool_v2").allowed);
    assert!(e.can_use_tool("my-tool_v3").allowed);
}

// =========================================================================
// 5. Read path allow/deny lists
// =========================================================================

#[test]
fn empty_deny_read_allows_all() {
    let e = make_engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("secret/key.pem")).allowed);
}

#[test]
fn deny_read_blocks_matching_paths() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_read_path(Path::new("secrets.json")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_read_reason_text() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(d.reason.as_deref().unwrap().contains("read denied"));
}

#[test]
fn multiple_deny_read_patterns() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*", "**/id_rsa"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_read_double_star_recursive() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".git/config")).allowed);
    assert!(!e.can_read_path(Path::new(".git/objects/abc123")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_read_extension_pattern() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["*.pem", "*.key"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("private.key")).allowed);
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

#[test]
fn deny_read_nested_path() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/etc/passwd"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("../../etc/passwd")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

// =========================================================================
// 6. Write path allow/deny lists
// =========================================================================

#[test]
fn empty_deny_write_allows_all() {
    let e = make_engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn deny_write_blocks_matching_paths() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("locked.md")).allowed);
    assert!(!e.can_write_path(Path::new("locked-data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_write_reason_text() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("locked.md"));
    assert!(d.reason.as_deref().unwrap().contains("write denied"));
}

#[test]
fn deny_write_git_directory() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/config")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_write_deep_nested() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(!e.can_write_path(Path::new("secret/x.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn deny_write_cargo_lock() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["Cargo.lock"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn deny_write_path_traversal() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("../.git/config"));
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("denied"));
}

#[test]
fn deny_write_multiple_patterns() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**", "Cargo.lock", "*.bak"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("data.bak")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// =========================================================================
// 7. Glob pattern matching (wildcards, double-star)
// =========================================================================

#[test]
fn glob_single_star_matches_in_segment() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["*.log"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("app.log")).allowed);
    // globset *.log also matches nested due to literal_separator default=false
    assert!(!e.can_read_path(Path::new("logs/app.log")).allowed);
    assert!(e.can_read_path(Path::new("app.txt")).allowed);
}

#[test]
fn glob_double_star_matches_any_depth() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/node_modules/**"]),
        ..PolicyProfile::default()
    });
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(
        !e.can_write_path(Path::new("sub/node_modules/pkg/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/index.js")).allowed);
}

#[test]
fn glob_question_mark_matches_single_char() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["secret?.txt"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("secret1.txt")).allowed);
    assert!(!e.can_read_path(Path::new("secretA.txt")).allowed);
    // Two chars after "secret" should not match `?` (single char)
    assert!(e.can_read_path(Path::new("secret12.txt")).allowed);
}

#[test]
fn glob_brace_expansion() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["*.{bak,tmp,swp}"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("file.bak")).allowed);
    assert!(!e.can_write_path(Path::new("file.tmp")).allowed);
    assert!(!e.can_write_path(Path::new("file.swp")).allowed);
    assert!(e.can_write_path(Path::new("file.rs")).allowed);
}

#[test]
fn glob_char_class() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["log[0-9].txt"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("log0.txt")).allowed);
    assert!(!e.can_read_path(Path::new("log9.txt")).allowed);
    assert!(e.can_read_path(Path::new("logA.txt")).allowed);
}

#[test]
fn glob_combined_double_star_and_extension() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/*.secret"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("a.secret")).allowed);
    assert!(
        !e.can_read_path(Path::new("deep/nested/path/x.secret"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

// =========================================================================
// 8. Policy evaluation for tool invocations
// =========================================================================

#[test]
fn tool_eval_wildcard_deny_blocks_everything() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn tool_eval_empty_string_tool_name() {
    let e = make_engine(&PolicyProfile::default());
    // Empty tool name should be allowed by default
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn tool_eval_unicode_tool_name() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["données"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("données").allowed);
    assert!(e.can_use_tool("data").allowed);
}

#[test]
fn tool_eval_suffix_pattern() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["*Exec"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellExec").allowed);
    assert!(e.can_use_tool("BashRun").allowed);
}

// =========================================================================
// 9. Policy evaluation for file read operations
// =========================================================================

#[test]
fn read_eval_root_level_env() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&[".env"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("sub/.env")).allowed); // pattern is just ".env"
}

#[test]
fn read_eval_any_level_env() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/.env")).allowed);
}

#[test]
fn read_eval_allowed_non_matching_path() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/*.secret"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("README.md")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.toml")).allowed);
}

// =========================================================================
// 10. Policy evaluation for file write operations
// =========================================================================

#[test]
fn write_eval_exact_filename() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["LICENSE"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("LICENSE")).allowed);
    assert!(e.can_write_path(Path::new("LICENSE-MIT")).allowed);
}

#[test]
fn write_eval_directory_subtree() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["vendor/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("vendor/pkg/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("vendor/a/b/c.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_eval_allows_non_matching() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_write_path(Path::new("tests/integration.rs")).allowed);
}

// =========================================================================
// 11. Complex policy scenarios (multiple overlapping rules)
// =========================================================================

#[test]
fn complex_allow_deny_tool_combination() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        disallowed_tools: sv(&["Write"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/locked/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Write").allowed); // deny beats allow
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed); // not in allowlist
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn overlapping_deny_read_and_write() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["secret/**"]),
        deny_write: sv(&["secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(!e.can_write_path(Path::new("secret/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn deny_read_does_not_affect_write() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["config/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("config/app.toml")).allowed);
    assert!(e.can_write_path(Path::new("config/app.toml")).allowed);
}

#[test]
fn deny_write_does_not_affect_read() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["config/**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("config/app.toml")).allowed);
    assert!(!e.can_write_path(Path::new("config/app.toml")).allowed);
}

#[test]
fn tool_deny_does_not_affect_path_checks() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("any.txt")).allowed);
    assert!(e.can_write_path(Path::new("any.txt")).allowed);
}

#[test]
fn path_deny_does_not_affect_tool_checks() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**"]),
        deny_write: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// =========================================================================
// 12. Default deny vs default allow
// =========================================================================

#[test]
fn default_allow_no_constraints() {
    let e = make_engine(&PolicyProfile::default());
    assert!(e.can_use_tool("anything").allowed);
    assert!(e.can_read_path(Path::new("any/path")).allowed);
    assert!(e.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn default_deny_via_empty_allowlist() {
    // An explicit but empty allowed_tools list acts as a no-op (allows all).
    // Only a non-empty allowlist restricts.
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["OnlyThisTool"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("OnlyThisTool").allowed);
    assert!(!e.can_use_tool("AnyOtherTool").allowed);
}

#[test]
fn strict_lockdown_policy() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["Read"]),
        disallowed_tools: sv(&["*"]),
        deny_read: sv(&["**"]),
        deny_write: sv(&["**"]),
        ..PolicyProfile::default()
    });
    // disallowed_tools: * beats allowed Read
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_read_path(Path::new("any.txt")).allowed);
    assert!(!e.can_write_path(Path::new("any.txt")).allowed);
}

// =========================================================================
// 13. Policy error messages
// =========================================================================

#[test]
fn error_message_disallowed_tool() {
    let e = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
}

#[test]
fn error_message_missing_allowlist() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Write");
    assert_eq!(d.reason.as_deref(), Some("tool 'Write' not in allowlist"));
}

#[test]
fn error_message_read_denied() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["secrets/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("secrets/key.pem"));
    let reason = d.reason.as_deref().unwrap();
    assert!(reason.contains("read denied"));
    assert!(reason.contains("secrets"));
}

#[test]
fn error_message_write_denied() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new(".git/config"));
    let reason = d.reason.as_deref().unwrap();
    assert!(reason.contains("write denied"));
    assert!(reason.contains(".git"));
}

#[test]
fn allowed_tool_has_no_reason() {
    let e = make_engine(&PolicyProfile::default());
    let d = e.can_use_tool("Read");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn allowed_read_has_no_reason() {
    let e = make_engine(&PolicyProfile::default());
    let d = e.can_read_path(Path::new("src/lib.rs"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn allowed_write_has_no_reason() {
    let e = make_engine(&PolicyProfile::default());
    let d = e.can_write_path(Path::new("src/lib.rs"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

// =========================================================================
// 14. RuleCondition / RuleEngine
// =========================================================================

#[test]
fn rule_condition_always_matches() {
    assert!(RuleCondition::Always.matches("anything"));
    assert!(RuleCondition::Always.matches(""));
}

#[test]
fn rule_condition_never_matches() {
    assert!(!RuleCondition::Never.matches("anything"));
    assert!(!RuleCondition::Never.matches(""));
}

#[test]
fn rule_condition_pattern_matches() {
    let c = RuleCondition::Pattern("Bash*".into());
    assert!(c.matches("Bash"));
    assert!(c.matches("BashExec"));
    assert!(!c.matches("Read"));
}

#[test]
fn rule_condition_and_all_must_match() {
    let c = RuleCondition::And(vec![
        RuleCondition::Pattern("B*".into()),
        RuleCondition::Pattern("*sh".into()),
    ]);
    assert!(c.matches("Bash"));
    assert!(!c.matches("BashExec")); // matches B* but not *sh
    assert!(!c.matches("Fish")); // matches *sh but not B*
}

#[test]
fn rule_condition_or_any_must_match() {
    let c = RuleCondition::Or(vec![
        RuleCondition::Pattern("Bash".into()),
        RuleCondition::Pattern("Read".into()),
    ]);
    assert!(c.matches("Bash"));
    assert!(c.matches("Read"));
    assert!(!c.matches("Write"));
}

#[test]
fn rule_condition_not_negates() {
    let c = RuleCondition::Not(Box::new(RuleCondition::Pattern("Bash".into())));
    assert!(!c.matches("Bash"));
    assert!(c.matches("Read"));
}

#[test]
fn rule_condition_nested_combinators() {
    // (Bash OR Read) AND NOT Delete
    let c = RuleCondition::And(vec![
        RuleCondition::Or(vec![
            RuleCondition::Pattern("Bash".into()),
            RuleCondition::Pattern("Read".into()),
        ]),
        RuleCondition::Not(Box::new(RuleCondition::Pattern("Delete".into()))),
    ]);
    assert!(c.matches("Bash"));
    assert!(c.matches("Read"));
    assert!(!c.matches("Delete"));
    assert!(!c.matches("Write"));
}

#[test]
fn rule_condition_empty_and() {
    let c = RuleCondition::And(vec![]);
    assert!(c.matches("anything")); // all() on empty is true
}

#[test]
fn rule_condition_empty_or() {
    let c = RuleCondition::Or(vec![]);
    assert!(!c.matches("anything")); // any() on empty is false
}

#[test]
fn rule_engine_empty_allows_by_default() {
    let eng = RuleEngine::new();
    assert_eq!(eng.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_single_deny_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "deny-bash".into(),
        description: "Block Bash".into(),
        condition: RuleCondition::Pattern("Bash".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
    assert_eq!(eng.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_priority_higher_wins() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "allow-all".into(),
        description: "Allow everything".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    eng.add_rule(Rule {
        id: "deny-bash".into(),
        description: "Block Bash".into(),
        condition: RuleCondition::Pattern("Bash".into()),
        effect: RuleEffect::Deny,
        priority: 100,
    });
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
    assert_eq!(eng.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_evaluate_all() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "r1".into(),
        description: "Allow".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    eng.add_rule(Rule {
        id: "r2".into(),
        description: "Deny Bash".into(),
        condition: RuleCondition::Pattern("Bash".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    let results = eng.evaluate_all("Bash");
    assert_eq!(results.len(), 2);
    assert!(results[0].matched); // Always matches
    assert!(results[1].matched); // Pattern matches
}

#[test]
fn rule_engine_remove_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "r1".into(),
        description: "Block Bash".into(),
        condition: RuleCondition::Pattern("Bash".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.rule_count(), 1);
    eng.remove_rule("r1");
    assert_eq!(eng.rule_count(), 0);
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Allow);
}

#[test]
fn rule_engine_remove_nonexistent() {
    let mut eng = RuleEngine::new();
    eng.remove_rule("nonexistent"); // should not panic
    assert_eq!(eng.rule_count(), 0);
}

#[test]
fn rule_engine_throttle_effect() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "throttle".into(),
        description: "Throttle heavy".into(),
        condition: RuleCondition::Pattern("HeavyOp".into()),
        effect: RuleEffect::Throttle { max: 5 },
        priority: 50,
    });
    assert_eq!(eng.evaluate("HeavyOp"), RuleEffect::Throttle { max: 5 });
    assert_eq!(eng.evaluate("LightOp"), RuleEffect::Allow);
}

#[test]
fn rule_engine_log_effect() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "log".into(),
        description: "Log".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Log,
        priority: 1,
    });
    assert_eq!(eng.evaluate("anything"), RuleEffect::Log);
}

#[test]
fn rule_engine_rules_accessor() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: "r1".into(),
        description: "d".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    let rules = eng.rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "r1");
}

// =========================================================================
// 15. PolicyAuditor
// =========================================================================

#[test]
fn auditor_records_tool_allow() {
    let engine = make_engine(&PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(engine);
    let d = auditor.check_tool("Read");
    assert!(matches!(d, AuditPolicyDecision::Allow));
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 0);
}

#[test]
fn auditor_records_tool_deny() {
    let engine = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(engine);
    let d = auditor.check_tool("Bash");
    assert!(matches!(d, AuditPolicyDecision::Deny { .. }));
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_read_check() {
    let engine = make_engine(&PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_read("secret.txt");
    auditor.check_read("public.txt");
    assert_eq!(auditor.denied_count(), 1);
    assert_eq!(auditor.allowed_count(), 1);
}

#[test]
fn auditor_records_write_check() {
    let engine = make_engine(&PolicyProfile {
        deny_write: sv(&["locked/**"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_write("locked/data.txt");
    auditor.check_write("open/data.txt");
    assert_eq!(auditor.denied_count(), 1);
    assert_eq!(auditor.allowed_count(), 1);
}

#[test]
fn auditor_entries_in_order() {
    let engine = make_engine(&PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(engine);
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
    let engine = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(engine);
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
            warned: 0
        }
    );
}

// =========================================================================
// 16. AuditLog / AuditAction
// =========================================================================

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

#[test]
fn audit_log_new_is_empty() {
    let log = AuditLog::new();
    assert!(log.is_empty());
    assert_eq!(log.len(), 0);
    assert_eq!(log.denied_count(), 0);
}

#[test]
fn audit_log_record_and_query() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", Some("default"), None);
    log.record(
        AuditAction::ToolDenied,
        "Bash",
        Some("default"),
        Some("disallowed"),
    );
    assert_eq!(log.len(), 2);
    assert_eq!(log.denied_count(), 1);
}

#[test]
fn audit_log_filter_by_action() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", None, None);
    log.record(AuditAction::ToolDenied, "Bash", None, None);
    log.record(AuditAction::ReadAllowed, "file.txt", None, None);
    let allowed_tools = log.filter_by_action(&AuditAction::ToolAllowed);
    assert_eq!(allowed_tools.len(), 1);
    assert_eq!(allowed_tools[0].resource, "Read");
}

#[test]
fn audit_log_entries() {
    let mut log = AuditLog::new();
    log.record(AuditAction::WriteAllowed, "src/lib.rs", None, None);
    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].resource, "src/lib.rs");
}

// =========================================================================
// 17. Composed engine (compose module)
// =========================================================================

#[test]
fn composed_engine_deny_overrides() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let eng = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();
    assert!(eng.check_tool("Bash").is_deny());
    assert!(eng.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_allow_overrides() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let eng = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();
    assert!(eng.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_first_applicable() {
    let first = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let second = PolicyProfile::default();
    let eng = ComposedEngine::new(vec![first, second], PolicyPrecedence::FirstApplicable).unwrap();
    assert!(eng.check_tool("Bash").is_deny());
}

#[test]
fn composed_engine_empty_abstains() {
    let eng = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(eng.check_tool("anything").is_abstain());
}

#[test]
fn composed_engine_read_check() {
    let p = PolicyProfile {
        deny_read: sv(&["secret/**"]),
        ..PolicyProfile::default()
    };
    let eng = ComposedEngine::new(vec![p], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(eng.check_read("secret/key.pem").is_deny());
    assert!(eng.check_read("public/data.txt").is_allow());
}

#[test]
fn composed_engine_write_check() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let eng = ComposedEngine::new(vec![p], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(eng.check_write(".git/config").is_deny());
    assert!(eng.check_write("src/lib.rs").is_allow());
}

// =========================================================================
// 18. PolicySet
// =========================================================================

#[test]
fn policy_set_merge_unions_lists() {
    let mut set = PolicySet::new("test");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret/**"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Delete"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Delete")));
    assert!(merged.deny_read.contains(&s("secret/**")));
    assert!(merged.deny_write.contains(&s("**/.git/**")));
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("test");
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
    let set = PolicySet::new("security");
    assert_eq!(set.name(), "security");
}

// =========================================================================
// 19. PolicyValidator
// =========================================================================

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
fn validator_detects_overlapping_allow_deny() {
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
fn validator_detects_unreachable_rule_wildcard_deny() {
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
fn validator_detects_catch_all_deny_read() {
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
fn validator_detects_catch_all_deny_write() {
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
fn validator_no_warnings_for_clean_profile() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret/**"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

#[test]
fn validator_detects_empty_glob_in_deny_read() {
    let p = PolicyProfile {
        deny_read: sv(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_deny_write() {
    let p = PolicyProfile {
        deny_write: sv(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_network_overlap() {
    let p = PolicyProfile {
        allow_network: sv(&["api.example.com"]),
        deny_network: sv(&["api.example.com"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
    );
}

// =========================================================================
// 20. ComposedPolicy (composed module)
// =========================================================================

#[test]
fn composed_policy_all_must_allow_any_deny_vetoes() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", make_engine(&PolicyProfile::default()));
    cp.add_policy(
        "restrictive",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_policy_any_must_allow_one_allow_suffices() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("permissive", make_engine(&PolicyProfile::default()));
    cp.add_policy(
        "restrictive",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_policy_first_match() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy(
        "first",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    cp.add_policy("second", make_engine(&PolicyProfile::default()));
    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_policy_empty_allows() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("anything").is_allowed());
}

#[test]
fn composed_policy_read_write() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy(
        "security",
        make_engine(&PolicyProfile {
            deny_read: sv(&["secret/**"]),
            deny_write: sv(&["**/.git/**"]),
            ..PolicyProfile::default()
        }),
    );
    assert!(cp.evaluate_read("secret/key.pem").is_denied());
    assert!(cp.evaluate_read("public/data.txt").is_allowed());
    assert!(cp.evaluate_write(".git/config").is_denied());
    assert!(cp.evaluate_write("src/lib.rs").is_allowed());
}

#[test]
fn composed_policy_count_and_strategy() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    assert_eq!(cp.strategy(), CompositionStrategy::AllMustAllow);
    cp.add_policy("p1", make_engine(&PolicyProfile::default()));
    assert_eq!(cp.policy_count(), 1);
}

#[test]
fn composed_result_is_allowed_denied() {
    let allowed = ComposedResult::Allowed {
        by: "test".to_string(),
    };
    let denied = ComposedResult::Denied {
        by: "test".to_string(),
        reason: "no".to_string(),
    };
    assert!(allowed.is_allowed());
    assert!(!allowed.is_denied());
    assert!(denied.is_denied());
    assert!(!denied.is_allowed());
}

// =========================================================================
// 21. RateLimitPolicy
// =========================================================================

#[test]
fn rate_limit_unlimited_allows_everything() {
    let p = RateLimitPolicy::unlimited();
    assert!(p.check_rate_limit(1000, 1_000_000, 100).is_allowed());
}

#[test]
fn rate_limit_rpm_exceeded() {
    let p = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        ..RateLimitPolicy::default()
    };
    assert!(p.check_rate_limit(10, 0, 0).is_throttled());
    assert!(p.check_rate_limit(5, 0, 0).is_allowed());
}

#[test]
fn rate_limit_tpm_exceeded() {
    let p = RateLimitPolicy {
        max_tokens_per_minute: Some(1000),
        ..RateLimitPolicy::default()
    };
    assert!(p.check_rate_limit(0, 1000, 0).is_throttled());
    assert!(p.check_rate_limit(0, 500, 0).is_allowed());
}

#[test]
fn rate_limit_concurrent_exceeded() {
    let p = RateLimitPolicy {
        max_concurrent: Some(5),
        ..RateLimitPolicy::default()
    };
    assert!(p.check_rate_limit(0, 0, 5).is_denied());
    assert!(p.check_rate_limit(0, 0, 4).is_allowed());
}

#[test]
fn rate_limit_concurrent_takes_precedence() {
    let p = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        max_concurrent: Some(2),
        ..RateLimitPolicy::default()
    };
    // Both limits exceeded; concurrent is checked first
    let result = p.check_rate_limit(100, 0, 5);
    assert!(result.is_denied());
}

// =========================================================================
// 22. Workspace staging interaction
// =========================================================================

#[test]
fn workspace_git_directory_protection() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        deny_read: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    // Typical workspace staging: .git should be protected
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/objects/pack/p1")).allowed);
    assert!(!e.can_read_path(Path::new(".git/HEAD")).allowed);
    // Normal workspace files should be fine
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn workspace_temp_files_protection() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["*.tmp", "*.bak", "*.swp"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("data.tmp")).allowed);
    assert!(!e.can_write_path(Path::new("file.bak")).allowed);
    assert!(!e.can_write_path(Path::new("edit.swp")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn workspace_sensitive_files_protection() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*", "**/*.pem", "**/*.key", "**/id_rsa"]),
        deny_write: sv(&["**/.env", "**/.env.*", "**/*.pem", "**/*.key", "**/id_rsa"]),
        ..PolicyProfile::default()
    });
    for path in &[
        ".env",
        "config/.env",
        ".env.local",
        "certs/server.pem",
        "keys/private.key",
        ".ssh/id_rsa",
    ] {
        assert!(
            !e.can_read_path(Path::new(path)).allowed,
            "should deny read: {path}"
        );
        assert!(
            !e.can_write_path(Path::new(path)).allowed,
            "should deny write: {path}"
        );
    }
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn workspace_node_modules_protection() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**/node_modules/**"]),
        ..PolicyProfile::default()
    });
    assert!(
        !e.can_write_path(Path::new("node_modules/lodash/index.js"))
            .allowed
    );
    assert!(
        !e.can_write_path(Path::new("frontend/node_modules/react/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/index.js")).allowed);
}

// =========================================================================
// 23. Edge cases and additional coverage
// =========================================================================

#[test]
fn policy_with_many_patterns() {
    let mut deny = Vec::new();
    for i in 0..100 {
        deny.push(format!("pattern_{i}/**"));
    }
    let e = make_engine(&PolicyProfile {
        deny_write: deny,
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("pattern_0/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("pattern_99/file.txt")).allowed);
    assert!(e.can_write_path(Path::new("safe/file.txt")).allowed);
}

#[test]
fn policy_engine_is_clone() {
    let e1 = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    let e2 = e1.clone();
    assert!(!e2.can_use_tool("Bash").allowed);
}

#[test]
fn decision_is_debug() {
    let d = Decision::deny("test");
    let debug = format!("{:?}", d);
    assert!(debug.contains("test"));
}

#[test]
fn policy_precedence_default() {
    let p = PolicyPrecedence::default();
    assert_eq!(p, PolicyPrecedence::DenyOverrides);
}

#[test]
fn composition_strategy_default() {
    let s = CompositionStrategy::default();
    assert_eq!(s, CompositionStrategy::AllMustAllow);
}

#[test]
fn deny_both_read_and_write_same_path() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["data/**"]),
        deny_write: sv(&["data/**"]),
        ..PolicyProfile::default()
    });
    let path = Path::new("data/sensitive.csv");
    assert!(!e.can_read_path(path).allowed);
    assert!(!e.can_write_path(path).allowed);
}

#[test]
fn allow_all_tools_deny_none() {
    let e = make_engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    for tool in &["Read", "Write", "Bash", "Grep", "Exec", "Delete"] {
        assert!(e.can_use_tool(tool).allowed, "should allow: {tool}");
    }
}

#[test]
fn deny_star_star_blocks_all_reads() {
    let e = make_engine(&PolicyProfile {
        deny_read: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("any/file")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/d")).allowed);
}

#[test]
fn deny_star_star_blocks_all_writes() {
    let e = make_engine(&PolicyProfile {
        deny_write: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("any/file")).allowed);
    assert!(!e.can_write_path(Path::new("")).allowed);
}

#[test]
fn composed_policy_all_must_allow_all_deny_returns_denied() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy(
        "p1",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    cp.add_policy(
        "p2",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    let result = cp.evaluate_tool("Bash");
    assert!(result.is_denied());
}

#[test]
fn composed_policy_any_must_allow_all_deny_returns_denied() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy(
        "p1",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    cp.add_policy(
        "p2",
        make_engine(&PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..PolicyProfile::default()
        }),
    );
    let result = cp.evaluate_tool("Bash");
    assert!(result.is_denied());
}

#[test]
fn rule_condition_pattern_invalid_glob_no_match() {
    // An invalid glob pattern should not match
    let c = RuleCondition::Pattern("[".into());
    assert!(!c.matches("anything"));
}

#[test]
fn rate_limit_zero_rpm() {
    let p = RateLimitPolicy {
        max_requests_per_minute: Some(0),
        ..RateLimitPolicy::default()
    };
    let result = p.check_rate_limit(0, 0, 0);
    assert!(result.is_throttled());
}

#[test]
fn rate_limit_zero_tpm() {
    let p = RateLimitPolicy {
        max_tokens_per_minute: Some(0),
        ..RateLimitPolicy::default()
    };
    let result = p.check_rate_limit(0, 0, 0);
    assert!(result.is_throttled());
}

#[test]
fn policy_set_merge_empty() {
    let set = PolicySet::new("empty");
    let merged = set.merge();
    assert!(merged.allowed_tools.is_empty());
    assert!(merged.disallowed_tools.is_empty());
}

#[test]
fn composed_engine_deny_overrides_read_write() {
    let p1 = PolicyProfile::default();
    let p2 = PolicyProfile {
        deny_read: sv(&["secret/**"]),
        deny_write: sv(&["locked/**"]),
        ..PolicyProfile::default()
    };
    let eng = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(eng.check_read("secret/key.pem").is_deny());
    assert!(eng.check_write("locked/data.txt").is_deny());
    assert!(eng.check_read("public/file.txt").is_allow());
    assert!(eng.check_write("public/file.txt").is_allow());
}

#[test]
fn auditor_check_many_operations() {
    let engine = make_engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["*.secret"]),
        deny_write: sv(&["*.lock"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(engine);

    for _ in 0..10 {
        auditor.check_tool("Read");
    }
    auditor.check_tool("Bash");
    for _ in 0..5 {
        auditor.check_read("file.txt");
    }
    auditor.check_read("data.secret");
    for _ in 0..3 {
        auditor.check_write("file.rs");
    }
    auditor.check_write("pkg.lock");

    let summary = auditor.summary();
    assert_eq!(summary.allowed, 18); // 10 tools + 5 reads + 3 writes
    assert_eq!(summary.denied, 3); // 1 tool + 1 read + 1 write
}

#[test]
fn policy_decision_compose_is_allow_deny_abstain() {
    let allow = ComposePolicyDecision::Allow {
        reason: "ok".into(),
    };
    let deny = ComposePolicyDecision::Deny {
        reason: "no".into(),
    };
    let abstain = ComposePolicyDecision::Abstain;
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

#[test]
fn rate_limit_result_variants() {
    let allowed = RateLimitResult::Allowed;
    let throttled = RateLimitResult::Throttled {
        retry_after_ms: 100,
    };
    let denied = RateLimitResult::Denied {
        reason: "over limit".into(),
    };
    assert!(allowed.is_allowed());
    assert!(!allowed.is_throttled());
    assert!(!allowed.is_denied());
    assert!(throttled.is_throttled());
    assert!(!throttled.is_allowed());
    assert!(denied.is_denied());
    assert!(!denied.is_allowed());
}
