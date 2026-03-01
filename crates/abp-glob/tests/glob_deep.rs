// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for `abp-glob` — covers complex patterns, priority semantics,
//! case sensitivity, separator handling, and error conditions.

use abp_glob::{IncludeExcludeGlobs, MatchDecision, build_globset};
use std::path::Path;

fn pats(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ===========================================================================
// 1. Complex nested patterns: **/src/**/*.rs
// ===========================================================================

#[test]
fn complex_nested_src_rs_pattern() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("crate/src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/src/c/d/e.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/mod/tests/test.rs"),
        MatchDecision::Allowed
    );
    // No "src" ancestor → denied.
    assert_eq!(
        g.decide_str("lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    // Wrong extension.
    assert_eq!(
        g.decide_str("src/lib.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn complex_multi_level_double_star() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/crates/**/tests/**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("crates/abp-core/tests/unit.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("workspace/crates/abp-glob/tests/deep/test.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("crates/abp-core/src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 2. Negative patterns — exclude after include
// ===========================================================================

#[test]
fn exclude_overrides_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/generated/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/output.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn exclude_on_specific_file_inside_included_tree() {
    let g =
        IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**/.DS_Store", "**/Thumbs.db"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/.DS_Store"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("docs/Thumbs.db"),
        MatchDecision::DeniedByExclude
    );
}

// ===========================================================================
// 3. Pattern priority — exclude always wins (not last-match-wins)
// ===========================================================================

#[test]
fn exclude_always_wins_regardless_of_order() {
    // Include everything, exclude *.log.
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn both_include_and_exclude_match_exclude_wins() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**"])).unwrap();
    assert_eq!(
        g.decide_str("literally_anything"),
        MatchDecision::DeniedByExclude
    );
}

// ===========================================================================
// 4. Empty patterns list
// ===========================================================================

#[test]
fn empty_include_empty_exclude_allows_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("deep/nested/path/file.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn build_globset_empty_returns_none() {
    assert!(build_globset(&[]).unwrap().is_none());
}

#[test]
fn build_globset_non_empty_returns_some() {
    let set = build_globset(&pats(&["*.rs"])).unwrap();
    assert!(set.is_some());
    assert!(set.unwrap().is_match("main.rs"));
}

// ===========================================================================
// 5. Literal vs glob characters
// ===========================================================================

#[test]
fn literal_filename_exact_match() {
    let g = IncludeExcludeGlobs::new(&pats(&["Makefile"]), &[]).unwrap();
    assert_eq!(g.decide_str("Makefile"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("makefile"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("Makefile.bak"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_star_is_not_literal() {
    let g = IncludeExcludeGlobs::new(&pats(&["*"]), &[]).unwrap();
    // * matches anything (globset default: no literal separator).
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("deep/nested/path"), MatchDecision::Allowed);
}

#[test]
fn special_regex_chars_are_literal() {
    let g = IncludeExcludeGlobs::new(&pats(&["file(1).txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("file(1).txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("file1.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 6. Case sensitivity behavior
// ===========================================================================

#[test]
fn glob_is_case_sensitive_for_extensions() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.RS"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("main.Rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_is_case_sensitive_for_directories() {
    let g = IncludeExcludeGlobs::new(&pats(&["Src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("Src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("SRC/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn case_sensitive_exclude() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.LOG"])).unwrap();
    assert_eq!(g.decide_str("debug.LOG"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("debug.log"), MatchDecision::Allowed);
}

// ===========================================================================
// 7. Path separator handling (forward vs backslash)
// ===========================================================================

#[test]
fn forward_slash_paths() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
}

#[cfg(windows)]
#[test]
fn backslash_via_decide_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // On Windows, backslash is a path separator; globset normalises it.
    let p = Path::new("src\\lib.rs");
    assert_eq!(g.decide_path(p), MatchDecision::Allowed);
}

#[test]
fn decide_path_and_decide_str_consistent() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/secret/**"])).unwrap();
    for c in &["src/lib.rs", "src/secret/key.pem", "README.md"] {
        assert_eq!(
            g.decide_str(c),
            g.decide_path(Path::new(c)),
            "mismatch for {c}"
        );
    }
}

// ===========================================================================
// 8. Very long path components
// ===========================================================================

#[test]
fn very_long_filename() {
    let name = "a".repeat(255) + ".rs";
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(&name), MatchDecision::Allowed);
}

#[test]
fn very_long_path_1000_chars() {
    let long = "d/".repeat(499) + "f.rs";
    assert!(long.len() >= 1000);
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(&long), MatchDecision::Allowed);
}

#[test]
fn very_long_path_excluded() {
    let long = "d/".repeat(499) + "f.log";
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/*.log"])).unwrap();
    assert_eq!(g.decide_str(&long), MatchDecision::DeniedByExclude);
}

// ===========================================================================
// 9. Patterns with character classes [abc]
// ===========================================================================

#[test]
fn character_class_basic() {
    let g = IncludeExcludeGlobs::new(&pats(&["[abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn character_class_range() {
    let g = IncludeExcludeGlobs::new(&pats(&["[0-9].txt"]), &[]).unwrap();
    for digit in '0'..='9' {
        let name = format!("{digit}.txt");
        assert_eq!(
            g.decide_str(&name),
            MatchDecision::Allowed,
            "{name} should match"
        );
    }
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn character_class_negation() {
    let g = IncludeExcludeGlobs::new(&pats(&["[!abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
}

#[test]
fn character_class_in_exclude() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/*.[oa]"])).unwrap();
    assert_eq!(g.decide_str("lib.o"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("lib.a"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("lib.so"), MatchDecision::Allowed);
}

// ===========================================================================
// 10. Pattern compilation errors
// ===========================================================================

#[test]
fn unclosed_bracket_is_error() {
    let result = IncludeExcludeGlobs::new(&pats(&["["]), &[]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid glob"));
}

#[test]
fn unclosed_bracket_in_exclude_is_error() {
    let result = IncludeExcludeGlobs::new(&[], &pats(&["["]));
    assert!(result.is_err());
}

#[test]
fn build_globset_invalid_pattern_errors() {
    let result = build_globset(&pats(&["["]));
    assert!(result.is_err());
}

#[test]
fn valid_patterns_compile_successfully() {
    // Ensure a wide variety of valid patterns compile without error.
    let patterns = pats(&[
        "**/*.rs",
        "src/**",
        "*.{rs,toml,json}",
        "[a-z].txt",
        "??.txt",
        "**",
        "*",
        "exact_name.txt",
        "**/deep/**/nested/**/*.rs",
        "{a,b,c}/**",
    ]);
    let result = IncludeExcludeGlobs::new(&patterns, &[]);
    assert!(result.is_ok());
}

// ===========================================================================
// 11. Alternation patterns {a,b,c}
// ===========================================================================

#[test]
fn alternation_extensions() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.{rs,toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn alternation_directories() {
    let g = IncludeExcludeGlobs::new(&pats(&["{src,tests,benches}/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/unit.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("benches/bench.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/guide.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 12. MatchDecision method coverage
// ===========================================================================

#[test]
fn is_allowed_returns_correct_values() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn match_decision_debug_and_eq() {
    // MatchDecision derives Debug, Clone, Copy, PartialEq, Eq.
    let a = MatchDecision::Allowed;
    let b = a;
    assert_eq!(a, b);
    assert_ne!(a, MatchDecision::DeniedByExclude);
    let _dbg = format!("{a:?}");
}
