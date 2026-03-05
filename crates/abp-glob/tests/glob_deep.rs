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
//! Deep tests for `abp-glob` — covers basic patterns, priority semantics,
//! case sensitivity, separator handling, edge cases, Unicode, and error
//! conditions. 90+ tests total.

use abp_glob::*;
use std::path::Path;

fn pats(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ===========================================================================
// 1. Basic glob: single star (*)
// ===========================================================================

#[test]
fn star_matches_any_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["*"]), &[]).unwrap();
    assert_eq!(g.decide_str("file.txt"), MatchDecision::Allowed);
}

#[test]
fn star_crosses_slashes_in_globset_default() {
    // globset default: literal_separator is false, so * crosses /.
    let g = IncludeExcludeGlobs::new(&pats(&["*"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c"), MatchDecision::Allowed);
}

#[test]
fn star_extension_matches_flat_file() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn star_prefix_matches() {
    let g = IncludeExcludeGlobs::new(&pats(&["test_*"]), &[]).unwrap();
    assert_eq!(g.decide_str("test_foo"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("test_"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("best_foo"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 2. Basic glob: double star (**)
// ===========================================================================

#[test]
fn double_star_matches_everything() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/d/e"), MatchDecision::Allowed);
}

#[test]
fn double_star_suffix_matches_nested_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/d.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/c/d.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_prefix_matches_any_ancestor() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/tests/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("tests/unit.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("crate/tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 3. Basic glob: question mark (?)
// ===========================================================================

#[test]
fn question_mark_matches_single_char() {
    let g = IncludeExcludeGlobs::new(&pats(&["?.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("ab.txt"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(g.decide_str(".txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn double_question_mark() {
    let g = IncludeExcludeGlobs::new(&pats(&["??.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("ab.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(
        g.decide_str("abc.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn question_mark_in_directory() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/?/lib.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/a/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/ab/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 4. Basic glob: character classes [abc]
// ===========================================================================

#[test]
fn char_class_basic() {
    let g = IncludeExcludeGlobs::new(&pats(&["[abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("b.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn char_class_range() {
    let g = IncludeExcludeGlobs::new(&pats(&["[0-9].txt"]), &[]).unwrap();
    for d in '0'..='9' {
        assert_eq!(
            g.decide_str(&format!("{d}.txt")),
            MatchDecision::Allowed,
            "{d} should match"
        );
    }
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn char_class_negation() {
    let g = IncludeExcludeGlobs::new(&pats(&["[!abc].txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a.txt"), MatchDecision::DeniedByMissingInclude);
    assert_eq!(g.decide_str("d.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z.txt"), MatchDecision::Allowed);
}

#[test]
fn char_class_in_exclude() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/*.[oa]"])).unwrap();
    assert_eq!(g.decide_str("lib.o"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("lib.a"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("lib.so"), MatchDecision::Allowed);
}

#[test]
fn char_class_alpha_range() {
    let g = IncludeExcludeGlobs::new(&pats(&["[a-f]_file"]), &[]).unwrap();
    assert_eq!(g.decide_str("a_file"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("f_file"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("g_file"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 5. Basic glob: alternation {a,b}
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

#[test]
fn alternation_three_extensions() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.{rs,toml,json}"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("data.json"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("notes.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn alternation_in_exclude() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.{log,tmp,bak}"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("old.bak"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 6. Include-only patterns
// ===========================================================================

#[test]
fn include_single_directory() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/unit.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_multiple_directories() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**", "tests/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("tests/it.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/readme.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_specific_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Cargo.toml"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_literal_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["Cargo.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Cargo.lock"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("src/Cargo.toml"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_multiple_extensions() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs", "**/*.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn include_deeply_nested_match() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/a/b/c/d/e/f/g.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 7. Exclude-only patterns
// ===========================================================================

#[test]
fn exclude_single_extension() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_directory_tree() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["target/**"])).unwrap();
    assert_eq!(
        g.decide_str("target/debug/bin"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_multiple_patterns() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log", "target/**", "*.tmp"])).unwrap();
    assert_eq!(g.decide_str("build.log"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("target/debug/app"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn exclude_specific_file_anywhere() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/secret.key"])).unwrap();
    assert_eq!(g.decide_str("secret.key"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("config/secret.key"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("config/public.key"), MatchDecision::Allowed);
}

#[test]
fn exclude_nothing_when_empty() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/anything"), MatchDecision::Allowed);
}

// ===========================================================================
// 8. Include + exclude combined (exclude takes precedence)
// ===========================================================================

#[test]
fn combined_include_exclude_basic() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/generated/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("tests/foo.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn combined_multiple_dirs_multiple_excludes() {
    let g = IncludeExcludeGlobs::new(
        &pats(&["src/**", "tests/**"]),
        &pats(&["src/generated/**", "tests/fixtures/**"]),
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
fn exclude_overrides_include_for_same_path() {
    // Both include and exclude match the exact same set.
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**"])).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::DeniedByExclude);
}

#[test]
fn exclude_specific_extension_inside_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["**/*.bak"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib.rs.bak"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn include_rs_exclude_test_rs() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &pats(&["**/*_test.rs"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/lib_test.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 9. Nested path matching with **
// ===========================================================================

#[test]
fn double_star_at_start() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/lib.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_in_middle() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**/test.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/test.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/test.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c/test.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("tests/test.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_at_end() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c/d"), MatchDecision::Allowed);
}

#[test]
fn complex_nested_src_rs_pattern() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/src/**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("crate/src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/src/c/d/e.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("src/lib.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 10. File extension matching
// ===========================================================================

#[test]
fn extension_rs() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.rsx"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn extension_toml() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.toml"]), &[]).unwrap();
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Cargo.lock"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.tar.gz"]), &[]).unwrap();
    assert_eq!(g.decide_str("archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("archive.tar"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("archive.gz"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn no_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("Makefile"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn extension_with_alternation() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.{js,ts,jsx,tsx}"]), &[]).unwrap();
    assert_eq!(g.decide_str("app.js"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("app.ts"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("app.jsx"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("app.tsx"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("app.css"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 11. Hidden file matching
// ===========================================================================

#[test]
fn hidden_file_gitignore() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**/.gitignore"])).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("src/.gitignore"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn hidden_directory() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&[".git/**"])).unwrap();
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str(".git/objects/abc"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn include_hidden_files_explicitly() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/.*"]), &[]).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/.hidden"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/visible.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_all_dotfiles() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/.*"])).unwrap();
    assert_eq!(g.decide_str(".env"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("config/.secret"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn hidden_file_ds_store_and_thumbs() {
    let g =
        IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**/.DS_Store", "**/Thumbs.db"])).unwrap();
    assert_eq!(
        g.decide_str("src/.DS_Store"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("docs/Thumbs.db"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 12. Edge cases: empty patterns, single char, very long paths
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
fn empty_string_path_with_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn single_character_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["?"]), &[]).unwrap();
    assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("ab"), MatchDecision::DeniedByMissingInclude);
}

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

#[test]
fn path_with_dots_in_name() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("my.crate.name/src/lib.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn path_with_spaces() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("my project/src/lib.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn path_with_hyphens_and_underscores() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("my-crate_name/src/lib.rs"),
        MatchDecision::Allowed
    );
}

// ===========================================================================
// 13. Unicode path handling
// ===========================================================================

#[test]
fn unicode_directory_name() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("src/données/fichier.rs"),
        MatchDecision::Allowed
    );
}

#[test]
fn unicode_outside_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("données/fichier.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn unicode_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("日本語.txt"), MatchDecision::Allowed);
}

#[test]
fn unicode_in_exclude() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/données/**"])).unwrap();
    assert_eq!(
        g.decide_str("src/données/file.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/data/file.rs"), MatchDecision::Allowed);
}

#[test]
fn emoji_in_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("🎉/party.txt"), MatchDecision::Allowed);
}

// ===========================================================================
// 14. Case sensitivity
// ===========================================================================

#[test]
fn case_sensitive_extension() {
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
fn case_sensitive_directory() {
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

#[test]
fn case_sensitive_literal() {
    let g = IncludeExcludeGlobs::new(&pats(&["Makefile"]), &[]).unwrap();
    assert_eq!(g.decide_str("Makefile"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("makefile"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("MAKEFILE"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 15. Leading/trailing slashes
// ===========================================================================

#[test]
fn trailing_slash_in_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // Trailing slash — still matches the prefix.
    assert_eq!(g.decide_str("src/"), MatchDecision::Allowed);
}

#[test]
fn leading_slash_in_path() {
    // A leading slash means the path starts with `/`, which is unusual but valid.
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("/src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn pattern_without_slash_matches_nested() {
    // globset without literal_separator: *.rs matches nested paths.
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn double_slash_in_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src//lib.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 16. Multiple ** segments
// ===========================================================================

#[test]
fn two_double_star_segments() {
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

#[test]
fn three_double_star_segments() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/a/**/b/**/c.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("x/a/y/b/z/c.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/d.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_only_as_entire_pattern() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/d/e/f"), MatchDecision::Allowed);
}

#[test]
fn double_star_between_dirs() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**/tests/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/tests/foo.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/module/tests/bar.rs"),
        MatchDecision::Allowed
    );
    // globset default: * crosses /, so sub/baz.rs still matches *.rs
    assert_eq!(
        g.decide_str("src/module/tests/sub/baz.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("other/tests/foo.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 17. Overlapping include/exclude patterns
// ===========================================================================

#[test]
fn overlapping_include_subset_of_exclude() {
    // Include *.rs, exclude *.rs — exclude wins.
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &pats(&["**/*.rs"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn exclude_narrows_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["docs/**"]), &pats(&["docs/internal/**"])).unwrap();
    assert_eq!(g.decide_str("docs/public.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("docs/internal/secret.md"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn exclude_broader_than_include() {
    // Include src/**, exclude ** — everything excluded.
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
}

#[test]
fn multiple_overlapping_includes_union() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**", "src/extra/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/extra/bonus.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 18. Empty include list (matches everything)
// ===========================================================================

#[test]
fn empty_include_with_exclude_only_denies_matches() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("data.json"), MatchDecision::Allowed);
}

#[test]
fn empty_include_no_exclude_allows_everything() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(
        g.decide_str("absolutely/anything.xyz"),
        MatchDecision::Allowed
    );
}

// ===========================================================================
// 19. Empty exclude list (excludes nothing)
// ===========================================================================

#[test]
fn empty_exclude_with_include() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // Nothing is excluded.
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/secret.key"), MatchDecision::Allowed);
}

#[test]
fn empty_exclude_no_include_allows_everything() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("a/b/c"), MatchDecision::Allowed);
}

// ===========================================================================
// 20. Path normalization / decide_path vs decide_str
// ===========================================================================

#[test]
fn decide_path_consistent_with_decide_str() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/secret/**"])).unwrap();
    for c in &["src/lib.rs", "src/secret/key.pem", "README.md"] {
        assert_eq!(
            g.decide_str(c),
            g.decide_path(Path::new(c)),
            "mismatch for {c}"
        );
    }
}

#[test]
fn decide_path_with_relative_components() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    // Path::new does not normalize `..`, so this doesn't match.
    assert_eq!(
        g.decide_path(Path::new("other/../src/lib.rs")),
        MatchDecision::DeniedByMissingInclude
    );
}

#[cfg(windows)]
#[test]
fn backslash_via_decide_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &[]).unwrap();
    let p = Path::new("src\\lib.rs");
    assert_eq!(g.decide_path(p), MatchDecision::Allowed);
}

// ===========================================================================
// 21. build_globset function
// ===========================================================================

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

#[test]
fn build_globset_multiple_patterns() {
    let set = build_globset(&pats(&["*.rs", "*.toml"])).unwrap().unwrap();
    assert!(set.is_match("main.rs"));
    assert!(set.is_match("Cargo.toml"));
    assert!(!set.is_match("README.md"));
}

#[test]
fn build_globset_invalid_pattern_errors() {
    assert!(build_globset(&pats(&["["])).is_err());
}

// ===========================================================================
// 22. MatchDecision API
// ===========================================================================

#[test]
fn is_allowed_returns_correct_values() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn match_decision_debug_clone_copy_eq() {
    let a = MatchDecision::Allowed;
    let b = a; // Copy
    assert_eq!(a, b);
    assert_ne!(a, MatchDecision::DeniedByExclude);
    assert_ne!(
        MatchDecision::DeniedByExclude,
        MatchDecision::DeniedByMissingInclude
    );
    let _debug = format!("{a:?}");
}

// ===========================================================================
// 23. Pattern compilation errors
// ===========================================================================

#[test]
fn unclosed_bracket_is_error() {
    let result = IncludeExcludeGlobs::new(&pats(&["["]), &[]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid glob"));
}

#[test]
fn unclosed_bracket_in_exclude_is_error() {
    assert!(IncludeExcludeGlobs::new(&[], &pats(&["["])).is_err());
}

#[test]
fn valid_wide_variety_compiles() {
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
    assert!(IncludeExcludeGlobs::new(&patterns, &[]).is_ok());
}

// ===========================================================================
// 24. Literal and special character patterns
// ===========================================================================

#[test]
fn literal_filename_exact_match() {
    let g = IncludeExcludeGlobs::new(&pats(&["Makefile"]), &[]).unwrap();
    assert_eq!(g.decide_str("Makefile"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("Makefile.bak"),
        MatchDecision::DeniedByMissingInclude
    );
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

#[test]
fn plus_sign_in_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["c++/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("c++/main.cpp"), MatchDecision::Allowed);
}

#[test]
fn dollar_sign_in_filename() {
    let g = IncludeExcludeGlobs::new(&pats(&["$RECYCLE.BIN/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("$RECYCLE.BIN/file"), MatchDecision::Allowed);
}

// ===========================================================================
// 25. Miscellaneous / additional coverage
// ===========================================================================

#[test]
fn exclude_only_lets_non_matching_through() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.exe"])).unwrap();
    assert_eq!(g.decide_str("app.exe"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("app"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn include_everything_exclude_nothing() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &[]).unwrap();
    assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn multiple_includes_are_unioned() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs", "**/*.md"]), &[]).unwrap();
    assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn multiple_excludes_are_unioned() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["*.log", "*.tmp"])).unwrap();
    assert_eq!(g.decide_str("a.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("b.tmp"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("c.rs"), MatchDecision::Allowed);
}

#[test]
fn pattern_with_only_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&[".gitignore"]), &[]).unwrap();
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/.gitignore"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn star_does_not_match_empty_when_suffix_expected() {
    // Pattern `*.rs` expects at least the `.rs` suffix.
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(".rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
}

#[test]
fn decide_str_many_paths_in_batch() {
    let g = IncludeExcludeGlobs::new(
        &pats(&["src/**", "tests/**"]),
        &pats(&["**/*.bak", "**/tmp/**"]),
    )
    .unwrap();

    let cases: &[(&str, MatchDecision)] = &[
        ("src/lib.rs", MatchDecision::Allowed),
        ("src/lib.rs.bak", MatchDecision::DeniedByExclude),
        ("tests/unit.rs", MatchDecision::Allowed),
        ("tests/tmp/scratch.rs", MatchDecision::DeniedByExclude),
        ("docs/guide.md", MatchDecision::DeniedByMissingInclude),
        ("README.md", MatchDecision::DeniedByMissingInclude),
    ];

    for &(path, expected) in cases {
        assert_eq!(g.decide_str(path), expected, "failed for {path}");
    }
}
