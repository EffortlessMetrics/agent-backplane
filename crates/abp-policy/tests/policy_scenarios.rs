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
//! Deep policy engine tests covering complex policy composition scenarios.
//!
//! Categories:
//!   1. Default policy (empty allows everything)
//!   2. Tool restrictions (allow/deny specific tools)
//!   3. File access control (allow/deny file paths via globs)
//!   4. Write restrictions (allow/deny write paths via globs)
//!   5. Combined policies (tool + file + write together)
//!   6. Policy precedence (deny overrides allow)
//!   7. Glob patterns (*, **, ?, {a,b})
//!   8. Nested directory rules (/src/** allows but /src/secret/** denies)
//!   9. Policy serialization (JSON/TOML roundtrip)
//!  10. Policy merging (multiple PolicyProfiles combined)
//!  11. Edge cases (empty patterns, root-only, no-match)
//!  12. Real-world scenarios

use abp_core::PolicyProfile;
use abp_policy::compose::{ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator};
use abp_policy::composed::{ComposedPolicy, CompositionStrategy};
use abp_policy::PolicyEngine;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine(p: PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&p).expect("compile policy")
}

fn profile_default() -> PolicyProfile {
    PolicyProfile::default()
}

fn sv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

// ===========================================================================
// 1. Default policy — empty allows everything
// ===========================================================================

#[test]
fn default_policy_allows_any_tool() {
    let e = engine(profile_default());
    for tool in &["Bash", "Read", "Write", "Grep", "DeleteFile", "RunCode"] {
        assert!(e.can_use_tool(tool).allowed);
    }
}

#[test]
fn default_policy_allows_any_read_path() {
    let e = engine(profile_default());
    for p in &[
        "file.txt",
        "src/main.rs",
        ".env",
        ".git/config",
        "deep/a/b/c/d.txt",
    ] {
        assert!(e.can_read_path(Path::new(p)).allowed);
    }
}

#[test]
fn default_policy_allows_any_write_path() {
    let e = engine(profile_default());
    for p in &["output.log", "build/artifact.bin", ".env.local"] {
        assert!(e.can_write_path(Path::new(p)).allowed);
    }
}

#[test]
fn default_policy_decision_has_no_reason() {
    let e = engine(profile_default());
    assert!(e.can_use_tool("Anything").reason.is_none());
    assert!(e.can_read_path(Path::new("any")).reason.is_none());
    assert!(e.can_write_path(Path::new("any")).reason.is_none());
}

// ===========================================================================
// 2. Tool restrictions — allow/deny specific tools
// ===========================================================================

#[test]
fn disallow_single_tool() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn disallow_multiple_tools() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash", "DeleteFile", "RunCode"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("DeleteFile").allowed);
    assert!(!e.can_use_tool("RunCode").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn allowlist_restricts_to_listed_tools_only() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn disallowed_tool_has_reason_message() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    };
    let e = engine(p);
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("Bash"));
}

#[test]
fn unlisted_tool_has_not_in_allowlist_reason() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..profile_default()
    };
    let e = engine(p);
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("not in allowlist"));
}

#[test]
fn wildcard_allowlist_permits_all_tools() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Anything").allowed);
}

#[test]
fn glob_pattern_in_tool_denylist() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// ===========================================================================
// 3. File access control — deny read paths via globs
// ===========================================================================

#[test]
fn deny_read_single_file() {
    let p = PolicyProfile {
        deny_read: sv(&["secret.txt"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(e.can_read_path(Path::new("public.txt")).allowed);
}

#[test]
fn deny_read_glob_pattern() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(e.can_read_path(Path::new(".env.local")).allowed);
}

#[test]
fn deny_read_multiple_patterns() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*", "**/id_rsa"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_read_recursive_glob() {
    let p = PolicyProfile {
        deny_read: sv(&["secrets/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("secrets/key.pem")).allowed);
    assert!(
        !e.can_read_path(Path::new("secrets/deep/nested.txt"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_read_reason_contains_path() {
    let p = PolicyProfile {
        deny_read: sv(&["secret*"]),
        ..profile_default()
    };
    let e = engine(p);
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(d.reason.as_deref().unwrap().contains("secret.txt"));
}

// ===========================================================================
// 4. Write restrictions — deny write paths via globs
// ===========================================================================

#[test]
fn deny_write_single_file() {
    let p = PolicyProfile {
        deny_write: sv(&["config.toml"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("config.toml")).allowed);
    assert!(e.can_write_path(Path::new("other.toml")).allowed);
}

#[test]
fn deny_write_directory_glob() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new(".git/hooks/pre-commit")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_write_multiple_patterns() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**", "**/node_modules/**", "*.lock"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn deny_write_deep_nested_path() {
    let p = PolicyProfile {
        deny_write: sv(&["protected/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("protected/a/b/c/d.txt")).allowed);
    assert!(e.can_write_path(Path::new("writable/file.txt")).allowed);
}

#[test]
fn deny_write_reason_contains_path() {
    let p = PolicyProfile {
        deny_write: sv(&["locked*"]),
        ..profile_default()
    };
    let e = engine(p);
    let d = e.can_write_path(Path::new("locked.cfg"));
    assert!(d.reason.as_deref().unwrap().contains("locked.cfg"));
}

// ===========================================================================
// 5. Combined policies — tool + file + write restrictions together
// ===========================================================================

#[test]
fn combined_tool_and_read_restrictions() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        deny_read: sv(&["**/.env"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn combined_tool_read_write_restrictions() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        disallowed_tools: sv(&["Write"]),
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/locked/**"]),
        ..profile_default()
    };
    let e = engine(p);
    // Write in both allow and deny → deny wins
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn combined_all_restrictions_independent() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret/**"]),
        deny_write: sv(&["readonly/**"]),
        ..profile_default()
    };
    let e = engine(p);
    // Tool restriction is independent of path restrictions
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    // Path restrictions are independent of tool restrictions
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("public/readme.md")).allowed);
    assert!(!e.can_write_path(Path::new("readonly/config.yml")).allowed);
    assert!(e.can_write_path(Path::new("writable/output.txt")).allowed);
}

// ===========================================================================
// 6. Policy precedence — deny overrides allow
// ===========================================================================

#[test]
fn deny_overrides_allow_for_tools() {
    let p = PolicyProfile {
        allowed_tools: sv(&["*"]),
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn deny_overrides_allow_when_tool_in_both_lists() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Bash", "Read"]),
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn wildcard_deny_overrides_specific_allow() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write", "Grep"]),
        disallowed_tools: sv(&["*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Grep").allowed);
}

#[test]
fn deny_glob_overrides_allow_glob() {
    let p = PolicyProfile {
        allowed_tools: sv(&["File*"]),
        disallowed_tools: sv(&["FileDelete"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileDelete").allowed);
}

// ===========================================================================
// 7. Glob patterns — *, **, ?, {a,b}
// ===========================================================================

#[test]
fn star_matches_single_segment() {
    let p = PolicyProfile {
        deny_read: sv(&["*.log"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("app.log")).allowed);
    assert!(!e.can_read_path(Path::new("error.log")).allowed);
    assert!(e.can_read_path(Path::new("app.txt")).allowed);
}

#[test]
fn doublestar_matches_any_depth() {
    let p = PolicyProfile {
        deny_read: sv(&["**/*.log"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("app.log")).allowed);
    assert!(!e.can_read_path(Path::new("logs/app.log")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/app.log")).allowed);
    assert!(e.can_read_path(Path::new("app.txt")).allowed);
}

#[test]
fn question_mark_matches_single_char() {
    let p = PolicyProfile {
        deny_write: sv(&["temp?.txt"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("temp1.txt")).allowed);
    assert!(!e.can_write_path(Path::new("tempA.txt")).allowed);
    assert!(e.can_write_path(Path::new("temp12.txt")).allowed);
    assert!(e.can_write_path(Path::new("temp.txt")).allowed);
}

#[test]
fn brace_alternatives_in_glob() {
    let p = PolicyProfile {
        deny_read: sv(&["*.{key,pem,crt}"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
    assert!(!e.can_read_path(Path::new("ca.crt")).allowed);
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

#[test]
fn doublestar_with_extension() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*.bak"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("file.bak")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/file.bak")).allowed);
    assert!(e.can_write_path(Path::new("file.txt")).allowed);
}

#[test]
fn glob_pattern_in_tool_allowlist() {
    let p = PolicyProfile {
        allowed_tools: sv(&["File*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Grep").allowed);
}

#[test]
fn question_mark_in_tool_denylist() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["Tool?"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("ToolA").allowed);
    assert!(!e.can_use_tool("Tool1").allowed);
    assert!(e.can_use_tool("ToolAB").allowed);
    assert!(e.can_use_tool("Tool").allowed);
}

// ===========================================================================
// 8. Nested directory rules
// ===========================================================================

#[test]
fn nested_deny_under_broader_allow_read() {
    // deny_read for secrets/ nested inside an otherwise-readable tree
    let p = PolicyProfile {
        deny_read: sv(&["src/secret/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_read_path(Path::new("src/utils/mod.rs")).allowed);
    assert!(!e.can_read_path(Path::new("src/secret/key.pem")).allowed);
    assert!(
        !e.can_read_path(Path::new("src/secret/nested/deep.txt"))
            .allowed
    );
}

#[test]
fn nested_deny_write_under_broader_tree() {
    let p = PolicyProfile {
        deny_write: sv(&["config/prod/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(
        e.can_write_path(Path::new("config/dev/settings.toml"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("config/test.toml")).allowed);
    assert!(!e.can_write_path(Path::new("config/prod/db.toml")).allowed);
    assert!(
        !e.can_write_path(Path::new("config/prod/nested/secret.toml"))
            .allowed
    );
}

#[test]
fn multiple_nested_deny_patterns() {
    let p = PolicyProfile {
        deny_read: sv(&["src/secret/**", "src/internal/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(!e.can_read_path(Path::new("src/secret/key.txt")).allowed);
    assert!(!e.can_read_path(Path::new("src/internal/auth.rs")).allowed);
}

#[test]
fn deny_write_nested_git_directories() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("submodule/.git/HEAD")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// 9. Policy serialization — JSON/TOML roundtrip
// ===========================================================================

#[test]
fn json_roundtrip_empty_policy() {
    let original = profile_default();
    let json = serde_json::to_string(&original).unwrap();
    let decoded: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert!(decoded.allowed_tools.is_empty());
    assert!(decoded.disallowed_tools.is_empty());
    assert!(decoded.deny_read.is_empty());
    assert!(decoded.deny_write.is_empty());
}

#[test]
fn json_roundtrip_complex_policy() {
    let original = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env", "secrets/**"]),
        deny_write: sv(&["**/.git/**", "*.lock"]),
        allow_network: sv(&["*.example.com"]),
        deny_network: sv(&["evil.com"]),
        require_approval_for: sv(&["DeleteFile"]),
    };
    let json = serde_json::to_string_pretty(&original).unwrap();
    let decoded: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.allowed_tools, original.allowed_tools);
    assert_eq!(decoded.disallowed_tools, original.disallowed_tools);
    assert_eq!(decoded.deny_read, original.deny_read);
    assert_eq!(decoded.deny_write, original.deny_write);
    assert_eq!(decoded.allow_network, original.allow_network);
    assert_eq!(decoded.deny_network, original.deny_network);
    assert_eq!(decoded.require_approval_for, original.require_approval_for);
}

#[test]
fn json_roundtrip_preserves_engine_behaviour() {
    let original = PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["secret*"]),
        deny_write: sv(&["locked*"]),
        ..profile_default()
    };
    let json = serde_json::to_string(&original).unwrap();
    let decoded: PolicyProfile = serde_json::from_str(&json).unwrap();
    let e = engine(decoded);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!e.can_write_path(Path::new("locked.cfg")).allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn json_deserialization_from_literal() {
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
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
}

// ===========================================================================
// 10. Policy merging — multiple PolicyProfiles combined via PolicySet
// ===========================================================================

#[test]
fn merge_two_profiles_unions_deny_lists() {
    let mut set = PolicySet::new("merged");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["DeleteFile"]),
        ..profile_default()
    });
    let merged = set.merge();
    assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
    assert!(merged.disallowed_tools.contains(&"DeleteFile".to_string()));
}

#[test]
fn merge_deduplicates_entries() {
    let mut set = PolicySet::new("dedup");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
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
fn merge_unions_deny_read_and_write() {
    let mut set = PolicySet::new("paths");
    set.add(PolicyProfile {
        deny_read: sv(&["**/.env"]),
        deny_write: sv(&["**/.git/**"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        deny_read: sv(&["secrets/**"]),
        deny_write: sv(&["*.lock"]),
        ..profile_default()
    });
    let merged = set.merge();
    assert_eq!(merged.deny_read.len(), 2);
    assert_eq!(merged.deny_write.len(), 2);
}

#[test]
fn merged_policy_engine_applies_combined_rules() {
    let mut set = PolicySet::new("combined");
    set.add(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        disallowed_tools: sv(&["DeleteFile"]),
        deny_write: sv(&["**/.git/**"]),
        ..profile_default()
    });
    let merged = set.merge();
    let e = engine(merged);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("DeleteFile").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn merge_unions_allow_tools() {
    let mut set = PolicySet::new("allow-union");
    set.add(PolicyProfile {
        allowed_tools: sv(&["Read"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        allowed_tools: sv(&["Grep"]),
        ..profile_default()
    });
    let merged = set.merge();
    let e = engine(merged);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn merge_unions_network_rules() {
    let mut set = PolicySet::new("net");
    set.add(PolicyProfile {
        allow_network: sv(&["*.example.com"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        deny_network: sv(&["evil.com"]),
        ..profile_default()
    });
    let merged = set.merge();
    assert!(merged.allow_network.contains(&"*.example.com".to_string()));
    assert!(merged.deny_network.contains(&"evil.com".to_string()));
}

#[test]
fn merge_unions_require_approval_for() {
    let mut set = PolicySet::new("approval");
    set.add(PolicyProfile {
        require_approval_for: sv(&["Bash"]),
        ..profile_default()
    });
    set.add(PolicyProfile {
        require_approval_for: sv(&["DeleteFile"]),
        ..profile_default()
    });
    let merged = set.merge();
    assert!(merged.require_approval_for.contains(&"Bash".to_string()));
    assert!(merged
        .require_approval_for
        .contains(&"DeleteFile".to_string()));
}

#[test]
fn policy_set_name() {
    let set = PolicySet::new("my-set");
    assert_eq!(set.name(), "my-set");
}

// ===========================================================================
// 11. Edge cases
// ===========================================================================

#[test]
fn empty_string_tool_name_allowed_by_default() {
    let e = engine(profile_default());
    assert!(e.can_use_tool("").allowed);
}

#[test]
fn empty_path_read_allowed_by_default() {
    let e = engine(profile_default());
    assert!(e.can_read_path(Path::new("")).allowed);
}

#[test]
fn empty_path_write_allowed_by_default() {
    let e = engine(profile_default());
    assert!(e.can_write_path(Path::new("")).allowed);
}

#[test]
fn tool_name_case_sensitivity() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["bash"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("bash").allowed);
    // Glob matching is typically case-sensitive
    assert!(e.can_use_tool("Bash").allowed);
}

#[test]
fn path_with_dots_and_special_chars() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.hidden_*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new(".hidden_file")).allowed);
    assert!(!e.can_read_path(Path::new("dir/.hidden_config")).allowed);
    assert!(e.can_read_path(Path::new("visible_file")).allowed);
}

#[test]
fn no_match_pattern_denies_nothing() {
    let p = PolicyProfile {
        deny_read: sv(&["nonexistent_specific_pattern_xyz"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_read_path(Path::new("any_file.txt")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn single_star_matches_any_txt_file() {
    let p = PolicyProfile {
        deny_read: sv(&["*.txt"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("readme.txt")).allowed);
    // globset *.txt matches txt files even in subdirectories
    assert!(!e.can_read_path(Path::new("dir/readme.txt")).allowed);
    assert!(e.can_read_path(Path::new("readme.md")).allowed);
}

#[test]
fn very_long_path_is_handled() {
    let long_path = "a/".repeat(100) + "file.txt";
    let e = engine(profile_default());
    assert!(e.can_read_path(Path::new(&long_path)).allowed);
    assert!(e.can_write_path(Path::new(&long_path)).allowed);
}

#[test]
fn unicode_tool_name() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["ツール"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("ツール").allowed);
    assert!(e.can_use_tool("Tool").allowed);
}

#[test]
fn multiple_extensions_brace_glob() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*.{tmp,bak,swp}"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("file.tmp")).allowed);
    assert!(!e.can_write_path(Path::new("dir/file.bak")).allowed);
    assert!(!e.can_write_path(Path::new("nested/dir/file.swp")).allowed);
    assert!(e.can_write_path(Path::new("file.txt")).allowed);
}

// ===========================================================================
// 12. Real-world scenarios
// ===========================================================================

#[test]
fn scenario_allow_code_editing_but_not_secrets() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*", "secrets/**", "**/*.key", "**/*.pem"]),
        deny_write: sv(&["**/.env", "**/.env.*", "secrets/**", "**/*.key", "**/*.pem"]),
        ..profile_default()
    };
    let e = engine(p);
    // Code editing allowed
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.toml")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
    // Secrets denied
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(!e.can_read_path(Path::new("secrets/api_key.txt")).allowed);
    assert!(
        !e.can_write_path(Path::new("secrets/db_password.txt"))
            .allowed
    );
    assert!(!e.can_read_path(Path::new("certs/server.key")).allowed);
    assert!(!e.can_read_path(Path::new("tls/cert.pem")).allowed);
}

#[test]
fn scenario_readonly_access_to_docs() {
    // Only allow Read/Grep tools and deny writing to docs/
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep", "ListDir"]),
        deny_write: sv(&["docs/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(e.can_read_path(Path::new("docs/README.md")).allowed);
    assert!(!e.can_write_path(Path::new("docs/README.md")).allowed);
    assert!(!e.can_write_path(Path::new("docs/guide/page.md")).allowed);
}

#[test]
fn scenario_full_access_except_env_files() {
    let p = PolicyProfile {
        deny_read: sv(&["**/.env", "**/.env.*"]),
        deny_write: sv(&["**/.env", "**/.env.*"]),
        ..profile_default()
    };
    let e = engine(p);
    // Full access to everything else
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.toml")).allowed);
    // .env denied
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.local")).allowed);
    assert!(
        !e.can_write_path(Path::new("config/.env.production"))
            .allowed
    );
}

#[test]
fn scenario_sandbox_only_tmp_directory() {
    // Deny writing anywhere except tmp/ by denying everything then relying
    // on the fact that abp-policy deny_write is an exclusion list.
    // In practice: deny everything at root, agent would need a wrapper.
    // For this test, deny critical paths.
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Write"]),
        deny_write: sv(&[
            "src/**",
            "config/**",
            "**/.git/**",
            "**/.env",
            "Cargo.toml",
            "Cargo.lock",
        ]),
        ..profile_default()
    };
    let e = engine(p);
    // tmp is not denied
    assert!(e.can_write_path(Path::new("tmp/output.txt")).allowed);
    assert!(e.can_write_path(Path::new("tmp/nested/data.csv")).allowed);
    // Critical paths denied
    assert!(!e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_write_path(Path::new("config/app.toml")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn scenario_ci_agent_read_only_no_bash() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep", "ListDir", "Search"]),
        disallowed_tools: sv(&["Bash*", "Shell*", "Exec*"]),
        deny_write: sv(&["**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("ExecCommand").allowed);
    // No writes anywhere
    assert!(!e.can_write_path(Path::new("any_file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("src/main.rs")).allowed);
    // Reads still allowed
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn scenario_frontend_only_workspace() {
    let p = PolicyProfile {
        deny_read: sv(&["backend/**", "infra/**", "**/*.sql"]),
        deny_write: sv(&["backend/**", "infra/**", "**/*.sql", "**/*.lock"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_read_path(Path::new("frontend/src/App.tsx")).allowed);
    assert!(e.can_write_path(Path::new("frontend/src/App.tsx")).allowed);
    assert!(!e.can_read_path(Path::new("backend/server.py")).allowed);
    assert!(!e.can_write_path(Path::new("backend/server.py")).allowed);
    assert!(!e.can_read_path(Path::new("infra/terraform.tf")).allowed);
    assert!(!e.can_read_path(Path::new("migrations/001.sql")).allowed);
    assert!(!e.can_write_path(Path::new("package-lock.lock")).allowed);
}

#[test]
fn scenario_security_review_agent() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep", "Search", "ListDir"]),
        deny_write: sv(&["**"]),
        ..profile_default()
    };
    let e = engine(p);
    // Can read everything
    assert!(e.can_read_path(Path::new("src/auth.rs")).allowed);
    assert!(e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("secrets/key.pem")).allowed);
    // Cannot write anything
    assert!(!e.can_write_path(Path::new("src/auth.rs")).allowed);
    assert!(!e.can_write_path(Path::new("output.txt")).allowed);
    // Limited tools
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

// ===========================================================================
// 13. PolicyValidator tests
// ===========================================================================

#[test]
fn validator_detects_overlapping_tool_allow_deny() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Bash"]),
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == abp_policy::compose::WarningKind::OverlappingAllowDeny));
}

#[test]
fn validator_detects_empty_glob() {
    let p = PolicyProfile {
        deny_read: sv(&[""]),
        ..profile_default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == abp_policy::compose::WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_unreachable_rule_wildcard_deny() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read"]),
        disallowed_tools: sv(&["*"]),
        ..profile_default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == abp_policy::compose::WarningKind::UnreachableRule));
}

#[test]
fn validator_detects_catch_all_deny_read() {
    let p = PolicyProfile {
        deny_read: sv(&["**"]),
        ..profile_default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == abp_policy::compose::WarningKind::UnreachableRule));
}

#[test]
fn validator_detects_catch_all_deny_write() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*"]),
        ..profile_default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings
        .iter()
        .any(|w| w.kind == abp_policy::compose::WarningKind::UnreachableRule));
}

#[test]
fn validator_no_warnings_for_clean_policy() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read", "Grep"]),
        disallowed_tools: sv(&["Bash"]),
        deny_read: sv(&["**/.env"]),
        ..profile_default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

// ===========================================================================
// 14. ComposedEngine tests (compose module)
// ===========================================================================

#[test]
fn composed_deny_overrides_with_two_profiles() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..profile_default()
        },
        PolicyProfile::default(),
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_allow_overrides_with_two_profiles() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..profile_default()
        },
        PolicyProfile::default(),
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::AllowOverrides).unwrap();
    // The permissive profile should override the deny
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_first_applicable_uses_first_profile() {
    let profiles = vec![
        PolicyProfile {
            disallowed_tools: sv(&["Bash"]),
            ..profile_default()
        },
        PolicyProfile::default(),
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::FirstApplicable).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn composed_deny_overrides_read_path() {
    let profiles = vec![
        PolicyProfile {
            deny_read: sv(&["secret/**"]),
            ..profile_default()
        },
        PolicyProfile::default(),
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read("secret/key.pem").is_deny());
    assert!(ce.check_read("src/main.rs").is_allow());
}

#[test]
fn composed_deny_overrides_write_path() {
    let profiles = vec![
        PolicyProfile {
            deny_write: sv(&["**/.git/**"]),
            ..profile_default()
        },
        PolicyProfile::default(),
    ];
    let ce = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/main.rs").is_allow());
}

#[test]
fn composed_empty_engines_returns_abstain() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("anything").is_abstain());
}

// ===========================================================================
// 15. ComposedPolicy tests (composed module)
// ===========================================================================

#[test]
fn composed_policy_all_must_allow_single_deny_vetoes() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let strict = engine(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    let permissive = engine(profile_default());
    cp.add_policy("strict", strict);
    cp.add_policy("permissive", permissive);
    assert!(cp.evaluate_tool("Bash").is_denied());
    assert!(cp.evaluate_tool("Read").is_allowed());
}

#[test]
fn composed_policy_any_must_allow_single_allow_suffices() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    let strict = engine(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    let permissive = engine(profile_default());
    cp.add_policy("strict", strict);
    cp.add_policy("permissive", permissive);
    assert!(cp.evaluate_tool("Bash").is_allowed());
}

#[test]
fn composed_policy_first_match_uses_first_engine() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    let first = engine(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    let second = engine(profile_default());
    cp.add_policy("first", first);
    cp.add_policy("second", second);
    assert!(cp.evaluate_tool("Bash").is_denied());
}

#[test]
fn composed_policy_empty_allows_all() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("Bash").is_allowed());
    assert!(cp.evaluate_read("any.txt").is_allowed());
    assert!(cp.evaluate_write("any.txt").is_allowed());
}

#[test]
fn composed_policy_read_evaluation() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let e = engine(PolicyProfile {
        deny_read: sv(&["**/.env"]),
        ..profile_default()
    });
    cp.add_policy("env-guard", e);
    assert!(cp.evaluate_read(".env").is_denied());
    assert!(cp.evaluate_read("src/main.rs").is_allowed());
}

#[test]
fn composed_policy_write_evaluation() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let e = engine(PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..profile_default()
    });
    cp.add_policy("git-guard", e);
    assert!(cp.evaluate_write(".git/config").is_denied());
    assert!(cp.evaluate_write("src/main.rs").is_allowed());
}

#[test]
fn composed_policy_count_and_strategy() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    assert_eq!(cp.policy_count(), 0);
    assert_eq!(cp.strategy(), CompositionStrategy::AnyMustAllow);
    cp.add_policy("one", engine(profile_default()));
    cp.add_policy("two", engine(profile_default()));
    assert_eq!(cp.policy_count(), 2);
}

#[test]
fn composed_policy_denied_result_has_engine_name() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    let e = engine(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    cp.add_policy("bash-blocker", e);
    let result = cp.evaluate_tool("Bash");
    if let abp_policy::composed::ComposedResult::Denied { by, .. } = &result {
        assert_eq!(by, "bash-blocker");
    } else {
        panic!("expected Denied");
    }
}

// ===========================================================================
// 16. Audit integration
// ===========================================================================

#[test]
fn auditor_records_tool_decisions() {
    let e = engine(PolicyProfile {
        disallowed_tools: sv(&["Bash"]),
        ..profile_default()
    });
    let mut auditor = abp_policy::audit::PolicyAuditor::new(e);
    let _ = auditor.check_tool("Bash");
    let _ = auditor.check_tool("Read");
    assert_eq!(auditor.entries().len(), 2);
    assert_eq!(auditor.denied_count(), 1);
    assert_eq!(auditor.allowed_count(), 1);
}

#[test]
fn auditor_records_read_write_decisions() {
    let e = engine(PolicyProfile {
        deny_read: sv(&["secret*"]),
        deny_write: sv(&["locked*"]),
        ..profile_default()
    });
    let mut auditor = abp_policy::audit::PolicyAuditor::new(e);
    let _ = auditor.check_read("secret.txt");
    let _ = auditor.check_read("public.txt");
    let _ = auditor.check_write("locked.cfg");
    let _ = auditor.check_write("writable.txt");
    let summary = auditor.summary();
    assert_eq!(summary.denied, 2);
    assert_eq!(summary.allowed, 2);
    assert_eq!(summary.warned, 0);
}

#[test]
fn audit_log_filter_by_action() {
    let mut log = abp_policy::audit::AuditLog::new();
    log.record(
        abp_policy::audit::AuditAction::ToolAllowed,
        "Read",
        None,
        None,
    );
    log.record(
        abp_policy::audit::AuditAction::ToolDenied,
        "Bash",
        Some("strict"),
        Some("disallowed"),
    );
    log.record(
        abp_policy::audit::AuditAction::ReadAllowed,
        "src/main.rs",
        None,
        None,
    );
    assert_eq!(
        log.filter_by_action(&abp_policy::audit::AuditAction::ToolDenied)
            .len(),
        1
    );
    assert_eq!(log.denied_count(), 1);
    assert_eq!(log.len(), 3);
    assert!(!log.is_empty());
}

// ===========================================================================
// 17. Path traversal protection
// ===========================================================================

#[test]
fn path_traversal_denied_in_read() {
    let p = PolicyProfile {
        deny_read: sv(&["**/etc/passwd"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("../../etc/passwd")).allowed);
}

#[test]
fn path_traversal_denied_in_write() {
    let p = PolicyProfile {
        deny_write: sv(&["**/.git/**"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("../.git/config")).allowed);
}

// ===========================================================================
// 18. Additional glob edge cases
// ===========================================================================

#[test]
fn multiple_star_patterns_in_deny() {
    let p = PolicyProfile {
        deny_read: sv(&["**/*.secret", "**/*.private"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_read_path(Path::new("data.secret")).allowed);
    assert!(!e.can_read_path(Path::new("deep/data.private")).allowed);
    assert!(e.can_read_path(Path::new("data.public")).allowed);
}

#[test]
fn exact_filename_deny() {
    let p = PolicyProfile {
        deny_write: sv(&["Makefile"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("Makefile")).allowed);
    assert!(e.can_write_path(Path::new("src/Makefile")).allowed);
}

#[test]
fn doublestar_prefix_exact_filename() {
    let p = PolicyProfile {
        deny_write: sv(&["**/Makefile"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("Makefile")).allowed);
    assert!(!e.can_write_path(Path::new("src/Makefile")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/c/Makefile")).allowed);
}

#[test]
fn brace_alternatives_in_write_deny() {
    let p = PolicyProfile {
        deny_write: sv(&["**/*.{exe,dll,so}"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_write_path(Path::new("bin/app.exe")).allowed);
    assert!(!e.can_write_path(Path::new("lib/native.dll")).allowed);
    assert!(!e.can_write_path(Path::new("lib/native.so")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// 19. Tool allowlist + denylist combination stress
// ===========================================================================

#[test]
fn allowlist_with_glob_and_specific_deny() {
    let p = PolicyProfile {
        allowed_tools: sv(&["File*", "Net*"]),
        disallowed_tools: sv(&["FileDelete", "NetAttack"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileDelete").allowed);
    assert!(e.can_use_tool("NetFetch").allowed);
    assert!(!e.can_use_tool("NetAttack").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn multiple_glob_patterns_in_allowlist() {
    let p = PolicyProfile {
        allowed_tools: sv(&["Read*", "Write*", "List*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(e.can_use_tool("WriteFile").allowed);
    assert!(e.can_use_tool("ListDir").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Delete").allowed);
}

#[test]
fn deny_all_tools_via_wildcard() {
    let p = PolicyProfile {
        disallowed_tools: sv(&["*"]),
        ..profile_default()
    };
    let e = engine(p);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Anything").allowed);
}
