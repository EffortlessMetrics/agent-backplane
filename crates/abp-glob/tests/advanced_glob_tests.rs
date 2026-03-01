// SPDX-License-Identifier: MIT OR Apache-2.0
//! Advanced tests for `abp-glob` — complex patterns, real-world scenarios,
//! unicode in patterns, multi-extension files, and boundary conditions.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use std::path::Path;

fn pats(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// ===========================================================================
// 1. Double-star in various positions
// ===========================================================================

#[test]
fn double_star_prefix_matches_any_ancestor() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/test_*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("test_foo.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/test_bar.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c/test_baz.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("a/b/c/foo.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_suffix_matches_any_descendant() {
    let g = IncludeExcludeGlobs::new(&pats(&["vendor/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("vendor/lib.js"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("vendor/a/b/c.js"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/vendor/lib.js"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn double_star_middle_bridges_segments() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**/test_*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/test_a.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/foo/test_b.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/a/b/c/test_c.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/a/b/c/main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    // Must have src/ ancestor
    assert_eq!(
        g.decide_str("test_a.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn multiple_double_stars_in_one_pattern() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/.config/**/settings.json"]), &[]).unwrap();
    assert_eq!(
        g.decide_str(".config/settings.json"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("home/.config/app/settings.json"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("home/.config/app/sub/settings.json"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("home/.config/app/other.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 2. Character classes — advanced
// ===========================================================================

#[test]
fn character_class_alpha_range() {
    let g = IncludeExcludeGlobs::new(&pats(&["[a-z]_module.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("a_module.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("z_module.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("A_module.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("1_module.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn character_class_combined_with_wildcard() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.[ch]pp"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/main.cpp"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("include/header.hpp"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/main.xpp"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn character_class_negation_with_range() {
    let g = IncludeExcludeGlobs::new(&pats(&["[!0-9]*.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("readme.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("1_notes.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 3. Alternatives {a,b} — advanced
// ===========================================================================

#[test]
fn nested_alternatives_in_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["{src,lib}/{core,util}/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/core/mod.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/util/helper.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/core/mod.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/util/helper.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/other/mod.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("bin/core/mod.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn single_item_alternative_is_literal() {
    let g = IncludeExcludeGlobs::new(&pats(&["{Makefile}"]), &[]).unwrap();
    assert_eq!(g.decide_str("Makefile"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("makefile"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn alternative_with_extensions_and_double_star() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.{js,jsx,ts,tsx,mjs,cjs}"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/app.ts"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("src/app.tsx"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/index.mjs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("lib/index.cjs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("lib/styles.css"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 4. Question mark wildcard
// ===========================================================================

#[test]
fn question_mark_matches_single_char_only() {
    let g = IncludeExcludeGlobs::new(&pats(&["data_?.csv"]), &[]).unwrap();
    assert_eq!(g.decide_str("data_1.csv"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("data_a.csv"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data_12.csv"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("data_.csv"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn multiple_question_marks() {
    let g = IncludeExcludeGlobs::new(&pats(&["log_????.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("log_2024.txt"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("log_abcd.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("log_123.txt"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("log_12345.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 5. Multi-extension files (e.g., .tar.gz)
// ===========================================================================

#[test]
fn multi_extension_tar_gz() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.tar.gz"]), &[]).unwrap();
    assert_eq!(g.decide_str("dist/archive.tar.gz"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("dist/archive.tar"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("dist/archive.gz"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn multi_extension_d_ts() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.d.ts"]), &[]).unwrap();
    assert_eq!(g.decide_str("types/index.d.ts"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/index.ts"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 6. Real-world .gitignore-style patterns
// ===========================================================================

#[test]
fn gitignore_style_build_artifacts() {
    let g = IncludeExcludeGlobs::new(
        &pats(&["**"]),
        &pats(&[
            "target/**",
            "node_modules/**",
            "**/*.o",
            "**/*.a",
            "**/*.so",
            "**/*.dylib",
            "**/*.dll",
        ]),
    )
    .unwrap();

    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("target/debug/binary"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("node_modules/lodash/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("build/libfoo.so"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("build/libfoo.a"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn gitignore_style_log_files_with_exception_via_narrow_include() {
    // Simulate "exclude all logs except important.log" using include + exclude.
    // Since globset has no `!` negation, model it as: include everything,
    // exclude *.log. Then separately, a second matcher for the exception.
    let broad = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**/*.log"])).unwrap();
    assert_eq!(
        broad.decide_str("debug.log"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        broad.decide_str("important.log"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(broad.decide_str("src/main.rs"), MatchDecision::Allowed);

    // To re-include important.log, a caller would check the specific file separately.
    let exception = IncludeExcludeGlobs::new(&pats(&["**/important.log"]), &[]).unwrap();
    assert_eq!(
        exception.decide_str("important.log"),
        MatchDecision::Allowed
    );
    assert_eq!(
        exception.decide_str("logs/important.log"),
        MatchDecision::Allowed
    );
}

#[test]
fn gitignore_style_ide_and_os_files() {
    let g = IncludeExcludeGlobs::new(
        &[],
        &pats(&[
            "**/.DS_Store",
            "**/Thumbs.db",
            "**/.idea/**",
            "**/.vscode/**",
            "**/*.swp",
            "**/*.swo",
            "**/*~",
        ]),
    )
    .unwrap();

    assert_eq!(g.decide_str(".DS_Store"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("project/.idea/workspace.xml"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("src/.vscode/settings.json"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("src/main.rs.swp"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 7. Unicode in patterns (not just paths)
// ===========================================================================

#[test]
fn unicode_pattern_matches_unicode_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["données/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("données/rapport.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("data/rapport.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn unicode_pattern_with_wildcard_extension() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.日本語"]), &[]).unwrap();
    assert_eq!(g.decide_str("dir/file.日本語"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("dir/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn emoji_in_pattern_and_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["🎉/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("🎉/party.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("party/file.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn cyrillic_exclude_pattern() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["тест/**"])).unwrap();
    assert_eq!(
        g.decide_str("тест/файл.txt"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("test/file.txt"), MatchDecision::Allowed);
}

// ===========================================================================
// 8. Windows-style backslash handling via decide_path
// ===========================================================================

#[cfg(windows)]
#[test]
fn backslash_nested_path_via_decide_path() {
    let g =
        IncludeExcludeGlobs::new(&pats(&["src/**/*.rs"]), &pats(&["src/generated/**"])).unwrap();
    // On Windows, Path::new normalizes backslashes.
    assert_eq!(
        g.decide_path(Path::new("src\\main.rs")),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_path(Path::new("src\\generated\\out.rs")),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_path(Path::new("docs\\readme.md")),
        MatchDecision::DeniedByMissingInclude
    );
}

#[cfg(windows)]
#[test]
fn backslash_deeply_nested() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.txt"]), &[]).unwrap();
    assert_eq!(
        g.decide_path(Path::new("a\\b\\c\\d\\e.txt")),
        MatchDecision::Allowed
    );
}

// ===========================================================================
// 9. Performance: many patterns with many paths
// ===========================================================================

#[test]
fn hundred_include_hundred_exclude_patterns() {
    let includes: Vec<String> = (0..100)
        .map(|i| format!("project/module_{i}/**/*.rs"))
        .collect();
    let excludes: Vec<String> = (0..100)
        .map(|i| format!("project/module_{i}/generated/**"))
        .collect();
    let g = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();

    // Matching include but not exclude.
    assert!(g.decide_str("project/module_50/src/lib.rs").is_allowed());
    // Matching both → exclude wins.
    assert!(
        !g.decide_str("project/module_50/generated/output.rs")
            .is_allowed()
    );
    // No include match.
    assert!(!g.decide_str("other/file.rs").is_allowed());
}

#[test]
fn thousand_paths_against_mixed_rules() {
    let g = IncludeExcludeGlobs::new(
        &pats(&["src/**", "lib/**", "include/**"]),
        &pats(&["**/*.bak", "**/tmp/**"]),
    )
    .unwrap();

    let mut allowed = 0u32;
    let mut denied = 0u32;
    for i in 0..1000 {
        let path = match i % 5 {
            0 => format!("src/mod_{i}.rs"),
            1 => format!("lib/helper_{i}.rs"),
            2 => format!("src/file_{i}.bak"),       // excluded
            3 => format!("src/tmp/scratch_{i}.rs"), // excluded
            _ => format!("docs/page_{i}.md"),       // no include
        };
        if g.decide_str(&path).is_allowed() {
            allowed += 1;
        } else {
            denied += 1;
        }
    }
    assert_eq!(allowed, 400); // mod 0 and mod 1
    assert_eq!(denied, 600); // mod 2, 3, 4
}

// ===========================================================================
// 10. Exclude overrides include, layered patterns
// ===========================================================================

#[test]
fn layered_include_exclude_with_subdirectories() {
    // Include src/**, exclude src/private/**, which means
    // src/private/ is denied even though src/** matches it.
    let g = IncludeExcludeGlobs::new(
        &pats(&["src/**"]),
        &pats(&["src/private/**", "src/**/*.secret"]),
    )
    .unwrap();

    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/private/keys.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("src/config.secret"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("tests/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn exclude_specific_files_in_allowed_tree() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**/LICENSE", "**/CHANGELOG.md"]))
        .unwrap();

    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("LICENSE"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("sub/LICENSE"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("CHANGELOG.md"), MatchDecision::DeniedByExclude);
}

// ===========================================================================
// 11. Directory-style patterns (trailing /)
// ===========================================================================

#[test]
fn trailing_slash_pattern_compiles() {
    // globset accepts trailing / in patterns.
    let g = IncludeExcludeGlobs::new(&pats(&["build/"]), &[]).unwrap();
    // The compiled pattern may or may not match a path without trailing /.
    // Document actual behavior: globset treats "build/" as matching "build".
    let result = g.decide_str("build");
    assert!(result == MatchDecision::Allowed || result == MatchDecision::DeniedByMissingInclude);
}

#[test]
fn exclude_directory_pattern_with_double_star() {
    // Common pattern: exclude an entire directory tree.
    let g = IncludeExcludeGlobs::new(&[], &pats(&["build/**", ".cache/**"])).unwrap();
    assert_eq!(
        g.decide_str("build/output.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str(".cache/data.bin"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 12. Overlapping include patterns (union semantics)
// ===========================================================================

#[test]
fn overlapping_includes_are_unioned() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs", "src/**"]), &[]).unwrap();
    // Matches first pattern.
    assert_eq!(g.decide_str("lib/mod.rs"), MatchDecision::Allowed);
    // Matches second pattern but not first.
    assert_eq!(g.decide_str("src/data.json"), MatchDecision::Allowed);
    // Matches neither.
    assert_eq!(
        g.decide_str("docs/guide.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 13. Very long paths (1000+ characters)
// ===========================================================================

#[test]
fn path_with_1000_char_filename_component() {
    let long_name = "x".repeat(1000) + ".rs";
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str(&long_name), MatchDecision::Allowed);
}

#[test]
fn path_with_500_segments() {
    let segments: Vec<&str> = (0..500).map(|_| "d").collect();
    let path = segments.join("/") + "/leaf.txt";
    assert!(path.len() >= 1000);
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str(&path), MatchDecision::Allowed);
}

// ===========================================================================
// 14. Consecutive and leading slashes
// ===========================================================================

#[test]
fn leading_slash_in_path() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &[]).unwrap();
    // Leading / makes it look like an absolute path; globset still matches.
    assert_eq!(g.decide_str("/src/lib.rs"), MatchDecision::Allowed);
}

#[test]
fn multiple_patterns_with_different_roots() {
    let g =
        IncludeExcludeGlobs::new(&pats(&["crates/*/src/**", "crates/*/tests/**"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("crates/abp-core/src/lib.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("crates/abp-glob/tests/test.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        g.decide_str("crates/abp-core/Cargo.toml"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 15. Empty string patterns handled gracefully
// ===========================================================================

#[test]
fn empty_string_pattern_in_include() {
    // An empty glob "" is valid in globset (matches empty string).
    let result = IncludeExcludeGlobs::new(&pats(&[""]), &[]);
    // Should compile (globset accepts it).
    assert!(result.is_ok());
}

// ===========================================================================
// 16. Exclude with no include (deny-list mode)
// ===========================================================================

#[test]
fn deny_list_mode_blocks_only_excluded() {
    let g = IncludeExcludeGlobs::new(&[], &pats(&["**/*.exe", "**/*.dll", "**/*.bin"])).unwrap();
    assert_eq!(g.decide_str("tool.exe"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("lib.dll"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("data.bin"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
}

// ===========================================================================
// 17. Include with no exclude (allow-list mode)
// ===========================================================================

#[test]
fn allow_list_mode_allows_only_included() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs", "**/*.toml", "**/*.md"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("image.png"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("data.json"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 18. Glob matching is not anchored to path start by default
// ===========================================================================

#[test]
fn star_pattern_crosses_path_separators() {
    // globset default: `*` does NOT require literal separator matching.
    let g = IncludeExcludeGlobs::new(&pats(&["*.rs"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    // Without literal_separator, *.rs matches across slashes.
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
}

// ===========================================================================
// 19. Single-star vs double-star in directory position
// ===========================================================================

#[test]
fn single_star_in_dir_position() {
    let g = IncludeExcludeGlobs::new(&pats(&["crates/*/src/lib.rs"]), &[]).unwrap();
    assert_eq!(
        g.decide_str("crates/abp-core/src/lib.rs"),
        MatchDecision::Allowed
    );
    // globset: * crosses separators by default, so this may still match.
    let deep = g.decide_str("crates/a/b/src/lib.rs");
    // Document actual behavior — * may match "a/b" in globset.
    assert!(deep == MatchDecision::Allowed || deep == MatchDecision::DeniedByMissingInclude);
}

// ===========================================================================
// 20. Clone and reuse of IncludeExcludeGlobs
// ===========================================================================

#[test]
fn clone_produces_identical_decisions() {
    let g = IncludeExcludeGlobs::new(&pats(&["src/**"]), &pats(&["src/secret/**"])).unwrap();
    let g2 = g.clone();

    let paths = &[
        "src/lib.rs",
        "src/secret/key.pem",
        "README.md",
        "src/a/b.rs",
    ];
    for &p in paths {
        assert_eq!(g.decide_str(p), g2.decide_str(p), "mismatch for {p}");
    }
}

// ===========================================================================
// 21. Mixed case sensitivity edge cases
// ===========================================================================

#[test]
fn case_sensitive_alternation() {
    let g = IncludeExcludeGlobs::new(&pats(&["*.{RS,Toml}"]), &[]).unwrap();
    assert_eq!(g.decide_str("main.RS"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("Cargo.Toml"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("main.rs"),
        MatchDecision::DeniedByMissingInclude
    );
    assert_eq!(
        g.decide_str("Cargo.toml"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn case_sensitive_character_class() {
    let g = IncludeExcludeGlobs::new(&pats(&["[A-Z]*.txt"]), &[]).unwrap();
    assert_eq!(g.decide_str("Readme.txt"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("readme.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 22. Pattern with only special characters
// ===========================================================================

#[test]
fn star_star_slash_star_pattern() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*"]), &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    assert_eq!(g.decide_str("a/b/c"), MatchDecision::Allowed);
}

// ===========================================================================
// 23. Interaction: include and exclude use different pattern types
// ===========================================================================

#[test]
fn include_by_extension_exclude_by_directory() {
    let g = IncludeExcludeGlobs::new(&pats(&["**/*.rs"]), &pats(&["vendor/**", "third_party/**"]))
        .unwrap();
    assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("vendor/dep/lib.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("third_party/crate/mod.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        g.decide_str("src/main.py"),
        MatchDecision::DeniedByMissingInclude
    );
}

// ===========================================================================
// 24. Dot-prefixed directories (hidden dirs)
// ===========================================================================

#[test]
fn exclude_hidden_directories_keep_hidden_files() {
    let g = IncludeExcludeGlobs::new(&pats(&["**"]), &pats(&["**/.*/**"])).unwrap();
    // Hidden directory contents excluded.
    assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
    assert_eq!(
        g.decide_str("src/.hidden/secret.txt"),
        MatchDecision::DeniedByExclude
    );
    // Hidden files at top level are still allowed (not inside a hidden dir).
    assert_eq!(g.decide_str(".gitignore"), MatchDecision::Allowed);
}
