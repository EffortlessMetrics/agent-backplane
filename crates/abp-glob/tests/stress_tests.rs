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
// 1. Compile 100 include patterns — should not panic or be extremely slow
// ---------------------------------------------------------------------------

#[test]
fn compile_100_include_patterns() {
    let includes: Vec<String> = (0..100).map(|i| format!("src/module_{i}/**/*.rs")).collect();
    let globs = IncludeExcludeGlobs::new(&includes, &[]).expect("compile 100 patterns");

    // A path matching one of the patterns should be allowed.
    assert!(globs.decide_str("src/module_42/lib.rs").is_allowed());
    // A path matching none should be denied (missing include).
    assert!(!globs.decide_str("other/file.txt").is_allowed());
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
    let includes = vec![
        s("**/src/**/tests/**/*.rs"),
        s("**/benches/**/*.rs"),
    ];
    let excludes = vec![s("**/target/**")];
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).expect("compile");

    // Deep nesting that matches.
    assert!(globs
        .decide_str("workspace/crate_a/src/module/tests/integration/test_foo.rs")
        .is_allowed());
    assert!(globs
        .decide_str("benches/criterion/bench_main.rs")
        .is_allowed());

    // Deep nesting that does NOT match includes.
    assert!(!globs.decide_str("workspace/crate_a/src/module/lib.rs").is_allowed());

    // Matches include but also matches exclude.
    assert!(!globs
        .decide_str("target/debug/build/src/tests/gen.rs")
        .is_allowed());
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
