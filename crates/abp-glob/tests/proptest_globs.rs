// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive property-based tests for `abp-glob`.
//!
//! Covers invariants around include/exclude semantics, pattern ordering,
//! idempotency, unicode paths, deeply nested paths, and error handling.

use abp_glob::{IncludeExcludeGlobs, MatchDecision, build_globset};
use proptest::prelude::*;

// ═══════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════

/// Safe ASCII path segment: starts with a letter, followed by alphanumerics/underscore/hyphen.
fn path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,7}"
}

/// File extension: 1–4 lowercase letters.
fn file_extension() -> impl Strategy<Value = String> {
    "[a-z]{1,4}"
}

/// Relative path with 1–5 segments and an optional extension.
fn relative_path() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(path_segment(), 1..=5),
        prop::option::of(file_extension()),
    )
        .prop_map(|(segs, ext)| {
            let joined = segs.join("/");
            match ext {
                Some(e) => format!("{joined}.{e}"),
                None => joined,
            }
        })
}

/// Deeply nested path with 6–15 segments.
fn deep_path() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(path_segment(), 6..=15),
        file_extension(),
    )
        .prop_map(|(segs, ext)| format!("{}.{ext}", segs.join("/")))
}

/// Path segment that may contain unicode characters.
fn unicode_segment() -> impl Strategy<Value = String> {
    prop_oneof![
        path_segment(),
        Just("données".to_string()),
        Just("日本語".to_string()),
        Just("über".to_string()),
        Just("café".to_string()),
        Just("naïve".to_string()),
        Just("ñoño".to_string()),
        Just("Ω_alpha".to_string()),
    ]
}

/// Relative path that may contain unicode segments.
fn unicode_path() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(unicode_segment(), 1..=4),
        prop::option::of(file_extension()),
    )
        .prop_map(|(segs, ext)| {
            let joined = segs.join("/");
            match ext {
                Some(e) => format!("{joined}.{e}"),
                None => joined,
            }
        })
}

/// Simple valid glob pattern drawn from a safe subset.
fn simple_glob() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("**".to_string()),
        Just("**/*".to_string()),
        Just("*".to_string()),
        path_segment().prop_map(|s| format!("{s}/**")),
        file_extension().prop_map(|ext| format!("**/*.{ext}")),
        file_extension().prop_map(|ext| format!("*.{ext}")),
        path_segment().prop_map(|s| format!("{s}/*")),
    ]
}

/// A small vec of simple glob patterns (0–3 patterns).
fn glob_vec(min: usize, max: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(simple_glob(), min..=max)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Include match + no exclude → Allowed
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn include_all_no_exclude_always_allowed(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Exclude match → DeniedByExclude regardless of includes
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_all_always_denied(path in relative_path(), use_inc in any::<bool>()) {
        let inc: Vec<String> = if use_inc { vec!["**".into()] } else { vec![] };
        let globs = IncludeExcludeGlobs::new(&inc, &["**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Empty include list → no include constraint (everything passes)
//    In abp-glob, empty patterns mean "no constraint", NOT "nothing matches".
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn empty_include_no_exclude_allows_everything(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Empty exclude list → nothing additionally excluded
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn empty_exclude_never_denies_by_exclude(
        path in relative_path(),
        inc in glob_vec(0, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &[]).unwrap();
        prop_assert_ne!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Double-star globs match across directory boundaries
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn double_star_include_crosses_directories(
        segs in prop::collection::vec(path_segment(), 2..=6),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("**/*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn double_star_exclude_crosses_directories(
        segs in prop::collection::vec(path_segment(), 2..=6),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[], &[format!("**/*.{ext}")]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Single-star behaviour in globset (literal_separator=false by default)
//    In globset's default config, `*` DOES match path separators.
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn single_star_matches_across_separators_in_globset(
        segs in prop::collection::vec(path_segment(), 2..=4),
        ext in file_extension(),
    ) {
        // globset default: literal_separator is false, so *.ext matches nested paths too.
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("*.{ext}")], &[]).unwrap();
        prop_assert_eq!(
            globs.decide_str(&path),
            MatchDecision::Allowed,
            "globset default: * crosses / so *.ext matches nested paths"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn single_star_rejects_wrong_extension(
        segs in prop::collection::vec(path_segment(), 1..=3),
    ) {
        let path = format!("{}.txt", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&["*.rs".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByMissingInclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Patterns are order-independent (include list)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn include_order_independent(path in relative_path()) {
        let a = vec!["src/**".into(), "tests/**".into(), "*.md".into()];
        let b = vec!["*.md".into(), "src/**".into(), "tests/**".into()];
        let c = vec!["tests/**".into(), "*.md".into(), "src/**".into()];

        let ga = IncludeExcludeGlobs::new(&a, &[]).unwrap();
        let gb = IncludeExcludeGlobs::new(&b, &[]).unwrap();
        let gc = IncludeExcludeGlobs::new(&c, &[]).unwrap();

        let ra = ga.decide_str(&path);
        prop_assert_eq!(ra, gb.decide_str(&path));
        prop_assert_eq!(ra, gc.decide_str(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Patterns are order-independent (exclude list)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_order_independent(path in relative_path()) {
        let a = vec!["*.log".into(), "target/**".into(), "*.tmp".into()];
        let b = vec!["target/**".into(), "*.tmp".into(), "*.log".into()];

        let ga = IncludeExcludeGlobs::new(&[], &a).unwrap();
        let gb = IncludeExcludeGlobs::new(&[], &b).unwrap();

        prop_assert_eq!(ga.decide_str(&path), gb.decide_str(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Idempotency: calling decide twice yields the same result
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn decide_is_idempotent(
        path in relative_path(),
        inc in glob_vec(0, 2),
        exc in glob_vec(0, 2),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let first = globs.decide_str(&path);
        let second = globs.decide_str(&path);
        prop_assert_eq!(first, second);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Idempotency: recompiling same patterns yields the same result
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn recompilation_is_idempotent(
        path in relative_path(),
        inc in glob_vec(0, 2),
        exc in glob_vec(0, 2),
    ) {
        let g1 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let g2 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        prop_assert_eq!(g1.decide_str(&path), g2.decide_str(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Complementary patterns: include=[*.rs] and exclude=[*.rs] → denied
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn complementary_patterns_deny_everything(ext in file_extension(), path in relative_path()) {
        let pat = format!("**/*.{ext}");
        let exc = pat.clone();
        let globs = IncludeExcludeGlobs::new(&[pat], &[exc]).unwrap();
        let result = globs.decide_str(&path);
        // Paths matching the extension are denied by exclude (which takes precedence).
        // Paths NOT matching are denied by missing include.
        prop_assert!(
            !result.is_allowed(),
            "complementary inc/exc should deny everything"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn complementary_doublestar_denies_everything(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(&["**".into()], &["**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Unicode in file paths
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn unicode_paths_with_include_all(path in unicode_path()) {
        let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn unicode_paths_with_exclude_all(path in unicode_path()) {
        let globs = IncludeExcludeGlobs::new(&[], &["**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn unicode_paths_no_patterns(path in unicode_path()) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Very deeply nested paths
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn deep_paths_matched_by_double_star(path in deep_path()) {
        let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn deep_paths_excluded_by_double_star(path in deep_path()) {
        let globs = IncludeExcludeGlobs::new(&[], &["**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn deep_paths_extension_matching(
        segs in prop::collection::vec(path_segment(), 8..=15),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("**/*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Compilation never panics for valid patterns
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn valid_glob_compilation_never_panics(pat in simple_glob()) {
        // Should succeed without panic.
        let result = IncludeExcludeGlobs::new(&[pat], &[]);
        prop_assert!(result.is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn valid_glob_compilation_as_exclude_never_panics(pat in simple_glob()) {
        let result = IncludeExcludeGlobs::new(&[], &[pat]);
        prop_assert!(result.is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn valid_glob_pair_compilation_never_panics(
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let result = IncludeExcludeGlobs::new(&inc, &exc);
        prop_assert!(result.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Invalid pattern handling
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn unbalanced_bracket_returns_error(seg in path_segment()) {
        // Unbalanced `[` is always invalid in globset.
        let pat = format!("{seg}/[invalid");
        let result = IncludeExcludeGlobs::new(&[pat], &[]);
        prop_assert!(result.is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn unbalanced_bracket_in_exclude_returns_error(seg in path_segment()) {
        let pat = format!("{seg}/[bad");
        let result = IncludeExcludeGlobs::new(&[], &[pat]);
        prop_assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. decide_str and decide_path consistency
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn decide_str_equals_decide_path(
        path in relative_path(),
        inc in glob_vec(0, 2),
        exc in glob_vec(0, 2),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let via_str = globs.decide_str(&path);
        let via_path = globs.decide_path(std::path::Path::new(&path));
        prop_assert_eq!(via_str, via_path);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Prefix include allows subtree, denies outside
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn prefix_include_allows_subtree(seg in path_segment(), rest in relative_path()) {
        let inside = format!("src/{seg}/{rest}");
        let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&inside), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn prefix_include_denies_outside(seg in path_segment(), rest in relative_path()) {
        let outside = format!("other/{seg}/{rest}");
        let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        prop_assert_eq!(
            globs.decide_str(&outside),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Prefix exclude denies subtree, allows outside
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn prefix_exclude_denies_subtree(seg in path_segment(), rest in relative_path()) {
        let inside = format!("secret/{seg}/{rest}");
        let globs = IncludeExcludeGlobs::new(&[], &["secret/**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&inside), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn prefix_exclude_allows_outside(seg in path_segment(), rest in relative_path()) {
        let outside = format!("public/{seg}/{rest}");
        let globs = IncludeExcludeGlobs::new(&[], &["secret/**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&outside), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Multiple includes form a union
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn multiple_includes_are_union(seg in path_segment(), rest in relative_path()) {
        let in_src = format!("src/{seg}/{rest}");
        let in_tests = format!("tests/{seg}/{rest}");
        let outside = format!("docs/{seg}/{rest}");

        let globs =
            IncludeExcludeGlobs::new(&["src/**".into(), "tests/**".into()], &[]).unwrap();

        prop_assert_eq!(globs.decide_str(&in_src), MatchDecision::Allowed);
        prop_assert_eq!(globs.decide_str(&in_tests), MatchDecision::Allowed);
        prop_assert_eq!(
            globs.decide_str(&outside),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Multiple excludes form a union
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn multiple_excludes_are_union(seg in path_segment(), rest in relative_path()) {
        let in_logs = format!("logs/{seg}/{rest}");
        let in_tmp = format!("tmp/{seg}/{rest}");
        let outside = format!("src/{seg}/{rest}");

        let globs =
            IncludeExcludeGlobs::new(&[], &["logs/**".into(), "tmp/**".into()]).unwrap();

        prop_assert_eq!(globs.decide_str(&in_logs), MatchDecision::DeniedByExclude);
        prop_assert_eq!(globs.decide_str(&in_tmp), MatchDecision::DeniedByExclude);
        prop_assert_eq!(globs.decide_str(&outside), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Include + exclude interaction: inc=src/** exc=src/gen/**
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn include_exclude_interaction(seg in path_segment(), rest in relative_path()) {
        let allowed = format!("src/{seg}/{rest}");
        let excluded = format!("src/generated/{seg}/{rest}");
        let outside = format!("docs/{seg}/{rest}");

        let globs = IncludeExcludeGlobs::new(
            &["src/**".into()],
            &["src/generated/**".into()],
        )
        .unwrap();

        prop_assert_eq!(globs.decide_str(&allowed), MatchDecision::Allowed);
        prop_assert_eq!(globs.decide_str(&excluded), MatchDecision::DeniedByExclude);
        prop_assert_eq!(
            globs.decide_str(&outside),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 22. The three-way invariant: allowed iff (include_matches ∧ ¬exclude_matches)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn allowed_iff_included_and_not_excluded(
        path in relative_path(),
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let result = globs.decide_str(&path);

        // Build reference matchers for individual checks.
        let inc_set = build_globset(&inc).unwrap();
        let exc_set = build_globset(&exc).unwrap();

        let inc_matches = inc_set
            .as_ref()
            .is_none_or(|s| s.is_match(&path));
        let exc_matches = exc_set
            .as_ref()
            .is_some_and(|s| s.is_match(&path));

        if exc_matches {
            prop_assert_eq!(result, MatchDecision::DeniedByExclude);
        } else if !inc_matches {
            prop_assert_eq!(result, MatchDecision::DeniedByMissingInclude);
        } else {
            prop_assert_eq!(result, MatchDecision::Allowed);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Include-only never returns DeniedByExclude
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn include_only_never_denied_by_exclude(
        path in relative_path(),
        inc in glob_vec(1, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &[]).unwrap();
        prop_assert_ne!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Exclude-only never returns DeniedByMissingInclude
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_only_never_denied_by_missing(
        path in relative_path(),
        exc in glob_vec(1, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&[], &exc).unwrap();
        prop_assert_ne!(
            globs.decide_str(&path),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Result is always a valid variant (never panics)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn decide_never_panics(
        path in relative_path(),
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let result = globs.decide_str(&path);
        prop_assert!(matches!(
            result,
            MatchDecision::Allowed
                | MatchDecision::DeniedByExclude
                | MatchDecision::DeniedByMissingInclude
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 26. build_globset: empty → None, non-empty → Some
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn build_globset_empty_is_none(_dummy in 0u8..1) {
        let result = build_globset(&[]).unwrap();
        prop_assert!(result.is_none());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn build_globset_nonempty_is_some(pat in simple_glob()) {
        let result = build_globset(&[pat]).unwrap();
        prop_assert!(result.is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Paths with dots and hyphens
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn paths_with_dots_and_hyphens(
        seg in "[a-z][-a-z0-9.]{1,10}",
        ext in file_extension(),
    ) {
        let path = format!("dir/{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&["dir/**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 28. Empty path string
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn empty_path_no_patterns_allowed(_dummy in 0u8..1) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(""), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn empty_path_with_include_denied(_dummy in 0u8..1) {
        let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(""), MatchDecision::DeniedByMissingInclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Superset include: ** should be a superset of any prefix/**
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn doublestar_is_superset_of_prefix(
        prefix in path_segment(),
        rest in relative_path(),
    ) {
        let path = format!("{prefix}/{rest}");

        let narrow = IncludeExcludeGlobs::new(&[format!("{prefix}/**")], &[]).unwrap();
        let wide = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();

        // If allowed by narrow, must be allowed by wide.
        if narrow.decide_str(&path).is_allowed() {
            prop_assert!(wide.decide_str(&path).is_allowed());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 30. Exclude subset: if excluded by prefix/**, also excluded by **
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn doublestar_exclude_is_superset(
        prefix in path_segment(),
        rest in relative_path(),
    ) {
        let path = format!("{prefix}/{rest}");

        let narrow = IncludeExcludeGlobs::new(&[], &[format!("{prefix}/**")]).unwrap();
        let wide = IncludeExcludeGlobs::new(&[], &["**".into()]).unwrap();

        // If denied by narrow, must also be denied by wide.
        if narrow.decide_str(&path) == MatchDecision::DeniedByExclude {
            prop_assert_eq!(wide.decide_str(&path), MatchDecision::DeniedByExclude);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 31. Adding more includes can only widen allowed set
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn adding_include_never_narrows(path in relative_path()) {
        let narrow = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        let wide =
            IncludeExcludeGlobs::new(&["src/**".into(), "tests/**".into()], &[]).unwrap();

        // If narrow allows, wide must also allow.
        if narrow.decide_str(&path).is_allowed() {
            prop_assert!(wide.decide_str(&path).is_allowed());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 32. Adding more excludes can only narrow allowed set
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn adding_exclude_never_widens(path in relative_path()) {
        let wide = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
        let narrow =
            IncludeExcludeGlobs::new(&[], &["*.log".into(), "*.tmp".into()]).unwrap();

        // If narrow allows, wide must also allow.
        if narrow.decide_str(&path).is_allowed() {
            prop_assert!(wide.decide_str(&path).is_allowed());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 33. Extension-based complementary: same ext in inc and exc → all denied
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn same_extension_include_exclude_denies_all(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        let path_matching = format!("{seg}.{ext}");
        let path_other = format!("{seg}.zzz");

        let pat = format!("*.{ext}");
        let exc = pat.clone();
        let globs = IncludeExcludeGlobs::new(&[pat], &[exc]).unwrap();

        // Matching path → excluded by exclude (takes precedence).
        prop_assert_eq!(
            globs.decide_str(&path_matching),
            MatchDecision::DeniedByExclude
        );
        // Non-matching path → denied by missing include.
        prop_assert_eq!(
            globs.decide_str(&path_other),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 34. MatchDecision::is_allowed roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn is_allowed_consistent_with_variant(
        path in relative_path(),
        inc in glob_vec(0, 2),
        exc in glob_vec(0, 2),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let result = globs.decide_str(&path);
        prop_assert_eq!(result.is_allowed(), result == MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 35. Unicode in include pattern literal matches unicode paths
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unicode_literal_in_pattern(ext in file_extension()) {
        let path = format!("données/fichier.{ext}");
        let globs = IncludeExcludeGlobs::new(&["données/**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 36. Paths matching an include pattern are always included (no exclude)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn extension_include_matches_generated_path(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        let path = format!("{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&[format!("*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn prefix_slash_doublestar_includes_any_subtree_path(
        prefix in path_segment(),
        segs in prop::collection::vec(path_segment(), 1..=4),
        ext in file_extension(),
    ) {
        let path = format!("{prefix}/{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("{prefix}/**")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 37. Paths matching an exclude pattern are always excluded
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn extension_exclude_always_denies(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        let path = format!("{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&[], &[format!("*.{ext}")]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn prefix_exclude_always_denies_subtree(
        prefix in path_segment(),
        segs in prop::collection::vec(path_segment(), 1..=4),
        ext in file_extension(),
    ) {
        let path = format!("{prefix}/{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[], &[format!("{prefix}/**")]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 38. Exclude always wins over include (precedence) — overlapping patterns
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_wins_same_extension_pattern(
        segs in prop::collection::vec(path_segment(), 1..=3),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(
            &[format!("**/*.{ext}")],
            &[format!("**/*.{ext}")],
        ).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_wins_same_prefix_pattern(
        prefix in path_segment(),
        rest in relative_path(),
    ) {
        let path = format!("{prefix}/{rest}");
        let globs = IncludeExcludeGlobs::new(
            &[format!("{prefix}/**")],
            &[format!("{prefix}/**")],
        ).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_wins_broader_include(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        // Include everything, but exclude a specific extension → denied
        let path = format!("{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(
            &["**".into()],
            &[format!("*.{ext}")],
        ).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 39. Empty include set means include everything (with exclude present)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn empty_include_with_non_matching_exclude_allows(
        seg in path_segment(),
    ) {
        // No include constraint + exclude only .zzz → .rs paths allowed
        let path = format!("{seg}.rs");
        let globs = IncludeExcludeGlobs::new(&[], &["*.zzz".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn empty_include_equivalent_to_doublestar_include(
        path in relative_path(),
        exc in glob_vec(0, 2),
    ) {
        let g_empty = IncludeExcludeGlobs::new(&[], &exc).unwrap();
        let g_star = IncludeExcludeGlobs::new(&["**".into()], &exc).unwrap();
        prop_assert_eq!(g_empty.decide_str(&path), g_star.decide_str(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 40. Empty exclude set means exclude nothing (with include present)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn empty_exclude_with_matching_include_allows(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        let path = format!("src/{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn empty_exclude_result_always_allowed_or_missing_include(
        path in relative_path(),
        inc in glob_vec(1, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &[]).unwrap();
        let result = globs.decide_str(&path);
        prop_assert!(
            result == MatchDecision::Allowed || result == MatchDecision::DeniedByMissingInclude,
            "with empty exclude, result must be Allowed or DeniedByMissingInclude, got {:?}",
            result,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 41. Double-star patterns match any depth (stress)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn doublestar_ext_matches_any_depth(
        segs in prop::collection::vec(path_segment(), 1..=12),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("**/*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn doublestar_mid_path_matches(
        prefix in path_segment(),
        mid in path_segment(),
        tail in relative_path(),
    ) {
        // Pattern: prefix/**/mid/** should match prefix/.../mid/...
        let path = format!("{prefix}/{mid}/{tail}");
        let globs = IncludeExcludeGlobs::new(
            &[format!("{prefix}/**")],
            &[],
        ).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 42. Single-star patterns match single segment (extension filtering)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn single_star_ext_rejects_different_ext(
        seg in path_segment(),
        ext1 in file_extension(),
        ext2 in file_extension(),
    ) {
        prop_assume!(ext1 != ext2);
        let path = format!("{seg}.{ext1}");
        let globs = IncludeExcludeGlobs::new(&[format!("*.{ext2}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByMissingInclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn single_star_ext_matches_same_ext(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        let path = format!("{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&[format!("*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 43. Compiled globs are deterministic — multiple compilations agree
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn triple_compilation_deterministic(
        path in relative_path(),
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let g1 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let g2 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let g3 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let r1 = g1.decide_str(&path);
        prop_assert_eq!(r1, g2.decide_str(&path));
        prop_assert_eq!(r1, g3.decide_str(&path));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn build_globset_deterministic(pat in simple_glob(), path in relative_path()) {
        let s1 = build_globset(std::slice::from_ref(&pat)).unwrap().unwrap();
        let s2 = build_globset(std::slice::from_ref(&pat)).unwrap().unwrap();
        prop_assert_eq!(s1.is_match(&path), s2.is_match(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 44. Very long paths work correctly
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn very_long_path_include_all(
        segs in prop::collection::vec(path_segment(), 20..=50),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn very_long_path_exclude_all(
        segs in prop::collection::vec(path_segment(), 20..=50),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[], &["**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn very_long_path_extension_filter(
        segs in prop::collection::vec(path_segment(), 20..=50),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("**/*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn very_long_path_prefix_filter(
        first in path_segment(),
        segs in prop::collection::vec(path_segment(), 20..=50),
        ext in file_extension(),
    ) {
        let path = format!("{first}/{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&[format!("{first}/**")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 45. Many patterns compile and match correctly
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn many_include_patterns_compile(
        pats in prop::collection::vec(simple_glob(), 10..=20),
        path in relative_path(),
    ) {
        let globs = IncludeExcludeGlobs::new(&pats, &[]).unwrap();
        let result = globs.decide_str(&path);
        prop_assert!(matches!(
            result,
            MatchDecision::Allowed | MatchDecision::DeniedByMissingInclude
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn many_exclude_patterns_compile(
        pats in prop::collection::vec(simple_glob(), 10..=20),
        path in relative_path(),
    ) {
        let globs = IncludeExcludeGlobs::new(&[], &pats).unwrap();
        let result = globs.decide_str(&path);
        prop_assert!(matches!(
            result,
            MatchDecision::Allowed | MatchDecision::DeniedByExclude
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn many_include_and_exclude_patterns_compile(
        inc in prop::collection::vec(simple_glob(), 5..=10),
        exc in prop::collection::vec(simple_glob(), 5..=10),
        path in relative_path(),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let result = globs.decide_str(&path);
        // Must still be a valid variant
        prop_assert!(matches!(
            result,
            MatchDecision::Allowed
                | MatchDecision::DeniedByExclude
                | MatchDecision::DeniedByMissingInclude
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn many_patterns_deterministic(
        inc in prop::collection::vec(simple_glob(), 5..=10),
        exc in prop::collection::vec(simple_glob(), 5..=10),
        path in relative_path(),
    ) {
        let g1 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let g2 = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        prop_assert_eq!(g1.decide_str(&path), g2.decide_str(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 46. Complement: !exclude(path) || include(path) semantics
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn allowed_implies_not_excluded(
        path in relative_path(),
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        if globs.decide_str(&path).is_allowed() {
            // Must NOT match exclude
            let exc_set = build_globset(&exc).unwrap();
            let exc_matches = exc_set.as_ref().is_some_and(|s| s.is_match(&path));
            prop_assert!(!exc_matches, "allowed path must not match exclude");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn allowed_implies_included(
        path in relative_path(),
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        if globs.decide_str(&path).is_allowed() {
            // Must match include (or include is empty)
            let inc_set = build_globset(&inc).unwrap();
            let inc_matches = inc_set.as_ref().is_none_or(|s| s.is_match(&path));
            prop_assert!(inc_matches, "allowed path must match include (or include empty)");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn denied_by_exclude_implies_exclude_matches(
        path in relative_path(),
        inc in glob_vec(0, 3),
        exc in glob_vec(0, 3),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        if globs.decide_str(&path) == MatchDecision::DeniedByExclude {
            let exc_set = build_globset(&exc).unwrap();
            let exc_matches = exc_set.as_ref().is_some_and(|s| s.is_match(&path));
            prop_assert!(exc_matches, "DeniedByExclude requires exclude match");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn denied_by_missing_include_implies_no_include_match(
        path in relative_path(),
        inc in glob_vec(1, 3),
        exc in glob_vec(0, 2),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        if globs.decide_str(&path) == MatchDecision::DeniedByMissingInclude {
            let inc_set = build_globset(&inc).unwrap();
            let inc_matches = inc_set.as_ref().is_some_and(|s| s.is_match(&path));
            prop_assert!(!inc_matches, "DeniedByMissingInclude requires no include match");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 47. Unicode in paths and patterns
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unicode_path_with_extension_include(
        seg in unicode_segment(),
        ext in file_extension(),
    ) {
        let path = format!("{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&[format!("**/*.{ext}")], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unicode_path_with_extension_exclude(
        seg in unicode_segment(),
        ext in file_extension(),
    ) {
        let path = format!("{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(&[], &[format!("**/*.{ext}")]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unicode_exclude_literal_pattern(ext in file_extension()) {
        let path = format!("über/secret.{ext}");
        let globs = IncludeExcludeGlobs::new(&[], &["über/**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unicode_include_exclude_interaction(
        seg in unicode_segment(),
        ext in file_extension(),
    ) {
        let path = format!("données/{seg}.{ext}");
        let globs = IncludeExcludeGlobs::new(
            &["données/**".into()],
            &[format!("données/{seg}/**")],
        ).unwrap();
        // Path is directly under données, not under données/seg/**, so not excluded
        // unless seg exactly equals the leaf (which it does for a flat file).
        // Just verify no panic and result is valid.
        let result = globs.decide_str(&path);
        prop_assert!(matches!(
            result,
            MatchDecision::Allowed | MatchDecision::DeniedByExclude
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unicode_deep_path_allowed(
        segs in prop::collection::vec(unicode_segment(), 3..=8),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 48. Monotonicity: narrowing include narrows allowed set
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn narrowing_include_never_widens(path in relative_path()) {
        let wide = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        let narrow = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();

        // If narrow allows, wide must also allow.
        if narrow.decide_str(&path).is_allowed() {
            prop_assert!(wide.decide_str(&path).is_allowed());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 49. Removing excludes can only widen allowed set
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn removing_exclude_never_narrows(path in relative_path()) {
        let with_exc = IncludeExcludeGlobs::new(
            &["src/**".into()],
            &["src/gen/**".into()],
        ).unwrap();
        let without_exc = IncludeExcludeGlobs::new(
            &["src/**".into()],
            &[],
        ).unwrap();

        // If allowed with exclude, must be allowed without exclude.
        if with_exc.decide_str(&path).is_allowed() {
            prop_assert!(without_exc.decide_str(&path).is_allowed());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 50. Disjoint include and exclude produce expected trichotomy
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn disjoint_include_exclude_trichotomy(
        seg in path_segment(),
        ext in file_extension(),
    ) {
        let globs = IncludeExcludeGlobs::new(
            &["src/**".into()],
            &["logs/**".into()],
        ).unwrap();

        // In src → Allowed (included, not excluded)
        let in_src = format!("src/{seg}.{ext}");
        prop_assert_eq!(globs.decide_str(&in_src), MatchDecision::Allowed);

        // In logs → DeniedByExclude even though not in include
        let in_logs = format!("logs/{seg}.{ext}");
        prop_assert_eq!(globs.decide_str(&in_logs), MatchDecision::DeniedByExclude);

        // In docs → DeniedByMissingInclude
        let in_docs = format!("docs/{seg}.{ext}");
        prop_assert_eq!(globs.decide_str(&in_docs), MatchDecision::DeniedByMissingInclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 51. Decide on same path across multiple globs instances is consistent
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn many_calls_same_path_same_result(
        path in relative_path(),
        inc in glob_vec(0, 2),
        exc in glob_vec(0, 2),
    ) {
        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let expected = globs.decide_str(&path);
        for _ in 0..10 {
            prop_assert_eq!(globs.decide_str(&path), expected);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 52. Build globset with many patterns
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn build_globset_many_patterns(
        pats in prop::collection::vec(simple_glob(), 15..=30),
    ) {
        let result = build_globset(&pats);
        prop_assert!(result.is_ok());
        prop_assert!(result.unwrap().is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 53. Exclude check is independent of include list contents
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn exclude_verdict_independent_of_includes(
        path in relative_path(),
        inc1 in glob_vec(0, 3),
        inc2 in glob_vec(0, 3),
        exc in glob_vec(1, 3),
    ) {
        let g1 = IncludeExcludeGlobs::new(&inc1, &exc).unwrap();
        let g2 = IncludeExcludeGlobs::new(&inc2, &exc).unwrap();

        let r1 = g1.decide_str(&path);
        let r2 = g2.decide_str(&path);

        // If one says DeniedByExclude, the other must too (exclude is checked first)
        if r1 == MatchDecision::DeniedByExclude {
            prop_assert_eq!(r2, MatchDecision::DeniedByExclude);
        }
        if r2 == MatchDecision::DeniedByExclude {
            prop_assert_eq!(r1, MatchDecision::DeniedByExclude);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 54. Include check is independent of exclude list contents (for non-excluded)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn include_verdict_stable_across_excludes(
        path in relative_path(),
        inc in glob_vec(1, 3),
    ) {
        let g_no_exc = IncludeExcludeGlobs::new(&inc, &[]).unwrap();
        let g_with_exc = IncludeExcludeGlobs::new(&inc, &["zzz_nonexistent/**".into()]).unwrap();

        // Exclude "zzz_nonexistent/**" won't match any generated path, so results should agree
        prop_assert_eq!(g_no_exc.decide_str(&path), g_with_exc.decide_str(&path));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 55. Doublestar include with extension exclude still filters correctly
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn doublestar_include_extension_exclude(
        segs in prop::collection::vec(path_segment(), 1..=4),
        ext in file_extension(),
    ) {
        let path = format!("{}.{ext}", segs.join("/"));
        let globs = IncludeExcludeGlobs::new(
            &["**".into()],
            &[format!("**/*.{ext}")],
        ).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}
