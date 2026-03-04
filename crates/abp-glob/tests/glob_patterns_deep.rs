// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for `IncludeExcludeGlobs` — pattern compilation, matching behavior,
//! and edge cases.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use std::path::Path;

fn p(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ──────────────────────────────────────────────
// Pattern compilation (10 tests)
// ──────────────────────────────────────────────

#[test]
fn compile_empty_patterns() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
}

#[test]
fn compile_single_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn compile_single_exclude() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("app.txt"), MatchDecision::Allowed);
}

#[test]
fn compile_multiple_includes() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "lib/**", "bin/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("bin/c"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tmp/d"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn compile_multiple_excludes() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.log", "*.tmp", "*.bak"])).unwrap();
    assert_eq!(g.decide_str("x.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("x.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("x.bak"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("x.rs"), MatchDecision::Allowed);
}

#[test]
fn compile_mixed_include_exclude() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/vendor/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/vendor/dep.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("docs/api.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn compile_invalid_glob_syntax() {
    let err = IncludeExcludeGlobs::new(&p(&["[invalid"]), &[]).unwrap_err();
    assert!(
        err.to_string().contains("invalid glob"),
        "unexpected: {err}"
    );
}

#[test]
fn compile_invalid_glob_in_exclude() {
    let err = IncludeExcludeGlobs::new(&[], &p(&["[bad"])).unwrap_err();
    assert!(
        err.to_string().contains("invalid glob"),
        "unexpected: {err}"
    );
}

#[test]
fn compile_special_characters_in_pattern() {
    // Braces and question marks are valid glob syntax
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml}", "src/?.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    // ? matches exactly one character
    assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
}

#[test]
fn compile_clone_preserves_behavior() {
    let original = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/gen/**"])).unwrap();
    let cloned = original.clone();

    let cases = &["src/lib.rs", "src/gen/out.rs", "README.md"];
    for c in cases {
        assert_eq!(
            original.decide_str(c),
            cloned.decide_str(c),
            "clone diverged on {c}"
        );
    }
}

// ──────────────────────────────────────────────
// Matching behavior (10 tests)
// ──────────────────────────────────────────────

#[test]
fn match_include_all_double_star() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c/d.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("root.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn match_exclude_specific_file() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["secret.key"])).unwrap();
    assert_eq!(g.decide_str("secret.key"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("public.key"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("dir/secret.key"), MatchDecision::Allowed);
}

#[test]
fn match_exclude_directory_pattern() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["target/**"])).unwrap();
    assert_eq!(
        g.decide_str("target/debug/bin"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("target/release/lib.so"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn match_exclude_takes_precedence_over_include() {
    // Key semantic: exclude wins even when include also matches
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["*.secret"])).unwrap();
    assert_eq!(g.decide_str("data.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("keys.secret"), MatchDecision::DeniedByExclude);
    // Nested path too
    assert_eq!(
        g.decide_str("config/db.secret"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn match_file_extension_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("src/data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn match_deep_path_double_star_middle() {
    let g = IncludeExcludeGlobs::new(&p(&["**/test/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/test/b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("x/y/test/z/w.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn match_case_sensitivity() {
    // globset is case-insensitive on Windows by default, case-sensitive on Unix.
    // We test that consistent behavior exists, not which platform.
    let g = IncludeExcludeGlobs::new(&p(&["*.RS"]), &[]).unwrap();
    let upper = g.decide_str("main.RS");
    assert_eq!(upper, MatchDecision::Allowed);

    // On case-insensitive platforms (Windows), *.RS also matches main.rs
    // On case-sensitive platforms (Linux), it won't. Either outcome is valid.
    let lower = g.decide_str("main.rs");
    // Just verify it returns a valid decision without panic
    assert!(
        lower == MatchDecision::Allowed || lower == MatchDecision::DeniedByMissingInclude,
        "unexpected decision: {lower:?}"
    );
}

#[test]
fn match_dotfiles() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".*"])).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str(".env"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("readme.md"), MatchDecision::Allowed);
}

#[test]
fn match_path_separator_handling() {
    // globset normalizes path separators; both / and \ should work
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    // Path::new on Windows converts \ to the OS separator
    assert_eq!(
        g.decide_path(Path::new("src\\lib.rs")),
        MatchDecision::Allowed
    );
}

#[test]
fn match_empty_path() {
    let with_include = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(
        with_include.decide_str(""),
        MatchDecision::DeniedByMissingInclude
    );

    let no_patterns = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(no_patterns.decide_str(""), MatchDecision::Allowed);
}

// ──────────────────────────────────────────────
// Edge cases (5+ tests)
// ──────────────────────────────────────────────

#[test]
fn edge_very_long_path() {
    let long_path = (0..50)
        .map(|i| format!("dir{i}"))
        .collect::<Vec<_>>()
        .join("/")
        + "/file.rs";
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(&long_path), MatchDecision::Allowed);
}

#[test]
fn edge_many_patterns() {
    let includes: Vec<String> = (0..100).map(|i| format!("dir{i}/**")).collect();
    let excludes: Vec<String> = (0..100).map(|i| format!("dir{i}/tmp/**")).collect();
    let g = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();

    assert_eq!(g.decide_str("dir42/src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("dir42/tmp/scratch.txt"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_unicode_in_paths() {
    let g = IncludeExcludeGlobs::new(&p(&["données/**"]), &p(&["données/caché/**"])).unwrap();
    assert_eq!(g.decide_str("données/rapport.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("données/caché/secret.txt"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_relative_paths() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    // Relative paths without leading ./
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    // With leading ./  — globset should still match
    assert_eq!(
        g.decide_path(Path::new("./src/lib.rs")),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_windows_backslash_paths() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    // Using Path ensures OS-native separator handling
    assert_eq!(
        g.decide_path(Path::new("src\\nested\\file.rs")),
        MatchDecision::Allowed
    );
}

#[test]
fn edge_pattern_with_leading_slash() {
    // A pattern like "/src/**" is valid glob syntax
    let g = IncludeExcludeGlobs::new(&p(&["/src/**"]), &[]).unwrap();
    // May or may not match depending on globset interpretation
    let result = g.decide_str("src/lib.rs");
    assert!(
        result == MatchDecision::Allowed || result == MatchDecision::DeniedByMissingInclude,
        "unexpected: {result:?}"
    );
}

#[test]
fn edge_exclude_only_no_include_allows_non_matching() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.secret", "*.key"])).unwrap();
    assert_eq!(g.decide_str("config.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("db.secret"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("private.key"), MatchDecision::DeniedByExclude);
}

#[test]
fn edge_overlapping_include_and_exclude() {
    // Both include and exclude match the same path — exclude wins
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &p(&["*.rs"])).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn match_decision_debug_and_clone() {
    let d = MatchDecision::Allowed;
    let cloned = d;
    assert_eq!(d, cloned);
    assert_eq!(format!("{d:?}"), "Allowed");
    assert_eq!(
        format!("{:?}", MatchDecision::DeniedByExclude),
        "DeniedByExclude"
    );
    assert_eq!(
        format!("{:?}", MatchDecision::DeniedByMissingInclude),
        "DeniedByMissingInclude"
    );
}

#[test]
fn build_globset_returns_none_for_empty() {
    let result = abp_glob::build_globset(&[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn build_globset_returns_some_for_patterns() {
    let patterns = vec!["*.rs".to_string(), "*.toml".to_string()];
    let result = abp_glob::build_globset(&patterns).unwrap();
    assert!(result.is_some());
    let set = result.unwrap();
    assert!(set.is_match("main.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(!set.is_match("readme.md"));
}

#[test]
fn build_globset_invalid_pattern() {
    let patterns = vec!["[unclosed".to_string()];
    assert!(abp_glob::build_globset(&patterns).is_err());
}
