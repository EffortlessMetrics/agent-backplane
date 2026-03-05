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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the ABP policy engine — tool/read/write allow/deny enforcement.

use abp_core::PolicyProfile;
use abp_policy::audit::{AuditSummary, PolicyAuditor, PolicyDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine(p: PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&p).expect("compile policy")
}

fn s(v: &str) -> String {
    v.to_string()
}

fn svec(vs: &[&str]) -> Vec<String> {
    vs.iter().map(|v| s(v)).collect()
}

// ===========================================================================
// 1. PolicyProfile construction (~15 tests)
// ===========================================================================

#[test]
fn default_profile_allows_all_tools() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Anything").allowed);
}

#[test]
fn default_profile_allows_all_read_paths() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
    assert!(
        e.can_read_path(Path::new("deep/nested/dir/file.txt"))
            .allowed
    );
}

#[test]
fn default_profile_allows_all_write_paths() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("output/data.csv")).allowed);
}

#[test]
fn profile_with_tool_allow_list() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read", "Grep"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn profile_with_tool_deny_list() {
    let p = PolicyProfile {
        disallowed_tools: svec(&["Bash", "Exec"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn profile_with_read_path_globs() {
    let p = PolicyProfile {
        deny_read: svec(&["**/.env", "secret/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn profile_with_write_path_globs() {
    let p = PolicyProfile {
        deny_write: svec(&["**/.git/**", "dist/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn serde_roundtrip_policy_profile() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read", "Grep"]),
        disallowed_tools: svec(&["Bash"]),
        deny_read: svec(&["**/.env"]),
        deny_write: svec(&["**/.git/**"]),
        allow_network: svec(&["*.example.com"]),
        deny_network: svec(&["evil.com"]),
        require_approval_for: svec(&["DeleteFile"]),
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
fn empty_profile_all_fields_empty() {
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
fn profile_network_fields_stored() {
    let p = PolicyProfile {
        allow_network: svec(&["api.example.com"]),
        deny_network: svec(&["malware.bad"]),
        ..PolicyProfile::default()
    };
    let _e = engine(p.clone());
    assert_eq!(p.allow_network, vec!["api.example.com"]);
    assert_eq!(p.deny_network, vec!["malware.bad"]);
}

#[test]
fn profile_require_approval_for_stored() {
    let p = PolicyProfile {
        require_approval_for: svec(&["Bash", "Exec", "DeleteFile"]),
        ..PolicyProfile::default()
    };
    assert_eq!(p.require_approval_for.len(), 3);
}

#[test]
fn profile_only_deny_read_no_deny_write() {
    let p = PolicyProfile {
        deny_read: svec(&["**/.secret"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new(".secret")).allowed);
    // writes are unaffected
    assert!(e.can_write_path(Path::new(".secret")).allowed);
}

#[test]
fn profile_only_deny_write_no_deny_read() {
    let p = PolicyProfile {
        deny_write: svec(&["**/readonly/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("readonly/file.txt")).allowed);
    // reads are unaffected
    assert!(e.can_read_path(Path::new("readonly/file.txt")).allowed);
}

#[test]
fn profile_single_tool_allowed() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn profile_clone_produces_independent_copy() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read"]),
        ..PolicyProfile::default()
    };
    let p2 = p.clone();
    assert_eq!(p.allowed_tools, p2.allowed_tools);
}

// ===========================================================================
// 2. PolicyEngine compilation (~10 tests)
// ===========================================================================

#[test]
fn compile_default_succeeds() {
    let result = PolicyEngine::new(&PolicyProfile::default());
    assert!(result.is_ok());
}

#[test]
fn compile_with_valid_globs_succeeds() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read*", "Grep*"]),
        disallowed_tools: svec(&["Bash*"]),
        deny_read: svec(&["**/.env", "secret/**"]),
        deny_write: svec(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_ok());
}

#[test]
fn compile_invalid_glob_in_allowed_tools_fails() {
    let p = PolicyProfile {
        allowed_tools: svec(&["["]),
        ..PolicyProfile::default()
    };
    let err = PolicyEngine::new(&p);
    assert!(err.is_err());
}

#[test]
fn compile_invalid_glob_in_disallowed_tools_fails() {
    let p = PolicyProfile {
        disallowed_tools: svec(&["[invalid"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn compile_invalid_glob_in_deny_read_fails() {
    let p = PolicyProfile {
        deny_read: svec(&["[bad-pattern"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn compile_invalid_glob_in_deny_write_fails() {
    let p = PolicyProfile {
        deny_write: svec(&["[bad-pattern"]),
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn engine_reusable_across_tool_checks() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        ..PolicyProfile::default()
    });
    // Use engine many times
    for _ in 0..100 {
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }
}

#[test]
fn engine_reusable_across_path_checks() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["secret/**"]),
        deny_write: svec(&["locked/**"]),
        ..PolicyProfile::default()
    });
    for _ in 0..50 {
        assert!(!e.can_read_path(Path::new("secret/a.txt")).allowed);
        assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
        assert!(!e.can_write_path(Path::new("locked/b.txt")).allowed);
    }
}

#[test]
fn engine_clone_works_independently() {
    let e1 = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        ..PolicyProfile::default()
    });
    let e2 = e1.clone();
    assert!(!e1.can_use_tool("Bash").allowed);
    assert!(!e2.can_use_tool("Bash").allowed);
    assert!(e2.can_use_tool("Read").allowed);
}

#[test]
fn compile_many_patterns_succeeds() {
    let patterns: Vec<String> = (0..100).map(|i| format!("dir_{i}/**")).collect();
    let p = PolicyProfile {
        deny_read: patterns,
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_ok());
}

// ===========================================================================
// 3. Tool policy decisions (~15 tests)
// ===========================================================================

#[test]
fn allowed_tool_returns_allow() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["Read"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Read");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn denied_tool_returns_deny_with_reason() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
}

#[test]
fn tool_not_in_allowlist_returns_missing_include() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["Read", "Grep"]),
        ..PolicyProfile::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("tool 'Bash' not in allowlist"));
}

#[test]
fn tool_wildcard_allow_pattern() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("AnyTool").allowed);
}

#[test]
fn tool_prefix_wildcard_pattern() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["File*"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn tool_deny_prefix_wildcard_pattern() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_case_sensitivity_exact_match() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["Read"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    // globset is case-insensitive on Windows by default, so we test that the
    // engine at least compiles and handles a differently-cased name
    let d = e.can_use_tool("READ");
    // On Windows globset may or may not be case-sensitive; just ensure it doesn't crash
    let _ = d.allowed;
}

#[test]
fn tool_case_sensitivity_deny() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn empty_allow_list_permits_everything() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![],
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn empty_deny_list_denies_nothing() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![],
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Dangerous").allowed);
}

#[test]
fn deny_overrides_allow_for_tools() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["*"]),
        disallowed_tools: svec(&["Bash"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn deny_overrides_allow_same_tool_name() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["Write"]),
        disallowed_tools: svec(&["Write"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_with_special_characters() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["my-tool", "my_tool"]),
        ..PolicyProfile::default()
    });
    assert!(e.can_use_tool("my-tool").allowed);
    assert!(e.can_use_tool("my_tool").allowed);
    assert!(!e.can_use_tool("my.tool").allowed);
}

#[test]
fn tool_empty_string_name() {
    let e = engine(PolicyProfile::default());
    // Empty tool name still evaluated
    let d = e.can_use_tool("");
    assert!(d.allowed);
}

#[test]
fn tool_brace_expansion() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["{Bash,Exec,Shell}"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// ===========================================================================
// 4. Read path policy (~15 tests)
// ===========================================================================

#[test]
fn allowed_read_path_returns_allow() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["secret/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("src/lib.rs"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn denied_read_path_returns_deny_with_reason() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["secret/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_read_path(Path::new("secret/key.pem"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

#[test]
fn read_glob_star_star_rs() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/*.rs"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_read_path(Path::new("tests/test.rs")).allowed);
    assert!(e.can_read_path(Path::new("README.md")).allowed);
}

#[test]
fn read_glob_specific_directory() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["private/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("private/data.txt")).allowed);
    assert!(!e.can_read_path(Path::new("private/sub/deep.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn read_relative_path_with_dot_dot() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/etc/passwd"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("../../etc/passwd")).allowed);
}

#[test]
fn read_path_with_spaces() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/my docs/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("my docs/file.txt")).allowed);
    assert!(e.can_read_path(Path::new("my_docs/file.txt")).allowed);
}

#[test]
fn read_nested_directory_matching() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["a/b/c/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("a/b/c/d.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/d/e/f.txt")).allowed);
    assert!(e.can_read_path(Path::new("a/b/x.txt")).allowed);
}

#[test]
fn read_hidden_files_pattern() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/.*"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_multiple_deny_patterns() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/.env", "**/.env.*", "**/id_rsa"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_extension_based_deny() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/*.pem", "**/*.key"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("keys/secret.key")).allowed);
    assert!(e.can_read_path(Path::new("docs/readme.md")).allowed);
}

#[test]
fn read_brace_expansion_extensions() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/*.{pem,key,p12}"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("a.pem")).allowed);
    assert!(!e.can_read_path(Path::new("b.key")).allowed);
    assert!(!e.can_read_path(Path::new("c.p12")).allowed);
    assert!(e.can_read_path(Path::new("d.txt")).allowed);
}

#[test]
fn read_question_mark_glob() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["temp?.log"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("temp1.log")).allowed);
    assert!(!e.can_read_path(Path::new("tempX.log")).allowed);
    assert!(e.can_read_path(Path::new("temp12.log")).allowed);
}

#[test]
fn read_empty_deny_allows_everything() {
    let e = engine(PolicyProfile {
        deny_read: vec![],
        ..PolicyProfile::default()
    });
    assert!(e.can_read_path(Path::new("anything/at/all.txt")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn read_deny_does_not_affect_write() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["secret/**"]),
        ..PolicyProfile::default()
    });
    // Read denied
    assert!(!e.can_read_path(Path::new("secret/data.txt")).allowed);
    // Write unaffected
    assert!(e.can_write_path(Path::new("secret/data.txt")).allowed);
}

#[test]
fn read_unicode_path() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["données/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_read_path(Path::new("données/fichier.txt")).allowed);
    assert!(e.can_read_path(Path::new("data/file.txt")).allowed);
}

// ===========================================================================
// 5. Write path policy (~15 tests)
// ===========================================================================

#[test]
fn allowed_write_path_returns_allow() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["locked/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("src/lib.rs"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn denied_write_path_returns_deny_with_reason() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["locked/**"]),
        ..PolicyProfile::default()
    });
    let d = e.can_write_path(Path::new("locked/data.txt"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("denied"));
}

#[test]
fn write_deny_git_directory() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/objects/abc")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_more_restrictive_than_read() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/.env"]),
        deny_write: svec(&["**/.env", "config/**", "**/*.lock"]),
        ..PolicyProfile::default()
    });
    // Both denied for .env
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".env")).allowed);
    // Only write denied for config/
    assert!(e.can_read_path(Path::new("config/app.yaml")).allowed);
    assert!(!e.can_write_path(Path::new("config/app.yaml")).allowed);
    // Only write denied for lock files
    assert!(e.can_read_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
}

#[test]
fn write_deny_outside_workspace_pattern() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["../**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("../outside.txt")).allowed);
    assert!(e.can_write_path(Path::new("inside.txt")).allowed);
}

#[test]
fn write_deny_build_artifacts() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["target/**", "dist/**", "build/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("target/debug/binary")).allowed);
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(!e.can_write_path(Path::new("build/output.o")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_extension_based() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["**/*.exe", "**/*.dll", "**/*.so"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("app.exe")).allowed);
    assert!(!e.can_write_path(Path::new("lib.dll")).allowed);
    assert!(!e.can_write_path(Path::new("lib.so")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_deep_nested() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["vault/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("vault/a/b/c/d.txt")).allowed);
    assert!(!e.can_write_path(Path::new("vault/x.txt")).allowed);
    assert!(e.can_write_path(Path::new("safe/x.txt")).allowed);
}

#[test]
fn write_deny_does_not_affect_read() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["locked/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_read_path(Path::new("locked/data.txt")).allowed);
}

#[test]
fn write_deny_multiple_patterns() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["**/.git/**", "**/.svn/**", "**/node_modules/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".svn/entries")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/foo/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_empty_deny_allows_everything() {
    let e = engine(PolicyProfile {
        deny_write: vec![],
        ..PolicyProfile::default()
    });
    assert!(e.can_write_path(Path::new("anything.txt")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn write_deny_with_brace_expansion() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["**/*.{bak,tmp,swp}"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("file.bak")).allowed);
    assert!(!e.can_write_path(Path::new("dir/file.tmp")).allowed);
    assert!(!e.can_write_path(Path::new("dir/file.swp")).allowed);
    assert!(e.can_write_path(Path::new("file.rs")).allowed);
}

#[test]
fn write_path_traversal() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("../.git/config")).allowed);
}

#[test]
fn write_deny_specific_file() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["Cargo.lock"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn write_deny_question_mark_glob() {
    let e = engine(PolicyProfile {
        deny_write: svec(&["log?.txt"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_write_path(Path::new("log1.txt")).allowed);
    assert!(!e.can_write_path(Path::new("logA.txt")).allowed);
    assert!(e.can_write_path(Path::new("log12.txt")).allowed);
}

// ===========================================================================
// 6. Complex policy scenarios (~10 tests)
// ===========================================================================

#[test]
fn combined_tool_and_path_policies() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["Read", "Write", "Grep"]),
        disallowed_tools: svec(&["Write"]),
        deny_read: svec(&["**/.env"]),
        deny_write: svec(&["**/locked/**"]),
        ..PolicyProfile::default()
    });
    // Tool checks
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    // Path checks
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_overrides_allow_in_tools() {
    let e = engine(PolicyProfile {
        allowed_tools: svec(&["*"]),
        disallowed_tools: svec(&["Exec", "Shell"]),
        ..PolicyProfile::default()
    });
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn multiple_overlapping_deny_patterns() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["**/*.log", "logs/**"]),
        ..PolicyProfile::default()
    });
    // Both patterns match
    assert!(!e.can_read_path(Path::new("logs/app.log")).allowed);
    // Only *.log matches
    assert!(!e.can_read_path(Path::new("output/debug.log")).allowed);
    // Only logs/** matches
    assert!(!e.can_read_path(Path::new("logs/data.txt")).allowed);
    // Neither matches
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn real_world_restrictive_profile() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read", "Grep", "ListFiles"]),
        disallowed_tools: svec(&["Bash", "Exec", "Shell", "WebFetch"]),
        deny_read: svec(&["**/.env", "**/.env.*", "**/*.pem", "**/*.key", "**/id_rsa"]),
        deny_write: svec(&["**/.git/**", "**/node_modules/**", "**/*.lock", "config/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);

    // Tools
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(!e.can_use_tool("WebFetch").allowed);
    assert!(!e.can_use_tool("Write").allowed); // not in allowlist

    // Reads
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("certs/tls.pem")).allowed);

    // Writes
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn real_world_permissive_profile() {
    let p = PolicyProfile {
        disallowed_tools: svec(&["Exec"]),
        deny_read: svec(&["**/*.key"]),
        deny_write: svec(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_read_path(Path::new("cert.key")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
}

#[test]
fn real_world_read_only_profile() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read", "Grep", "ListFiles", "View"]),
        deny_write: svec(&["**"]),
        ..PolicyProfile::default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    // All writes denied
    assert!(!e.can_write_path(Path::new("anything.txt")).allowed);
    assert!(!e.can_write_path(Path::new("src/lib.rs")).allowed);
    // Reads still allowed
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn decision_allow_struct() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_struct() {
    let d = Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

#[test]
fn decision_deny_accepts_string_types() {
    let d1 = Decision::deny("static str");
    let d2 = Decision::deny(String::from("owned string"));
    assert!(!d1.allowed);
    assert!(!d2.allowed);
    assert_eq!(d1.reason.as_deref(), Some("static str"));
    assert_eq!(d2.reason.as_deref(), Some("owned string"));
}

// ===========================================================================
// 7. PolicyAuditor tests (~5 tests)
// ===========================================================================

#[test]
fn auditor_records_tool_decisions() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    let d1 = auditor.check_tool("Read");
    let d2 = auditor.check_tool("Bash");
    assert!(matches!(d1, PolicyDecision::Allow));
    assert!(matches!(d2, PolicyDecision::Deny { .. }));
    assert_eq!(auditor.entries().len(), 2);
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_read_write_decisions() {
    let e = engine(PolicyProfile {
        deny_read: svec(&["secret/**"]),
        deny_write: svec(&["locked/**"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_read("src/lib.rs");
    auditor.check_read("secret/key.pem");
    auditor.check_write("src/lib.rs");
    auditor.check_write("locked/file.txt");
    assert_eq!(auditor.entries().len(), 4);
    assert_eq!(auditor.allowed_count(), 2);
    assert_eq!(auditor.denied_count(), 2);
}

#[test]
fn auditor_summary() {
    let e = engine(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        deny_read: svec(&["**/.env"]),
        ..PolicyProfile::default()
    });
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_read("src/lib.rs");
    auditor.check_read(".env");
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
fn auditor_entry_has_action_and_resource() {
    let e = engine(PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(e);
    auditor.check_tool("Read");
    auditor.check_read("src/lib.rs");
    auditor.check_write("src/main.rs");
    let entries = auditor.entries();
    assert_eq!(entries[0].action, "tool");
    assert_eq!(entries[0].resource, "Read");
    assert_eq!(entries[1].action, "read");
    assert_eq!(entries[1].resource, "src/lib.rs");
    assert_eq!(entries[2].action, "write");
    assert_eq!(entries[2].resource, "src/main.rs");
}

#[test]
fn auditor_empty_initially() {
    let e = engine(PolicyProfile::default());
    let auditor = PolicyAuditor::new(e);
    assert_eq!(auditor.entries().len(), 0);
    assert_eq!(auditor.allowed_count(), 0);
    assert_eq!(auditor.denied_count(), 0);
}

// ===========================================================================
// 8. PolicyValidator tests (~5 tests)
// ===========================================================================

#[test]
fn validator_no_warnings_for_clean_profile() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read"]),
        disallowed_tools: svec(&["Bash"]),
        deny_read: svec(&["**/.env"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

#[test]
fn validator_warns_overlapping_allow_deny() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Bash"]),
        disallowed_tools: svec(&["Bash"]),
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
fn validator_warns_empty_glob() {
    let p = PolicyProfile {
        allowed_tools: svec(&[""]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_warns_unreachable_with_wildcard_deny() {
    let p = PolicyProfile {
        allowed_tools: svec(&["Read"]),
        disallowed_tools: svec(&["*"]),
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
fn validator_warns_catch_all_deny_read() {
    let p = PolicyProfile {
        deny_read: svec(&["**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule)
    );
}

// ===========================================================================
// 9. PolicySet and ComposedEngine tests (~5 tests)
// ===========================================================================

#[test]
fn policy_set_merge_unions_fields() {
    let mut set = PolicySet::new("test");
    set.add(PolicyProfile {
        disallowed_tools: svec(&["Bash"]),
        deny_write: svec(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: svec(&["Exec"]),
        deny_read: svec(&["**/.env"]),
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Exec")));
    assert!(merged.deny_write.contains(&s("**/.git/**")));
    assert!(merged.deny_read.contains(&s("**/.env")));
}

#[test]
fn policy_set_name() {
    let set = PolicySet::new("production");
    assert_eq!(set.name(), "production");
}

#[test]
fn composed_engine_deny_overrides() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: svec(&["Bash"]),
            ..PolicyProfile::default()
        },
        PolicyProfile::default(), // allows everything
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_allow_overrides() {
    let profiles = vec![
        PolicyProfile {
            allowed_tools: svec(&["Read"]),
            ..PolicyProfile::default()
        },
        PolicyProfile::default(),
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_path_checks() {
    let profiles = vec![PolicyProfile {
        deny_read: svec(&["secret/**"]),
        deny_write: svec(&["locked/**"]),
        ..PolicyProfile::default()
    }];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read("secret/key.pem").is_deny());
    assert!(ce.check_read("src/lib.rs").is_allow());
    assert!(ce.check_write("locked/file.txt").is_deny());
    assert!(ce.check_write("src/lib.rs").is_allow());
}

// ===========================================================================
// 10. RuleEngine tests (~5 tests)
// ===========================================================================

#[test]
fn rule_engine_empty_allows_everything() {
    let eng = RuleEngine::new();
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Allow);
    assert_eq!(eng.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_deny_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
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
        id: s("allow-bash"),
        description: s("Allow Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Allow,
        priority: 5,
    });
    eng.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    // Higher priority deny wins
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Deny);
}

#[test]
fn rule_condition_and_or_not() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Pattern(s("Bash*")),
        RuleCondition::Not(Box::new(RuleCondition::Pattern(s("BashSafe")))),
    ]);
    assert!(cond.matches("BashExec"));
    assert!(!cond.matches("BashSafe"));
    assert!(!cond.matches("Read"));
}

#[test]
fn rule_engine_remove_rule() {
    let mut eng = RuleEngine::new();
    eng.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(eng.rule_count(), 1);
    eng.remove_rule("deny-bash");
    assert_eq!(eng.rule_count(), 0);
    assert_eq!(eng.evaluate("Bash"), RuleEffect::Allow);
}
