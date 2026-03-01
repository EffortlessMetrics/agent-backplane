// SPDX-License-Identifier: MIT OR Apache-2.0
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
// 1. Empty policy — all allowed by default
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

// ===========================================================================
// 2. Deny-all policy
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

// ===========================================================================
// 3. Tool-specific allow/deny with wildcards
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
    assert!(e.can_use_tool("Execute").allowed); // not matched by "Exec" (exact)
}

#[test]
fn tool_question_mark_wildcard() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("X?Z")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("XAZ").allowed);
    assert!(!e.can_use_tool("X1Z").allowed);
    assert!(e.can_use_tool("XAAZ").allowed); // two chars don't match ?
    assert!(e.can_use_tool("XZ").allowed); // zero chars don't match ?
}

// ===========================================================================
// 4. Read path restrictions with nested globs
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
    assert!(e.can_read_path(Path::new("data.secret")).allowed); // no "src" ancestor
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

// ===========================================================================
// 5. Write path restrictions
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
    // All writes blocked, but reads are fine.
    assert!(!e.can_write_path(Path::new("anything")).allowed);
    assert!(e.can_read_path(Path::new("anything")).allowed);
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
    // Deny always wins.
    assert!(!e.can_use_tool("AnyTool").allowed);
}

// ===========================================================================
// 7. Multiple policy profiles merged (manual field-level union)
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

    // Simulate merge by unioning the Vec fields.
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
// 8. Unicode tool names
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

// ===========================================================================
// 9. Very long patterns
// ===========================================================================

#[test]
fn very_long_deny_pattern_compiles() {
    // A 200-segment deep deny pattern.
    let segments: Vec<&str> = (0..200).map(|_| "d").collect();
    let pattern = segments.join("/") + "/**";
    let e = engine(PolicyProfile {
        deny_write: vec![pattern.clone()],
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

// ===========================================================================
// 10. Policy serialization roundtrip
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
    let roundtripped: PolicyProfile = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(policy.allowed_tools, roundtripped.allowed_tools);
    assert_eq!(policy.disallowed_tools, roundtripped.disallowed_tools);
    assert_eq!(policy.deny_read, roundtripped.deny_read);
    assert_eq!(policy.deny_write, roundtripped.deny_write);
    assert_eq!(policy.allow_network, roundtripped.allow_network);
    assert_eq!(policy.deny_network, roundtripped.deny_network);
    assert_eq!(
        policy.require_approval_for,
        roundtripped.require_approval_for
    );

    // Roundtripped engine should behave identically.
    let e1 = engine(policy);
    let e2 = engine(roundtripped);
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

// ===========================================================================
// 11. All MatchDecision variants tested via PolicyEngine
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
    // Directly test the glob layer's three variants surface through the engine.
    use abp_glob::IncludeExcludeGlobs;

    let g = IncludeExcludeGlobs::new(&[s("Read"), s("Write")], &[s("Write")]).unwrap();

    assert_eq!(g.decide_str("Read"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Write"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("Bash"), MatchDecision::DeniedByMissingInclude);
}

// ===========================================================================
// 12. Boundary: single-char patterns
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
// 13. Boundary: exact match vs glob match
// ===========================================================================

#[test]
fn exact_tool_name_vs_glob() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    // Exact match denied.
    assert!(!e.can_use_tool("Bash").allowed);
    // Prefix/suffix of exact match — allowed (no wildcard).
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
    // globset's default: `*` does not add implicit separator matching for literal patterns.
    assert!(e.can_write_path(Path::new("sub/Cargo.lock")).allowed);
}

// ===========================================================================
// 14. Performance: 1000 rules → still fast
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

    // Spot-check denied.
    assert!(!e.can_use_tool("DeniedTool_500").allowed);
    assert!(
        !e.can_read_path(Path::new("secret_999/deep/file.txt"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("locked_0/file.bin")).allowed);

    // Spot-check allowed.
    assert!(e.can_use_tool("AllowedTool").allowed);
    assert!(e.can_read_path(Path::new("public/index.html")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);

    // Bulk evaluation — 10 000 checks should complete without issue.
    let mut denied_count = 0u32;
    for i in 0..10_000 {
        if !e.can_use_tool(&format!("DeniedTool_{}", i % 1000)).allowed {
            denied_count += 1;
        }
    }
    assert_eq!(denied_count, 10_000);
}

// ===========================================================================
// 15. Realistic scenario: restrict agent to src/ only
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

    // Allowed tools.
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(e.can_use_tool("ListDir").allowed);

    // Denied tools.
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCommand").allowed);
    assert!(!e.can_use_tool("UnlistedTool").allowed);

    // Allowed reads.
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.toml")).allowed);

    // Denied reads.
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("secrets/api_key.txt")).allowed);

    // Allowed writes.
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/new_module.rs")).allowed);

    // Denied writes.
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("target/debug/binary")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
}

// ===========================================================================
// 16. Decision reason messages
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

// ===========================================================================
// 17. Edge: empty strings
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

// ===========================================================================
// 18. Invalid glob pattern → compilation error
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
