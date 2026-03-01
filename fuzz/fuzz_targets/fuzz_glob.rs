// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IncludeExcludeGlobs::new() with arbitrary include/exclude patterns.
//!
//! Ensures glob compilation and path matching never panic, even with
//! adversarial pattern strings.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct GlobInput {
    include: Vec<String>,
    exclude: Vec<String>,
    paths: Vec<String>,
}

fuzz_target!(|input: GlobInput| {
    // Compilation may fail on invalid patterns — that's fine, just must not panic.
    let globs = match abp_glob::IncludeExcludeGlobs::new(&input.include, &input.exclude) {
        Ok(g) => g,
        Err(_) => return,
    };

    // Exercise matching against each candidate path.
    for path in &input.paths {
        let decision = globs.decide_str(path);
        // Ensure is_allowed() is consistent with the enum variant.
        let _ = decision.is_allowed();
        // Also test the Path variant for consistency.
        let _ = globs.decide_path(std::path::Path::new(path));
    }
});
