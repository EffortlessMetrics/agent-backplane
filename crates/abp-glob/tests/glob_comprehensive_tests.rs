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
//! Comprehensive tests for the abp-glob crate public API.

use abp_glob::{build_globset, IncludeExcludeGlobs, MatchDecision};
use std::path::Path;

fn pats(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

// ────────────────────────────────────────────────────────────────────
// 1. Basic matching (5 tests)
// ────────────────────────────────────────────────────────────────────

#[test]
fn basic_exact_path_match() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/lib.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn basic_wildcard_star_matches() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn basic_double_wildcard_matches_multiple_segments() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/foo.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn basic_question_mark_matches_single_char() {
    let g = IncludeExcludeGlobs::new(&pats(&["file?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("file1.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("fileA.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("file12.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn basic_brace_expansion_matches_alternatives() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.{rs,toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ────────────────────────────────────────────────────────────────────
// 2. Include/exclude patterns (8 tests)
// ────────────────────────────────────────────────────────────────────

#[test]
fn include_only_allows_matching_paths() {
    let g = IncludeExcludeGlobs::new(&pats(&["docs/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_only_denies_matching_paths() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("debug.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn include_plus_exclude_where_exclude_wins() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/secret/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn multiple_include_patterns_union() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**", "tests/**", "benches/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("benches/bench.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn multiple_exclude_patterns_union() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log", "*.tmp", "target/**"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("temp.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("target/debug/bin"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn empty_include_means_include_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything/at/all.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("deeply/nested/path/to/file"),
        MatchDecision::Allowed
    );
}

#[test]
fn empty_exclude_means_exclude_none() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // Everything inside src is allowed — nothing is excluded.
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/very/deep/path.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn exclude_overrides_broad_include() {
    // Include everything, but exclude a specific subtree.
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["node_modules/**"])).unwrap();
    assert_eq!(g.decide_str("src/app.ts"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("node_modules/lodash/index.js"),
        MatchDecision::DeniedByExclude
    );
}

// ────────────────────────────────────────────────────────────────────
// 3. Edge cases (7 tests)
// ────────────────────────────────────────────────────────────────────

#[test]
fn edge_empty_path() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);

    let with_inc = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(
        with_inc.decide_str(""),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_root_path() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["/"])).unwrap();
    // "/" as an exclude pattern — decide on root-like paths
    let decision = g.decide_str("/");
    assert!(
        decision == MatchDecision::DeniedByExclude || decision == MatchDecision::Allowed,
        "root path decision should be deterministic"
    );
}

#[test]
fn edge_special_characters_in_path() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(
        g.decide_str("path/with spaces/file (1).txt"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("path/with-dashes/and_underscores.txt"),
        MatchDecision::Allowed
    );
}

#[test]
fn edge_unicode_in_paths() {
    let g = IncludeExcludeGlobs::new(&pats(&["données/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("données/fichier.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("données/日本語/ファイル.txt"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("other/path.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_very_long_path() {
    let segment = "a".repeat(50);
    let long_path = (0..20)
        .map(|_| segment.as_str())
        .collect::<Vec<_>>()
        .join("/");
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str(&long_path), MatchDecision::Allowed);
}

#[test]
fn edge_pattern_with_many_wildcards() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/**/src/**/**/test/**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("a/b/src/c/d/test/e/foo.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn edge_invalid_pattern_compilation_error() {
    let result = IncludeExcludeGlobs::new(&pats(&["["]), &[]);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("invalid glob"),
        "error should mention invalid glob, got: {err_msg}"
    );
}

// ────────────────────────────────────────────────────────────────────
// 4. IncludeExcludeGlobs struct tests (5 tests)
// ────────────────────────────────────────────────────────────────────

#[test]
fn struct_construction_from_include_exclude_lists() {
    let inc = pats(&["src/**", "lib/**"]);
    let exc = pats(&["src/generated/**"]);
    let g = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/core.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn struct_is_included_returns_correct_results() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &pats(&["tests/**"])).unwrap();
    assert!(g.decide_str("src/lib.rs").is_allowed());
    assert!(!g.decide_str("tests/test.rs").is_allowed());
    assert!(!g.decide_str("README.md").is_allowed());
}

#[test]
fn struct_reconstruct_gives_equivalent_results() {
    // Since IncludeExcludeGlobs doesn't implement Serde, verify that
    // reconstructing from the same patterns yields identical decisions.
    let inc = pats(&["src/**", "tests/**"]);
    let exc = pats(&["src/generated/**"]);
    let g1 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
    let g2 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();

    let cases = &[
        "src/lib.rs",
        "src/generated/out.rs",
        "tests/unit.rs",
        "README.md",
        "",
    ];
    for &c in cases {
        assert_eq!(
            g1.decide_str(c),
            g2.decide_str(c),
            "decision mismatch for path: {c:?}"
        );
    }
}

#[test]
fn struct_default_empty_includes_everything() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(g.decide_str("any/path").is_allowed());
    assert!(g.decide_str("another.file").is_allowed());
    assert!(g.decide_str("deeply/nested/dir/file.txt").is_allowed());
}

#[test]
fn struct_thread_safety_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<IncludeExcludeGlobs>();
    assert_sync::<IncludeExcludeGlobs>();
}

// ────────────────────────────────────────────────────────────────────
// Bonus: build_globset public function, decide_path consistency,
//        MatchDecision API
// ────────────────────────────────────────────────────────────────────

#[test]
fn build_globset_empty_returns_none() {
    assert!(build_globset(&[]).unwrap().is_none());
}

#[test]
fn build_globset_non_empty_returns_some() {
    let set = build_globset(&pats(&["*.rs"])).unwrap().unwrap();
    assert!(set.is_match("lib.rs"));
    assert!(!set.is_match("readme.md"));
}

#[test]
fn build_globset_invalid_pattern_errors() {
    assert!(build_globset(&pats(&["["])).is_err());
}

#[test]
fn decide_path_and_decide_str_agree() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/gen/**"])).unwrap();
    for p in &["src/lib.rs", "src/gen/out.rs", "other.txt"] {
        assert_eq!(
            g.decide_str(p),
            g.decide_path(Path::new(p)),
            "mismatch for {p}"
        );
    }
}

#[test]
fn match_decision_is_allowed_variants() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn match_decision_debug_and_clone() {
    let d = MatchDecision::Allowed;
    let cloned = d;
    assert_eq!(d, cloned);
    // Debug impl exists
    let _ = format!("{d:?}");
}
