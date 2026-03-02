// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the policy engine covering PolicyProfile construction,
//! PolicyEngine compilation, tool/read/write checks, glob patterns, edge cases,
//! empty/restrictive policies, include/exclude priority, and serialization roundtrip.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn s(v: &str) -> String {
    v.to_string()
}

fn sv(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("failed to compile policy")
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. PolicyProfile construction
// ═══════════════════════════════════════════════════════════════════════════════

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
fn profile_with_allowed_tools_only() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.allowed_tools.len(), 2);
    assert!(p.disallowed_tools.is_empty());
}

#[test]
fn profile_with_disallowed_tools_only() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash", "Shell"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.disallowed_tools.len(), 2);
    assert!(p.allowed_tools.is_empty());
}

#[test]
fn profile_with_deny_read_patterns() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env", "**/secret*"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.deny_read.len(), 2);
}

#[test]
fn profile_with_deny_write_patterns() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**", "**/node_modules/**"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.deny_write.len(), 2);
}

#[test]
fn profile_with_network_fields() {
    let p = PolicyProfile {
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.example.com"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.allow_network, vec!["*.example.com"]);
    assert_eq!(p.deny_network, vec!["evil.example.com"]);
}

#[test]
fn profile_with_require_approval() {
    let p = PolicyProfile {
        require_approval_for: sv(&["Bash", "DeleteFile"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.require_approval_for.len(), 2);
}

#[test]
fn profile_clone_is_independent() {
    let p1 = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    };
    let mut p2 = p1.clone();
    p2.allowed_tools.push(s("Write"));
    assert_eq!(p1.allowed_tools.len(), 1);
    assert_eq!(p2.allowed_tools.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. PolicyEngine compilation from profile
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn engine_compiles_default_profile() {
    let _e = engine(&PolicyProfile::default());
}

#[test]
fn engine_compiles_with_all_fields() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.com"]),
        require_approval_for: sv(&["Bash"]),
    };
    let _e = engine(&p);
}

#[test]
fn engine_rejects_invalid_tool_glob() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn engine_rejects_invalid_deny_read_glob() {
    let p = PolicyProfile {
        deny_read: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn engine_rejects_invalid_deny_write_glob() {
    let p = PolicyProfile {
        deny_write: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn engine_rejects_invalid_allowed_tools_glob() {
    let p = PolicyProfile {
        allowed_tools: sv(&["["]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Tool allow/deny checking
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn tool_allowed_when_in_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn tool_denied_when_not_in_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("not in allowlist"));
}

#[test]
fn tool_denied_when_in_denylist() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("disallowed"));
}

#[test]
fn denylist_beats_allowlist_for_tools() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_allowed_when_no_rules() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("AnyTool").allowed);
}

#[test]
fn tool_deny_reason_contains_tool_name() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["DangerousTool"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("DangerousTool");
    assert!(d.reason.as_deref().unwrap().contains("DangerousTool"));
}

#[test]
fn tool_multiple_denied() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash", "Shell", "Exec"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_allowlist_with_multiple_entries() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Delete").allowed);
}

#[test]
fn tool_both_allow_and_deny_same_tool() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Bash"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    // Deny takes precedence
    assert!(!e.can_use_tool("Bash").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Read path allow/deny checking
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn read_allowed_when_no_deny() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn read_denied_matching_pattern() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
}

#[test]
fn read_allowed_non_matching_path() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_deny_reason_contains_path() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("secret.txt"));
}

#[test]
fn read_deny_multiple_patterns() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*", "**/id_rsa"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_deny_deep_nested_path() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("secret/a/b/c/d.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn read_deny_with_extension_pattern() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["*.key", "*.pem"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Write path allow/deny checking
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn write_allowed_when_no_deny() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_denied_matching_pattern() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/hooks/pre-commit")).allowed);
}

#[test]
fn write_allowed_non_matching_path() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_reason_contains_path() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("locked.md"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("locked.md"));
}

#[test]
fn write_deny_multiple_patterns() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**", "**/node_modules/**", "*.lock"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("node_modules/pkg/index.js")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_deny_deep_nested_path() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["build/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("build/a/b/c/d.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_with_extension_pattern() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["*.exe", "*.dll"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("app.exe")).allowed);
    assert!(!e.can_write_path(Path::new("lib.dll")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Glob patterns (wildcards, multiple patterns)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn glob_star_matches_any_tool_name() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_question_mark_single_char() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["Bas?"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Base").allowed);
    // "Ba" doesn't match "Bas?" — needs exactly one char after "Bas"
    assert!(e.can_use_tool("Ba").allowed);
}

#[test]
fn glob_double_star_path_traversal() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**/secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("a/b/secret/c.txt")).allowed);
    assert!(!e.can_write_path(Path::new("secret/file.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/file.txt")).allowed);
}

#[test]
fn glob_extension_matching() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/*.log"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("app.log")).allowed);
    assert!(!e.can_read_path(Path::new("logs/debug.log")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_braces_alternative_patterns() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["*.{key,pem,crt}"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
    assert!(!e.can_read_path(Path::new("ca.crt")).allowed);
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

#[test]
fn glob_multiple_tool_patterns() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash*", "Shell*", "Exec*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCmd").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_wildcard_allowlist_permits_all() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Anything").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Bash").allowed);
}

#[test]
fn glob_character_class_in_deny_read() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/.[a-z]*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_combined_read_write_deny() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/.env*"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_write_path(Path::new(".env")).allowed); // only read is denied for .env
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Empty policy (allow-all) behavior
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn empty_policy_allows_all_tools() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Delete").allowed);
    assert!(e.can_use_tool("AnyToolName").allowed);
}

#[test]
fn empty_policy_allows_all_read_paths() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new(".git/config")).allowed);
    assert!(e.can_read_path(Path::new("/etc/passwd")).allowed);
}

#[test]
fn empty_policy_allows_all_write_paths() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_write_path(Path::new("node_modules/x.js")).allowed);
}

#[test]
fn empty_policy_decision_has_no_reason() {
    let e = engine(&PolicyProfile::default());
    let d = e.can_use_tool("Bash");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. Fully restrictive policy (deny-all) behavior
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn deny_all_tools_via_denylist_wildcard() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn deny_all_reads_via_double_star() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(!e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_all_writes_via_double_star() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_all_tools_with_empty_allowlist() {
    // Empty allowlist = no constraint (allow all), but wildcard deny overrides
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Read").allowed);
}

#[test]
fn fully_restrictive_combined() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["*"]),
        deny_read: sv(&["**"]),
        deny_write: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new("x.txt")).allowed);
    assert!(!e.can_write_path(Path::new("x.txt")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. Include/exclude priority rules
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn exclude_overrides_include_for_tools() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn allowlist_restricts_to_only_listed_tools() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn deny_overrides_for_path_specific_tool() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Bash"]),
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn read_deny_independent_of_write_deny() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["*.secret"]),
        deny_write: sv(&["*.lock"]),
        ..PolicyProfile::default()
    });
    // .secret is read-denied but write-allowed
    assert!(!e.can_read_path(Path::new("data.secret")).allowed);
    assert!(e.can_write_path(Path::new("data.secret")).allowed);
    // .lock is write-denied but read-allowed
    assert!(e.can_read_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
}

#[test]
fn wildcard_deny_beats_wildcard_allow() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["*"]),
        ..PolicyProfile::default()
    });
    // Deny should override allow
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn specific_deny_in_broader_allow() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["File*"]),
        disallowed_tools: sv(&["FileDelete"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileDelete").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. Policy serialization roundtrip
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn profile_json_roundtrip_default() {
    let p = PolicyProfile::default();
    let json = serde_json::to_string(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert!(p2.allowed_tools.is_empty());
    assert!(p2.disallowed_tools.is_empty());
    assert!(p2.deny_read.is_empty());
    assert!(p2.deny_write.is_empty());
}

#[test]
fn profile_json_roundtrip_all_fields() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.com"]),
        require_approval_for: sv(&["Bash"]),
    };
    let json = serde_json::to_string(&p).unwrap();
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
fn profile_json_roundtrip_preserves_engine_behavior() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    let e1 = engine(&p);
    let e2 = engine(&p2);

    assert_eq!(e1.can_use_tool("Read").allowed, e2.can_use_tool("Read").allowed);
    assert_eq!(e1.can_use_tool("Bash").allowed, e2.can_use_tool("Bash").allowed);
    assert_eq!(
        e1.can_read_path(Path::new(".env")).allowed,
        e2.can_read_path(Path::new(".env")).allowed
    );
    assert_eq!(
        e1.can_write_path(Path::new(".git/config")).allowed,
        e2.can_write_path(Path::new(".git/config")).allowed
    );
}

#[test]
fn profile_pretty_json_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let json = serde_json::to_string_pretty(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(p.allowed_tools, p2.allowed_tools);
    assert_eq!(p.deny_write, p2.deny_write);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Additional edge cases and combinations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn path_traversal_in_read_deny() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/etc/passwd"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

#[test]
fn path_traversal_in_write_deny() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("../.git/config"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

#[test]
fn complex_combined_policy() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        disallowed_tools: sv(&["Write"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/locked/**"]),
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
fn decision_allow_has_no_reason() {
    let d = abp_policy::Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_has_reason() {
    let d = abp_policy::Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

#[test]
fn decision_deny_accepts_string() {
    let d = abp_policy::Decision::deny(String::from("custom reason"));
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("custom reason"));
}

#[test]
fn tool_case_sensitivity() {
    let e = engine(&PolicyProfile {
        disallowed_tools: sv(&["bash"]),
        ..PolicyProfile::default()
    });
    // Globs are case-sensitive by default
    assert!(!e.can_use_tool("bash").allowed);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("BASH").allowed);
}

#[test]
fn empty_string_tool_name() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn empty_string_tool_with_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("").allowed);
}

#[test]
fn single_tool_allowlist() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["OnlyThis"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("OnlyThis").allowed);
    assert!(!e.can_use_tool("SomethingElse").allowed);
}

#[test]
fn deny_read_does_not_affect_write() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("file.txt")).allowed);
    assert!(e.can_write_path(Path::new("file.txt")).allowed);
}

#[test]
fn deny_write_does_not_affect_read() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("file.txt")).allowed);
}

#[test]
fn many_deny_read_patterns_all_checked() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&[
            "*.key", "*.pem", "*.crt", "*.p12", "*.jks", "*.pfx",
        ]),
        ..PolicyProfile::default()
    });
    for ext in &["key", "pem", "crt", "p12", "jks", "pfx"] {
        let path = format!("cert.{ext}");
        assert!(!e.can_read_path(Path::new(&path)).allowed, "should deny {path}");
    }
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

#[test]
fn many_deny_write_patterns_all_checked() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&[
            "**/.git/**", "**/node_modules/**", "**/target/**",
            "*.lock", "*.log",
        ]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("node_modules/x/y.js")).allowed);
    assert!(!e.can_write_path(Path::new("target/debug/app")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("app.log")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn engine_clone_preserves_behavior() {
    let e1 = engine(&PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    let e2 = e1.clone();
    assert_eq!(e1.can_use_tool("Bash").allowed, e2.can_use_tool("Bash").allowed);
    assert_eq!(
        e1.can_read_path(Path::new(".env")).allowed,
        e2.can_read_path(Path::new(".env")).allowed
    );
    assert_eq!(
        e1.can_write_path(Path::new(".git/config")).allowed,
        e2.can_write_path(Path::new(".git/config")).allowed
    );
}

#[test]
fn overlapping_deny_read_patterns() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/secret*", "secret/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("secret/file.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/secret_data")).allowed);
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn overlapping_deny_write_patterns() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["build/**", "**/build/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("build/out.js")).allowed);
    assert!(!e.can_write_path(Path::new("a/build/out.js")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn profile_with_only_network_fields_compiles() {
    let p = PolicyProfile {
        allow_network: sv(&["api.example.com"]),
        deny_network: sv(&["evil.com"]),
        ..PolicyProfile::default()
    };
    // Engine should compile even though network checks aren't enforced yet
    let _e = engine(&p);
}

#[test]
fn profile_with_only_require_approval_compiles() {
    let p = PolicyProfile {
        require_approval_for: sv(&["Bash", "Delete"]),
        ..PolicyProfile::default()
    };
    let _e = engine(&p);
}

#[test]
fn engine_multiple_compilations_same_profile() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..PolicyProfile::default()
    };
    let e1 = engine(&p);
    let e2 = engine(&p);
    assert_eq!(e1.can_use_tool("Bash").allowed, e2.can_use_tool("Bash").allowed);
    assert_eq!(e1.can_use_tool("Read").allowed, e2.can_use_tool("Read").allowed);
}

#[test]
fn tool_name_with_special_chars() {
    let e = engine(&PolicyProfile {
        allowed_tools: sv(&["my-tool", "my_tool", "my.tool"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("my-tool").allowed);
    assert!(e.can_use_tool("my_tool").allowed);
    assert!(e.can_use_tool("my.tool").allowed);
    assert!(!e.can_use_tool("other").allowed);
}

#[test]
fn read_path_with_many_segments() {
    let e = engine(&PolicyProfile {
        deny_read: sv(&["**/deep/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("a/b/c/deep/d/e/f.txt")).allowed);
    assert!(e.can_read_path(Path::new("a/b/c/d/e/f.txt")).allowed);
}

#[test]
fn write_path_with_many_segments() {
    let e = engine(&PolicyProfile {
        deny_write: sv(&["**/protected/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("x/y/protected/z/w.txt")).allowed);
    assert!(e.can_write_path(Path::new("x/y/z/w.txt")).allowed);
}
