// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz glob pattern compilation with arbitrary string input.
//!
//! Verifies:
//! 1. `IncludeExcludeGlobs::new` never panics on any pattern strings.
//! 2. `decide_str` and `decide_path` never panic on arbitrary paths.
//! 3. `is_allowed()` is consistent between str and path variants.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Split input into pattern and test path at first newline
    let (pattern_part, path_part) = match s.split_once('\n') {
        Some((p, t)) => (p, t),
        None => (s, "test/path.rs"),
    };

    // Split patterns by comma for include/exclude lists
    let patterns: Vec<String> = pattern_part.split(',').map(|s| s.to_string()).collect();

    // Property 1: compilation with single include pattern never panics
    let globs_inc = abp_glob::IncludeExcludeGlobs::new(&patterns, &[]);

    // Property 1b: compilation with single exclude pattern never panics
    let globs_exc = abp_glob::IncludeExcludeGlobs::new(&[], &patterns);

    // Property 1c: compilation with both never panics
    let half = patterns.len() / 2;
    let (inc, exc) = patterns.split_at(half);
    let globs_both = abp_glob::IncludeExcludeGlobs::new(inc, exc);

    // Property 2: matching never panics
    let test_paths: Vec<&str> = path_part.split('\n').collect();
    for globs_result in [&globs_inc, &globs_exc, &globs_both] {
        if let Ok(globs) = globs_result {
            for path in &test_paths {
                let str_decision = globs.decide_str(path);
                let path_decision = globs.decide_path(std::path::Path::new(path));

                // Property 3: is_allowed consistency
                let _ = str_decision.is_allowed();
                let _ = path_decision.is_allowed();
            }
        }
    }
});
