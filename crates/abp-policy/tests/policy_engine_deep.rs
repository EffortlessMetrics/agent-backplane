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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive tests for the abp-policy engine — tool/read/write allow/deny,
//! network policies, defaults, priority, glob patterns, profiles, composition,
//! edge cases, serde roundtrips, and error messages.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine(p: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(p).expect("compile policy")
}

fn s(v: &str) -> String {
    v.to_string()
}

// ===========================================================================
// 1. Tool allow/deny — glob patterns for tool access
// ===========================================================================

#[test]
fn tool_deny_overrides_wildcard_allow() {
    let p = PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn tool_allowlist_blocks_unlisted_tools() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_glob_pattern_deny() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bash*"), s("Shell*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(!e.can_use_tool("ShellCmd").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_empty_policy_permits_all() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Anything").allowed);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn tool_multiple_deny_patterns() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bash"), s("Shell"), s("Exec"), s("Run*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(!e.can_use_tool("RunCommand").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
}

#[test]
fn tool_deny_and_allow_same_name() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Bash")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn tool_case_sensitive_names() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("bash").allowed);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("BASH").allowed);
}

#[test]
fn tool_question_mark_glob() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bas?")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Bass").allowed);
    assert!(e.can_use_tool("Ba").allowed);
    assert!(e.can_use_tool("Basher").allowed);
}

// ===========================================================================
// 2. Read allow/deny — file read path patterns
// ===========================================================================

#[test]
fn read_deny_blocks_env_files() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.env"), s("**/.env.*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_deny_double_star_recursive() {
    let p = PolicyProfile {
        deny_read: vec![s("**/secrets/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secrets/key.pem")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/secrets/key.pem")).allowed);
    assert!(
        !e.can_read_path(Path::new("secrets/nested/deep/key"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_no_deny_allows_everything() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/path/here.txt")).allowed);
    assert!(e.can_read_path(Path::new(".git/config")).allowed);
}

#[test]
fn read_multiple_deny_patterns() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.env"), s("**/.env.*"), s("**/id_rsa"), s("**/*.key")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.key")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_extension_based_deny() {
    let p = PolicyProfile {
        deny_read: vec![s("**/*.pem"), s("**/*.key")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
    assert!(!e.can_read_path(Path::new("deep/nested/server.key")).allowed);
    assert!(e.can_read_path(Path::new("cert.txt")).allowed);
}

#[test]
fn read_dotfile_deny() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_deny_specific_directory() {
    let p = PolicyProfile {
        deny_read: vec![s("vendor/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("vendor/pkg/lib.js")).allowed);
    assert!(
        !e.can_read_path(Path::new("vendor/deep/nested/file"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// 3. Write allow/deny — file write path patterns
// ===========================================================================

#[test]
fn write_deny_git_directory() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/HEAD")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_multiple_patterns() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**"), s("**/node_modules/**"), s("*.lock")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn write_deny_deeply_nested() {
    let p = PolicyProfile {
        deny_write: vec![s("locked/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("locked/a/b/c/d.txt")).allowed);
    assert!(!e.can_write_path(Path::new("locked/x.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn write_deny_lock_files() {
    let p = PolicyProfile {
        deny_write: vec![s("*.lock"), s("**/package-lock.json")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("package-lock.json")).allowed);
    assert!(!e.can_write_path(Path::new("sub/package-lock.json")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_specific_file() {
    let p = PolicyProfile {
        deny_write: vec![s("**/README.md")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("README.md")).allowed);
    assert!(!e.can_write_path(Path::new("sub/README.md")).allowed);
    assert!(e.can_write_path(Path::new("CHANGELOG.md")).allowed);
}

#[test]
fn write_no_deny_allows_everything() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn write_deny_combined_with_read_deny() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    // .env denied for read, but allowed for write
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_write_path(Path::new(".env")).allowed);
    // .git denied for write, but allowed for read
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_read_path(Path::new(".git/config")).allowed);
}

// ===========================================================================
// 4. Network policies — network access control
// ===========================================================================

#[test]
fn network_allow_stored_on_profile() {
    let p = PolicyProfile {
        allow_network: vec![s("*.example.com"), s("api.github.com")],
        ..PolicyProfile::default()
    };
    assert_eq!(p.allow_network.len(), 2);
    assert!(p.allow_network.contains(&s("*.example.com")));
    assert!(p.allow_network.contains(&s("api.github.com")));
}

#[test]
fn network_deny_stored_on_profile() {
    let p = PolicyProfile {
        deny_network: vec![s("evil.com"), s("*.malware.net")],
        ..PolicyProfile::default()
    };
    assert_eq!(p.deny_network.len(), 2);
    assert!(p.deny_network.contains(&s("evil.com")));
}

#[test]
fn network_both_allow_and_deny() {
    let p = PolicyProfile {
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.example.com")],
        ..PolicyProfile::default()
    };
    let _e = engine(&p);
    assert_eq!(p.allow_network, vec!["*.example.com"]);
    assert_eq!(p.deny_network, vec!["evil.example.com"]);
}

#[test]
fn network_empty_by_default() {
    let p = PolicyProfile::default();
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
}

#[test]
fn network_serde_roundtrip() {
    let p = PolicyProfile {
        allow_network: vec![s("*.trusted.com")],
        deny_network: vec![s("evil.com")],
        ..PolicyProfile::default()
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(p.allow_network, p2.allow_network);
    assert_eq!(p.deny_network, p2.deny_network);
}

// ===========================================================================
// 5. Default policies — default allow/deny behavior
// ===========================================================================

#[test]
fn default_profile_has_empty_fields() {
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
fn default_engine_compiles() {
    let _e = engine(&PolicyProfile::default());
}

#[test]
fn default_engine_permits_any_tool() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("AnyToolName").allowed);
}

#[test]
fn default_engine_permits_any_read() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_read_path(Path::new(".git/config")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn default_engine_permits_any_write() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_write_path(Path::new("node_modules/pkg")).allowed);
}

// ===========================================================================
// 6. Priority — deny overrides allow
// ===========================================================================

#[test]
fn priority_tool_deny_beats_wildcard_allow() {
    let p = PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn priority_tool_in_both_lists_deny_wins() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Bash"), s("Read")],
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn priority_wildcard_deny_overrides_specific_allow() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn priority_composed_deny_overrides_strategy() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
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
fn priority_all_must_allow_single_deny_vetoes() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", engine(&PolicyProfile::default()));
    cp.add_policy(
        "restrictive",
        engine(&PolicyProfile {
            disallowed_tools: vec![s("Bash")],
            ..PolicyProfile::default()
        }),
    );
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn priority_deny_write_overrides_unspecified() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// 7. Glob patterns — complex glob pattern matching
// ===========================================================================

#[test]
fn glob_question_mark_matches_single_char() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bas?")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Bass").allowed);
    assert!(e.can_use_tool("Ba").allowed);
    assert!(e.can_use_tool("Basher").allowed);
}

#[test]
fn glob_brace_expansion() {
    let p = PolicyProfile {
        deny_write: vec![s("*.{lock,bak}")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("data.bak")).allowed);
    assert!(e.can_write_path(Path::new("main.rs")).allowed);
}

#[test]
fn glob_double_star_matches_across_directories() {
    let p = PolicyProfile {
        deny_read: vec![s("**/secrets/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secrets/key")).allowed);
    assert!(!e.can_read_path(Path::new("a/secrets/key")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/secrets/deep/key")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_star_matches_single_segment() {
    let p = PolicyProfile {
        deny_read: vec![s("secret*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_read_path(Path::new("secrets")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_extension_pattern() {
    let p = PolicyProfile {
        deny_read: vec![s("**/*.pem"), s("**/*.key"), s("**/*.p12")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
    assert!(!e.can_read_path(Path::new("nested/server.key")).allowed);
    assert!(!e.can_read_path(Path::new("deep/client.p12")).allowed);
    assert!(e.can_read_path(Path::new("cert.txt")).allowed);
}

#[test]
fn glob_complex_nested_pattern() {
    let p = PolicyProfile {
        deny_write: vec![s("**/config/**/*.secret"), s("**/.git/**")],
        deny_read: vec![s("**/.env"), s("**/.env.*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_write_path(Path::new("config/db/conn.secret")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.local")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_unicode_in_patterns() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("outil*")],
        deny_read: vec![s("données/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("outil_spécial").allowed);
    assert!(!e.can_read_path(Path::new("données/fichier.txt")).allowed);
    assert!(e.can_use_tool("normal").allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_multiple_star_patterns_in_allowlist() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read*"), s("Grep*"), s("List*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(e.can_use_tool("GrepSearch").allowed);
    assert!(e.can_use_tool("ListDir").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

// ===========================================================================
// 8. Policy profiles — named policy profiles
// ===========================================================================

#[test]
fn profile_with_all_fields() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.com")],
        require_approval_for: vec![s("DeleteFile")],
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
fn profile_require_approval_for() {
    let p = PolicyProfile {
        require_approval_for: vec![s("Bash"), s("DeleteFile"), s("Write")],
        ..PolicyProfile::default()
    };
    assert_eq!(p.require_approval_for.len(), 3);
    assert!(p.require_approval_for.contains(&s("Bash")));
    assert!(p.require_approval_for.contains(&s("DeleteFile")));
}

#[test]
fn profile_engine_compiles_with_complex_globs() {
    let p = PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash*"), s("Shell*")],
        deny_read: vec![s("**/.env"), s("**/.env.*"), s("**/id_rsa")],
        deny_write: vec![s("**/.git/**"), s("**/locked/**")],
        ..PolicyProfile::default()
    };
    let _e = engine(&p);
}

#[test]
fn profile_with_many_deny_patterns() {
    let p = PolicyProfile {
        deny_read: vec![
            s("**/.env"),
            s("**/.env.*"),
            s("**/id_rsa"),
            s("**/*.pem"),
            s("**/*.key"),
            s("**/*.p12"),
            s("**/secrets/**"),
        ],
        deny_write: vec![
            s("**/.git/**"),
            s("**/node_modules/**"),
            s("*.lock"),
            s("**/dist/**"),
            s("**/build/**"),
        ],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn profile_policy_set_merge() {
    let mut set = PolicySet::new("merged");
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Shell")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Shell")));
    assert!(merged.deny_read.contains(&s("**/.env")));
    assert!(merged.deny_write.contains(&s("**/.git/**")));
}

// ===========================================================================
// 9. Policy composition — multiple policies combined
// ===========================================================================

fn make_permissive() -> PolicyEngine {
    engine(&PolicyProfile::default())
}

fn make_restrictive() -> PolicyEngine {
    engine(&PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    })
}

#[test]
fn composed_all_must_allow_denies_on_single_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", make_permissive());
    cp.add_policy("restrictive", make_restrictive());
    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_all_must_allow_allows_when_all_agree() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("a", make_permissive());
    cp.add_policy("b", make_permissive());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_any_must_allow_permits_single_allow() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("restrictive", make_restrictive());
    cp.add_policy("permissive", make_permissive());
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_any_must_allow_denies_when_all_deny() {
    let a = engine(&PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..PolicyProfile::default()
    });
    let b = engine(&PolicyProfile {
        allowed_tools: vec![s("Grep")],
        ..PolicyProfile::default()
    });
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("a", a);
    cp.add_policy("b", b);
    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_first_match_uses_first_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy("restrictive", make_restrictive());
    cp.add_policy("permissive", make_permissive());
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_empty_returns_allowed() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("Anything").is_allowed());
}

#[test]
fn composed_policy_count() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    cp.add_policy("a", make_permissive());
    assert_eq!(cp.policy_count(), 1);
    cp.add_policy("b", make_restrictive());
    assert_eq!(cp.policy_count(), 2);
}

#[test]
fn composed_evaluate_read_and_write() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("restrictive", make_restrictive());
    assert!(cp.evaluate_read("src/main.rs").is_allowed());
    assert!(cp.evaluate_write(".git/config").is_denied());
    assert!(cp.evaluate_write("src/main.rs").is_allowed());
}

#[test]
fn composed_result_attributes_deny_to_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("strict-policy", make_restrictive());
    if let ComposedResult::Denied { by, .. } = cp.evaluate_tool("Bash") {
        assert_eq!(by, "strict-policy");
    } else {
        panic!("expected denial");
    }
}

#[test]
fn composed_engine_allow_overrides() {
    let restrictive = PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile::default();
    let ce = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_first_applicable() {
    let restrictive = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile::default();
    let ce = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_empty_returns_abstain() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Anything").is_abstain());
}

#[test]
fn composed_engine_read_and_write() {
    let p = PolicyProfile {
        deny_read: vec![s("**/secret*")],
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(vec![p], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read("secret.txt").is_deny());
    assert!(ce.check_read("src/main.rs").is_allow());
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/main.rs").is_allow());
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
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
    let set = PolicySet::new("test-set");
    assert_eq!(set.name(), "test-set");
}

// ===========================================================================
// 10. Edge cases — empty patterns, wildcard-only, nested paths
// ===========================================================================

#[test]
fn edge_empty_tool_name() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn edge_empty_path() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("")).allowed);
    assert!(e.can_write_path(Path::new("")).allowed);
}

#[test]
fn edge_wildcard_only_deny_all_tools() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("*")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("").allowed);
}

#[test]
fn edge_path_traversal_read() {
    let p = PolicyProfile {
        deny_read: vec![s("**/etc/passwd")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("denied"));
}

#[test]
fn edge_path_traversal_write() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d = e.can_write_path(Path::new("../.git/config"));
    assert!(!d.allowed);
    assert!(d.reason.unwrap().contains("denied"));
}

#[test]
fn edge_invalid_glob_rejected() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("[invalid")],
        ..PolicyProfile::default()
    };
    assert!(PolicyEngine::new(&p).is_err());
}

#[test]
fn edge_validator_detects_empty_globs() {
    let p = PolicyProfile {
        allowed_tools: vec![s("")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn edge_validator_detects_overlapping_allow_deny() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Bash")],
        disallowed_tools: vec![s("Bash")],
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
fn edge_validator_detects_unreachable_rules() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![s("*")],
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
fn edge_validator_catch_all_deny_read() {
    let p = PolicyProfile {
        deny_read: vec![s("**")],
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
fn edge_validator_clean_profile_no_warnings() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

// ===========================================================================
// 11. Serde roundtrip — policy serializes correctly
// ===========================================================================

#[test]
fn serde_policy_profile_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.com")],
        require_approval_for: vec![s("DeleteFile")],
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
fn serde_policy_profile_from_json() {
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

#[test]
fn serde_composition_strategy_roundtrip() {
    let strategies = vec![
        CompositionStrategy::AllMustAllow,
        CompositionStrategy::AnyMustAllow,
        CompositionStrategy::FirstMatch,
    ];
    for strat in &strategies {
        let json = serde_json::to_string(strat).unwrap();
        let s2: CompositionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*strat, s2);
    }
}

#[test]
fn serde_composed_result_roundtrip() {
    let allowed = ComposedResult::Allowed { by: s("test") };
    let denied = ComposedResult::Denied {
        by: s("strict"),
        reason: s("disallowed"),
    };
    for r in &[allowed, denied] {
        let json = serde_json::to_string(r).unwrap();
        let r2: ComposedResult = serde_json::from_str(&json).unwrap();
        assert_eq!(*r, r2);
    }
}

#[test]
fn serde_decision_allow_fields() {
    let d = abp_policy::Decision::allow();
    let json = serde_json::to_string(&d).unwrap();
    let d2: abp_policy::Decision = serde_json::from_str(&json).unwrap();
    assert!(d2.allowed);
    assert!(d2.reason.is_none());
}

#[test]
fn serde_decision_deny_fields() {
    let d = abp_policy::Decision::deny("not allowed");
    let json = serde_json::to_string(&d).unwrap();
    let d2: abp_policy::Decision = serde_json::from_str(&json).unwrap();
    assert!(!d2.allowed);
    assert_eq!(d2.reason.as_deref(), Some("not allowed"));
}

// ===========================================================================
// 12. Error messages — clear policy violation messages
// ===========================================================================

#[test]
fn error_tool_deny_reason_contains_tool_name() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("DangerousTool")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_use_tool("DangerousTool");
    assert!(!d.allowed);
    let reason = d.reason.unwrap();
    assert!(reason.contains("DangerousTool"), "reason was: {reason}");
    assert!(reason.contains("disallowed"), "reason was: {reason}");
}

#[test]
fn error_missing_allowlist_reason() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_use_tool("Write");
    assert!(!d.allowed);
    let reason = d.reason.unwrap();
    assert!(reason.contains("Write"), "reason was: {reason}");
    assert!(reason.contains("not in allowlist"), "reason was: {reason}");
}

#[test]
fn error_write_deny_reason_contains_path() {
    let p = PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_write_path(Path::new(".git/config"));
    assert!(!d.allowed);
    let reason = d.reason.unwrap();
    assert!(reason.contains("denied"), "reason was: {reason}");
    assert!(reason.contains(".git"), "reason was: {reason}");
}

#[test]
fn error_read_deny_reason_contains_path() {
    let p = PolicyProfile {
        deny_read: vec![s("**/.env")],
        ..PolicyProfile::default()
    };
    let d = engine(&p).can_read_path(Path::new(".env"));
    assert!(!d.allowed);
    let reason = d.reason.unwrap();
    assert!(reason.contains("denied"), "reason was: {reason}");
    assert!(reason.contains(".env"), "reason was: {reason}");
}

#[test]
fn error_composed_deny_includes_engine_name() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy(
        "security-policy",
        engine(&PolicyProfile {
            disallowed_tools: vec![s("Bash")],
            ..PolicyProfile::default()
        }),
    );
    if let ComposedResult::Denied { by, reason } = cp.evaluate_tool("Bash") {
        assert_eq!(by, "security-policy");
        assert!(reason.contains("Bash"), "reason was: {reason}");
    } else {
        panic!("expected denial");
    }
}

#[test]
fn error_composed_engine_deny_has_reason() {
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..PolicyProfile::default()
    };
    let ce = ComposedEngine::new(vec![p], PolicyPrecedence::DenyOverrides).unwrap();
    let d = ce.check_tool("Bash");
    assert!(d.is_deny());
    if let abp_policy::compose::PolicyDecision::Deny { reason } = d {
        assert!(reason.contains("Bash"), "reason was: {reason}");
    }
}
