// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-glob` — ensures invariants hold across random inputs.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use proptest::prelude::*;

/// Strategy: safe path segments (alphanumeric, underscore, hyphen).
fn path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,7}".prop_map(|s| s.to_string())
}

/// Strategy: relative path with optional extension (1–5 segments).
fn relative_path() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(path_segment(), 1..=5),
        prop::option::of("[a-z]{1,4}"),
    )
        .prop_map(|(segs, ext)| {
            let joined = segs.join("/");
            match ext {
                Some(e) => format!("{joined}.{e}"),
                None => joined,
            }
        })
}

/// Strategy: a simple valid glob pattern (safe subset).
fn simple_glob_pattern() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("**".to_string()),
        Just("**/*".to_string()),
        Just("*".to_string()),
        path_segment().prop_map(|s| format!("{s}/**")),
        prop_oneof![Just("rs"), Just("txt"), Just("md"), Just("json")]
            .prop_map(|ext| format!("**/*.{ext}")),
    ]
}

// ── 1. Decide never panics on arbitrary paths ───────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn decide_never_panics(
        path in relative_path(),
        use_include in any::<bool>(),
        use_exclude in any::<bool>(),
    ) {
        let inc = if use_include { vec!["**".to_string()] } else { vec![] };
        let exc = if use_exclude { vec!["**".to_string()] } else { vec![] };
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();

        // Must not panic; result must be a valid variant.
        let result = globs.decide_str(&path);
        prop_assert!(
            result == MatchDecision::Allowed
                || result == MatchDecision::DeniedByExclude
                || result == MatchDecision::DeniedByMissingInclude
        );
    }
}

// ── 2. Include-only: matching path never returns DeniedByExclude ────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn include_only_never_returns_denied_by_exclude(path in relative_path()) {
        // No exclude patterns at all.
        let globs = IncludeExcludeGlobs::new(
            &["**".to_string()],
            &[],
        ).unwrap();
        let result = globs.decide_str(&path);
        prop_assert_ne!(
            result,
            MatchDecision::DeniedByExclude,
            "include-only matcher must never return DeniedByExclude"
        );
    }
}

// ── 3. Exclude-only: never returns DeniedByMissingInclude ───────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn exclude_only_never_returns_denied_by_missing_include(path in relative_path()) {
        // No include patterns → include constraint is absent.
        let globs = IncludeExcludeGlobs::new(
            &[],
            &["**".to_string()],
        ).unwrap();
        let result = globs.decide_str(&path);
        prop_assert_ne!(
            result,
            MatchDecision::DeniedByMissingInclude,
            "exclude-only matcher must never return DeniedByMissingInclude"
        );
    }
}

// ── 4. Empty patterns always yield Allowed ──────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn empty_patterns_always_allowed(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(
            globs.decide_str(&path),
            MatchDecision::Allowed,
            "no patterns means no constraints"
        );
    }
}

// ── 5. Include-only with non-matching pattern → DeniedByMissingInclude

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    #[test]
    fn include_only_non_matching_is_denied_by_missing(path in relative_path()) {
        // Include only paths under "zzz_nonexistent_prefix/".
        // Our generated paths never start with "zzz_nonexistent_prefix".
        let globs = IncludeExcludeGlobs::new(
            &["zzz_nonexistent_prefix/**".to_string()],
            &[],
        ).unwrap();
        prop_assert_eq!(
            globs.decide_str(&path),
            MatchDecision::DeniedByMissingInclude,
        );
    }
}

// ── 6. Exclude always takes precedence over include ─────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn exclude_always_overrides_include(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(
            &["**".to_string()],
            &["**".to_string()],
        ).unwrap();
        prop_assert_eq!(
            globs.decide_str(&path),
            MatchDecision::DeniedByExclude,
            "exclude must override include"
        );
    }
}

// ── 7. decide_str and decide_path agree for arbitrary paths ─────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn decide_str_and_decide_path_agree(
        path in relative_path(),
        inc_pattern in simple_glob_pattern(),
        exc_pattern in simple_glob_pattern(),
        use_inc in any::<bool>(),
        use_exc in any::<bool>(),
    ) {
        let inc = if use_inc { vec![inc_pattern] } else { vec![] };
        let exc = if use_exc { vec![exc_pattern] } else { vec![] };
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();

        let via_str = globs.decide_str(&path);
        let via_path = globs.decide_path(std::path::Path::new(&path));
        prop_assert_eq!(via_str, via_path);
    }
}

// ── 8. No-exclude matcher: result is either Allowed or DeniedByMissingInclude

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    #[test]
    fn no_exclude_can_only_allow_or_deny_missing(
        path in relative_path(),
        inc_pattern in simple_glob_pattern(),
    ) {
        let globs = IncludeExcludeGlobs::new(&[inc_pattern], &[]).unwrap();
        let result = globs.decide_str(&path);
        prop_assert!(
            result == MatchDecision::Allowed
                || result == MatchDecision::DeniedByMissingInclude,
            "without excludes, DeniedByExclude should never appear"
        );
    }
}

// ── 9. No-include matcher: result is either Allowed or DeniedByExclude

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    #[test]
    fn no_include_can_only_allow_or_deny_exclude(
        path in relative_path(),
        exc_pattern in simple_glob_pattern(),
    ) {
        let globs = IncludeExcludeGlobs::new(&[], &[exc_pattern]).unwrap();
        let result = globs.decide_str(&path);
        prop_assert!(
            result == MatchDecision::Allowed
                || result == MatchDecision::DeniedByExclude,
            "without includes, DeniedByMissingInclude should never appear"
        );
    }
}
