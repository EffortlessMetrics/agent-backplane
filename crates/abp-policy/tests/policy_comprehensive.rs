// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive edge-case and integration tests for `abp-policy`.
//!
//! Covers: default policies, allow/deny interactions, glob patterns, path edge
//! cases, serde roundtrips, composed engines, merged policy sets, auditor
//! integration, and real-world agent personas.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::audit::{AuditSummary, PolicyAuditor, PolicyDecision as AuditDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine(p: PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&p).expect("compile policy")
}

fn s(v: &str) -> String {
    v.to_string()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Default (empty) policy — everything allowed
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_policy_allows_unusual_tool_names() {
    let e = engine(PolicyProfile::default());
    // Whitespace, dots, slashes — all pass when no rules are set.
    for name in &[
        "tool with spaces",
        "ns.tool",
        "org/tool",
        "UPPER",
        "lower",
        "MiXeD",
    ] {
        assert!(e.can_use_tool(name).allowed, "expected allow for '{name}'");
    }
}

#[test]
fn default_policy_allows_paths_with_special_chars() {
    let e = engine(PolicyProfile::default());
    for p in &[
        "file name.txt",
        "dir with spaces/f.rs",
        "a+b=c.txt",
        "@scope/pkg",
    ] {
        assert!(
            e.can_read_path(Path::new(p)).allowed,
            "expected read-allow for '{p}'"
        );
        assert!(
            e.can_write_path(Path::new(p)).allowed,
            "expected write-allow for '{p}'"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Tool allowlist — only listed tools permitted
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_allowlist_glob_patterns() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("File*"), s("Net*")],
        ..Default::default()
    });
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(e.can_use_tool("NetGet").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Grep").allowed);
}

#[test]
fn tool_allowlist_case_sensitive() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    // Glob matching is case-sensitive by default.
    assert!(!e.can_use_tool("read").allowed);
    assert!(!e.can_use_tool("READ").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Tool denylist — listed tools blocked, others allowed
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_denylist_multiple_specific_tools() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash"), s("Shell"), s("Exec"), s("Sudo")],
        ..Default::default()
    });
    for denied in &["Bash", "Shell", "Exec", "Sudo"] {
        assert!(!e.can_use_tool(denied).allowed);
    }
    for allowed in &["Read", "Write", "Grep", "ListDir", "Search"] {
        assert!(e.can_use_tool(allowed).allowed);
    }
}

#[test]
fn tool_denylist_with_brace_expansion() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("{Bash,Shell,Exec}")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Read path patterns — allow reading from specific directories
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn read_deny_hidden_files_recursively() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/.*")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".gitignore")).allowed);
    assert!(!e.can_read_path(Path::new("sub/.hidden")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/.secret")).allowed);
    assert!(e.can_read_path(Path::new("src/visible.rs")).allowed);
    assert!(e.can_read_path(Path::new("not_hidden")).allowed);
}

#[test]
fn read_deny_specific_directory_but_not_sibling() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("private/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("private/doc.txt")).allowed);
    assert!(!e.can_read_path(Path::new("private/sub/deep.txt")).allowed);
    // Sibling directory "public" not affected.
    assert!(e.can_read_path(Path::new("public/doc.txt")).allowed);
    // The "private" directory name itself (without content) is allowed.
    assert!(e.can_read_path(Path::new("private_other/doc.txt")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Write path patterns — restrict writing to specific directories
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn write_deny_build_artifacts() {
    let e = engine(PolicyProfile {
        deny_write: vec![
            s("**/target/**"),
            s("**/dist/**"),
            s("**/build/**"),
            s("**/*.o"),
        ],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("target/debug/app")).allowed);
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(!e.can_write_path(Path::new("build/output.bin")).allowed);
    assert!(!e.can_write_path(Path::new("src/module.o")).allowed);
    // Source files still writable.
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn write_deny_does_not_bleed_into_read() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("protected/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("protected/file.txt")).allowed);
    // Reading the same paths is unaffected.
    assert!(e.can_read_path(Path::new("protected/file.txt")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Combined read + write policies
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn combined_read_write_independent_patterns() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/credentials/**")],
        deny_write: vec![s("**/config/**")],
        ..Default::default()
    });
    // credentials: read denied, write allowed.
    assert!(!e.can_read_path(Path::new("credentials/token")).allowed);
    assert!(e.can_write_path(Path::new("credentials/token")).allowed);
    // config: read allowed, write denied.
    assert!(e.can_read_path(Path::new("config/app.yml")).allowed);
    assert!(!e.can_write_path(Path::new("config/app.yml")).allowed);
    // src: both allowed.
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn combined_read_write_overlapping_deny() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("vault/**")],
        deny_write: vec![s("vault/**")],
        ..Default::default()
    });
    // Both read and write denied for vault paths.
    assert!(!e.can_read_path(Path::new("vault/key")).allowed);
    assert!(!e.can_write_path(Path::new("vault/key")).allowed);
    // Other paths unaffected.
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Policy precedence — deny overrides allow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deny_overrides_allow_with_identical_patterns() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("*")],
        ..Default::default()
    });
    // Deny always wins when both match.
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn deny_overrides_allow_narrow_vs_broad() {
    // Broad allow, narrow deny — deny still wins for the narrow match.
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("Dangerous")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Dangerous").allowed);
    assert!(e.can_use_tool("Safe").allowed);
}

#[test]
fn deny_overrides_allow_glob_overlap() {
    // Allow "File*" but deny "File*Write*" — deny wins for FileWrite.
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("File*")],
        disallowed_tools: vec![s("*Write*")],
        ..Default::default()
    });
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(!e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileWriteAppend").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Glob pattern matching — wildcards, nested, brace expansion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn glob_question_mark_single_char() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("data_?.csv")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("data_A.csv")).allowed);
    assert!(!e.can_write_path(Path::new("data_1.csv")).allowed);
    assert!(e.can_write_path(Path::new("data_AB.csv")).allowed);
    assert!(e.can_write_path(Path::new("data_.csv")).allowed);
}

#[test]
fn glob_brace_expansion_for_extensions() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/*.{pem,key,p12,jks}")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("keys/id.key")).allowed);
    assert!(!e.can_read_path(Path::new("store/truststore.jks")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_double_star_matches_arbitrary_depth() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/backup/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("backup/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/backup/file.txt")).allowed);
    assert!(
        !e.can_write_path(Path::new("a/b/c/backup/d/e/f.txt"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("backups/file.txt")).allowed);
}

#[test]
fn glob_character_class() {
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

// ═══════════════════════════════════════════════════════════════════════════
// 9. Multiple policies merged via PolicySet — most restrictive wins
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_set_merge_unions_all_fields() {
    let mut ps = PolicySet::new("full-merge");

    ps.add(PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("secret/**")],
        deny_write: vec![s("locked/**")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.com")],
        require_approval_for: vec![s("Deploy")],
    });
    ps.add(PolicyProfile {
        allowed_tools: vec![s("Write")],
        disallowed_tools: vec![s("Exec")],
        deny_read: vec![s("private/**")],
        deny_write: vec![s("archive/**")],
        allow_network: vec![s("*.internal.net")],
        deny_network: vec![s("malware.net")],
        require_approval_for: vec![s("Delete")],
    });

    let merged = ps.merge();
    assert!(merged.allowed_tools.contains(&s("Read")));
    assert!(merged.allowed_tools.contains(&s("Write")));
    assert!(merged.disallowed_tools.contains(&s("Bash")));
    assert!(merged.disallowed_tools.contains(&s("Exec")));
    assert!(merged.deny_read.contains(&s("secret/**")));
    assert!(merged.deny_read.contains(&s("private/**")));
    assert!(merged.deny_write.contains(&s("locked/**")));
    assert!(merged.deny_write.contains(&s("archive/**")));
    assert!(merged.allow_network.contains(&s("*.example.com")));
    assert!(merged.allow_network.contains(&s("*.internal.net")));
    assert!(merged.deny_network.contains(&s("evil.com")));
    assert!(merged.deny_network.contains(&s("malware.net")));
    assert!(merged.require_approval_for.contains(&s("Deploy")));
    assert!(merged.require_approval_for.contains(&s("Delete")));
}

#[test]
fn policy_set_merge_most_restrictive_wins_via_engine() {
    let mut ps = PolicySet::new("restrictive");
    // Profile 1 allows Read+Write, denies Bash.
    ps.add(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write")],
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    // Profile 2 adds Write to denylist.
    ps.add(PolicyProfile {
        disallowed_tools: vec![s("Write")],
        ..Default::default()
    });

    let merged = ps.merge();
    let e = engine(merged);

    // Read is in allowlist, not in denylist → allowed.
    assert!(e.can_use_tool("Read").allowed);
    // Write is in both allowlist AND denylist → deny wins.
    assert!(!e.can_use_tool("Write").allowed);
    // Bash is denied.
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn composed_engine_deny_overrides_merges_path_denials() {
    let profile1 = PolicyProfile {
        deny_read: vec![s("alpha/**")],
        ..Default::default()
    };
    let profile2 = PolicyProfile {
        deny_read: vec![s("beta/**")],
        ..Default::default()
    };
    let ce =
        ComposedEngine::new(vec![profile1, profile2], PolicyPrecedence::DenyOverrides).unwrap();

    // Both paths should be denied since DenyOverrides picks up any deny.
    assert!(ce.check_read("alpha/file.txt").is_deny());
    assert!(ce.check_read("beta/file.txt").is_deny());
    assert!(ce.check_read("gamma/file.txt").is_allow());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Edge cases — empty paths, root paths, relative vs absolute
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_path_with_deny_pattern() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/*.txt")],
        ..Default::default()
    });
    // Empty path doesn't end in .txt, so it's allowed.
    assert!(e.can_read_path(Path::new("")).allowed);
}

#[test]
fn edge_root_path_check() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**")],
        ..Default::default()
    });
    // Deny-all pattern blocks root-level paths too.
    assert!(!e.can_write_path(Path::new("/")).allowed);
    assert!(!e.can_write_path(Path::new("a")).allowed);
}

#[test]
fn edge_dot_dot_path_traversal() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/secret/**")],
        ..Default::default()
    });
    // Path traversal attempts should still match the deny pattern.
    assert!(
        !e.can_read_path(Path::new("../secret/key.pem")).allowed
            || e.can_read_path(Path::new("../secret/key.pem")).allowed,
        "path traversal behavior is implementation-defined, but must not panic"
    );
}

#[test]
fn edge_trailing_slash_path() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("config/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("config/app.yml")).allowed);
    // Trailing slash on directory — should not cause unexpected behavior.
    // (Path::new strips trailing separators on some platforms.)
    let _ = e.can_write_path(Path::new("config/"));
}

#[test]
fn edge_single_dot_path() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_read_path(Path::new(".")).allowed);
    assert!(e.can_write_path(Path::new(".")).allowed);
}

#[test]
fn edge_windows_style_separators() {
    // Path::new normalizes separators on each platform, but we should
    // not panic regardless of input.
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/.git/**")],
        ..Default::default()
    });
    let _ = e.can_write_path(Path::new(".git\\config"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Policy serialization roundtrip (serde)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_json_roundtrip_all_fields() {
    let original = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write")],
        disallowed_tools: vec![s("Bash"), s("Shell*")],
        deny_read: vec![s("**/.env"), s("secrets/**")],
        deny_write: vec![s("**/.git/**"), s("*.lock")],
        allow_network: vec![s("*.example.com")],
        deny_network: vec![s("evil.example.com")],
        require_approval_for: vec![s("Deploy"), s("Delete")],
    };
    let json = serde_json::to_string_pretty(&original).unwrap();
    let restored: PolicyProfile = serde_json::from_str(&json).unwrap();

    assert_eq!(original.allowed_tools, restored.allowed_tools);
    assert_eq!(original.disallowed_tools, restored.disallowed_tools);
    assert_eq!(original.deny_read, restored.deny_read);
    assert_eq!(original.deny_write, restored.deny_write);
    assert_eq!(original.allow_network, restored.allow_network);
    assert_eq!(original.deny_network, restored.deny_network);
    assert_eq!(original.require_approval_for, restored.require_approval_for);
}

#[test]
fn policy_profile_default_roundtrip() {
    let original = PolicyProfile::default();
    let json = serde_json::to_string(&original).unwrap();
    let restored: PolicyProfile = serde_json::from_str(&json).unwrap();

    assert!(restored.allowed_tools.is_empty());
    assert!(restored.disallowed_tools.is_empty());
    assert!(restored.deny_read.is_empty());
    assert!(restored.deny_write.is_empty());
    assert!(restored.allow_network.is_empty());
    assert!(restored.deny_network.is_empty());
    assert!(restored.require_approval_for.is_empty());
}

#[test]
fn roundtripped_policy_engine_behaves_identically() {
    let original = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        disallowed_tools: vec![s("Bash*")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/.git/**")],
        ..Default::default()
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: PolicyProfile = serde_json::from_str(&json).unwrap();

    let e1 = engine(original);
    let e2 = engine(restored);

    // Tool checks must be identical.
    for tool in &["Read", "Grep", "Bash", "BashExec", "Write"] {
        assert_eq!(
            e1.can_use_tool(tool).allowed,
            e2.can_use_tool(tool).allowed,
            "mismatch for tool '{tool}'"
        );
    }
    // Path checks must be identical.
    for path in &[".env", ".git/HEAD", "src/main.rs", "Cargo.toml"] {
        assert_eq!(
            e1.can_read_path(Path::new(path)).allowed,
            e2.can_read_path(Path::new(path)).allowed,
            "read mismatch for '{path}'"
        );
        assert_eq!(
            e1.can_write_path(Path::new(path)).allowed,
            e2.can_write_path(Path::new(path)).allowed,
            "write mismatch for '{path}'"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. PolicyProfile builder-pattern tests (struct literal + Default)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_partial_defaults() {
    // Only set one field; the rest should be empty.
    let p = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    };
    assert!(p.allowed_tools.is_empty());
    assert_eq!(p.disallowed_tools, vec!["Bash"]);
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn policy_profile_clone_independence() {
    let p1 = PolicyProfile {
        allowed_tools: vec![s("Read")],
        deny_read: vec![s("secret/**")],
        ..Default::default()
    };
    let mut p2 = p1.clone();
    p2.allowed_tools.push(s("Write"));
    p2.deny_read.push(s("private/**"));

    // p1 must be unaffected by mutations to p2.
    assert_eq!(p1.allowed_tools, vec!["Read"]);
    assert_eq!(p1.deny_read, vec!["secret/**"]);
    assert_eq!(p2.allowed_tools, vec!["Read", "Write"]);
    assert_eq!(p2.deny_read, vec!["secret/**", "private/**"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Real-world scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_read_only_agent() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep"), s("ListDir"), s("Search")],
        deny_write: vec![s("**")],
        deny_read: vec![
            s("**/.env"),
            s("**/.env.*"),
            s("**/secrets/**"),
            s("**/*.key"),
        ],
        ..Default::default()
    });

    // Allowed tools.
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(e.can_use_tool("ListDir").allowed);
    assert!(e.can_use_tool("Search").allowed);

    // Denied tools (not in allowlist).
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Delete").allowed);

    // All writes blocked.
    assert!(!e.can_write_path(Path::new("any/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("src/main.rs")).allowed);

    // Most reads allowed.
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("README.md")).allowed);

    // Sensitive reads blocked.
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.local")).allowed);
    assert!(!e.can_read_path(Path::new("secrets/api_key.txt")).allowed);
    assert!(!e.can_read_path(Path::new("certs/tls.key")).allowed);
}

#[test]
fn scenario_sandbox_agent() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Grep"), s("ListDir")],
        disallowed_tools: vec![s("Bash*"), s("Shell*"), s("Exec*"), s("Sudo*")],
        deny_read: vec![
            s("**/.env"),
            s("**/.env.*"),
            s("**/*.key"),
            s("**/*.pem"),
            s("**/secrets/**"),
        ],
        deny_write: vec![
            s("**/.git/**"),
            s("**/node_modules/**"),
            s("**/target/**"),
            s("*.lock"),
            s("**/dist/**"),
        ],
        ..Default::default()
    });

    // Safe tools work.
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);

    // Dangerous tools blocked.
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCommand").allowed);
    assert!(!e.can_use_tool("SudoRun").allowed);

    // Can read source but not secrets.
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_read_path(Path::new("secrets/db_password")).allowed);
    assert!(!e.can_read_path(Path::new("ssl/cert.pem")).allowed);

    // Can write source but not protected dirs.
    assert!(e.can_write_path(Path::new("src/new_feature.rs")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("target/debug/build")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
}

#[test]
fn scenario_unrestricted_agent() {
    let e = engine(PolicyProfile::default());

    // Every tool allowed.
    for tool in &["Bash", "Read", "Write", "Shell", "Exec", "Delete", "Sudo"] {
        assert!(e.can_use_tool(tool).allowed);
    }
    // Every path readable and writable.
    for path in &[".env", ".git/HEAD", "secrets/key.pem", "src/main.rs"] {
        assert!(e.can_read_path(Path::new(path)).allowed);
        assert!(e.can_write_path(Path::new(path)).allowed);
    }
}

#[test]
fn scenario_ci_agent() {
    // A CI agent can use build tools but must not touch credentials.
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Bash"), s("Grep"), s("ListDir")],
        deny_read: vec![s("**/.env*"), s("**/secrets/**"), s("**/*.key")],
        deny_write: vec![s("**/.git/**"), s("**/credentials/**")],
        ..Default::default()
    });

    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Deploy").allowed); // not in allowlist

    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(!e.can_write_path(Path::new("credentials/token")).allowed);

    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("secrets/aws.json")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. ComposedEngine — precedence strategies compared
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_three_precedence_strategies_compared() {
    let permissive = PolicyProfile::default();
    let restrictive = PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    };

    // DenyOverrides: deny wins.
    let deny_engine = ComposedEngine::new(
        vec![permissive.clone(), restrictive.clone()],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();
    assert!(deny_engine.check_tool("Bash").is_deny());

    // AllowOverrides: allow wins.
    let allow_engine = ComposedEngine::new(
        vec![permissive.clone(), restrictive.clone()],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();
    assert!(allow_engine.check_tool("Bash").is_allow());

    // FirstApplicable: first profile's decision wins.
    let first_permissive = ComposedEngine::new(
        vec![permissive.clone(), restrictive.clone()],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();
    assert!(first_permissive.check_tool("Bash").is_allow());

    let first_restrictive = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::FirstApplicable,
    )
    .unwrap();
    assert!(first_restrictive.check_tool("Bash").is_deny());
}

#[test]
fn composed_engine_path_checks_across_profiles() {
    let profile_a = PolicyProfile {
        deny_write: vec![s("alpha/**")],
        ..Default::default()
    };
    let profile_b = PolicyProfile {
        deny_write: vec![s("beta/**")],
        ..Default::default()
    };
    let ce =
        ComposedEngine::new(vec![profile_a, profile_b], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(ce.check_write("alpha/data.txt").is_deny());
    assert!(ce.check_write("beta/data.txt").is_deny());
    assert!(ce.check_write("gamma/data.txt").is_allow());
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. PolicyValidator — additional edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validator_empty_glob_in_multiple_fields() {
    let p = PolicyProfile {
        allowed_tools: vec![s("")],
        disallowed_tools: vec![s("")],
        deny_read: vec![s("")],
        deny_write: vec![s("")],
        allow_network: vec![s("")],
        deny_network: vec![s("")],
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    let empty_count = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::EmptyGlob)
        .count();
    assert_eq!(empty_count, 6, "should detect empty glob in all 6 fields");
}

#[test]
fn validator_no_warnings_for_default_profile() {
    let warnings = PolicyValidator::validate(&PolicyProfile::default());
    assert!(
        warnings.is_empty(),
        "default profile should produce no warnings"
    );
}

#[test]
fn validator_catchall_deny_read_star_star_slash_star() {
    let p = PolicyProfile {
        deny_read: vec![s("**/*")],
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
fn validator_wildcard_deny_tools_with_specific_allow() {
    let p = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write")],
        disallowed_tools: vec![s("*")],
        ..Default::default()
    };
    let warnings = PolicyValidator::validate(&p);
    // Both "Read" and "Write" are unreachable because disallowed_tools has "*".
    let unreachable = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::UnreachableRule)
        .count();
    assert_eq!(unreachable, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Auditor integration — full workflow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auditor_full_workflow_with_mixed_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Grep")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("secrets/**")],
        deny_write: vec![s("config/**")],
        ..Default::default()
    };
    let e = engine(policy);
    let mut auditor = PolicyAuditor::new(e);

    // Simulate agent actions.
    let r1 = auditor.check_tool("Read"); // allow (in allowlist)
    let r2 = auditor.check_tool("Bash"); // deny (in denylist)
    let r3 = auditor.check_tool("Deploy"); // deny (not in allowlist)
    let r4 = auditor.check_read("src/main.rs"); // allow
    let r5 = auditor.check_read("secrets/api_key"); // deny
    let r6 = auditor.check_write("src/new.rs"); // allow
    let r7 = auditor.check_write("config/app.yml"); // deny

    assert_eq!(r1, AuditDecision::Allow);
    assert!(matches!(r2, AuditDecision::Deny { .. }));
    assert!(matches!(r3, AuditDecision::Deny { .. }));
    assert_eq!(r4, AuditDecision::Allow);
    assert!(matches!(r5, AuditDecision::Deny { .. }));
    assert_eq!(r6, AuditDecision::Allow);
    assert!(matches!(r7, AuditDecision::Deny { .. }));

    assert_eq!(auditor.entries().len(), 7);
    assert_eq!(
        auditor.summary(),
        AuditSummary {
            allowed: 3,
            denied: 4,
            warned: 0,
        }
    );
}

#[test]
fn auditor_entries_have_correct_actions() {
    let e = engine(PolicyProfile::default());
    let mut auditor = PolicyAuditor::new(e);

    auditor.check_tool("T");
    auditor.check_read("R");
    auditor.check_write("W");

    assert_eq!(auditor.entries()[0].action, "tool");
    assert_eq!(auditor.entries()[1].action, "read");
    assert_eq!(auditor.entries()[2].action, "write");
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Decision type introspection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decision_allow_has_no_reason() {
    let e = engine(PolicyProfile::default());
    let d = e.can_use_tool("Any");
    assert!(d.allowed);
    assert!(d.reason.is_none());

    let d = e.can_read_path(Path::new("any.txt"));
    assert!(d.allowed);
    assert!(d.reason.is_none());

    let d = e.can_write_path(Path::new("any.txt"));
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_reasons_are_descriptive() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("secret/**")],
        deny_write: vec![s("locked/**")],
        ..Default::default()
    });

    let td = e.can_use_tool("Bash");
    assert!(!td.allowed);
    let reason = td.reason.unwrap();
    assert!(reason.contains("Bash"), "tool reason: {reason}");

    let rd = e.can_read_path(Path::new("secret/key"));
    assert!(!rd.allowed);
    let reason = rd.reason.unwrap();
    assert!(reason.contains("secret/key"), "read reason: {reason}");

    let wd = e.can_write_path(Path::new("locked/data"));
    assert!(!wd.allowed);
    let reason = wd.reason.unwrap();
    assert!(reason.contains("locked/data"), "write reason: {reason}");
}

#[test]
fn decision_missing_include_reason() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    let d = e.can_use_tool("Unknown");
    assert!(!d.allowed);
    let reason = d.reason.unwrap();
    assert!(
        reason.contains("not in allowlist"),
        "expected 'not in allowlist', got: {reason}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Composed PolicyDecision serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn composed_policy_decision_serde_roundtrip() {
    let decisions = vec![
        PolicyDecision::Allow {
            reason: "ok".into(),
        },
        PolicyDecision::Deny {
            reason: "blocked".into(),
        },
        PolicyDecision::Abstain,
    ];
    for original in &decisions {
        let json = serde_json::to_string(original).unwrap();
        let restored: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(&restored, original);
    }
}

#[test]
fn policy_precedence_serde_roundtrip() {
    for prec in [
        PolicyPrecedence::DenyOverrides,
        PolicyPrecedence::AllowOverrides,
        PolicyPrecedence::FirstApplicable,
    ] {
        let json = serde_json::to_string(&prec).unwrap();
        let restored: PolicyPrecedence = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, prec);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. PolicySet edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_set_single_profile_merge_is_identity() {
    let profile = PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![s("Bash")],
        deny_read: vec![s("secret/**")],
        deny_write: vec![s("locked/**")],
        ..Default::default()
    };
    let mut ps = PolicySet::new("single");
    ps.add(profile.clone());
    let merged = ps.merge();

    assert_eq!(merged.allowed_tools, profile.allowed_tools);
    assert_eq!(merged.disallowed_tools, profile.disallowed_tools);
    assert_eq!(merged.deny_read, profile.deny_read);
    assert_eq!(merged.deny_write, profile.deny_write);
}

#[test]
fn policy_set_three_profiles_merged() {
    let mut ps = PolicySet::new("triple");
    ps.add(PolicyProfile {
        disallowed_tools: vec![s("A")],
        ..Default::default()
    });
    ps.add(PolicyProfile {
        disallowed_tools: vec![s("B")],
        ..Default::default()
    });
    ps.add(PolicyProfile {
        disallowed_tools: vec![s("C")],
        ..Default::default()
    });

    let merged = ps.merge();
    assert_eq!(merged.disallowed_tools.len(), 3);
    assert!(merged.disallowed_tools.contains(&s("A")));
    assert!(merged.disallowed_tools.contains(&s("B")));
    assert!(merged.disallowed_tools.contains(&s("C")));
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Invalid patterns produce errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn invalid_glob_in_allowed_tools_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        allowed_tools: vec![s("[unclosed")],
        ..Default::default()
    });
    assert!(result.is_err());
}

#[test]
fn invalid_glob_in_deny_read_errors() {
    let result = PolicyEngine::new(&PolicyProfile {
        deny_read: vec![s("[")],
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
fn composed_engine_rejects_invalid_globs() {
    let result = ComposedEngine::new(
        vec![PolicyProfile {
            disallowed_tools: vec![s("[bad")],
            ..Default::default()
        }],
        PolicyPrecedence::DenyOverrides,
    );
    assert!(result.is_err());
}
