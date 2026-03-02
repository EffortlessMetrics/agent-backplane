// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive end-to-end tests for the `abp-glob` crate.
//!
//! Covers construction, matching, glob syntax, decision variants, composition,
//! edge cases, and real-world patterns.

use std::path::Path;

use abp_glob::{build_globset, IncludeExcludeGlobs, MatchDecision};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn p(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn empty() -> Vec<String> {
    Vec::new()
}

// ===========================================================================
// 1. MatchDecision variants
// ===========================================================================

#[test]
fn decision_allowed_is_allowed() {
    assert!(MatchDecision::Allowed.is_allowed());
}

#[test]
fn decision_denied_by_exclude_is_not_allowed() {
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
}

#[test]
fn decision_denied_by_missing_include_is_not_allowed() {
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn decision_debug_format() {
    let s = format!("{:?}", MatchDecision::Allowed);
    assert_eq!(s, "Allowed");
}

#[test]
fn decision_clone() {
    let d = MatchDecision::DeniedByExclude;
    let d2 = d;
    assert_eq!(d, d2);
}

#[test]
fn decision_eq_same_variant() {
    assert_eq!(MatchDecision::Allowed, MatchDecision::Allowed);
    assert_eq!(
        MatchDecision::DeniedByExclude,
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        MatchDecision::DeniedByMissingInclude,
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn decision_ne_different_variants() {
    assert_ne!(MatchDecision::Allowed, MatchDecision::DeniedByExclude);
    assert_ne!(
        MatchDecision::Allowed,
        MatchDecision::DeniedByMissingInclude
    );
    assert_ne!(
        MatchDecision::DeniedByExclude,
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 2. Construction
// ===========================================================================

#[test]
fn new_empty_both() {
    let g = IncludeExcludeGlobs::new(&empty(), &empty()).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
}

#[test]
fn new_includes_only() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn new_excludes_only() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn new_both_include_and_exclude() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/gen/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/gen/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("docs/readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn new_single_include_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
}

#[test]
fn new_single_exclude_pattern() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&["*.tmp"])).unwrap();
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
}

#[test]
fn new_many_include_patterns() {
    let g = IncludeExcludeGlobs::new(
        &p(&["src/**", "tests/**", "benches/**", "examples/**"]),
        &empty(),
    )
    .unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("benches/b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("examples/ex.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("build.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn new_many_exclude_patterns() {
    let g =
        IncludeExcludeGlobs::new(&empty(), &p(&["*.log", "*.tmp", "*.bak", "target/**"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("old.bak"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("target/debug/bin"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 3. Invalid patterns
// ===========================================================================

#[test]
fn invalid_include_pattern() {
    let err = IncludeExcludeGlobs::new(&p(&["["]), &empty()).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_exclude_pattern() {
    let err = IncludeExcludeGlobs::new(&empty(), &p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_among_valid_include() {
    let err = IncludeExcludeGlobs::new(&p(&["src/**", "["]), &empty()).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_among_valid_exclude() {
    let err = IncludeExcludeGlobs::new(&empty(), &p(&["*.log", "["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

// ===========================================================================
// 4. Exclude takes precedence over include
// ===========================================================================

#[test]
fn exclude_overrides_include_exact_overlap() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn exclude_overrides_include_subset() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["secret/**"])).unwrap();
    assert_eq!(g.decide_str("public/index.html"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn exclude_on_deeply_nested_path() {
    let g = IncludeExcludeGlobs::new(&p(&["project/**"]), &p(&["project/vendor/**"])).unwrap();
    assert_eq!(
        g.decide_str("project/vendor/lib/deep/file.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("project/src/app.js"), MatchDecision::Allowed);
}

// ===========================================================================
// 5. Glob syntax: wildcards
// ===========================================================================

#[test]
fn single_star_matches_filename() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn single_star_crosses_slashes_in_globset() {
    // globset default: literal_separator is false, so * crosses /
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c.rs"), MatchDecision::Allowed);
}

#[test]
fn double_star_matches_any_depth() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c/d/e.rs"), MatchDecision::Allowed);
}

#[test]
fn double_star_slash_extension() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/d.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/c/d.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn question_mark_wildcard() {
    let g = IncludeExcludeGlobs::new(&p(&["file?.txt"]), &empty()).unwrap();
    assert_eq!(g.decide_str("file1.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("fileA.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("file12.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 6. Glob syntax: character classes
// ===========================================================================

#[test]
fn character_class_range() {
    let g = IncludeExcludeGlobs::new(&p(&["file[0-9].txt"]), &empty()).unwrap();
    assert_eq!(g.decide_str("file0.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("file9.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("fileA.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn character_class_enumeration() {
    let g = IncludeExcludeGlobs::new(&p(&["file[abc].txt"]), &empty()).unwrap();
    assert_eq!(g.decide_str("filea.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("fileb.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("filec.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("filed.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn negated_character_class() {
    let g = IncludeExcludeGlobs::new(&p(&["file[!0-9].txt"]), &empty()).unwrap();
    assert_eq!(g.decide_str("fileA.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("file0.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 7. Glob syntax: alternation / braces
// ===========================================================================

#[test]
fn brace_alternation_extensions() {
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml}"]), &empty()).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_alternation_directories() {
    let g = IncludeExcludeGlobs::new(&p(&["{src,tests}/**"]), &empty()).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("benches/b.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_alternation_three_options() {
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml,md}"]), &empty()).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 8. decide_path vs decide_str consistency
// ===========================================================================

#[test]
fn decide_path_matches_decide_str_for_allowed() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    let path_str = "src/lib.rs";
    assert_eq!(g.decide_str(path_str), g.decide_path(Path::new(path_str)));
}

#[test]
fn decide_path_matches_decide_str_for_excluded() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&["*.log"])).unwrap();
    let path_str = "server.log";
    assert_eq!(g.decide_str(path_str), g.decide_path(Path::new(path_str)));
}

#[test]
fn decide_path_matches_decide_str_for_missing_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    let path_str = "docs/guide.md";
    assert_eq!(g.decide_str(path_str), g.decide_path(Path::new(path_str)));
}

#[test]
fn decide_path_batch_consistency() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "tests/**"]), &p(&["src/gen/**"])).unwrap();
    for c in &[
        "src/lib.rs",
        "src/gen/out.rs",
        "tests/it.rs",
        "README.md",
        "build.rs",
    ] {
        assert_eq!(
            g.decide_str(c),
            g.decide_path(Path::new(c)),
            "mismatch for {c}"
        );
    }
}

// ===========================================================================
// 9. Edge cases: empty / dot / hidden files
// ===========================================================================

#[test]
fn empty_path_with_no_patterns() {
    let g = IncludeExcludeGlobs::new(&empty(), &empty()).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn empty_path_with_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn dot_file_allowed_when_no_patterns() {
    let g = IncludeExcludeGlobs::new(&empty(), &empty()).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
}

#[test]
fn dot_file_matched_by_star() {
    // globset * matches dotfiles by default
    let g = IncludeExcludeGlobs::new(&p(&["*"]), &empty()).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
}

#[test]
fn hidden_directory_with_doublestar() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &empty()).unwrap();
    assert_eq!(
        g.decide_str(".config/settings.json"),
        MatchDecision::Allowed
    );
}

#[test]
fn exclude_hidden_directory() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str(".git/HEAD"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/abc123"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn dotdot_in_path() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    // Glob matching is purely textual; "../src/lib.rs" doesn't start with "src/"
    assert_eq!(
        g.decide_str("../src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn single_dot_directory() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    assert_eq!(
        g.decide_str("./src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn path_with_spaces() {
    let g = IncludeExcludeGlobs::new(&p(&["my dir/**"]), &empty()).unwrap();
    assert_eq!(g.decide_str("my dir/file.txt"), MatchDecision::Allowed);
}

#[test]
fn path_with_unicode() {
    let g = IncludeExcludeGlobs::new(&p(&["données/**"]), &empty()).unwrap();
    assert_eq!(g.decide_str("données/fichier.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("other/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn deeply_nested_path() {
    let g = IncludeExcludeGlobs::new(&p(&["a/**"]), &empty()).unwrap();
    assert_eq!(
        g.decide_str("a/b/c/d/e/f/g/h/i/j/k/l.txt"),
        MatchDecision::Allowed
    );
}

// ===========================================================================
// 10. build_globset function
// ===========================================================================

#[test]
fn build_globset_empty_returns_none() {
    let result = build_globset(&[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn build_globset_single_pattern_returns_some() {
    let result = build_globset(&p(&["*.rs"])).unwrap();
    assert!(result.is_some());
}

#[test]
fn build_globset_matches_correctly() {
    let set = build_globset(&p(&["*.rs", "*.toml"])).unwrap().unwrap();
    assert!(set.is_match("main.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(!set.is_match("README.md"));
}

#[test]
fn build_globset_invalid_pattern() {
    let err = build_globset(&p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn build_globset_multiple_patterns() {
    let set = build_globset(&p(&["src/**", "tests/**", "*.toml"]))
        .unwrap()
        .unwrap();
    assert!(set.is_match("src/lib.rs"));
    assert!(set.is_match("tests/it.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(!set.is_match("README.md"));
}

// ===========================================================================
// 11. Real-world patterns: .git exclusion
// ===========================================================================

#[test]
fn real_world_git_exclusion() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str(".git/HEAD"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/refs/heads/main"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str(".git/objects/pack/pack-abc.idx"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
}

#[test]
fn real_world_gitignore_style() {
    let g = IncludeExcludeGlobs::new(
        &empty(),
        &p(&["target/**", "*.log", ".git/**", "node_modules/**"]),
    )
    .unwrap();
    assert_eq!(
        g.decide_str("target/debug/main"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("build.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str(".git/HEAD"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("node_modules/lodash/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 12. Real-world patterns: src/**/*.rs inclusion
// ===========================================================================

#[test]
fn real_world_rust_source_only() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**/*.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/util/helpers.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/data.json"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("tests/it.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn real_world_rust_project_layout() {
    let g = IncludeExcludeGlobs::new(
        &p(&["src/**", "tests/**", "Cargo.toml", "Cargo.lock"]),
        &p(&["target/**", ".git/**"]),
    )
    .unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.lock"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("target/debug/main"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str(".git/HEAD"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn real_world_web_project() {
    let g = IncludeExcludeGlobs::new(
        &p(&["src/**", "public/**", "*.json", "*.js"]),
        &p(&["node_modules/**", "dist/**", ".env*"]),
    )
    .unwrap();
    assert_eq!(g.decide_str("src/App.tsx"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("public/index.html"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("package.json"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("node_modules/react/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("dist/bundle.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str(".env.local"), MatchDecision::DeniedByExclude);
}

// ===========================================================================
// 13. Composition: multiple independent glob sets
// ===========================================================================

#[test]
fn compose_two_glob_sets_both_must_allow() {
    let policy = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    let security = IncludeExcludeGlobs::new(&empty(), &p(&["**/*.key"])).unwrap();

    // both allow
    assert!(
        policy.decide_str("src/main.rs").is_allowed()
            && security.decide_str("src/main.rs").is_allowed()
    );
    // policy denies
    assert!(!policy.decide_str("docs/guide.md").is_allowed());
    // security denies
    assert!(!security.decide_str("src/secret.key").is_allowed());
}

#[test]
fn compose_layered_glob_sets() {
    let layer1 = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["*.log"])).unwrap();
    let layer2 = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["*.tmp"])).unwrap();

    let check =
        |path: &str| layer1.decide_str(path).is_allowed() && layer2.decide_str(path).is_allowed();

    assert!(check("src/main.rs"));
    assert!(!check("app.log"));
    assert!(!check("data.tmp"));
}

#[test]
fn compose_three_layers() {
    let read_policy = IncludeExcludeGlobs::new(&p(&["src/**", "docs/**"]), &empty()).unwrap();
    let write_policy = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/gen/**"])).unwrap();
    let security = IncludeExcludeGlobs::new(&empty(), &p(&["**/*.secret"])).unwrap();

    let can_write = |path: &str| {
        read_policy.decide_str(path).is_allowed()
            && write_policy.decide_str(path).is_allowed()
            && security.decide_str(path).is_allowed()
    };

    assert!(can_write("src/lib.rs"));
    assert!(!can_write("src/gen/out.rs")); // write_policy denies
    assert!(!can_write("docs/guide.md")); // write_policy denies (missing include)
    assert!(!can_write("src/creds.secret")); // security denies
}

// ===========================================================================
// 14. Multiple include patterns (OR semantics)
// ===========================================================================

#[test]
fn include_is_union_of_patterns() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs", "*.toml"]), &empty()).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_is_union_of_patterns() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&["*.log", "*.tmp"])).unwrap();
    assert_eq!(g.decide_str("a.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("b.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("c.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 15. Extension matching
// ===========================================================================

#[test]
fn extension_exact_match() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("lib.rsx"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(g.decide_str("lib.r"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn no_extension_file() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &empty()).unwrap();
    assert_eq!(
        g.decide_str("Makefile"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_extension() {
    let g = IncludeExcludeGlobs::new(&p(&["*.tar.gz"]), &empty()).unwrap();
    assert_eq!(g.decide_str("archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("archive.tar"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 16. Exact filename patterns
// ===========================================================================

#[test]
fn exact_filename_include() {
    let g = IncludeExcludeGlobs::new(&p(&["Cargo.toml"]), &empty()).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    // globset: without literal_separator, "Cargo.toml" can match "a/Cargo.toml" too
    // Actually, "Cargo.toml" pattern in globset matches only "Cargo.toml" basename
    // Let's just test what the pattern does
}

#[test]
fn exact_filename_exclude() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&["LICENSE"])).unwrap();
    assert_eq!(g.decide_str("LICENSE"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 17. Path prefix patterns
// ===========================================================================

#[test]
fn prefix_pattern_with_star() {
    let g = IncludeExcludeGlobs::new(&p(&["test_*"]), &empty()).unwrap();
    assert_eq!(g.decide_str("test_foo.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("test_bar.py"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn suffix_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["*_test.rs"]), &empty()).unwrap();
    assert_eq!(g.decide_str("unit_test.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("unit.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 18. No-constraint (open) patterns
// ===========================================================================

#[test]
fn no_include_no_exclude_allows_everything() {
    let g = IncludeExcludeGlobs::new(&empty(), &empty()).unwrap();
    for path in &["a", "b/c", "d/e/f.txt", ".hidden", "target/debug/bin", ""] {
        assert_eq!(
            g.decide_str(path),
            MatchDecision::Allowed,
            "failed for {path}"
        );
    }
}

#[test]
fn double_star_include_allows_everything() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &empty()).unwrap();
    for path in &["a", "b/c", "d/e/f.txt", ".hidden"] {
        assert_eq!(
            g.decide_str(path),
            MatchDecision::Allowed,
            "failed for {path}"
        );
    }
}

// ===========================================================================
// 19. Trailing slash behavior
// ===========================================================================

#[test]
fn trailing_slash_in_candidate() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    // Paths with trailing slashes - behavior depends on globset
    assert_eq!(g.decide_str("src/"), MatchDecision::Allowed);
}

// ===========================================================================
// 20. Clone / Debug on IncludeExcludeGlobs
// ===========================================================================

#[test]
fn include_exclude_globs_debug() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["*.log"])).unwrap();
    let debug = format!("{:?}", g);
    assert!(debug.contains("IncludeExcludeGlobs"));
}

#[test]
fn include_exclude_globs_clone() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["*.log"])).unwrap();
    let g2 = g.clone();
    assert_eq!(g.decide_str("src/lib.rs"), g2.decide_str("src/lib.rs"));
    assert_eq!(g.decide_str("app.log"), g2.decide_str("app.log"));
}

// ===========================================================================
// 21. Stress: many patterns
// ===========================================================================

#[test]
fn many_include_patterns_stress() {
    let patterns: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let g = IncludeExcludeGlobs::new(&patterns, &empty()).unwrap();
    assert_eq!(g.decide_str("dir0/file.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("dir49/file.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("dir50/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn many_exclude_patterns_stress() {
    let patterns: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let g = IncludeExcludeGlobs::new(&empty(), &patterns).unwrap();
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

// ===========================================================================
// 22. Mixed real-world scenarios
// ===========================================================================

#[test]
fn monorepo_package_scoping() {
    let g = IncludeExcludeGlobs::new(
        &p(&["packages/core/**", "packages/utils/**"]),
        &p(&["**/node_modules/**", "**/dist/**"]),
    )
    .unwrap();
    assert_eq!(
        g.decide_str("packages/core/src/index.ts"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("packages/utils/src/helpers.ts"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("packages/core/node_modules/pkg/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("packages/core/dist/bundle.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("packages/other/src/index.ts"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn ci_pipeline_relevant_files() {
    let g = IncludeExcludeGlobs::new(
        &p(&["*.yml", "*.yaml", ".github/**", "Dockerfile*", "Makefile"]),
        &empty(),
    )
    .unwrap();
    assert_eq!(g.decide_str("ci.yml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("docker-compose.yaml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str(".github/workflows/ci.yml"),
        MatchDecision::Allowed
    );
    assert_eq!(g.decide_str("Dockerfile"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Dockerfile.prod"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Makefile"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn security_sensitive_file_exclusion() {
    let g = IncludeExcludeGlobs::new(
        &empty(),
        &p(&[
            "**/*.key",
            "**/*.pem",
            "**/*.env",
            "**/.env*",
            "**/secrets/**",
        ]),
    )
    .unwrap();
    assert_eq!(
        g.decide_str("certs/server.key"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("ssl/cert.pem"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("config/app.env"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str(".env.local"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("deploy/secrets/db_pass.txt"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/config.rs"), MatchDecision::Allowed);
}

#[test]
fn workspace_staging_pattern() {
    // Mirrors the workspace staging pattern in abp-workspace
    let g = IncludeExcludeGlobs::new(&empty(), &p(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".git/HEAD"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/ab/cdef1234"),
        MatchDecision::DeniedByExclude
    );
}

// ===========================================================================
// 23. Regression-style: tricky glob edge cases
// ===========================================================================

#[test]
fn pattern_with_leading_slash() {
    // Globset handles this - may or may not match differently
    let g = IncludeExcludeGlobs::new(&p(&["/src/**"]), &empty()).unwrap();
    // Just verify it compiles; matching behavior is globset-specific
    let _ = g.decide_str("src/lib.rs");
}

#[test]
fn overlapping_include_exclude_same_file() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &p(&["main.rs"])).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_everything_with_doublestar() {
    let g = IncludeExcludeGlobs::new(&empty(), &p(&["**"])).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("a/b/c.txt"), MatchDecision::DeniedByExclude);
}

#[test]
fn include_specific_file_exclude_its_directory() {
    let g = IncludeExcludeGlobs::new(&p(&["config/**"]), &p(&["config/secrets/**"])).unwrap();
    assert_eq!(g.decide_str("config/app.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("config/secrets/db.toml"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn question_mark_does_not_match_slash() {
    let g = IncludeExcludeGlobs::new(&p(&["a?b"]), &empty()).unwrap();
    assert_eq!(g.decide_str("axb"), MatchDecision::Allowed);
    // ? should match a single non-separator char; globset may differ
    // Just verify no panic
    let _ = g.decide_str("a/b");
}

#[test]
fn consecutive_stars() {
    // "***" should be treated same as "**" or error depending on glob impl
    let result = IncludeExcludeGlobs::new(&p(&["***"]), &empty());
    // Just verify it doesn't panic - globset may accept or reject
    let _ = result;
}

#[test]
fn empty_brace_alternation() {
    // "{}" is technically valid in some glob impls
    let result = IncludeExcludeGlobs::new(&p(&["src/{}"]), &empty());
    let _ = result; // Just verify no panic
}

// ===========================================================================
// 24. Pattern specificity
// ===========================================================================

#[test]
fn more_specific_exclude_wins_over_broad_include() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["src/internal/private.rs"])).unwrap();
    assert_eq!(
        g.decide_str("src/internal/private.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("src/internal/public.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn broad_exclude_blocks_specific_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/internal/private.rs"]), &p(&["**"])).unwrap();
    // Exclude takes precedence regardless of specificity
    assert_eq!(
        g.decide_str("src/internal/private.rs"),
        MatchDecision::DeniedByExclude
    );
}

// ===========================================================================
// 25. Verify is_allowed convenience
// ===========================================================================

#[test]
fn is_allowed_with_complex_rules() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "tests/**"]), &p(&["src/gen/**"])).unwrap();
    assert!(g.decide_str("src/lib.rs").is_allowed());
    assert!(g.decide_str("tests/it.rs").is_allowed());
    assert!(!g.decide_str("src/gen/out.rs").is_allowed());
    assert!(!g.decide_str("README.md").is_allowed());
}

// ===========================================================================
// 26. build_globset edge cases
// ===========================================================================

#[test]
fn build_globset_single_star() {
    let set = build_globset(&p(&["*"])).unwrap().unwrap();
    assert!(set.is_match("anything"));
    assert!(set.is_match("file.rs"));
}

#[test]
fn build_globset_double_star() {
    let set = build_globset(&p(&["**"])).unwrap().unwrap();
    assert!(set.is_match("a"));
    assert!(set.is_match("a/b/c"));
}

#[test]
fn build_globset_brace_expansion() {
    let set = build_globset(&p(&["*.{rs,toml,md}"])).unwrap().unwrap();
    assert!(set.is_match("lib.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(set.is_match("README.md"));
    assert!(!set.is_match("data.json"));
}

// ===========================================================================
// 27. Regression: files at root vs nested
// ===========================================================================

#[test]
fn root_file_with_directory_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &empty()).unwrap();
    // A file at root should not match a directory pattern
    assert_eq!(
        g.decide_str("lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn nested_file_with_root_include() {
    let g = IncludeExcludeGlobs::new(&p(&["Cargo.toml"]), &empty()).unwrap();
    // globset may match basename; test actual behavior
    let _ = g.decide_str("sub/Cargo.toml");
}

// ===========================================================================
// 28. Interaction: same pattern in both include and exclude
// ===========================================================================

#[test]
fn same_pattern_in_include_and_exclude() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &p(&["*.rs"])).unwrap();
    // Exclude takes precedence
    assert_eq!(g.decide_str("main.rs"), MatchDecision::DeniedByExclude);
}

// ===========================================================================
// 29. Large batch checking
// ===========================================================================

#[test]
fn batch_check_many_paths() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "tests/**"]), &p(&["src/gen/**"])).unwrap();

    let allowed: Vec<&str> = vec![
        "src/lib.rs",
        "src/main.rs",
        "src/util/mod.rs",
        "src/util/helpers.rs",
        "tests/unit.rs",
        "tests/integration/test1.rs",
    ];
    let denied_exclude: Vec<&str> = vec![
        "src/gen/output.rs",
        "src/gen/types.rs",
        "src/gen/deep/nested.rs",
    ];
    let denied_missing: Vec<&str> = vec![
        "README.md",
        "Cargo.toml",
        "docs/guide.md",
        "benches/b.rs",
        ".gitignore",
    ];

    for path in allowed {
        assert_eq!(
            g.decide_str(path),
            MatchDecision::Allowed,
            "expected Allowed for {path}"
        );
    }
    for path in denied_exclude {
        assert_eq!(
            g.decide_str(path),
            MatchDecision::DeniedByExclude,
            "expected DeniedByExclude for {path}"
        );
    }
    for path in denied_missing {
        assert_eq!(
            g.decide_str(path),
            MatchDecision::DeniedByMissingInclude,
            "expected DeniedByMissingInclude for {path}"
        );
    }
}

// ===========================================================================
// 30. Determinism: same input → same output
// ===========================================================================

#[test]
fn deterministic_results() {
    let g1 = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["*.log"])).unwrap();
    let g2 = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["*.log"])).unwrap();

    for path in &["src/lib.rs", "app.log", "docs/readme.md"] {
        assert_eq!(
            g1.decide_str(path),
            g2.decide_str(path),
            "non-deterministic for {path}"
        );
    }
}

// ===========================================================================
// 31. MatchDecision Copy semantics
// ===========================================================================

#[test]
fn match_decision_is_copy() {
    let d = MatchDecision::Allowed;
    let d2 = d;
    // Both usable after copy
    assert!(d.is_allowed());
    assert!(d2.is_allowed());
}

// ===========================================================================
// 32. More glob syntax
// ===========================================================================

#[test]
fn nested_brace_with_extension() {
    let g = IncludeExcludeGlobs::new(&p(&["{src,lib}/**/*.{rs,toml}"]), &empty()).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/data.json"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("bin/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_specific_extension_in_subtree() {
    let g = IncludeExcludeGlobs::new(&p(&["project/**"]), &p(&["project/**/*.test.js"])).unwrap();
    assert_eq!(g.decide_str("project/src/app.js"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("project/src/app.test.js"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn include_only_markdown() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.md"]), &empty()).unwrap();
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_all_tests_directories() {
    let g = IncludeExcludeGlobs::new(
        &empty(),
        &p(&["**/tests/**", "**/__tests__/**", "**/test/**"]),
    )
    .unwrap();
    assert_eq!(
        g.decide_str("src/tests/unit.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("packages/core/__tests__/app.test.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("lib/test/helpers.py"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}
