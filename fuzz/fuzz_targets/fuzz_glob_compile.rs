// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz glob pattern compilation with arbitrary strings.
//!
//! Tests that `IncludeExcludeGlobs::new()` and `build_globset()` never panic
//! on any input pattern. Exercises single-pattern and many-pattern compilation,
//! path matching against compiled globs, and verifies consistency between
//! `decide_str` and `decide_path`.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct GlobFuzzInput {
    /// Individual patterns to test with build_globset.
    single_patterns: Vec<String>,
    /// Include patterns for IncludeExcludeGlobs.
    include: Vec<String>,
    /// Exclude patterns for IncludeExcludeGlobs.
    exclude: Vec<String>,
    /// Paths to match against compiled globs.
    test_paths: Vec<String>,
}

fuzz_target!(|input: GlobFuzzInput| {
    // --- build_globset with individual patterns ---
    // Must never panic; errors on invalid patterns are fine.
    let _ = abp_glob::build_globset(&input.single_patterns);

    // Also test with include and exclude separately.
    let _ = abp_glob::build_globset(&input.include);
    let _ = abp_glob::build_globset(&input.exclude);

    // Test empty input.
    let empty: Vec<String> = vec![];
    let _ = abp_glob::build_globset(&empty);

    // --- IncludeExcludeGlobs compilation ---
    let globs = match abp_glob::IncludeExcludeGlobs::new(&input.include, &input.exclude) {
        Ok(g) => g,
        Err(_) => return,
    };

    // --- Path matching ---
    for path in &input.test_paths {
        let str_decision = globs.decide_str(path);
        let path_decision = globs.decide_path(std::path::Path::new(path));

        // Both methods must agree on the allow/deny outcome.
        assert_eq!(
            str_decision.is_allowed(),
            path_decision.is_allowed(),
            "decide_str and decide_path must agree for path: {path:?}"
        );
    }

    // --- Edge cases: empty patterns with non-empty paths ---
    if let Ok(empty_globs) = abp_glob::IncludeExcludeGlobs::new(&empty, &empty) {
        for path in &input.test_paths {
            // With no include/exclude patterns, all paths should be allowed.
            let d = empty_globs.decide_str(path);
            assert!(d.is_allowed(), "empty globs must allow all paths");
        }
    }

    // --- Single-pattern include, no exclude ---
    for pat in &input.single_patterns {
        if let Ok(g) = abp_glob::IncludeExcludeGlobs::new(&[pat.clone()], &empty) {
            for path in &input.test_paths {
                let _ = g.decide_str(path);
            }
        }
    }
});
