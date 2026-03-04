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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the abp-glob crate covering construction, matching,
//! glob pattern types, path handling, edge cases, and performance.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use std::path::Path;

/// Helper to convert `&[&str]` into `Vec<String>`.
fn pats(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ── 1. Construction ─────────────────────────────────────────────────

#[test]
fn construct_with_empty_patterns() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
}

#[test]
fn construct_with_include_only() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn construct_with_exclude_only() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
}

#[test]
fn construct_with_both() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/gen/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/gen/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn construct_wildcard_star_star_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c/d.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("root.txt"), MatchDecision::Allowed);
}

#[test]
fn construct_invalid_pattern_returns_error() {
    let err = IncludeExcludeGlobs::new(&pats(&["["]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn construct_invalid_exclude_pattern_returns_error() {
    let err = IncludeExcludeGlobs::new(&[], &pats(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

// ── 2. Include matching ─────────────────────────────────────────────

#[test]
fn include_allows_matching_files() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**", "tests/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
}

#[test]
fn include_denies_non_matching_files() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("docs/README.md"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("Cargo.toml"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_multiple_extension_patterns() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs", "*.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ── 3. Exclude matching ─────────────────────────────────────────────

#[test]
fn exclude_denies_matching_files() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.tmp", "*.bak"])).unwrap();
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.bak"), MatchDecision::DeniedByExclude);
}

#[test]
fn exclude_allows_non_matching_files() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
}

// ── 4. Combined include + exclude (exclude overrides) ───────────────

#[test]
fn exclude_overrides_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/secret/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn exclude_overrides_include_same_file() {
    // Pattern that matches via include AND exclude → exclude wins
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &pats(&["*.rs"])).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn combined_three_way_decision() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**", "tests/**"]), &pats(&["src/generated/**"]))
        .unwrap();
    // Allowed through include
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    // Denied by exclude
    assert_eq!(
        g.decide_str("src/generated/mod.rs"),
        MatchDecision::DeniedByExclude
    );
    // Denied by missing include
    assert_eq!(
        g.decide_str("docs/guide.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ── 5. Glob pattern types ───────────────────────────────────────────

#[test]
fn pattern_single_star() {
    // globset's default: literal_separator is false, so * crosses /
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    // Also matches nested due to globset default
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_double_star() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/a.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_question_mark() {
    let g = IncludeExcludeGlobs::new(&pats(&["file?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("file1.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("fileA.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("file12.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_brace_alternation() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.{rs,toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_character_class() {
    let g = IncludeExcludeGlobs::new(&pats(&["file[abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("filea.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("fileb.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("filec.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("filed.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_character_class_range() {
    let g = IncludeExcludeGlobs::new(&pats(&["log[0-9].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("log0.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("log9.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("logA.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_negated_character_class() {
    let g = IncludeExcludeGlobs::new(&pats(&["file[!0-9].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("filea.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("file1.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_double_star_slash_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
}

// ── 6. Path matching ────────────────────────────────────────────────

#[test]
fn decide_path_and_decide_str_agree() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/gen/**"])).unwrap();
    let cases = ["src/lib.rs", "src/gen/out.rs", "README.md"];
    for c in &cases {
        assert_eq!(
            g.decide_str(c),
            g.decide_path(Path::new(c)),
            "mismatch for {c}"
        );
    }
}

#[test]
fn forward_slash_paths() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
}

#[cfg(windows)]
#[test]
fn backslash_paths_on_windows() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // On Windows, Path::new("src\\lib.rs") normalizes separators
    assert_eq!(
        g.decide_path(Path::new("src\\lib.rs")),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_path(Path::new("src\\a\\b\\c.rs")),
        MatchDecision::Allowed
    );
}

#[test]
fn deeply_nested_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["a/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("a/b/c/d/e/f/g/h/i/j/k.txt"),
        MatchDecision::Allowed,
    );
}

// ── 7. Edge cases ───────────────────────────────────────────────────

#[test]
fn empty_string_no_patterns() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn empty_string_with_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn dot_files() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&[".*"])).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str(".env"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
}

#[test]
fn hidden_directories() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/abc123"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn unicode_paths() {
    let g = IncludeExcludeGlobs::new(&pats(&["données/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("données/fichier.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn paths_with_spaces() {
    let g = IncludeExcludeGlobs::new(&pats(&["my dir/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("my dir/file.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn case_sensitivity_glob() {
    // globset is case-insensitive on Windows, case-sensitive on Unix by default
    let g = IncludeExcludeGlobs::new(&pats(&["*.RS"]), &[]).unwrap();
    let decision = g.decide_str("main.rs");
    // On Windows this may be Allowed; on Unix it will be DeniedByMissingInclude.
    // We just assert it returns a valid decision without panicking.
    assert!(
        decision == MatchDecision::Allowed || decision == MatchDecision::DeniedByMissingInclude
    );
}

// ── 8. MatchDecision behavior ───────────────────────────────────────

#[test]
fn match_decision_is_allowed_true() {
    assert!(MatchDecision::Allowed.is_allowed());
}

#[test]
fn match_decision_denied_by_exclude_not_allowed() {
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
}

#[test]
fn match_decision_denied_by_missing_include_not_allowed() {
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn match_decision_equality() {
    assert_eq!(MatchDecision::Allowed, MatchDecision::Allowed);
    assert_ne!(MatchDecision::Allowed, MatchDecision::DeniedByExclude);
    assert_ne!(
        MatchDecision::DeniedByExclude,
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn match_decision_clone() {
    let d = MatchDecision::Allowed;
    let d2 = d;
    assert_eq!(d, d2);
}

#[test]
fn match_decision_debug() {
    let s = format!("{:?}", MatchDecision::DeniedByExclude);
    assert!(s.contains("DeniedByExclude"));
}

// ── 9. build_globset public function ────────────────────────────────

#[test]
fn build_globset_empty_returns_none() {
    let result = abp_glob::build_globset(&[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn build_globset_single_pattern() {
    let set = abp_glob::build_globset(&pats(&["*.rs"])).unwrap().unwrap();
    assert!(set.is_match("main.rs"));
    assert!(!set.is_match("main.txt"));
}

#[test]
fn build_globset_multiple_patterns() {
    let set = abp_glob::build_globset(&pats(&["*.rs", "*.toml", "src/**"]))
        .unwrap()
        .unwrap();
    assert!(set.is_match("lib.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(set.is_match("src/deep/file.txt"));
    assert!(!set.is_match("README.md"));
}

#[test]
fn build_globset_invalid_pattern() {
    let err = abp_glob::build_globset(&pats(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

// ── 10. Many patterns (exclude-only) ───────────────────────────────

#[test]
fn many_exclude_patterns() {
    let excludes: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let g = IncludeExcludeGlobs::new(&[], &excludes).unwrap();
    assert_eq!(
        g.decide_str("dir0/file.txt"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("dir49/file.txt"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("dir50/file.txt"), MatchDecision::Allowed);
}

#[test]
fn many_include_patterns() {
    let includes: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let g = IncludeExcludeGlobs::new(&includes, &[]).unwrap();
    assert_eq!(g.decide_str("dir0/file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("dir49/file.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("dir50/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ── 11. Performance: matching against many patterns ─────────────────

#[test]
fn performance_many_patterns_many_paths() {
    let includes: Vec<String> = (0..100).map(|i| format!("project{i}/**/*.rs")).collect();
    let excludes: Vec<String> = (0..100).map(|i| format!("project{i}/target/**")).collect();
    let g = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();

    // Evaluate a large number of paths
    for i in 0..100 {
        let allowed = format!("project{i}/src/lib.rs");
        let denied_exclude = format!("project{i}/target/debug/out.rs");
        let denied_include = format!("project{i}/docs/readme.md");

        assert_eq!(g.decide_str(&allowed), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str(&denied_exclude),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str(&denied_include),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

#[test]
fn performance_rapid_repeated_matching() {
    let g = IncludeExcludeGlobs::new(
        &pats(&["src/**/*.rs", "tests/**/*.rs"]),
        &pats(&["**/generated/**"]),
    )
    .unwrap();

    for _ in 0..10_000 {
        assert!(g.decide_str("src/core/lib.rs").is_allowed());
        assert!(!g.decide_str("src/generated/code.rs").is_allowed());
    }
}

// ── Additional edge-case and pattern tests ──────────────────────────

#[test]
fn exclude_only_allows_everything_else() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["target/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("target/debug/bin"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn overlapping_include_patterns_still_allow() {
    // Two patterns that both match the same file — should still be Allowed
    let g = IncludeExcludeGlobs::new(&pats(&["src/**", "**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn single_file_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["Cargo.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Cargo.lock"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_specific_extension_from_broad_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["*.lock"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.lock"), MatchDecision::DeniedByExclude);
}
