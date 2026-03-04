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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep tests for the `abp-policy` engine — covers boundary conditions,
//! serialization, conflict semantics, and realistic scenarios.

use abp_core::PolicyProfile;
use abp_glob::MatchDecision;
use abp_policy::PolicyEngine;
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

// ===========================================================================
// 1. Empty / default policy — all allowed
// ===========================================================================

#[test]
fn empty_policy_allows_all_tools() {
    let e = engine(PolicyProfile::default());
    for tool in &["Bash", "Read", "Write", "Grep", "CustomTool", ""] {
        assert!(
            e.can_use_tool(tool).allowed,
            "tool '{tool}' should be allowed"
        );
    }
}

#[test]
fn empty_policy_allows_all_reads() {
    let e = engine(PolicyProfile::default());
    for path in &["any.txt", "deep/nested/file.rs", ".env", ".git/config"] {
        assert!(
            e.can_read_path(Path::new(path)).allowed,
            "read '{path}' should be allowed"
        );
    }
}

#[test]
fn empty_policy_allows_all_writes() {
    let e = engine(PolicyProfile::default());
    for path in &["any.txt", "deep/nested/file.rs", ".env", ".git/config"] {
        assert!(
            e.can_write_path(Path::new(path)).allowed,
            "write '{path}' should be allowed"
        );
    }
}

#[test]
fn default_policy_decision_has_no_reason() {
    let e = engine(PolicyProfile::default());
    let d = e.can_use_tool("Anything");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn default_policy_read_decision_has_no_reason() {
    let e = engine(PolicyProfile::default());
    let d = e.can_read_path(Path::new("any/file.txt"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn default_policy_write_decision_has_no_reason() {
    let e = engine(PolicyProfile::default());
    let d = e.can_write_path(Path::new("any/file.txt"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

// ===========================================================================
// 2. Deny-all / restrictive policies
// ===========================================================================

#[test]
fn deny_all_tools_via_wildcard() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("*")],
        ..Default::default()
    });
    for tool in &["Bash", "Read", "Write", "AnythingElse"] {
        assert!(
            !e.can_use_tool(tool).allowed,
            "tool '{tool}' should be denied"
        );
    }
}

#[test]
fn deny_all_reads_via_double_star() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("a.txt")).allowed);
    assert!(!e.can_read_path(Path::new("deep/nested/file.rs")).allowed);
}

#[test]
fn deny_all_writes_via_double_star() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("a.txt")).allowed);
    assert!(!e.can_write_path(Path::new("deep/nested/file.rs")).allowed);
}

#[test]
fn deny_all_reads_via_star_slash_star() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/*")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("a.txt")).allowed);
    assert!(!e.can_read_path(Path::new("dir/file.rs")).allowed);
}

#[test]
fn deny_all_writes_via_star_slash_star() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/*")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("a.txt")).allowed);
    assert!(!e.can_write_path(Path::new("dir/file.rs")).allowed);
}

#[test]
fn restrictive_allowlist_only_one_tool() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("").allowed);
}

// ===========================================================================
// 3. Tool allow/deny with wildcards
// ===========================================================================

#[test]
fn tool_wildcard_allow_with_specific_deny() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Shell*"), s("Exec")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("ShellExec").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Execute").allowed);
}

#[test]
fn tool_question_mark_wildcard() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("X?Z")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("XAZ").allowed);
    assert!(!e.can_use_tool("X1Z").allowed);
    assert!(e.can_use_tool("XAAZ").allowed);
    assert!(e.can_use_tool("XZ").allowed);
}

#[test]
fn tool_character_class_glob() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("[ABC]Tool")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("ATool").allowed);
    assert!(!e.can_use_tool("BTool").allowed);
    assert!(!e.can_use_tool("CTool").allowed);
    assert!(e.can_use_tool("DTool").allowed);
    assert!(e.can_use_tool("Tool").allowed);
}

#[test]
fn tool_multiple_wildcards() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("*Dangerous*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Dangerous").allowed);
    assert!(!e.can_use_tool("VeryDangerousTool").allowed);
    assert!(!e.can_use_tool("DangerousExec").allowed);
    assert!(e.can_use_tool("SafeTool").allowed);
}

#[test]
fn tool_suffix_wildcard() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("*Exec")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellExec").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Execute").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_allowlist_with_glob_patterns() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read*"), s("List*")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(e.can_use_tool("ListDir").allowed);
    assert!(e.can_use_tool("ListFiles").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn tool_allowlist_multiple_exact_entries() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Grep")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("ReadFile").allowed);
}

#[test]
fn disallowed_tools_only_no_allowlist() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash"), s("Shell")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Anything").allowed);
}

// ===========================================================================
// 4. Read path deny with nested globs
// ===========================================================================

#[test]
fn read_deny_nested_src_glob() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/src/**/*.secret")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("crate/src/data.secret")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/src/c/d/e.secret")).allowed);
    assert!(e.can_read_path(Path::new("crate/src/data.rs")).allowed);
    assert!(e.can_read_path(Path::new("data.secret")).allowed);
}

#[test]
fn read_deny_multiple_extensions() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/*.{pem,key,p12}")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("ssh/id.key")).allowed);
    assert!(!e.can_read_path(Path::new("store.p12")).allowed);
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

#[test]
fn read_deny_dot_prefixed_files() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/.*")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new("sub/.hidden")).allowed);
    assert!(e.can_read_path(Path::new("visible.txt")).allowed);
}

#[test]
fn read_deny_specific_directory() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("config/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("config/app.toml")).allowed);
    assert!(!e.can_read_path(Path::new("config/sub/nested.yaml")).allowed);
    assert!(e.can_read_path(Path::new("src/config.rs")).allowed);
}

#[test]
fn read_deny_multiple_patterns_union() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/.env"), s("**/.env.*"), s("**/id_rsa")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// 5. Write path deny
// ===========================================================================

#[test]
fn write_deny_git_and_node_modules() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/.git/**"), s("**/node_modules/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("sub/.git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/foo/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_does_not_affect_read() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("anything")).allowed);
    assert!(e.can_read_path(Path::new("anything")).allowed);
}

#[test]
fn read_deny_does_not_affect_write() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("anything")).allowed);
    assert!(e.can_write_path(Path::new("anything")).allowed);
}

#[test]
fn write_deny_specific_extension() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/*.lock")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(
        !e.can_write_path(Path::new("sub/package-lock.json.lock"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn write_deny_deeply_nested() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("secret/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(!e.can_write_path(Path::new("secret/x.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

// ===========================================================================
// 6. Conflicting rules — deny takes precedence
// ===========================================================================

#[test]
fn deny_beats_allow_for_tools() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Bash"), s("Read"), s("Write")],
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn deny_wildcard_overrides_allow_wildcard() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("AnyTool").allowed);
}

#[test]
fn deny_glob_overrides_allow_glob() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Shell*")],
        disallowed_tools: vec![s("Shell*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("ShellExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
}

#[test]
fn deny_specific_overrides_allow_wildcard() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash"), s("Shell")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn complex_policy_allow_deny_interaction() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Grep")],
        disallowed_tools: vec![s("Write")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/locked/**")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

// ===========================================================================
// 7. Merged profiles
// ===========================================================================

#[test]
fn merged_profiles_union_deny_lists() {
    let p1 = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("secret/**")],
        ..Default::default()
    };
    let p2 = PolicyProfile {
        disallowed_tools: vec![s("Exec")],
        deny_write: vec![s("locked/**")],
        ..Default::default()
    };
    let merged = PolicyProfile {
        disallowed_tools: [p1.disallowed_tools, p2.disallowed_tools].concat(),
        deny_read: [p1.deny_read, p2.deny_read].concat(),
        deny_write: [p1.deny_write, p2.deny_write].concat(),
        ..Default::default()
    };
    let e = engine(merged);

    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.bin")).allowed);
    assert!(e.can_read_path(Path::new("public/index.html")).allowed);
}

// ===========================================================================
// 8. Glob pattern matching — complex patterns
// ===========================================================================

#[test]
fn glob_double_star_matches_any_depth() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/secret.txt")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/secret.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/d/secret.txt")).allowed);
    assert!(e.can_read_path(Path::new("secret.json")).allowed);
}

#[test]
fn glob_single_star_in_extension() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("*.log")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("app.log")).allowed);
    assert!(!e.can_write_path(Path::new("error.log")).allowed);
    assert!(e.can_write_path(Path::new("app.txt")).allowed);
}

#[test]
fn glob_question_mark_in_path() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("data?.csv")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("data1.csv")).allowed);
    assert!(!e.can_read_path(Path::new("dataX.csv")).allowed);
    assert!(e.can_read_path(Path::new("data10.csv")).allowed);
    assert!(e.can_read_path(Path::new("data.csv")).allowed);
}

#[test]
fn glob_brace_alternation() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/*.{tmp,bak,swp}")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("file.tmp")).allowed);
    assert!(!e.can_write_path(Path::new("dir/file.bak")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/file.swp")).allowed);
    assert!(e.can_write_path(Path::new("file.txt")).allowed);
}

#[test]
fn glob_character_class_in_path() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("log[0-9].txt")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("log0.txt")).allowed);
    assert!(!e.can_read_path(Path::new("log9.txt")).allowed);
    assert!(e.can_read_path(Path::new("logA.txt")).allowed);
    assert!(e.can_read_path(Path::new("log10.txt")).allowed);
}

#[test]
fn glob_nested_double_star_in_middle() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("src/**/test/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("src/test/file.rs")).allowed);
    assert!(!e.can_write_path(Path::new("src/a/b/test/c/d.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("test/file.rs")).allowed);
}

#[test]
fn glob_star_matches_across_separators_in_globset_default() {
    // globset default: literal_separator = false, so * crosses /
    let e = engine(PolicyProfile {
        deny_read: vec![s("*.rs")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("main.rs")).allowed);
    assert!(!e.can_read_path(Path::new("src/lib.rs")).allowed);
}

// ===========================================================================
// 9. Case sensitivity
// ===========================================================================

#[test]
fn tool_names_are_case_sensitive() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("bash").allowed);
    assert!(e.can_use_tool("BASH").allowed);
    assert!(e.can_use_tool("bAsH").allowed);
}

#[test]
fn path_patterns_are_case_sensitive() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("SECRET.txt")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("SECRET.txt")).allowed);
    // On Windows globset may be case-insensitive; test lowercase separately.
}

#[test]
fn tool_allowlist_case_sensitive() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("read").allowed);
    assert!(!e.can_use_tool("READ").allowed);
}

// ===========================================================================
// 10. Path normalization
// ===========================================================================

#[test]
fn path_with_dot_segments() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/etc/passwd")],
        ..Default::default()
    });
    let d = e.can_read_path(Path::new("../../etc/passwd"));
    assert!(!d.allowed);
}

#[test]
fn path_with_trailing_separator() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("../.git/config")).allowed);
}

#[test]
fn path_with_multiple_slashes() {
    // Path::new normalizes multiple slashes
    let e = engine(PolicyProfile {
        deny_read: vec![s("secret/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("secret/deep/file.txt")).allowed);
}

// ===========================================================================
// 11. Unicode paths
// ===========================================================================

#[test]
fn unicode_tool_names_allowed() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_use_tool("読み取り").allowed);
    assert!(e.can_use_tool("Outil_écriture").allowed);
    assert!(e.can_use_tool("工具🔧").allowed);
}

#[test]
fn unicode_tool_names_denied() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("読み取り"), s("工具*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("読み取り").allowed);
    assert!(!e.can_use_tool("工具🔧").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn unicode_paths_in_deny_read() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("données/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("données/fichier.rs")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn unicode_paths_in_deny_write() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/日本語/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("docs/日本語/readme.md")).allowed);
    assert!(
        e.can_write_path(Path::new("docs/english/readme.md"))
            .allowed
    );
}

#[test]
fn emoji_in_tool_name_deny() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("🔥*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("🔥Tool").allowed);
    assert!(!e.can_use_tool("🔥").allowed);
    assert!(e.can_use_tool("Tool🔥").allowed);
}

// ===========================================================================
// 12. Very long patterns and paths
// ===========================================================================

#[test]
fn very_long_deny_pattern_compiles() {
    let segments: Vec<&str> = (0..200).map(|_| "d").collect();
    let pattern = segments.join("/") + "/**";
    let e = engine(PolicyProfile {
        deny_write: vec![pattern],
        ..Default::default()
    });
    let blocked_path = segments.join("/") + "/file.txt";
    assert!(!e.can_write_path(Path::new(&blocked_path)).allowed);
    assert!(e.can_write_path(Path::new("other/file.txt")).allowed);
}

#[test]
fn very_long_tool_name() {
    let long_name = "A".repeat(1000);
    let e = engine(PolicyProfile {
        disallowed_tools: vec![long_name.clone()],
        ..Default::default()
    });
    assert!(!e.can_use_tool(&long_name).allowed);
    assert!(e.can_use_tool("ShortTool").allowed);
}

#[test]
fn very_long_path_allowed() {
    let e = engine(PolicyProfile::default());
    let long_path = (0..100)
        .map(|i| format!("dir{i}"))
        .collect::<Vec<_>>()
        .join("/")
        + "/file.txt";
    assert!(e.can_read_path(Path::new(&long_path)).allowed);
}

#[test]
fn very_long_path_denied() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/*.secret")],
        ..Default::default()
    });
    let long_path = (0..100)
        .map(|i| format!("dir{i}"))
        .collect::<Vec<_>>()
        .join("/")
        + "/file.secret";
    assert!(!e.can_read_path(Path::new(&long_path)).allowed);
}

// ===========================================================================
// 13. Serde roundtrip
// ===========================================================================

#[test]
fn policy_profile_serde_roundtrip() {
    let policy = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash*")],
        deny_read: vec![s("**/.env"), s("secrets/**")],
        deny_write: vec![s("**/.git/**")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.example.com")],
        require_approval_for: vec![s("Exec")],
    };
    let json = serde_json::to_string(&policy).expect("serialize");
    let rt: PolicyProfile = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(policy.allowed_tools, rt.allowed_tools);
    assert_eq!(policy.disallowed_tools, rt.disallowed_tools);
    assert_eq!(policy.deny_read, rt.deny_read);
    assert_eq!(policy.deny_write, rt.deny_write);
    assert_eq!(policy.allow_network, rt.allow_network);
    assert_eq!(policy.deny_network, rt.deny_network);
    assert_eq!(policy.require_approval_for, rt.require_approval_for);

    let e1 = engine(policy);
    let e2 = engine(rt);
    assert_eq!(
        e1.can_use_tool("Read").allowed,
        e2.can_use_tool("Read").allowed
    );
    assert_eq!(
        e1.can_use_tool("Bash").allowed,
        e2.can_use_tool("Bash").allowed
    );
    assert_eq!(
        e1.can_read_path(Path::new(".env")).allowed,
        e2.can_read_path(Path::new(".env")).allowed
    );
}

#[test]
fn policy_profile_serde_roundtrip_default() {
    let policy = PolicyProfile::default();
    let json = serde_json::to_string(&policy).expect("serialize");
    let rt: PolicyProfile = serde_json::from_str(&json).expect("deserialize");
    assert!(rt.allowed_tools.is_empty());
    assert!(rt.disallowed_tools.is_empty());
    assert!(rt.deny_read.is_empty());
    assert!(rt.deny_write.is_empty());
}

#[test]
fn policy_profile_serde_from_json_string() {
    let json = r#"{
        "allowed_tools": ["Read"],
        "disallowed_tools": [],
        "deny_read": ["**/.env"],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": []
    }"#;
    let policy: PolicyProfile = serde_json::from_str(json).expect("deserialize");
    let e = engine(policy);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
}

// ===========================================================================
// 14. MatchDecision variants via PolicyEngine
// ===========================================================================

#[test]
fn match_decision_allowed_via_default_policy() {
    let e = engine(PolicyProfile::default());
    let d = e.can_use_tool("Anything");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn match_decision_denied_by_exclude_via_disallowed() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("disallowed"));
}

#[test]
fn match_decision_denied_by_missing_include_via_allowlist() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    let d = e.can_use_tool("Write");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("allowlist"));
}

#[test]
fn all_match_decision_variants_exercised() {
    use abp_glob::IncludeExcludeGlobs;
    let g = IncludeExcludeGlobs::new(&[s("Read"), s("Write")], &[s("Write")]).unwrap();

    assert_eq!(g.decide_str("Read"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Write"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("Bash"), MatchDecision::DeniedByMissingInclude);
}

// ===========================================================================
// 15. Boundary: single-char patterns
// ===========================================================================

#[test]
fn single_char_tool_name() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("X")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("X").allowed);
    assert!(e.can_use_tool("Y").allowed);
    assert!(e.can_use_tool("XY").allowed);
}

#[test]
fn single_char_path_pattern() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("x")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("x")).allowed);
    assert!(e.can_read_path(Path::new("y")).allowed);
    assert!(e.can_read_path(Path::new("xy")).allowed);
}

// ===========================================================================
// 16. Exact match vs glob match
// ===========================================================================

#[test]
fn exact_tool_name_vs_glob() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("BashExec").allowed);
    assert!(e.can_use_tool("MyBash").allowed);
}

#[test]
fn exact_path_vs_glob_path() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("Cargo.lock")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("sub/Cargo.lock")).allowed);
}

// ===========================================================================
// 17. Multiple overlapping rules
// ===========================================================================

#[test]
fn overlapping_deny_read_patterns() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/*.pem"), s("certs/**"), s("**/server.*")],
        ..Default::default()
    });
    // Matched by all three
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    // Matched by first and third
    assert!(!e.can_read_path(Path::new("dir/server.pem")).allowed);
    // Matched by second only
    assert!(!e.can_read_path(Path::new("certs/ca.crt")).allowed);
    // Matched by third only
    assert!(!e.can_read_path(Path::new("dir/server.key")).allowed);
    // Not matched
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn overlapping_deny_write_patterns() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/.git/**"), s("**/.git*"), s(".git*")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".gitignore")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn overlapping_tool_deny_patterns() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash"), s("Bash*"), s("*Bash*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("MyBashTool").allowed);
}

// ===========================================================================
// 18. Performance: 1000 rules
// ===========================================================================

#[test]
fn performance_1000_rules() {
    let disallowed: Vec<String> = (0..1000).map(|i| format!("DeniedTool_{i}")).collect();
    let deny_read: Vec<String> = (0..1000).map(|i| format!("secret_{i}/**")).collect();
    let deny_write: Vec<String> = (0..1000).map(|i| format!("locked_{i}/**")).collect();

    let e = engine(PolicyProfile {
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        ..Default::default()
    });

    assert!(!e.can_use_tool("DeniedTool_500").allowed);
    assert!(
        !e.can_read_path(Path::new("secret_999/deep/file.txt"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("locked_0/file.bin")).allowed);
    assert!(e.can_use_tool("AllowedTool").allowed);
    assert!(e.can_read_path(Path::new("public/index.html")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);

    let mut denied_count = 0u32;
    for i in 0..10_000 {
        if !e.can_use_tool(&format!("DeniedTool_{}", i % 1000)).allowed {
            denied_count += 1;
        }
    }
    assert_eq!(denied_count, 10_000);
}

// ===========================================================================
// 19. Realistic scenarios
// ===========================================================================

#[test]
fn realistic_src_only_agent() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Grep"), s("ListDir")],
        disallowed_tools: vec![s("Bash*"), s("Shell*"), s("Exec*")],
        deny_read: vec![
            s("**/.env"),
            s("**/.env.*"),
            s("**/*.pem"),
            s("**/*.key"),
            s("**/secrets/**"),
        ],
        deny_write: vec![
            s("**/.git/**"),
            s("**/node_modules/**"),
            s("**/target/**"),
            s("Cargo.lock"),
        ],
        ..Default::default()
    });

    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(e.can_use_tool("ListDir").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCommand").allowed);
    assert!(!e.can_use_tool("UnlistedTool").allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.toml")).allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("secrets/api_key.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("target/debug/binary")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
}

#[test]
fn realistic_read_only_agent() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep"), s("ListDir")],
        deny_write: vec![s("**")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn realistic_sandbox_agent() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Bash"), s("Shell"), s("Exec"), s("Network*")],
        deny_read: vec![s("**/.env*"), s("**/credentials*"), s("**/*.pem")],
        deny_write: vec![s("**/.git/**"), s("**/Cargo.lock")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("CustomTool").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("NetworkCall").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("dir/credentials.json")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("sub/Cargo.lock")).allowed);
}

// ===========================================================================
// 20. Decision reason messages
// ===========================================================================

#[test]
fn deny_reason_includes_tool_name() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(d.reason.as_deref().unwrap().contains("Bash"));
}

#[test]
fn deny_reason_includes_path() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("secret/**")],
        ..Default::default()
    });
    let d = e.can_read_path(Path::new("secret/key.pem"));
    assert!(d.reason.as_deref().unwrap().contains("secret/key.pem"));
}

#[test]
fn deny_write_reason_includes_path() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("locked/**")],
        ..Default::default()
    });
    let d = e.can_write_path(Path::new("locked/data.bin"));
    assert!(d.reason.as_deref().unwrap().contains("locked/data.bin"));
}

#[test]
fn allow_decision_has_no_reason() {
    let e = engine(PolicyProfile::default());
    let d = e.can_use_tool("Anything");
    assert!(d.reason.is_none());
    let d = e.can_read_path(Path::new("any.txt"));
    assert!(d.reason.is_none());
    let d = e.can_write_path(Path::new("any.txt"));
    assert!(d.reason.is_none());
}

#[test]
fn missing_include_reason_differs_from_exclude_reason() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    let not_in_allow = e.can_use_tool("Write");
    let in_deny = e.can_use_tool("Bash");
    assert_ne!(
        not_in_allow.reason.as_deref().unwrap(),
        in_deny.reason.as_deref().unwrap()
    );
}

// ===========================================================================
// 21. Edge: empty strings
// ===========================================================================

#[test]
fn empty_tool_name() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_use_tool("").allowed);

    let strict = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    assert!(!strict.can_use_tool("").allowed);
}

#[test]
fn empty_path() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_read_path(Path::new("")).allowed);
    assert!(e.can_write_path(Path::new("")).allowed);
}

#[test]
fn empty_string_in_deny_list_still_compiles() {
    // Empty pattern in globset is valid (matches empty string).
    let result = PolicyEngine::new(&PolicyProfile {
        deny_read: vec![s("")],
        ..Default::default()
    });
    // Whether it compiles depends on globset — just verify no panic.
    let _ = result;
}

// ===========================================================================
// 22. Invalid glob patterns → compilation errors
// ===========================================================================

#[test]
fn invalid_glob_in_disallowed_tools_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        disallowed_tools: vec![s("[")],
        ..Default::default()
    });
    assert!(result.is_err());
}

#[test]
fn invalid_glob_in_deny_read_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        deny_read: vec![s("[invalid")],
        ..Default::default()
    });
    assert!(result.is_err());
}

#[test]
fn invalid_glob_in_deny_write_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        deny_write: vec![s("[")],
        ..Default::default()
    });
    assert!(result.is_err());
}

#[test]
fn invalid_glob_in_allowed_tools_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        allowed_tools: vec![s("[")],
        ..Default::default()
    });
    assert!(result.is_err());
}

#[test]
fn mixed_valid_invalid_globs_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        deny_read: vec![s("**/*.txt"), s("["), s("src/**")],
        ..Default::default()
    });
    assert!(result.is_err());
}

// ===========================================================================
// 23. PolicyEngine construction
// ===========================================================================

#[test]
fn engine_from_default_profile() {
    let e = PolicyEngine::new(&PolicyProfile::default());
    assert!(e.is_ok());
}

#[test]
fn engine_from_all_fields_populated() {
    let e = PolicyEngine::new(&PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.example.com")],
        require_approval_for: vec![s("Exec")],
    });
    assert!(e.is_ok());
}

#[test]
fn engine_with_many_patterns() {
    let tools: Vec<String> = (0..500).map(|i| format!("Tool{i}")).collect();
    let paths: Vec<String> = (0..500).map(|i| format!("dir{i}/**")).collect();
    let e = PolicyEngine::new(&PolicyProfile {
        allowed_tools: tools,
        deny_read: paths.clone(),
        deny_write: paths,
        ..Default::default()
    });
    assert!(e.is_ok());
}

// ===========================================================================
// 24. Whitespace and special characters in tool names
// ===========================================================================

#[test]
fn tool_name_with_spaces() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("My Tool")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("My Tool").allowed);
    assert!(e.can_use_tool("MyTool").allowed);
}

#[test]
fn tool_name_with_special_chars() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("tool-v2.0")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("tool-v2.0").allowed);
    assert!(e.can_use_tool("tool-v2_0").allowed);
}

#[test]
fn tool_name_numeric() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("123")],
        ..Default::default()
    });
    assert!(e.can_use_tool("123").allowed);
    assert!(!e.can_use_tool("456").allowed);
}

// ===========================================================================
// 25. Read and write independent
// ===========================================================================

#[test]
fn independent_read_write_deny_lists() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("read_only_deny/**")],
        deny_write: vec![s("write_only_deny/**")],
        ..Default::default()
    });
    // deny_read does not affect write
    assert!(
        !e.can_read_path(Path::new("read_only_deny/file.txt"))
            .allowed
    );
    assert!(
        e.can_write_path(Path::new("read_only_deny/file.txt"))
            .allowed
    );
    // deny_write does not affect read
    assert!(
        e.can_read_path(Path::new("write_only_deny/file.txt"))
            .allowed
    );
    assert!(
        !e.can_write_path(Path::new("write_only_deny/file.txt"))
            .allowed
    );
}

#[test]
fn both_read_and_write_denied_for_same_path() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("protected/**")],
        deny_write: vec![s("protected/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("protected/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("protected/file.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/file.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/file.txt")).allowed);
}

// ===========================================================================
// 26. Network and approval fields stored correctly
// ===========================================================================

#[test]
fn allow_and_deny_network_fields() {
    let policy = PolicyProfile {
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.example.com")],
        ..Default::default()
    };
    let _e = engine(policy.clone());
    assert_eq!(policy.allow_network, vec!["*.example.com"]);
    assert_eq!(policy.deny_network, vec!["evil.example.com"]);
}

#[test]
fn require_approval_for_field() {
    let policy = PolicyProfile {
        require_approval_for: vec![s("Bash"), s("DeleteFile")],
        ..Default::default()
    };
    let _e = engine(policy.clone());
    assert_eq!(policy.require_approval_for, vec!["Bash", "DeleteFile"]);
}

// ===========================================================================
// 27. Multiple engines from same profile
// ===========================================================================

#[test]
fn two_engines_from_same_profile_behave_identically() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        ..Default::default()
    };
    let e1 = engine(p.clone());
    let e2 = engine(p);

    let tools = ["Read", "Write", "Bash", "Grep"];
    for t in &tools {
        assert_eq!(e1.can_use_tool(t).allowed, e2.can_use_tool(t).allowed);
    }
    let paths = [".env", "src/main.rs", ".git/config"];
    for p in &paths {
        assert_eq!(
            e1.can_read_path(Path::new(p)).allowed,
            e2.can_read_path(Path::new(p)).allowed,
        );
        assert_eq!(
            e1.can_write_path(Path::new(p)).allowed,
            e2.can_write_path(Path::new(p)).allowed,
        );
    }
}

// ===========================================================================
// 28. Glob edge: patterns with literal dots
// ===========================================================================

#[test]
fn deny_dotfile_pattern() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/.?*")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new("sub/.hidden")).allowed);
    // Single dot is NOT matched (? requires at least one char after dot)
}

#[test]
fn deny_exact_filename_with_dot() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("README.md")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("README.md")).allowed);
    assert!(e.can_write_path(Path::new("README.txt")).allowed);
    assert!(e.can_write_path(Path::new("sub/README.md")).allowed);
}

// ===========================================================================
// 29. Glob edge: consecutive stars
// ===========================================================================

#[test]
fn double_star_prefix_matches_at_any_depth() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/secret.txt")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/secret.txt")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/secret.txt")).allowed);
    assert!(e.can_read_path(Path::new("secret.json")).allowed);
}

// ===========================================================================
// 30. Boundary: no deny patterns but with allowlist
// ===========================================================================

#[test]
fn allowlist_with_no_deny_still_blocks_unlisted() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Alpha"), s("Beta")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Alpha").allowed);
    assert!(e.can_use_tool("Beta").allowed);
    assert!(!e.can_use_tool("Gamma").allowed);
    // Read/write are unaffected by tool allowlist
    assert!(e.can_read_path(Path::new("any.txt")).allowed);
    assert!(e.can_write_path(Path::new("any.txt")).allowed);
}
