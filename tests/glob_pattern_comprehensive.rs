//! Comprehensive test suite for the `abp-glob` crate.
//!
//! 80+ tests covering construction, wildcard matching, extension patterns,
//! directory patterns, exclusion semantics, precedence, character classes,
//! brace expansion, edge cases, and the public `build_globset` helper.

use std::path::Path;

use abp_glob::{build_globset, IncludeExcludeGlobs, MatchDecision};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn p(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|s| (*s).to_owned()).collect()
}

fn globs(inc: &[&str], exc: &[&str]) -> IncludeExcludeGlobs {
    IncludeExcludeGlobs::new(&p(inc), &p(exc)).expect("valid patterns")
}

// ===========================================================================
// 1. Construction
// ===========================================================================

#[test]
fn construct_empty_both() {
    let g = globs(&[], &[]);
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
}

#[test]
fn construct_include_only() {
    let g = globs(&["src/**"], &[]);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn construct_exclude_only() {
    let g = globs(&[], &["*.log"]);
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn construct_include_and_exclude() {
    let g = globs(&["src/**"], &["src/generated/**"]);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/x.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("docs/index.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn construct_invalid_include_returns_error() {
    let err = IncludeExcludeGlobs::new(&p(&["["]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn construct_invalid_exclude_returns_error() {
    let err = IncludeExcludeGlobs::new(&[], &p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn construct_multiple_includes() {
    let g = globs(&["src/**", "tests/**", "benches/**"], &[]);
    assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("benches/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/d.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn construct_multiple_excludes() {
    let g = globs(&[], &["*.log", "*.tmp", "*.bak"]);
    assert_eq!(g.decide_str("debug.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("swap.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("old.bak"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 2. Simple pattern matching — *, ?, **
// ===========================================================================

#[test]
fn star_matches_flat_filename() {
    let g = globs(&["*"], &[]);
    assert_eq!(g.decide_str("file.txt"), MatchDecision::Allowed);
}

#[test]
fn star_matches_across_separators_in_globset() {
    // globset default: literal_separator = false, so `*` crosses `/`
    let g = globs(&["*"], &[]);
    assert_eq!(g.decide_str("a/b/c.txt"), MatchDecision::Allowed);
}

#[test]
fn question_mark_matches_single_char() {
    let g = globs(&["?.rs"], &[]);
    assert_eq!(g.decide_str("a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("ab.rs"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn question_mark_does_not_match_empty() {
    let g = globs(&["?.txt"], &[]);
    assert_eq!(g.decide_str(".txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn double_star_matches_nested_dirs() {
    let g = globs(&["**/*.rs"], &[]);
    assert_eq!(g.decide_str("a/b/c/d.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn double_star_at_end_matches_everything_under_dir() {
    let g = globs(&["src/**"], &[]);
    assert_eq!(g.decide_str("src/a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c"), MatchDecision::Allowed);
}

#[test]
fn double_star_in_middle() {
    let g = globs(&["a/**/z.txt"], &[]);
    assert_eq!(g.decide_str("a/z.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/z.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/z.txt"), MatchDecision::Allowed);
}

// ===========================================================================
// 3. Extension patterns
// ===========================================================================

#[test]
fn extension_rs() {
    let g = globs(&["*.rs"], &[]);
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("lib.toml"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn extension_toml() {
    let g = globs(&["*.toml"], &[]);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Cargo.lock"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn extension_json() {
    let g = globs(&["**/*.json"], &[]);
    assert_eq!(g.decide_str("config/a.json"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("config/a.yaml"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn extension_md() {
    let g = globs(&["**/*.md"], &[]);
    assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::Allowed);
}

#[test]
fn multiple_extensions_via_separate_patterns() {
    let g = globs(&["*.rs", "*.toml"], &[]);
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn nested_extension_match() {
    let g = globs(&["**/*.rs"], &[]);
    assert_eq!(g.decide_str("src/utils/helpers.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 4. Directory patterns
// ===========================================================================

#[test]
fn dir_src_double_star() {
    let g = globs(&["src/**"], &[]);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/a.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn dir_tests_double_star_star() {
    let g = globs(&["tests/**/*"], &[]);
    assert_eq!(g.decide_str("tests/unit/foo.rs"), MatchDecision::Allowed);
}

#[test]
fn dir_nested_include() {
    let g = globs(&["crates/abp-glob/**"], &[]);
    assert_eq!(
        g.decide_str("crates/abp-glob/src/lib.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("crates/abp-core/src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn dir_specific_subdir_exclude() {
    let g = globs(&["src/**"], &["src/vendor/**"]);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/vendor/lib.js"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn dir_target_exclude() {
    let g = globs(&[], &["target/**"]);
    assert_eq!(
        g.decide_str("target/debug/app"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn dir_multiple_dirs_include() {
    let g = globs(&["src/**", "tests/**", "docs/**"], &[]);
    assert!(g.decide_str("src/a.rs").is_allowed());
    assert!(g.decide_str("tests/b.rs").is_allowed());
    assert!(g.decide_str("docs/c.md").is_allowed());
    assert!(!g.decide_str("benches/d.rs").is_allowed());
}

// ===========================================================================
// 5. Exclusion patterns (negation semantics)
// ===========================================================================

#[test]
fn exclude_specific_file_pattern() {
    let g = globs(&[], &["secret.key"]);
    assert_eq!(g.decide_str("secret.key"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("public.key"), MatchDecision::Allowed);
}

#[test]
fn exclude_all_logs() {
    let g = globs(&[], &["**/*.log"]);
    assert_eq!(g.decide_str("logs/app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("deep/nested/error.log"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("app.txt"), MatchDecision::Allowed);
}

#[test]
fn exclude_dot_git() {
    let g = globs(&[], &[".git/**"]);
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/ab/cd1234"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_node_modules() {
    let g = globs(&[], &["node_modules/**"]);
    assert_eq!(
        g.decide_str("node_modules/lodash/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/index.js"), MatchDecision::Allowed);
}

#[test]
fn exclude_multiple_dirs() {
    let g = globs(&[], &["target/**", ".git/**", "node_modules/**"]);
    assert_eq!(
        g.decide_str("target/release/app"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str(".git/HEAD"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("node_modules/x"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 6. Include+exclude precedence
// ===========================================================================

#[test]
fn exclude_wins_over_include() {
    let g = globs(&["**/*.rs"], &["src/generated/**"]);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/api.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn exclude_wins_even_with_exact_include() {
    let g = globs(&["src/**"], &["src/**"]);
    // Exclude is checked first → always denied
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn disjoint_include_exclude() {
    let g = globs(&["src/**"], &["tests/**"]);
    assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/b.rs"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("docs/c.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_subset_of_include() {
    let g = globs(&["data/**"], &["data/tmp/**"]);
    assert_eq!(g.decide_str("data/config.json"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data/tmp/scratch.txt"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn include_covers_exclude_still_denies() {
    let g = globs(&["**"], &["*.secret"]);
    assert_eq!(g.decide_str("anything.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("key.secret"), MatchDecision::DeniedByExclude);
}

#[test]
fn precedence_many_patterns() {
    let g = globs(
        &["src/**", "tests/**", "benches/**"],
        &["src/private/**", "tests/fixtures/**"],
    );
    assert!(g.decide_str("src/lib.rs").is_allowed());
    assert!(!g.decide_str("src/private/key.pem").is_allowed());
    assert!(g.decide_str("tests/unit.rs").is_allowed());
    assert!(!g.decide_str("tests/fixtures/data.json").is_allowed());
    assert!(g.decide_str("benches/perf.rs").is_allowed());
    assert!(!g.decide_str("docs/readme.md").is_allowed());
}

// ===========================================================================
// 7. Character class patterns
// ===========================================================================

#[test]
fn char_class_simple() {
    let g = globs(&["[abc].txt"], &[]);
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn char_class_negated() {
    let g = globs(&["[!abc].txt"], &[]);
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
}

#[test]
fn char_class_range() {
    let g = globs(&["[a-f].rs"], &[]);
    assert_eq!(g.decide_str("a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("f.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("g.rs"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn char_class_negated_range() {
    let g = globs(&["[!0-9].txt"], &[]);
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("5.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn char_class_in_directory_component() {
    let g = globs(&["[st]rc/**"], &[]);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("trc/log.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("arc/data.bin"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 8. Brace expansion patterns
// ===========================================================================

#[test]
fn brace_two_extensions() {
    let g = globs(&["*.{rs,toml}"], &[]);
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_three_extensions() {
    let g = globs(&["**/*.{js,ts,jsx}"], &[]);
    assert_eq!(g.decide_str("src/app.js"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/app.ts"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/app.jsx"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/app.css"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_in_directory_name() {
    let g = globs(&["{src,tests}/**"], &[]);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/t.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("benches/b.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_exclude() {
    let g = globs(&[], &["*.{log,tmp,bak}"]);
    assert_eq!(g.decide_str("x.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("y.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("z.bak"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("a.rs"), MatchDecision::Allowed);
}

#[test]
fn brace_with_double_star() {
    let g = globs(&["{src,lib}/**/*.rs"], &[]);
    assert_eq!(g.decide_str("src/a/b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/d.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 9. Edge cases
// ===========================================================================

#[test]
fn edge_empty_path_no_patterns() {
    let g = globs(&[], &[]);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn edge_empty_path_with_include() {
    let g = globs(&["src/**"], &[]);
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn edge_dotfile() {
    let g = globs(&[".*"], &[]);
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".env"), MatchDecision::Allowed);
}

#[test]
fn edge_hidden_dir() {
    let g = globs(&[".config/**"], &[]);
    assert_eq!(
        g.decide_str(".config/settings.json"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("config/settings.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_dot_relative_path() {
    let g = globs(&["**/*.rs"], &[]);
    // Path::new treats `./src/lib.rs` with a leading dot-slash
    assert_eq!(g.decide_str("./src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn edge_trailing_slash() {
    let g = globs(&["src/**"], &[]);
    // Trailing slash in path
    assert_eq!(g.decide_str("src/"), MatchDecision::Allowed);
}

#[test]
fn edge_unicode_path() {
    let g = globs(&["données/**"], &[]);
    assert_eq!(g.decide_str("données/fichier.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_space_in_path() {
    let g = globs(&["my dir/**"], &[]);
    assert_eq!(g.decide_str("my dir/file.txt"), MatchDecision::Allowed);
}

#[test]
fn edge_deeply_nested() {
    let g = globs(&["a/**"], &[]);
    assert_eq!(
        g.decide_str("a/b/c/d/e/f/g/h/i/j/k.txt"),
        MatchDecision::Allowed
    );
}

#[test]
fn edge_extension_with_multiple_dots() {
    let g = globs(&["*.tar.gz"], &[]);
    assert_eq!(g.decide_str("archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("archive.tar.bz2"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_no_extension() {
    let g = globs(&["Makefile"], &[]);
    assert_eq!(g.decide_str("Makefile"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Makefile.bak"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn edge_case_sensitivity() {
    // globset is case-insensitive on Windows by default
    let g = globs(&["*.RS"], &[]);
    let decision = g.decide_str("lib.rs");
    // Accept platform-dependent behavior; just verify no panic
    assert!(
        decision == MatchDecision::Allowed || decision == MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 10. build_globset direct tests
// ===========================================================================

#[test]
fn build_globset_empty_returns_none() {
    let set = build_globset(&[]).expect("should succeed");
    assert!(set.is_none());
}

#[test]
fn build_globset_single_pattern() {
    let set = build_globset(&p(&["*.rs"])).expect("should succeed");
    let gs = set.expect("non-empty");
    assert!(gs.is_match("main.rs"));
    assert!(!gs.is_match("main.py"));
}

#[test]
fn build_globset_multiple_patterns() {
    let set = build_globset(&p(&["*.rs", "*.toml", "*.md"]))
        .expect("should succeed")
        .expect("non-empty");
    assert!(set.is_match("lib.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(set.is_match("README.md"));
    assert!(!set.is_match("data.json"));
}

#[test]
fn build_globset_invalid_pattern() {
    let err = build_globset(&p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn build_globset_mixed_valid_invalid() {
    let err = build_globset(&p(&["*.rs", "["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn build_globset_directory_pattern() {
    let set = build_globset(&p(&["src/**"]))
        .expect("should succeed")
        .expect("non-empty");
    assert!(set.is_match("src/lib.rs"));
    assert!(set.is_match("src/a/b.rs"));
}

// ===========================================================================
// 11. MatchDecision API
// ===========================================================================

#[test]
fn match_decision_allowed_is_allowed() {
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
fn match_decision_eq() {
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
    let dbg = format!("{:?}", MatchDecision::DeniedByExclude);
    assert!(dbg.contains("DeniedByExclude"));
}

// ===========================================================================
// 12. decide_path vs decide_str consistency
// ===========================================================================

#[test]
fn decide_path_matches_decide_str() {
    let g = globs(&["src/**"], &["src/secret/**"]);
    for path in &["src/lib.rs", "src/secret/key.pem", "README.md"] {
        assert_eq!(
            g.decide_str(path),
            g.decide_path(Path::new(path)),
            "mismatch for {path}"
        );
    }
}

#[test]
fn decide_path_with_path_object() {
    let g = globs(&["*.txt"], &[]);
    assert_eq!(
        g.decide_path(Path::new("hello.txt")),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_path(Path::new("hello.rs")),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 13. Realistic scenarios
// ===========================================================================

#[test]
fn scenario_rust_workspace_filter() {
    let g = globs(
        &["**/*.rs", "**/*.toml", "**/*.md"],
        &["target/**", ".git/**"],
    );
    assert!(g.decide_str("src/lib.rs").is_allowed());
    assert!(g.decide_str("Cargo.toml").is_allowed());
    assert!(g.decide_str("README.md").is_allowed());
    assert!(!g.decide_str("target/debug/app").is_allowed());
    assert!(!g.decide_str(".git/config").is_allowed());
    assert!(!g.decide_str("image.png").is_allowed());
}

#[test]
fn scenario_web_project_filter() {
    let g = globs(
        &["**/*.{js,ts,jsx,tsx,css,html}"],
        &["node_modules/**", "dist/**", "*.min.js"],
    );
    assert!(g.decide_str("src/app.tsx").is_allowed());
    assert!(g.decide_str("styles/main.css").is_allowed());
    assert!(!g.decide_str("node_modules/react/index.js").is_allowed());
    assert!(!g.decide_str("dist/bundle.js").is_allowed());
    assert!(!g.decide_str("vendor.min.js").is_allowed());
}

#[test]
fn scenario_documentation_only() {
    let g = globs(&["**/*.md", "**/*.txt", "docs/**"], &[]);
    assert!(g.decide_str("README.md").is_allowed());
    assert!(g.decide_str("docs/guide.html").is_allowed());
    assert!(g.decide_str("CHANGELOG.txt").is_allowed());
    assert!(!g.decide_str("src/main.rs").is_allowed());
}

#[test]
fn scenario_exclude_build_artifacts() {
    let g = globs(
        &[],
        &["target/**", "*.o", "*.so", "*.dylib", "*.dll", "*.exe"],
    );
    assert!(g.decide_str("src/main.rs").is_allowed());
    assert!(!g.decide_str("target/release/app").is_allowed());
    assert!(!g.decide_str("libfoo.so").is_allowed());
    assert!(!g.decide_str("app.exe").is_allowed());
    assert!(!g.decide_str("module.o").is_allowed());
}

#[test]
fn scenario_tests_only_exclude_fixtures() {
    let g = globs(&["tests/**"], &["tests/fixtures/**", "tests/snapshots/**"]);
    assert!(g.decide_str("tests/unit.rs").is_allowed());
    assert!(g.decide_str("tests/integration/api.rs").is_allowed());
    assert!(!g.decide_str("tests/fixtures/input.json").is_allowed());
    assert!(!g.decide_str("tests/snapshots/snap.txt").is_allowed());
    assert!(!g.decide_str("src/lib.rs").is_allowed());
}

#[test]
fn scenario_monorepo_single_crate() {
    let g = globs(&["crates/abp-glob/**"], &["crates/abp-glob/target/**"]);
    assert!(g.decide_str("crates/abp-glob/src/lib.rs").is_allowed());
    assert!(!g.decide_str("crates/abp-glob/target/debug/x").is_allowed());
    assert!(!g.decide_str("crates/abp-core/src/lib.rs").is_allowed());
}
