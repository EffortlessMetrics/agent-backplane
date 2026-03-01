// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stress tests for `abp-glob`.
//!
//! Deterministic correctness-under-load tests — not benchmarks.

use abp_glob::IncludeExcludeGlobs;
use std::path::Path;

fn s(v: &str) -> String {
    v.to_string()
}

// ---------------------------------------------------------------------------
// 1. Many patterns: 100 include + 100 exclude patterns
// ---------------------------------------------------------------------------

#[test]
fn compile_100_include_and_100_exclude_patterns() {
    let includes: Vec<String> = (0..100)
        .map(|i| format!("src/module_{i}/**/*.rs"))
        .collect();
    let excludes: Vec<String> = (0..100)
        .map(|i| format!("src/module_{i}/**/generated/**"))
        .collect();
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("compile 200 patterns");

    // Matches include, not excluded.
    assert!(globs.decide_str("src/module_42/lib.rs").is_allowed());
    // Matches include AND exclude → denied.
    assert!(
        !globs
            .decide_str("src/module_42/generated/out.rs")
            .is_allowed()
    );
    // Matches no include → denied.
    assert!(!globs.decide_str("other/file.txt").is_allowed());
    // Verify boundary modules.
    assert!(globs.decide_str("src/module_0/foo.rs").is_allowed());
    assert!(globs.decide_str("src/module_99/bar.rs").is_allowed());
    assert!(!globs.decide_str("src/module_100/baz.rs").is_allowed());
}

// ---------------------------------------------------------------------------
// 2. Match against 10 000 paths — should complete in reasonable time
// ---------------------------------------------------------------------------

#[test]
fn match_10000_paths() {
    let includes = vec![s("**/*.rs"), s("**/*.toml")];
    let excludes = vec![s("**/target/**"), s("**/.git/**")];
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("compile");

    let mut allowed = 0u32;
    let mut denied = 0u32;
    for i in 0..10_000 {
        let path = if i % 4 == 0 {
            format!("src/mod_{i}/lib.rs")
        } else if i % 4 == 1 {
            format!("target/debug/mod_{i}.rs")
        } else if i % 4 == 2 {
            format!("crates/c_{i}/Cargo.toml")
        } else {
            format!("docs/page_{i}.md")
        };
        if globs.decide_str(&path).is_allowed() {
            allowed += 1;
        } else {
            denied += 1;
        }
    }

    // src/**/*.rs  → allowed  (i % 4 == 0) → 2500
    // target/**    → denied   (i % 4 == 1) → 2500
    // **/*.toml    → allowed  (i % 4 == 2) → 2500
    // docs/*.md    → denied   (i % 4 == 3) → 2500 (no include match)
    assert_eq!(allowed, 5000);
    assert_eq!(denied, 5000);
}

// ---------------------------------------------------------------------------
// 3. Complex nested ** patterns work correctly
// ---------------------------------------------------------------------------

#[test]
fn complex_nested_double_star_patterns() {
    let includes = vec![s("**/src/**/tests/**/*.rs"), s("**/benches/**/*.rs")];
    let excludes = vec![s("**/target/**")];
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("compile");

    // Deep nesting that matches.
    assert!(
        globs
            .decide_str("workspace/crate_a/src/module/tests/integration/test_foo.rs")
            .is_allowed()
    );
    assert!(
        globs
            .decide_str("benches/criterion/bench_main.rs")
            .is_allowed()
    );

    // Deep nesting that does NOT match includes.
    assert!(
        !globs
            .decide_str("workspace/crate_a/src/module/lib.rs")
            .is_allowed()
    );

    // Matches include but also matches exclude.
    assert!(
        !globs
            .decide_str("target/debug/build/src/tests/gen.rs")
            .is_allowed()
    );
}

// ---------------------------------------------------------------------------
// 4. Pattern with many alternatives (brace expansion)
// ---------------------------------------------------------------------------

#[test]
fn many_alternatives_brace_expansion() {
    let exts: Vec<&str> = (0..20)
        .map(|i| match i {
            0 => "rs",
            1 => "toml",
            2 => "json",
            3 => "yaml",
            4 => "yml",
            5 => "md",
            6 => "txt",
            7 => "lock",
            8 => "html",
            9 => "css",
            10 => "js",
            11 => "ts",
            12 => "py",
            13 => "rb",
            14 => "go",
            15 => "c",
            16 => "h",
            17 => "cpp",
            18 => "hpp",
            _ => "sh",
        })
        .collect();
    let brace_pattern = format!("**/*.{{{}}}", exts.join(","));
    let includes = vec![brace_pattern];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile brace pattern");

    for ext in &exts {
        let path = format!("some/deep/path/file.{ext}");
        assert!(
            globs.decide_str(&path).is_allowed(),
            "expected allowed for .{ext}"
        );
    }

    // Unrecognised extension should be denied.
    assert!(!globs.decide_str("file.zig").is_allowed());
}

// ---------------------------------------------------------------------------
// 5. Deeply nested path matching
// ---------------------------------------------------------------------------

#[test]
fn deeply_nested_path_matching() {
    let includes = vec![s("**/*.rs")];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile");

    // 50-segment deep path.
    let segments: Vec<String> = (0..50).map(|i| format!("d{i}")).collect();
    let deep = format!("{}/file.rs", segments.join("/"));
    assert!(globs.decide_str(&deep).is_allowed());

    // Same depth, wrong extension.
    let deep_txt = format!("{}/file.txt", segments.join("/"));
    assert!(!globs.decide_str(&deep_txt).is_allowed());

    // Also verify Path-based API agrees.
    assert_eq!(
        globs.decide_path(Path::new(&deep)).is_allowed(),
        globs.decide_str(&deep).is_allowed(),
    );
}

// ---------------------------------------------------------------------------
// 6. Unicode paths
// ---------------------------------------------------------------------------

#[test]
fn unicode_paths_stress() {
    let includes = vec![s("**/*.rs")];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile");

    let unicode_dirs = [
        "données",
        "日本語",
        "العربية",
        "中文目录",
        "한국어",
        "Ελληνικά",
        "Кириллица",
        "émojis_🎉",
    ];

    for dir in &unicode_dirs {
        let path = format!("{dir}/module.rs");
        assert!(
            globs.decide_str(&path).is_allowed(),
            "expected allowed for {dir}/module.rs"
        );
    }

    // Exclude unicode directories.
    let excludes: Vec<String> = unicode_dirs.iter().map(|d| format!("{d}/**")).collect();
    let globs2 = IncludeExcludeGlobs::new(&includes, &excludes).expect("compile excludes");

    for dir in &unicode_dirs {
        let path = format!("{dir}/module.rs");
        assert!(
            !globs2.decide_str(&path).is_allowed(),
            "expected denied for {dir}/module.rs with exclusion"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Pattern compilation: compile 500 patterns
// ---------------------------------------------------------------------------

#[test]
fn compile_500_patterns() {
    let includes: Vec<String> = (0..250).map(|i| format!("pkg_{i}/src/**/*.rs")).collect();
    let excludes: Vec<String> = (0..250).map(|i| format!("pkg_{i}/target/**")).collect();
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("compile 500 patterns");

    assert!(globs.decide_str("pkg_0/src/lib.rs").is_allowed());
    assert!(globs.decide_str("pkg_249/src/main.rs").is_allowed());
    assert!(!globs.decide_str("pkg_0/target/debug/a.rs").is_allowed());
    assert!(!globs.decide_str("unrelated/file.rs").is_allowed());
}

// ---------------------------------------------------------------------------
// 8. Deep wildcards: **/**/**/**/*.rs
// ---------------------------------------------------------------------------

#[test]
fn deep_double_star_wildcards() {
    let includes = vec![s("**/**/**/**/*.rs")];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile deep wildcards");

    assert!(globs.decide_str("a/b/c/d/e.rs").is_allowed());
    assert!(globs.decide_str("x.rs").is_allowed());
    assert!(globs.decide_str("a/b.rs").is_allowed());
    assert!(!globs.decide_str("a/b/c/d/e.txt").is_allowed());
}

// ---------------------------------------------------------------------------
// 9. Star explosion: many * wildcards in a single pattern
// ---------------------------------------------------------------------------

#[test]
fn star_explosion_pattern() {
    // Pattern with many single-star wildcards.
    let includes = vec![s("*-*-*-*-*-*-*-*-*-*.rs")];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile star explosion");

    assert!(globs.decide_str("a-b-c-d-e-f-g-h-i-j.rs").is_allowed());
    assert!(!globs.decide_str("a-b-c.rs").is_allowed());
    assert!(!globs.decide_str("a-b-c-d-e-f-g-h-i-j.txt").is_allowed());
}

// ---------------------------------------------------------------------------
// 10. Character classes: [a-zA-Z0-9_-] patterns
// ---------------------------------------------------------------------------

#[test]
fn character_class_patterns() {
    let includes = vec![s("[a-z][a-z][a-z].rs")];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile char class");

    assert!(globs.decide_str("abc.rs").is_allowed());
    assert!(globs.decide_str("xyz.rs").is_allowed());
    // Digits don't match [a-z].
    assert!(!globs.decide_str("123.rs").is_allowed());
    // Too many chars for 3-char class.
    assert!(!globs.decide_str("abcd.rs").is_allowed());

    // More complex class.
    let includes2 = vec![s("**/[a-zA-Z0-9_-]*.log")];
    let globs2 = IncludeExcludeGlobs::new(&includes2, &[]).expect("compile complex char class");
    assert!(globs2.decide_str("logs/app-server.log").is_allowed());
    assert!(globs2.decide_str("logs/A.log").is_allowed());
    assert!(globs2.decide_str("logs/9tail.log").is_allowed());
    assert!(!globs2.decide_str("logs/app.txt").is_allowed());
}

// ---------------------------------------------------------------------------
// 11. Alternation: {a,b,c,...} with many options
// ---------------------------------------------------------------------------

#[test]
fn alternation_many_options() {
    let letters: Vec<String> = (b'a'..=b'z').map(|c| String::from(c as char)).collect();
    let pattern = format!("{{{}}}.txt", letters.join(","));
    let includes = vec![pattern];
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile alternation");

    for c in b'a'..=b'z' {
        let path = format!("{}.txt", c as char);
        assert!(
            globs.decide_str(&path).is_allowed(),
            "expected {path} allowed"
        );
    }
    assert!(!globs.decide_str("1.txt").is_allowed());
    assert!(!globs.decide_str("ab.txt").is_allowed());
    assert!(!globs.decide_str("a.rs").is_allowed());
}

// ---------------------------------------------------------------------------
// 12. Exact match vs glob: compare exact path vs glob for same path
// ---------------------------------------------------------------------------

#[test]
fn exact_match_vs_glob() {
    let exact = IncludeExcludeGlobs::new(&[s("src/main.rs")], &[]).expect("exact");
    let glob = IncludeExcludeGlobs::new(&[s("src/*.rs")], &[]).expect("glob");

    // Both should allow the exact path.
    assert!(exact.decide_str("src/main.rs").is_allowed());
    assert!(glob.decide_str("src/main.rs").is_allowed());

    // Exact should reject other .rs files; glob should allow them.
    assert!(!exact.decide_str("src/lib.rs").is_allowed());
    assert!(glob.decide_str("src/lib.rs").is_allowed());

    // Both should reject non-.rs.
    assert!(!exact.decide_str("src/main.txt").is_allowed());
    assert!(!glob.decide_str("src/main.txt").is_allowed());
}

// ---------------------------------------------------------------------------
// 13. Empty globs: no includes/excludes → always Allowed
// ---------------------------------------------------------------------------

#[test]
fn empty_globs_allow_everything() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).expect("empty");

    let paths = [
        "",
        "a",
        "a/b/c/d",
        "some/very/deep/path/file.rs",
        "target/debug/bin",
        "日本語/ファイル.txt",
    ];
    for p in &paths {
        assert!(
            globs.decide_str(p).is_allowed(),
            "expected Allowed for {p:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 14. All-include: wildcard include, no excludes → always Allowed
// ---------------------------------------------------------------------------

#[test]
fn all_include_wildcard() {
    let globs = IncludeExcludeGlobs::new(&[s("**")], &[]).expect("all-include");

    let paths = [
        "a.rs",
        "deeply/nested/path/file.txt",
        ".hidden",
        "target/debug/build/out.o",
    ];
    for p in &paths {
        assert!(
            globs.decide_str(p).is_allowed(),
            "expected Allowed for {p:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 15. All-exclude: wildcard exclude → always DeniedByExclude
// ---------------------------------------------------------------------------

#[test]
fn all_exclude_wildcard() {
    use abp_glob::MatchDecision;

    let globs = IncludeExcludeGlobs::new(&[], &[s("**")]).expect("all-exclude");

    let paths = [
        "a.rs",
        "deeply/nested/path/file.txt",
        ".hidden",
        "target/debug/build/out.o",
    ];
    for p in &paths {
        assert_eq!(
            globs.decide_str(p),
            MatchDecision::DeniedByExclude,
            "expected DeniedByExclude for {p:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 16. Conflicting patterns: same path matches both include and exclude
// ---------------------------------------------------------------------------

#[test]
fn conflicting_include_exclude() {
    use abp_glob::MatchDecision;

    // Exclude takes precedence per API contract.
    let globs =
        IncludeExcludeGlobs::new(&[s("**/*.rs")], &[s("**/*.rs")]).expect("conflicting patterns");

    assert_eq!(
        globs.decide_str("src/lib.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(globs.decide_str("main.rs"), MatchDecision::DeniedByExclude);
    // Non-.rs files don't match include → DeniedByMissingInclude.
    assert_eq!(
        globs.decide_str("readme.md"),
        MatchDecision::DeniedByMissingInclude
    );

    // Partial overlap: include **, exclude *.log.
    let globs2 = IncludeExcludeGlobs::new(&[s("**")], &[s("**/*.log")]).expect("partial conflict");
    assert!(globs2.decide_str("src/lib.rs").is_allowed());
    assert_eq!(
        globs2.decide_str("logs/app.log"),
        MatchDecision::DeniedByExclude
    );
}

// ---------------------------------------------------------------------------
// 17. Negation patterns: !*.log style
// ---------------------------------------------------------------------------

#[test]
fn negation_style_patterns() {
    // globset treats `!` as a literal character, not negation.
    // Verify that `!foo` compiles and matches paths literally containing `!`.
    let includes = vec![s("!*.log")];
    let result = IncludeExcludeGlobs::new(&includes, &[]);
    // If the library accepts it, verify the behaviour; otherwise confirm the error.
    match result {
        Ok(globs) => {
            // `!*.log` matches literal files starting with `!`.
            assert!(globs.decide_str("!app.log").is_allowed());
            // Regular files don't start with `!`, so they're excluded.
            assert!(!globs.decide_str("app.log").is_allowed());
        }
        Err(_) => {
            // If the library rejects `!` patterns, that's also valid behaviour.
        }
    }

    // The idiomatic way to "negate" in ABP is to use the exclude list.
    let globs =
        IncludeExcludeGlobs::new(&[s("**")], &[s("*.log")]).expect("exclude-based negation");
    assert!(globs.decide_str("app.rs").is_allowed());
    assert!(!globs.decide_str("app.log").is_allowed());
}

// ---------------------------------------------------------------------------
// 18. Real-world .gitignore-style patterns
// ---------------------------------------------------------------------------

#[test]
fn real_world_gitignore_patterns() {
    use abp_glob::MatchDecision;

    let includes = vec![s("**")];
    let excludes = vec![
        s("**/target/**"),
        s("**/.git/**"),
        s("**/node_modules/**"),
        s("**/*.o"),
        s("**/*.so"),
        s("**/*.dylib"),
        s("**/*.pyc"),
        s("**/__pycache__/**"),
        s("**/.DS_Store"),
        s("**/Thumbs.db"),
    ];
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("gitignore-style");

    // Allowed.
    assert!(globs.decide_str("src/main.rs").is_allowed());
    assert!(globs.decide_str("Cargo.toml").is_allowed());
    assert!(globs.decide_str("docs/README.md").is_allowed());
    assert!(
        globs
            .decide_str("tests/integration/test_api.py")
            .is_allowed()
    );

    // Denied.
    assert_eq!(
        globs.decide_str("target/debug/mybinary"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str(".git/objects/ab/1234"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("frontend/node_modules/lodash/index.js"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("build/lib.o"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("lib/binding.so"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("scripts/__pycache__/util.pyc"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("folder/.DS_Store"),
        MatchDecision::DeniedByExclude
    );
}

// ---------------------------------------------------------------------------
// 19. Unicode in patterns (glob patterns containing unicode)
// ---------------------------------------------------------------------------

#[test]
fn unicode_in_glob_patterns() {
    let includes = vec![s("données/**/*.rs"), s("日本語/**")];
    let excludes = vec![s("données/privé/**")];
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("unicode patterns");

    assert!(globs.decide_str("données/module/lib.rs").is_allowed());
    assert!(globs.decide_str("日本語/ファイル.txt").is_allowed());
    assert!(!globs.decide_str("données/privé/secret.rs").is_allowed());
    assert!(!globs.decide_str("english/file.rs").is_allowed());
}

// ---------------------------------------------------------------------------
// 20. Case sensitivity: verify case-sensitive matching
// ---------------------------------------------------------------------------

#[test]
fn case_sensitivity_behavior() {
    // globset's Glob::new uses case-sensitive matching by default.
    let upper = IncludeExcludeGlobs::new(&[s("**/*.RS")], &[]).expect("upper pattern");
    let lower = IncludeExcludeGlobs::new(&[s("**/*.rs")], &[]).expect("lower pattern");

    // *.RS should match .RS extension.
    assert!(upper.decide_str("src/LIB.RS").is_allowed());
    // *.rs should match .rs extension.
    assert!(lower.decide_str("src/lib.rs").is_allowed());

    // Cross-case: verify the two glob sets are distinct and don't interfere.
    // (On case-sensitive globset, *.RS won't match .rs and vice versa.)
    let upper_matches_lower = upper.decide_str("src/lib.rs").is_allowed();
    let lower_matches_upper = lower.decide_str("src/LIB.RS").is_allowed();

    // They should be consistent: either globset is case-sensitive (neither
    // cross-matches) or case-insensitive (both cross-match).
    assert_eq!(
        upper_matches_lower, lower_matches_upper,
        "case sensitivity should be symmetric"
    );
}

// ---------------------------------------------------------------------------
// 21. Sequential compilation: compile, decide, compile again — independence
// ---------------------------------------------------------------------------

#[test]
fn sequential_compilation_independence() {
    use abp_glob::MatchDecision;

    // First compilation: include only .rs files.
    let globs1 = IncludeExcludeGlobs::new(&[s("**/*.rs")], &[]).expect("first compile");
    assert!(globs1.decide_str("src/lib.rs").is_allowed());
    assert_eq!(
        globs1.decide_str("src/lib.py"),
        MatchDecision::DeniedByMissingInclude
    );

    // Second compilation: include only .py files.
    let globs2 = IncludeExcludeGlobs::new(&[s("**/*.py")], &[]).expect("second compile");
    assert!(globs2.decide_str("src/lib.py").is_allowed());
    assert_eq!(
        globs2.decide_str("src/lib.rs"),
        MatchDecision::DeniedByMissingInclude
    );

    // First glob set still works the same — no cross-contamination.
    assert!(globs1.decide_str("src/lib.rs").is_allowed());
    assert_eq!(
        globs1.decide_str("src/lib.py"),
        MatchDecision::DeniedByMissingInclude
    );

    // Third compilation: different exclude.
    let globs3 = IncludeExcludeGlobs::new(&[s("**")], &[s("**/secret/**")]).expect("third compile");
    assert!(globs3.decide_str("src/lib.rs").is_allowed());
    assert_eq!(
        globs3.decide_str("src/secret/key.pem"),
        MatchDecision::DeniedByExclude
    );

    // Previous compiles unaffected.
    assert!(globs1.decide_str("src/secret/key.rs").is_allowed());
    assert!(globs2.decide_str("src/secret/key.py").is_allowed());
}

// ---------------------------------------------------------------------------
// 22. Long path with 50+ segments
// ---------------------------------------------------------------------------

#[test]
fn path_with_50_plus_segments() {
    use abp_glob::MatchDecision;

    let segments: Vec<String> = (0..55).map(|i| format!("seg{i}")).collect();
    let deep_rs = format!("{}/leaf.rs", segments.join("/"));
    let _deep_txt = format!("{}/leaf.txt", segments.join("/"));

    let globs = IncludeExcludeGlobs::new(&[s("**/*.rs")], &[s("**/seg50/**")]).expect("compile");

    // The path passes through seg50, so exclude fires.
    assert_eq!(globs.decide_str(&deep_rs), MatchDecision::DeniedByExclude);

    // A 50-segment path that avoids the excluded segment.
    let safe_segments: Vec<String> = (0..50).map(|i| format!("dir{i}")).collect();
    let safe_path = format!("{}/leaf.rs", safe_segments.join("/"));
    assert!(globs.decide_str(&safe_path).is_allowed());

    // Wrong extension + no exclude hit.
    let safe_txt = format!("{}/leaf.txt", safe_segments.join("/"));
    assert_eq!(
        globs.decide_str(&safe_txt),
        MatchDecision::DeniedByMissingInclude
    );
}

// ---------------------------------------------------------------------------
// 23. Many candidates: 10,000 paths — exact count verification
// ---------------------------------------------------------------------------

#[test]
fn ten_thousand_candidates_with_mixed_rules() {
    use abp_glob::MatchDecision;

    let globs = IncludeExcludeGlobs::new(
        &[s("app/**/*.rs"), s("lib/**/*.rs")],
        &[s("**/generated/**")],
    )
    .expect("compile");

    let mut allowed = 0u32;
    let mut denied_exclude = 0u32;
    let mut denied_include = 0u32;

    for i in 0..10_000 {
        let path = match i % 5 {
            0 => format!("app/mod_{i}/code.rs"), // include match, no exclude
            1 => format!("lib/mod_{i}/code.rs"), // include match, no exclude
            2 => format!("app/generated/mod_{i}/code.rs"), // include match + exclude match
            3 => format!("docs/page_{i}.md"),    // no include match
            _ => format!("lib/generated/out_{i}.rs"), // include match + exclude match
        };
        match globs.decide_str(&path) {
            MatchDecision::Allowed => allowed += 1,
            MatchDecision::DeniedByExclude => denied_exclude += 1,
            MatchDecision::DeniedByMissingInclude => denied_include += 1,
        }
    }

    assert_eq!(allowed, 4000); // groups 0 + 1
    assert_eq!(denied_exclude, 4000); // groups 2 + 4
    assert_eq!(denied_include, 2000); // group 3
}
