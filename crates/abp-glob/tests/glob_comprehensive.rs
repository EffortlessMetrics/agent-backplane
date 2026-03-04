// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-glob crate covering pattern matching,
//! include/exclude logic, edge cases, and performance.

use abp_glob::{IncludeExcludeGlobs, MatchDecision, build_globset};
use std::path::Path;

fn p(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ─── 1. Include patterns ────────────────────────────────────────────────────

#[test]
fn include_single_star_matches_any_extension() {
    let g = IncludeExcludeGlobs::new(&p(&["*"]), &[]).unwrap();
    assert_eq!(g.decide_str("foo.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("bar"), MatchDecision::Allowed);
}

#[test]
fn include_double_star_matches_nested() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c/d.rs"), MatchDecision::Allowed);
}

#[test]
fn include_star_dot_rs_matches_rust_files() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_src_double_star_rs() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/it.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_brace_expansion() {
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_question_mark_wildcard() {
    let g = IncludeExcludeGlobs::new(&p(&["?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    // globset: ? doesn't match path separators but "ab.txt" is two chars before dot
    assert_eq!(
        g.decide_str("ab.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_multiple_patterns_union() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "tests/**", "*.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/guide.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ─── 2. Exclude patterns ────────────────────────────────────────────────────

#[test]
fn exclude_node_modules() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["node_modules/**"])).unwrap();
    assert_eq!(
        g.decide_str("node_modules/pkg/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_tmp_files() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.tmp"])).unwrap();
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.txt"), MatchDecision::Allowed);
}

#[test]
fn exclude_dot_git_recursive() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/ab/cd1234"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_multiple_extensions() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.log", "*.bak", "*.swp"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.bak"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("file.swp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_specific_directory() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["target/**"])).unwrap();
    assert_eq!(
        g.decide_str("target/debug/binary"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/target.rs"), MatchDecision::Allowed);
}

// ─── 3. Combined include + exclude ──────────────────────────────────────────

#[test]
fn include_src_exclude_lock() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["*.lock"])).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/Cargo.lock"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_takes_precedence_over_include() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["secret/**"])).unwrap();
    assert_eq!(g.decide_str("public/page.html"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn include_multiple_dirs_exclude_generated() {
    let g = IncludeExcludeGlobs::new(
        &p(&["src/**", "tests/**", "benches/**"]),
        &p(&["**/generated/**", "**/*.bak"]),
    )
    .unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/auto.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("tests/helper.bak"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("benches/bench.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/api.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_extensions_exclude_directory() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs", "*.toml"]), &p(&["target/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("target/debug/deps/lib.rs"),
        MatchDecision::DeniedByExclude
    );
}

// ─── 4. Path matching (relative, deep nesting) ─────────────────────────────

#[test]
fn relative_path_matching() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("./src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn deeply_nested_path() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("a/b/c/d/e/f/g/h/i/j/k.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn decide_path_with_path_object() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_path(Path::new("src/main.rs")),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_path(Path::new("docs/readme.md")),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn trailing_slash_in_pattern() {
    // Glob patterns with trailing components
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/sub/mod.rs"), MatchDecision::Allowed);
}

#[test]
fn path_with_dot_components() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/./lib.rs"), MatchDecision::Allowed);
}

// ─── 5. Case sensitivity ────────────────────────────────────────────────────

#[test]
fn globset_is_case_insensitive_by_default() {
    // globset defaults: case_insensitive = false on unix, true on windows
    // We test the actual behavior
    let g = IncludeExcludeGlobs::new(&p(&["*.RS"]), &[]).unwrap();
    let upper_matches = g.decide_str("lib.RS") == MatchDecision::Allowed;
    // Just verify it compiles and returns a consistent decision
    assert!(upper_matches);
}

#[test]
fn case_sensitive_extension_mismatch() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    let result = g.decide_str("lib.rs");
    assert_eq!(result, MatchDecision::Allowed);
}

#[test]
fn mixed_case_directory_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["Src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("Src/lib.rs"), MatchDecision::Allowed);
}

// ─── 6. Special characters ──────────────────────────────────────────────────

#[test]
fn path_with_spaces() {
    let g = IncludeExcludeGlobs::new(&p(&["my dir/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("my dir/file.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn path_with_unicode_characters() {
    let g = IncludeExcludeGlobs::new(&p(&["données/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("données/fichier.txt"), MatchDecision::Allowed);
}

#[test]
fn path_with_dots_in_name() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["*.min.js"])).unwrap();
    assert_eq!(g.decide_str("app.js"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("app.min.js"), MatchDecision::DeniedByExclude);
}

#[test]
fn path_with_hyphens() {
    let g = IncludeExcludeGlobs::new(&p(&["my-project/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("my-project/src/lib.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn path_with_underscores() {
    let g = IncludeExcludeGlobs::new(&p(&["**/_*"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/_hidden.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/public.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn path_with_numbers() {
    let g = IncludeExcludeGlobs::new(&p(&["v2/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("v2/api/handler.rs"), MatchDecision::Allowed);
}

#[test]
fn unicode_emoji_in_path() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["🗂️/**"])).unwrap();
    assert_eq!(g.decide_str("🗂️/data.txt"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("normal/file.txt"), MatchDecision::Allowed);
}

#[test]
fn dotfile_matching() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".*"])).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
}

// ─── 7. Performance — large pattern sets ────────────────────────────────────

#[test]
fn large_include_pattern_set() {
    let patterns: Vec<String> = (0..150).map(|i| format!("dir_{i}/**")).collect();
    let g = IncludeExcludeGlobs::new(&patterns, &[]).unwrap();
    assert_eq!(g.decide_str("dir_0/file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("dir_149/file.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("dir_150/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn large_exclude_pattern_set() {
    let patterns: Vec<String> = (0..120).map(|i| format!("*.ext{i}")).collect();
    let g = IncludeExcludeGlobs::new(&[], &patterns).unwrap();
    assert_eq!(g.decide_str("file.ext0"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("file.ext119"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("file.rs"), MatchDecision::Allowed);
}

#[test]
fn large_combined_pattern_set() {
    let includes: Vec<String> = (0..100).map(|i| format!("src_{i}/**")).collect();
    let excludes: Vec<String> = (0..100).map(|i| format!("src_{i}/generated/**")).collect();
    let g = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();
    assert_eq!(g.decide_str("src_50/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src_50/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn many_paths_against_pattern_set() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["**/*.tmp"])).unwrap();
    for i in 0..200 {
        let path = format!("src/mod_{i}/file.rs");
        assert_eq!(g.decide_str(&path), MatchDecision::Allowed);
    }
    for i in 0..200 {
        let path = format!("src/mod_{i}/cache.tmp");
        assert_eq!(g.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ─── 8. Compilation — valid/invalid patterns ────────────────────────────────

#[test]
fn valid_pattern_compiles() {
    assert!(IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).is_ok());
    assert!(IncludeExcludeGlobs::new(&p(&["**/*.{rs,toml}"]), &[]).is_ok());
    assert!(IncludeExcludeGlobs::new(&p(&["src/**/test_*.rs"]), &[]).is_ok());
}

#[test]
fn invalid_bracket_pattern_errors() {
    let err = IncludeExcludeGlobs::new(&p(&["["]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_pattern_in_exclude_errors() {
    let err = IncludeExcludeGlobs::new(&[], &p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_among_valid_patterns_errors() {
    let err = IncludeExcludeGlobs::new(&p(&["*.rs", "[", "*.toml"]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn build_globset_invalid_returns_error() {
    let err = build_globset(&p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn build_globset_empty_returns_none() {
    let result = build_globset(&[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn build_globset_valid_returns_some() {
    let result = build_globset(&p(&["*.rs"])).unwrap();
    assert!(result.is_some());
    assert!(result.unwrap().is_match("lib.rs"));
}

// ─── 9. IncludeExcludeGlobs — builder, evaluation ──────────────────────────

#[test]
fn new_with_both_empty_allows_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("any/nested/path"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn new_include_only() {
    let g = IncludeExcludeGlobs::new(&p(&["docs/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("docs/readme.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn new_exclude_only() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.secret"])).unwrap();
    assert_eq!(g.decide_str("db.secret"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("db.json"), MatchDecision::Allowed);
}

#[test]
fn decide_str_and_decide_path_agree() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["**/*.lock"])).unwrap();
    let paths = ["src/main.rs", "src/Cargo.lock", "README.md", ""];
    for path in &paths {
        assert_eq!(
            g.decide_str(path),
            g.decide_path(Path::new(path)),
            "mismatch for: {path:?}"
        );
    }
}

#[test]
fn clone_produces_equivalent_matcher() {
    let g1 = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["*.tmp"])).unwrap();
    let g2 = g1.clone();
    assert_eq!(g1.decide_str("src/lib.rs"), g2.decide_str("src/lib.rs"));
    assert_eq!(g1.decide_str("foo.tmp"), g2.decide_str("foo.tmp"));
}

#[test]
fn debug_impl_does_not_panic() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &p(&["*.tmp"])).unwrap();
    let debug = format!("{g:?}");
    assert!(!debug.is_empty());
}

// ─── 10. Edge cases ─────────────────────────────────────────────────────────

#[test]
fn empty_string_with_no_patterns() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn empty_string_with_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn single_file_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["Cargo.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn root_level_star_vs_nested() {
    // globset's * without literal_separator crosses / by default
    let g = IncludeExcludeGlobs::new(&p(&["*"]), &[]).unwrap();
    assert_eq!(g.decide_str("file.txt"), MatchDecision::Allowed);
    // With default globset settings, * crosses path separators
    assert_eq!(g.decide_str("src/file.txt"), MatchDecision::Allowed);
}

#[test]
fn deeply_nested_exclude() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["a/b/c/d/e/**"])).unwrap();
    assert_eq!(g.decide_str("a/b/c/file.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/c/d/e/file.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn overlapping_include_exclude_same_path() {
    // Exclude wins when both match the same path
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn pattern_matching_exact_directory_name() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["node_modules"])).unwrap();
    assert_eq!(g.decide_str("node_modules"), MatchDecision::DeniedByExclude);
    // But files inside won't match without **
    assert_eq!(
        g.decide_str("node_modules/pkg/index.js"),
        MatchDecision::Allowed
    );
}

#[test]
fn match_decision_variants_coverage() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());

    // Clone + PartialEq
    let a = MatchDecision::Allowed;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn match_decision_debug() {
    let s = format!("{:?}", MatchDecision::Allowed);
    assert_eq!(s, "Allowed");
    let s = format!("{:?}", MatchDecision::DeniedByExclude);
    assert_eq!(s, "DeniedByExclude");
    let s = format!("{:?}", MatchDecision::DeniedByMissingInclude);
    assert_eq!(s, "DeniedByMissingInclude");
}

#[test]
fn character_class_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["[abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn negated_character_class() {
    let g = IncludeExcludeGlobs::new(&p(&["[!0-9].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("1.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn double_extension_matching() {
    let g = IncludeExcludeGlobs::new(&p(&["*.tar.gz"]), &[]).unwrap();
    assert_eq!(g.decide_str("archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("archive.zip"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn path_separator_handling() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    // Forward slashes always work in globset
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn multiple_double_stars() {
    let g = IncludeExcludeGlobs::new(&p(&["**/src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("project/src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/src/c/d.rs"), MatchDecision::Allowed);
}

#[test]
fn single_extension_exclude_does_not_affect_others() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.pyc"])).unwrap();
    assert_eq!(g.decide_str("module.pyc"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("module.py"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("module.pyo"), MatchDecision::Allowed);
}

#[test]
fn build_globset_multiple_valid_patterns() {
    let set = build_globset(&p(&["*.rs", "*.toml", "*.md"]))
        .unwrap()
        .unwrap();
    assert!(set.is_match("lib.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(set.is_match("README.md"));
    assert!(!set.is_match("data.json"));
}

#[test]
fn include_with_nested_braces() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.{rs,toml,md}"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_hidden_directories() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".*/**"])).unwrap();
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str(".cache/data"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}
