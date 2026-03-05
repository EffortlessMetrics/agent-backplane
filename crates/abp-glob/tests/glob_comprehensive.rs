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
//! Comprehensive tests for the abp-glob crate covering pattern matching,
//! include/exclude logic, edge cases, and performance.

use abp_glob::{IncludeExcludeGlobs, MatchDecision, build_globset};
use std::path::Path;

fn p(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ─── 1. Basic patterns (* matches any segment chars, ** across segments) ────

#[test]
fn star_matches_any_filename() {
    let g = IncludeExcludeGlobs::new(&p(&["*"]), &[]).unwrap();
    assert_eq!(g.decide_str("foo.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("bar"), MatchDecision::Allowed);
}

#[test]
fn star_with_extension_filter() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_matches_across_segments() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c/d.rs"), MatchDecision::Allowed);
}

#[test]
fn double_star_slash_star_rs() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/c.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn star_alone_crosses_separators_globset_default() {
    // globset default: literal_separator = false, so * crosses /
    let g = IncludeExcludeGlobs::new(&p(&["*"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/file.txt"), MatchDecision::Allowed);
}

#[test]
fn double_star_prefix_matches_any_depth() {
    let g = IncludeExcludeGlobs::new(&p(&["**/test.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("test.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/test.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/test.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/c/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ─── 2. ? matches single character ──────────────────────────────────────────

#[test]
fn question_mark_matches_single_char() {
    let g = IncludeExcludeGlobs::new(&p(&["?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
}

#[test]
fn question_mark_rejects_zero_or_multiple_chars() {
    let g = IncludeExcludeGlobs::new(&p(&["?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str(".txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(
        g.decide_str("ab.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn multiple_question_marks() {
    let g = IncludeExcludeGlobs::new(&p(&["???.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("foo.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("ba.rs"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(
        g.decide_str("quux.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn question_mark_in_directory_segment() {
    let g = IncludeExcludeGlobs::new(&p(&["v?/data.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("v1/data.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("v2/data.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("v10/data.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ─── 3. {a,b} alternation patterns ─────────────────────────────────────────

#[test]
fn brace_two_extensions() {
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_three_alternatives() {
    let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml,md}"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_in_directory_segment() {
    let g = IncludeExcludeGlobs::new(&p(&["{src,tests}/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/api.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn brace_nested_with_double_star() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.{rs,toml,md}"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ─── 4. Character classes [abc], [a-z], [!abc] ─────────────────────────────

#[test]
fn char_class_explicit_chars() {
    let g = IncludeExcludeGlobs::new(&p(&["[abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn char_class_range_lowercase() {
    let g = IncludeExcludeGlobs::new(&p(&["[a-z].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("m.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
}

#[test]
fn char_class_digit_range() {
    let g = IncludeExcludeGlobs::new(&p(&["file[0-9].dat"]), &[]).unwrap();
    assert_eq!(g.decide_str("file0.dat"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("file9.dat"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("filea.dat"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn negated_char_class_bang() {
    let g = IncludeExcludeGlobs::new(&p(&["[!0-9].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("1.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn negated_char_class_excludes_listed() {
    let g = IncludeExcludeGlobs::new(&p(&["[!abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
}

// ─── 5. Include patterns (whitelist specific files/dirs) ────────────────────

#[test]
fn include_single_dir() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_multiple_dirs_union() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**", "tests/**", "*.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/guide.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_specific_extension_only() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_nested_dir_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/it.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_exact_file() {
    let g = IncludeExcludeGlobs::new(&p(&["Cargo.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ─── 6. Exclude patterns (blacklist specific files/dirs) ────────────────────

#[test]
fn exclude_single_extension() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.tmp"])).unwrap();
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.txt"), MatchDecision::Allowed);
}

#[test]
fn exclude_entire_directory() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["node_modules/**"])).unwrap();
    assert_eq!(
        g.decide_str("node_modules/pkg/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
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
fn exclude_nested_dir() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["target/**"])).unwrap();
    assert_eq!(
        g.decide_str("target/debug/binary"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/target.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_dotfiles() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".*"])).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
}

#[test]
fn exclude_only_affects_matching_extension() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.pyc"])).unwrap();
    assert_eq!(g.decide_str("module.pyc"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("module.py"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("module.pyo"), MatchDecision::Allowed);
}

// ─── 7. Include+Exclude combination (exclude takes precedence) ──────────────

#[test]
fn exclude_overrides_include_simple() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["secret/**"])).unwrap();
    assert_eq!(g.decide_str("public/page.html"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn exclude_overrides_include_complex() {
    let g = IncludeExcludeGlobs::new(
        &p(&["src/**", "tests/**"]),
        &p(&["src/generated/**", "tests/fixtures/**"]),
    )
    .unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/output.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("tests/unit.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/fixtures/data.json"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("docs/readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_ext_exclude_subdir() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs", "*.toml"]), &p(&["target/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("target/debug/deps/lib.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn include_src_exclude_lock_files() {
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
fn overlapping_include_exclude_same_path_exclude_wins() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn include_multiple_dirs_exclude_generated_and_bak() {
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

// ─── 8. IncludeExcludeGlobs compilation (from pattern lists) ────────────────

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

#[test]
fn build_globset_invalid_returns_error() {
    let err = build_globset(&p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn build_globset_multiple_patterns() {
    let set = build_globset(&p(&["*.rs", "*.toml", "*.md"]))
        .unwrap()
        .unwrap();
    assert!(set.is_match("lib.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(set.is_match("README.md"));
    assert!(!set.is_match("data.json"));
}

#[test]
fn new_compiles_both_empty() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
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

#[test]
fn decide_path_vs_decide_str_consistency() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/secret/**"])).unwrap();
    let cases = ["src/lib.rs", "src/secret/key.pem", "README.md"];
    for c in &cases {
        assert_eq!(
            g.decide_str(c),
            g.decide_path(Path::new(c)),
            "mismatch for: {c:?}"
        );
    }
}

// ─── 9. .git exclusion (always excluded by default in workspace context) ────

#[test]
fn git_dir_excluded_with_explicit_pattern() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/ab/cd1234"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn git_dir_name_alone_without_recursive_glob() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".git"])).unwrap();
    assert_eq!(g.decide_str(".git"), MatchDecision::DeniedByExclude);
    // Without /**, nested files won't match the bare name
    assert_eq!(g.decide_str(".git/config"), MatchDecision::Allowed);
}

#[test]
fn git_named_file_not_excluded_by_dot_git() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str("src/git-hook.sh"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
}

#[test]
fn exclude_hidden_directories_catches_git() {
    let g = IncludeExcludeGlobs::new(&[], &p(&[".*/**"])).unwrap();
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str(".cache/data"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

// ─── 10. Path separator handling (/ vs \ on different platforms) ────────────

#[test]
fn forward_slash_paths_always_work() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/sub/mod.rs"), MatchDecision::Allowed);
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
fn dot_slash_prefix_not_normalized() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    // ./src/lib.rs doesn't start with "src/", so doesn't match src/**
    assert_eq!(
        g.decide_str("./src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn path_with_dot_components() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/./lib.rs"), MatchDecision::Allowed);
}

// ─── 11. Case sensitivity (exact match on case-sensitive systems) ───────────

#[test]
fn exact_case_match() {
    let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
}

#[test]
fn upper_case_pattern_matches_upper_case_path() {
    let g = IncludeExcludeGlobs::new(&p(&["*.RS"]), &[]).unwrap();
    // Always matches the exact case
    assert_eq!(g.decide_str("lib.RS"), MatchDecision::Allowed);
}

#[test]
fn mixed_case_directory() {
    let g = IncludeExcludeGlobs::new(&p(&["Src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("Src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn case_sensitivity_on_extension() {
    // On case-sensitive systems, *.rs won't match .RS; on Windows it will
    let g_lower = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
    let g_upper = IncludeExcludeGlobs::new(&p(&["*.RS"]), &[]).unwrap();
    // Each should match its own case at minimum
    assert_eq!(g_lower.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g_upper.decide_str("lib.RS"), MatchDecision::Allowed);
}

// ─── 12. Nested directory patterns (src/**/*.rs, !src/test/**) ──────────────

#[test]
fn src_double_star_rs() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/it.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_src_double_star() {
    let g = IncludeExcludeGlobs::new(&p(&["**/src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("project/src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/src/c/d.rs"), MatchDecision::Allowed);
}

#[test]
fn deeply_nested_include() {
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("a/b/c/d/e/f/g/h/i/j/k.rs"),
        MatchDecision::Allowed
    );
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
fn include_src_exclude_test_dir() {
    let g =
        IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/test/**", "src/tests/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/test/unit.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("src/tests/integration.rs"),
        MatchDecision::DeniedByExclude
    );
}

// ─── 13. Empty patterns (no includes = include all, no excludes = none) ─────

#[test]
fn no_includes_no_excludes_allows_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("any/nested/path"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn include_only_gates_matches() {
    let g = IncludeExcludeGlobs::new(&p(&["docs/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("docs/readme.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_only_denies_matches_allows_rest() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["*.secret"])).unwrap();
    assert_eq!(g.decide_str("db.secret"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("db.json"), MatchDecision::Allowed);
}

// ─── 14. Invalid patterns (unmatched brackets, bad syntax) ──────────────────

#[test]
fn invalid_unmatched_bracket_in_include() {
    let err = IncludeExcludeGlobs::new(&p(&["["]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_unmatched_bracket_in_exclude() {
    let err = IncludeExcludeGlobs::new(&[], &p(&["["])).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn invalid_pattern_among_valid_ones() {
    let err = IncludeExcludeGlobs::new(&p(&["*.rs", "[", "*.toml"]), &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn valid_patterns_compile_ok() {
    assert!(IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).is_ok());
    assert!(IncludeExcludeGlobs::new(&p(&["**/*.{rs,toml}"]), &[]).is_ok());
    assert!(IncludeExcludeGlobs::new(&p(&["src/**/test_*.rs"]), &[]).is_ok());
    assert!(IncludeExcludeGlobs::new(&p(&["[a-z].txt"]), &[]).is_ok());
}

// ─── 15. Performance (many patterns, many paths, complex nesting) ───────────

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
fn large_combined_include_exclude() {
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
fn many_paths_against_single_pattern_set() {
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

#[test]
fn complex_nesting_many_levels() {
    let g = IncludeExcludeGlobs::new(&p(&["**/deep/**/*.rs"]), &[]).unwrap();
    for depth in 1..=10 {
        let segments: String = (0..depth)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join("/");
        let path = format!("{segments}/deep/inner/file.rs");
        assert_eq!(
            g.decide_str(&path),
            MatchDecision::Allowed,
            "failed at depth {depth}: {path}"
        );
    }
}

// ─── 16. Edge cases ─────────────────────────────────────────────────────────

#[test]
fn empty_string_path_no_patterns() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn empty_string_path_with_include() {
    let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn single_file_exact_match() {
    let g = IncludeExcludeGlobs::new(&p(&["Cargo.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn pattern_without_glob_metacharacters() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["node_modules"])).unwrap();
    assert_eq!(g.decide_str("node_modules"), MatchDecision::DeniedByExclude);
    // Without ** suffix, nested files won't match the bare name
    assert_eq!(
        g.decide_str("node_modules/pkg/index.js"),
        MatchDecision::Allowed
    );
}

#[test]
fn double_extension_tar_gz() {
    let g = IncludeExcludeGlobs::new(&p(&["*.tar.gz"]), &[]).unwrap();
    assert_eq!(g.decide_str("archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("archive.zip"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn min_js_double_extension_exclude() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["*.min.js"])).unwrap();
    assert_eq!(g.decide_str("app.js"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("app.min.js"), MatchDecision::DeniedByExclude);
}

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
fn path_with_unicode() {
    let g = IncludeExcludeGlobs::new(&p(&["données/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("données/fichier.txt"), MatchDecision::Allowed);
}

#[test]
fn path_with_emoji() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["🗂️/**"])).unwrap();
    assert_eq!(g.decide_str("🗂️/data.txt"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("normal/file.txt"), MatchDecision::Allowed);
}

#[test]
fn very_long_path() {
    let segment = "a".repeat(50);
    let path = std::iter::repeat(segment.as_str())
        .take(20)
        .collect::<Vec<_>>()
        .join("/")
        + "/file.rs";
    let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(&path), MatchDecision::Allowed);
}

#[test]
fn path_with_hyphens_and_underscores() {
    let g = IncludeExcludeGlobs::new(&p(&["my-project/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("my-project/src/lib.rs"),
        MatchDecision::Allowed
    );
    let g2 = IncludeExcludeGlobs::new(&p(&["**/_*"]), &[]).unwrap();
    assert_eq!(g2.decide_str("src/_hidden.rs"), MatchDecision::Allowed);
    assert_eq!(
        g2.decide_str("src/public.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn path_with_numbers_in_dir() {
    let g = IncludeExcludeGlobs::new(&p(&["v2/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("v2/api/handler.rs"), MatchDecision::Allowed);
}

#[test]
fn match_decision_is_allowed_helper() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn match_decision_debug_strings() {
    assert_eq!(format!("{:?}", MatchDecision::Allowed), "Allowed");
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
fn match_decision_copy_semantics() {
    let a = MatchDecision::Allowed;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn root_only_path() {
    let g = IncludeExcludeGlobs::new(&p(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("/"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("."), MatchDecision::Allowed);
}

#[test]
fn multiple_stars_in_one_pattern() {
    let g = IncludeExcludeGlobs::new(&p(&["src/*/test_*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/foo/test_bar.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/foo/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_with_double_star_prefix() {
    let g = IncludeExcludeGlobs::new(&[], &p(&["**/__pycache__/**"])).unwrap();
    assert_eq!(
        g.decide_str("pkg/__pycache__/mod.pyc"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("a/b/__pycache__/c.pyc"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.py"), MatchDecision::Allowed);
}
