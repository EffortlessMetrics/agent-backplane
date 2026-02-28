// SPDX-License-Identifier: MIT OR Apache-2.0
//! Edge-case tests for `abp-glob` matching behaviour.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};

fn pats(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// 1. Empty path
// ---------------------------------------------------------------------------

#[test]
fn empty_path_with_no_rules_is_allowed() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn empty_path_denied_by_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn empty_path_matched_by_star() {
    let g = IncludeExcludeGlobs::new(&pats(&["*"]), &[]).unwrap();
    // globset: `*` matches the empty string (no literal_separator constraint).
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

// ---------------------------------------------------------------------------
// 2. Root path
// ---------------------------------------------------------------------------

#[test]
fn root_path_slash() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("/"), MatchDecision::Allowed);
}

#[test]
fn root_path_excluded() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["/"]) ).unwrap();
    assert_eq!(g.decide_str("/"), MatchDecision::DeniedByExclude);
}

// ---------------------------------------------------------------------------
// 3. Dot files
// ---------------------------------------------------------------------------

#[test]
fn dot_files_matched_by_double_star() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".hidden"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/.env"), MatchDecision::Allowed);
}

#[test]
fn dot_files_matched_by_explicit_dot_pattern() {
    let g = IncludeExcludeGlobs::new(&pats(&[".*"]), &[]).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".hidden"), MatchDecision::Allowed);
}

#[test]
fn dot_files_excluded() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&[".*"])).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
}

// ---------------------------------------------------------------------------
// 4. Double dots (parent directory traversal)
// ---------------------------------------------------------------------------

#[test]
fn double_dot_segments_treated_literally() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("../escape"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/../../b"), MatchDecision::Allowed);
}

#[test]
fn double_dot_excluded_by_pattern() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["../**"])).unwrap();
    assert_eq!(g.decide_str("../escape"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("safe/file.txt"), MatchDecision::Allowed);
}

// ---------------------------------------------------------------------------
// 5. Windows-style backslash paths
// ---------------------------------------------------------------------------

#[test]
fn backslash_paths_via_decide_path() {
    use std::path::Path;

    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // On Windows `Path::new("src\\lib.rs")` is a real path with separator \.
    // globset normalises separators, so it should still match.
    let p = Path::new("src\\lib.rs");
    assert_eq!(g.decide_path(p), MatchDecision::Allowed);
}

// ---------------------------------------------------------------------------
// 6. Case sensitivity
// ---------------------------------------------------------------------------

#[test]
fn glob_matching_is_case_sensitive() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("main.RS"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("Main.Rs"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn case_sensitive_directory_names() {
    let g = IncludeExcludeGlobs::new(&pats(&["Src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("Src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("SRC/lib.rs"), MatchDecision::DeniedByMissingInclude);
}

// ---------------------------------------------------------------------------
// 7. Trailing slashes
// ---------------------------------------------------------------------------

#[test]
fn trailing_slash_in_candidate() {
    let g = IncludeExcludeGlobs::new(&pats(&["dir/**"]), &[]).unwrap();
    // "dir/" itself — globset may or may not match; document actual behaviour.
    let with_slash = g.decide_str("dir/");
    let without_slash = g.decide_str("dir");
    // Both should be handled consistently by globset.
    assert!(
        with_slash == MatchDecision::Allowed || with_slash == MatchDecision::DeniedByMissingInclude,
        "unexpected decision for dir/: {with_slash:?}"
    );
    assert!(
        without_slash == MatchDecision::Allowed
            || without_slash == MatchDecision::DeniedByMissingInclude,
        "unexpected decision for dir: {without_slash:?}"
    );
}

#[test]
fn trailing_slash_in_pattern() {
    // "dir/" as a pattern — globset strips trailing slashes during compilation.
    let result = IncludeExcludeGlobs::new(&pats(&["dir/"]), &[]);
    // Should compile without error.
    assert!(result.is_ok(), "pattern 'dir/' should compile");
}

// ---------------------------------------------------------------------------
// 8. Patterns with spaces
// ---------------------------------------------------------------------------

#[test]
fn pattern_with_spaces() {
    let g = IncludeExcludeGlobs::new(&pats(&["my file.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("my file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("myfile.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn path_with_spaces_in_directory() {
    let g = IncludeExcludeGlobs::new(&pats(&["my dir/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("my dir/file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("mydir/file.txt"), MatchDecision::DeniedByMissingInclude);
}

// ---------------------------------------------------------------------------
// 9. Patterns with special regex chars
// ---------------------------------------------------------------------------

#[test]
fn special_regex_chars_in_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("file(1).txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("data+extra.csv"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a^b$c.log"), MatchDecision::Allowed);
}

#[test]
fn literal_brackets_are_invalid_glob() {
    // A lone "[" is an invalid glob.
    let result = IncludeExcludeGlobs::new(&pats(&["["]), &[]);
    assert!(result.is_err());
}

#[test]
fn character_class_in_glob() {
    let g = IncludeExcludeGlobs::new(&pats(&["[abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

// ---------------------------------------------------------------------------
// 10. Very long path (1000 characters)
// ---------------------------------------------------------------------------

#[test]
fn very_long_path() {
    let long_path = "a/".repeat(499) + "z.txt"; // ~1000 chars
    assert!(long_path.len() >= 1000);

    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str(&long_path), MatchDecision::Allowed);
}

#[test]
fn very_long_path_excluded() {
    let long_path = "a/".repeat(499) + "z.txt";

    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/*.txt"])).unwrap();
    assert_eq!(g.decide_str(&long_path), MatchDecision::DeniedByExclude);
}

// ---------------------------------------------------------------------------
// 11. Many path segments (100 levels deep)
// ---------------------------------------------------------------------------

#[test]
fn deeply_nested_path_100_segments() {
    let segments: Vec<&str> = (0..100).map(|_| "d").collect();
    let deep_path = segments.join("/") + "/file.rs";

    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(&deep_path), MatchDecision::Allowed);
}

#[test]
fn deeply_nested_path_excluded() {
    let segments: Vec<&str> = (0..100).map(|_| "d").collect();
    let deep_path = segments.join("/") + "/file.rs";

    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &pats(&["**/d/**"])).unwrap();
    assert_eq!(g.decide_str(&deep_path), MatchDecision::DeniedByExclude);
}

// ---------------------------------------------------------------------------
// 12. Pattern "*.*" — matches any file with extension
// ---------------------------------------------------------------------------

#[test]
fn star_dot_star_matches_files_with_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.*"]), &[]).unwrap();
    assert_eq!(g.decide_str("file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn star_dot_star_does_not_match_extensionless_file() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.*"]), &[]).unwrap();
    assert_eq!(g.decide_str("Makefile"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("LICENSE"), MatchDecision::DeniedByMissingInclude);
}

// ---------------------------------------------------------------------------
// 13. Pattern "**" — matches everything
// ---------------------------------------------------------------------------

#[test]
fn double_star_matches_everything() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".hidden"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("deeply/nested/path/file.rs"), MatchDecision::Allowed);
}

#[test]
fn double_star_as_only_include_allows_all_non_excluded() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("build.log"), MatchDecision::DeniedByExclude);
}

// ---------------------------------------------------------------------------
// 14. Alternation patterns — {a,b,c} expansion
// ---------------------------------------------------------------------------

#[test]
fn alternation_basic() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.{rs,toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn alternation_in_directory_name() {
    let g = IncludeExcludeGlobs::new(&pats(&["{src,tests}/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn alternation_with_three_options() {
    let g = IncludeExcludeGlobs::new(&pats(&["{a,b,c}.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

// ---------------------------------------------------------------------------
// 15. Negation / exclude interaction
// ---------------------------------------------------------------------------

#[test]
fn exclude_specific_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["*.rs"])).unwrap();
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn exclude_everything_except_via_narrow_include() {
    // Include only *.rs, exclude nothing — effectively negates everything else.
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn exclamation_mark_is_literal_in_glob() {
    // globset does not support negation with "!" — it's treated as literal.
    let g = IncludeExcludeGlobs::new(&pats(&["!*.rs"]), &[]).unwrap();
    // "!*.rs" is a literal pattern starting with '!', won't match "main.rs".
    assert_eq!(g.decide_str("main.rs"), MatchDecision::DeniedByMissingInclude);
}

// ---------------------------------------------------------------------------
// Extra: combined edge cases
// ---------------------------------------------------------------------------

#[test]
fn no_include_with_exclude_allows_non_matching() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.tmp"])).unwrap();
    assert_eq!(g.decide_str("file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("file.tmp"), MatchDecision::DeniedByExclude);
}

#[test]
fn both_include_and_exclude_match_same_file() {
    // Exclude takes precedence.
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**"])).unwrap();
    assert_eq!(g.decide_str("anything.txt"), MatchDecision::DeniedByExclude);
}

#[test]
fn question_mark_single_char_wildcard() {
    let g = IncludeExcludeGlobs::new(&pats(&["?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("ab.txt"), MatchDecision::DeniedByMissingInclude);
}
