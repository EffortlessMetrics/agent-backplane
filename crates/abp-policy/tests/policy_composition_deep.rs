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
//! Deep tests for policy composition and layering in abp-policy.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::compose::{
    ComposedEngine, PolicyDecision, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::composed::{ComposedPolicy, ComposedResult, CompositionStrategy};

// ===== Helpers =====

fn engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("compile policy")
}

fn profile_allow_tools(tools: &[&str]) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: tools.iter().map(|s| s.to_string()).collect(),
        ..PolicyProfile::default()
    }
}

fn profile_deny_tools(tools: &[&str]) -> PolicyProfile {
    PolicyProfile {
        disallowed_tools: tools.iter().map(|s| s.to_string()).collect(),
        ..PolicyProfile::default()
    }
}

fn profile_deny_read(patterns: &[&str]) -> PolicyProfile {
    PolicyProfile {
        deny_read: patterns.iter().map(|s| s.to_string()).collect(),
        ..PolicyProfile::default()
    }
}

fn profile_deny_write(patterns: &[&str]) -> PolicyProfile {
    PolicyProfile {
        deny_write: patterns.iter().map(|s| s.to_string()).collect(),
        ..PolicyProfile::default()
    }
}

// =========================================================================
// 1. Single policy — allow all, deny all, specific tool allow/deny
// =========================================================================

#[test]
fn single_empty_policy_allows_all_tools() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Anything").allowed);
}

#[test]
fn single_empty_policy_allows_all_reads() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_read_path(Path::new("deep/nested/path.txt")).allowed);
}

#[test]
fn single_empty_policy_allows_all_writes() {
    let e = engine(&PolicyProfile::default());
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(e.can_write_path(Path::new("build/output.o")).allowed);
}

#[test]
fn single_deny_all_tools_via_wildcard() {
    let e = engine(&profile_deny_tools(&["*"]));
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn single_specific_tool_allow() {
    let e = engine(&profile_allow_tools(&["Read"]));
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn single_specific_tool_deny() {
    let e = engine(&profile_deny_tools(&["Bash"]));
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn single_multiple_allowed_tools() {
    let e = engine(&profile_allow_tools(&["Read", "Write", "Grep"]));
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
}

#[test]
fn single_multiple_denied_tools() {
    let e = engine(&profile_deny_tools(&["Bash", "Execute", "Shell"]));
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// =========================================================================
// 2. Policy with globs — *.py allow, src/** deny, complex patterns
// =========================================================================

#[test]
fn glob_tool_pattern_star_suffix() {
    let e = engine(&profile_deny_tools(&["Bash*"]));
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("BashRun").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_deny_read_py_files() {
    let e = engine(&profile_deny_read(&["*.py"]));
    assert!(!e.can_read_path(Path::new("script.py")).allowed);
    assert!(!e.can_read_path(Path::new("test.py")).allowed);
    assert!(e.can_read_path(Path::new("script.rs")).allowed);
}

#[test]
fn glob_deny_read_recursive() {
    let e = engine(&profile_deny_read(&["src/**"]));
    assert!(!e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(
        !e.can_read_path(Path::new("src/deep/nested/file.rs"))
            .allowed
    );
    assert!(e.can_read_path(Path::new("tests/test.rs")).allowed);
}

#[test]
fn glob_deny_write_dot_files() {
    let e = engine(&profile_deny_write(&["**/.*"]));
    assert!(!e.can_write_path(Path::new(".env")).allowed);
    assert!(!e.can_write_path(Path::new("config/.gitignore")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn glob_deny_write_specific_extension() {
    let e = engine(&profile_deny_write(&["*.lock"]));
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("package-lock.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn glob_complex_deny_read_nested_env_files() {
    let e = engine(&profile_deny_read(&["**/.env", "**/.env.*"]));
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("config/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.production")).allowed);
    assert!(e.can_read_path(Path::new("src/config.rs")).allowed);
}

#[test]
fn glob_multiple_patterns_deny_write() {
    let e = engine(&profile_deny_write(&["**/.git/**", "**/node_modules/**"]));
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn glob_question_mark_single_char() {
    let e = engine(&profile_deny_read(&["?.txt"]));
    assert!(!e.can_read_path(Path::new("a.txt")).allowed);
    assert!(!e.can_read_path(Path::new("z.txt")).allowed);
    assert!(e.can_read_path(Path::new("ab.txt")).allowed);
}

// =========================================================================
// 3. Multiple profiles — overlay behavior, priority ordering
// =========================================================================

#[test]
fn policy_set_merge_unions_deny_lists() {
    let mut set = PolicySet::new("merged");
    set.add(profile_deny_tools(&["Bash"]));
    set.add(profile_deny_tools(&["Execute"]));
    let merged = set.merge();
    let e = engine(&merged);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn policy_set_merge_unions_allow_lists() {
    let mut set = PolicySet::new("merged");
    set.add(profile_allow_tools(&["Read"]));
    set.add(profile_allow_tools(&["Write"]));
    let merged = set.merge();
    let e = engine(&merged);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn policy_set_merge_deny_read_union() {
    let mut set = PolicySet::new("merged");
    set.add(profile_deny_read(&["*.py"]));
    set.add(profile_deny_read(&["*.sh"]));
    let merged = set.merge();
    let e = engine(&merged);
    assert!(!e.can_read_path(Path::new("run.py")).allowed);
    assert!(!e.can_read_path(Path::new("run.sh")).allowed);
    assert!(e.can_read_path(Path::new("run.rs")).allowed);
}

#[test]
fn policy_set_merge_deny_write_union() {
    let mut set = PolicySet::new("merged");
    set.add(profile_deny_write(&["**/.git/**"]));
    set.add(profile_deny_write(&["**/target/**"]));
    let merged = set.merge();
    let e = engine(&merged);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("target/debug/out")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    set.add(profile_deny_tools(&["Bash", "Bash"]));
    set.add(profile_deny_tools(&["Bash"]));
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
fn policy_set_name_accessor() {
    let set = PolicySet::new("my-set");
    assert_eq!(set.name(), "my-set");
}

// =========================================================================
// 4. Read policies — path-based read allow/deny
// =========================================================================

#[test]
fn read_deny_specific_file() {
    let e = engine(&profile_deny_read(&["secrets.json"]));
    assert!(!e.can_read_path(Path::new("secrets.json")).allowed);
    assert!(e.can_read_path(Path::new("config.json")).allowed);
}

#[test]
fn read_deny_entire_directory() {
    let e = engine(&profile_deny_read(&["private/**"]));
    assert!(!e.can_read_path(Path::new("private/data.txt")).allowed);
    assert!(!e.can_read_path(Path::new("private/sub/deep.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn read_deny_all_files() {
    let e = engine(&profile_deny_read(&["**"]));
    assert!(!e.can_read_path(Path::new("anything.txt")).allowed);
    assert!(!e.can_read_path(Path::new("deep/nested/file.rs")).allowed);
}

#[test]
fn read_deny_by_extension_in_any_dir() {
    let e = engine(&profile_deny_read(&["**/*.pem"]));
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(!e.can_read_path(Path::new("deep/path/key.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn read_deny_reason_contains_path() {
    let e = engine(&profile_deny_read(&["secret*"]));
    let d = e.can_read_path(Path::new("secret.txt"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("secret.txt"));
}

// =========================================================================
// 5. Write policies — path-based write allow/deny
// =========================================================================

#[test]
fn write_deny_specific_file() {
    let e = engine(&profile_deny_write(&["Cargo.lock"]));
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn write_deny_entire_directory() {
    let e = engine(&profile_deny_write(&["vendor/**"]));
    assert!(!e.can_write_path(Path::new("vendor/lib/file.js")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn write_deny_all_files() {
    let e = engine(&profile_deny_write(&["**"]));
    assert!(!e.can_write_path(Path::new("anything.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/c.rs")).allowed);
}

#[test]
fn write_deny_multiple_extensions() {
    let e = engine(&profile_deny_write(&["*.exe", "*.dll", "*.so"]));
    assert!(!e.can_write_path(Path::new("app.exe")).allowed);
    assert!(!e.can_write_path(Path::new("lib.dll")).allowed);
    assert!(!e.can_write_path(Path::new("lib.so")).allowed);
    assert!(e.can_write_path(Path::new("app.rs")).allowed);
}

#[test]
fn write_deny_reason_contains_path() {
    let e = engine(&profile_deny_write(&["locked*"]));
    let d = e.can_write_path(Path::new("locked.md"));
    assert!(!d.allowed);
    assert!(d.reason.as_deref().unwrap().contains("locked.md"));
}

// =========================================================================
// 6. Tool policies — specific tool names, glob patterns for tools
// =========================================================================

#[test]
fn tool_allow_wildcard_permits_all() {
    let e = engine(&profile_allow_tools(&["*"]));
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("AnyTool").allowed);
}

#[test]
fn tool_allow_wildcard_with_deny_specific() {
    let p = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_deny_glob_prefix() {
    let e = engine(&profile_deny_tools(&["File*"]));
    assert!(!e.can_use_tool("FileRead").allowed);
    assert!(!e.can_use_tool("FileWrite").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_allow_glob_pattern() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read*".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_deny_reason_messages() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    let d1 = e.can_use_tool("Bash");
    assert!(d1.reason.as_deref().unwrap().contains("disallowed"));
    let d2 = e.can_use_tool("Write");
    assert!(d2.reason.as_deref().unwrap().contains("not in allowlist"));
}

// =========================================================================
// 7. Intersection/union — composing multiple policy layers
// =========================================================================

#[test]
fn composed_deny_overrides_any_deny_wins() {
    let p1 = PolicyProfile::default(); // allows all tools
    let p2 = profile_deny_tools(&["Bash"]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn composed_allow_overrides_any_allow_wins() {
    let p1 = profile_deny_tools(&["Bash"]);
    let p2 = PolicyProfile::default(); // allows Bash
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_first_applicable_uses_first_decision() {
    let p1 = profile_deny_tools(&["Bash"]);
    let p2 = PolicyProfile::default();
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::FirstApplicable).unwrap();
    // First profile denies Bash
    assert!(ce.check_tool("Bash").is_deny());
}

#[test]
fn composed_first_applicable_order_matters() {
    let p1 = PolicyProfile::default(); // allows Bash
    let p2 = profile_deny_tools(&["Bash"]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::FirstApplicable).unwrap();
    // First profile allows Bash — first applicable wins
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn composed_deny_overrides_read_path() {
    let p1 = PolicyProfile::default();
    let p2 = profile_deny_read(&["**/.env"]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_read(".env").is_deny());
    assert!(ce.check_read("src/lib.rs").is_allow());
}

#[test]
fn composed_deny_overrides_write_path() {
    let p1 = PolicyProfile::default();
    let p2 = profile_deny_write(&["**/.git/**"]);
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_write(".git/config").is_deny());
    assert!(ce.check_write("src/lib.rs").is_allow());
}

#[test]
fn composed_allow_overrides_write() {
    let p1 = profile_deny_write(&["**/*.lock"]);
    let p2 = PolicyProfile::default();
    let ce = ComposedEngine::new(vec![p1, p2], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_write("Cargo.lock").is_allow());
}

#[test]
fn composed_engine_empty_abstains() {
    let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_abstain());
    assert!(ce.check_read("any.txt").is_abstain());
    assert!(ce.check_write("any.txt").is_abstain());
}

// =========================================================================
// 8. Default behaviors — empty policy = allow all, missing section = allow
// =========================================================================

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
fn missing_tool_section_allows_tools() {
    let p = PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("AnyTool").allowed);
}

#[test]
fn missing_read_section_allows_reads() {
    let p = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn missing_write_section_allows_writes() {
    let p = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn composed_policy_empty_is_allowed() {
    let cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert!(cp.evaluate_tool("Bash").is_allowed());
    assert!(cp.evaluate_read("any.txt").is_allowed());
    assert!(cp.evaluate_write("any.txt").is_allowed());
}

// =========================================================================
// 9. Conflict resolution — when allow and deny overlap, deny wins
// =========================================================================

#[test]
fn tool_in_both_allow_and_deny_deny_wins() {
    let p = PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn wildcard_allow_specific_deny_deny_wins() {
    let p = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into(), "Execute".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Execute").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn glob_tool_allow_with_specific_deny() {
    let p = PolicyProfile {
        allowed_tools: vec!["File*".into()],
        disallowed_tools: vec!["FileDelete".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    assert!(e.can_use_tool("FileRead").allowed);
    assert!(e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileDelete").allowed);
}

#[test]
fn composed_all_must_allow_single_deny_vetoes() {
    let p_allow = PolicyProfile::default();
    let p_deny = profile_deny_tools(&["Bash"]);
    let e_allow = engine(&p_allow);
    let e_deny = engine(&p_deny);

    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", e_allow);
    cp.add_policy("restrictive", e_deny);
    let r = cp.evaluate_tool("Bash");
    assert!(r.is_denied());
}

#[test]
fn composed_any_must_allow_single_allow_suffices() {
    let p_deny = profile_deny_tools(&["Bash"]);
    let p_allow = PolicyProfile::default();
    let e_deny = engine(&p_deny);
    let e_allow = engine(&p_allow);

    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("restrictive", e_deny);
    cp.add_policy("permissive", e_allow);
    let r = cp.evaluate_tool("Bash");
    assert!(r.is_allowed());
}

#[test]
fn composed_first_match_strategy() {
    let p1 = profile_deny_tools(&["Bash"]);
    let p2 = PolicyProfile::default();
    let e1 = engine(&p1);
    let e2 = engine(&p2);

    let mut cp = ComposedPolicy::new(CompositionStrategy::FirstMatch);
    cp.add_policy("first", e1);
    cp.add_policy("second", e2);
    let r = cp.evaluate_tool("Bash");
    assert!(r.is_denied());
}

// =========================================================================
// 10. Edge cases — empty globs, overlapping patterns, recursive globs
// =========================================================================

#[test]
fn validator_detects_empty_glob_in_disallowed_tools() {
    let p = PolicyProfile {
        disallowed_tools: vec!["".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_empty_glob_in_deny_read() {
    let p = PolicyProfile {
        deny_read: vec!["".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
}

#[test]
fn validator_detects_overlapping_allow_deny_tools() {
    let p = PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
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
fn validator_detects_unreachable_wildcard_deny() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["*".into()],
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
fn validator_detects_deny_read_catchall() {
    let p = PolicyProfile {
        deny_read: vec!["**".into()],
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
fn validator_detects_deny_write_catchall() {
    let p = PolicyProfile {
        deny_write: vec!["**/*".into()],
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
fn validator_clean_policy_has_no_warnings() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.lock".into()],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&p);
    assert!(warnings.is_empty());
}

#[test]
fn overlapping_deny_read_patterns_both_apply() {
    let e = engine(&profile_deny_read(&["src/**", "src/*.rs"]));
    assert!(!e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_read_path(Path::new("src/deep/file.rs")).allowed);
}

#[test]
fn recursive_glob_deny_write_nested() {
    let e = engine(&profile_deny_write(&["**/secret/**"]));
    assert!(!e.can_write_path(Path::new("secret/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/secret/c/d.txt")).allowed);
    assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn deny_read_and_write_independent() {
    let p = PolicyProfile {
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.lock".into()],
        ..PolicyProfile::default()
    };
    let e = engine(&p);
    // Read denied for .secret but not .lock
    assert!(!e.can_read_path(Path::new("data.secret")).allowed);
    assert!(e.can_read_path(Path::new("Cargo.lock")).allowed);
    // Write denied for .lock but not .secret
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("data.secret")).allowed);
}

#[test]
fn composed_policy_count() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    assert_eq!(cp.policy_count(), 0);
    cp.add_policy("a", engine(&PolicyProfile::default()));
    assert_eq!(cp.policy_count(), 1);
    cp.add_policy("b", engine(&PolicyProfile::default()));
    assert_eq!(cp.policy_count(), 2);
}

#[test]
fn composed_policy_strategy_accessor() {
    let cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    assert_eq!(cp.strategy(), CompositionStrategy::AnyMustAllow);
}

#[test]
fn composed_result_by_field_on_allow() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("engine-a", engine(&PolicyProfile::default()));
    let r = cp.evaluate_tool("Read");
    match r {
        ComposedResult::Allowed { by } => assert_eq!(by, "engine-a"),
        _ => panic!("expected allowed"),
    }
}

#[test]
fn composed_result_by_field_on_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("strict", engine(&profile_deny_tools(&["Bash"])));
    let r = cp.evaluate_tool("Bash");
    match r {
        ComposedResult::Denied { by, reason } => {
            assert_eq!(by, "strict");
            assert!(reason.contains("disallowed"));
        }
        _ => panic!("expected denied"),
    }
}

#[test]
fn composed_read_all_must_allow_deny_wins() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("permissive", engine(&PolicyProfile::default()));
    cp.add_policy("strict", engine(&profile_deny_read(&["*.key"])));
    assert!(cp.evaluate_read("server.key").is_denied());
    assert!(cp.evaluate_read("readme.md").is_allowed());
}

#[test]
fn composed_write_any_must_allow_allow_wins() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AnyMustAllow);
    cp.add_policy("strict", engine(&profile_deny_write(&["**"])));
    cp.add_policy("permissive", engine(&PolicyProfile::default()));
    assert!(cp.evaluate_write("anything.txt").is_allowed());
}

#[test]
fn composed_write_all_must_allow_both_deny() {
    let mut cp = ComposedPolicy::new(CompositionStrategy::AllMustAllow);
    cp.add_policy("a", engine(&profile_deny_write(&["*.lock"])));
    cp.add_policy("b", engine(&profile_deny_write(&["*.lock"])));
    assert!(cp.evaluate_write("Cargo.lock").is_denied());
}

#[test]
fn policy_decision_enum_variants() {
    let allow = PolicyDecision::Allow {
        reason: "ok".into(),
    };
    let deny = PolicyDecision::Deny {
        reason: "no".into(),
    };
    let abstain = PolicyDecision::Abstain;
    assert!(allow.is_allow());
    assert!(!allow.is_deny());
    assert!(!allow.is_abstain());
    assert!(deny.is_deny());
    assert!(!deny.is_allow());
    assert!(abstain.is_abstain());
}

#[test]
fn three_layer_composition_deny_overrides() {
    let p1 = PolicyProfile::default();
    let p2 = profile_allow_tools(&["*"]);
    let p3 = profile_deny_tools(&["Bash"]);
    let ce = ComposedEngine::new(vec![p1, p2, p3], PolicyPrecedence::DenyOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_deny());
    assert!(ce.check_tool("Read").is_allow());
}

#[test]
fn three_layer_composition_allow_overrides() {
    let p1 = profile_deny_tools(&["Bash"]);
    let p2 = profile_deny_tools(&["Bash"]);
    let p3 = PolicyProfile::default();
    let ce = ComposedEngine::new(vec![p1, p2, p3], PolicyPrecedence::AllowOverrides).unwrap();
    assert!(ce.check_tool("Bash").is_allow());
}

#[test]
fn complex_real_world_scenario() {
    // Org policy: deny dangerous tools, deny secrets
    let org = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Shell*".into()],
        deny_read: vec!["**/.env".into(), "**/*.pem".into(), "**/id_rsa".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        ..PolicyProfile::default()
    };
    // Project policy: also deny writing lock files
    let project = PolicyProfile {
        deny_write: vec!["*.lock".into()],
        ..PolicyProfile::default()
    };
    // Merge
    let mut set = PolicySet::new("combined");
    set.add(org);
    set.add(project);
    let merged = set.merge();
    let e = engine(&merged);

    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("ShellExec").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("certs/server.pem")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn decision_clone_and_debug() {
    let d = abp_policy::Decision::deny("test");
    let d2 = d.clone();
    assert!(!d2.allowed);
    assert_eq!(d2.reason.as_deref(), Some("test"));
    // Debug impl exists
    let _ = format!("{d:?}");
}
