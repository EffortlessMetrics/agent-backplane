// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-glob` using proptest.

use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use proptest::prelude::*;

/// Strategy producing simple safe path segments (alphanumeric + underscore).
fn path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(|s| s.to_string())
}

/// Strategy producing a relative path like `seg1/seg2/seg3.ext`.
fn relative_path() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(path_segment(), 1..=4),
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

// ── 1. Include pattern that matches ⇒ Allowed ──────────────────────

proptest! {
    #[test]
    fn include_matching_path_returns_allowed(path in relative_path()) {
        // Use a wildcard include that matches every path.
        let globs = IncludeExcludeGlobs::new(
            &["**".to_string()],
            &Vec::new(),
        ).unwrap();

        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ── 2. Exclude pattern that matches ⇒ DeniedByExclude ──────────────

proptest! {
    #[test]
    fn exclude_matching_path_returns_denied(path in relative_path()) {
        // Use a wildcard exclude that matches every path.
        let globs = IncludeExcludeGlobs::new(
            &Vec::new(),
            &["**".to_string()],
        ).unwrap();

        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ── 3. Both include and exclude match ⇒ exclude wins ───────────────

proptest! {
    #[test]
    fn exclude_beats_include(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(
            &["**".to_string()],
            &["**".to_string()],
        ).unwrap();

        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }
}

// ── 4. Empty patterns ⇒ everything allowed ─────────────────────────

proptest! {
    #[test]
    fn empty_patterns_allow_everything(path in relative_path()) {
        let globs = IncludeExcludeGlobs::new(&Vec::new(), &Vec::new()).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }
}

// ── 5. Determinism — same input always gives same output ────────────

proptest! {
    #[test]
    fn decision_is_deterministic(
        path in relative_path(),
        include_all in any::<bool>(),
        exclude_all in any::<bool>(),
    ) {
        let inc = if include_all { vec!["**".to_string()] } else { Vec::new() };
        let exc = if exclude_all { vec!["**".to_string()] } else { Vec::new() };

        let globs = IncludeExcludeGlobs::new(&inc, &exc).unwrap();
        let first  = globs.decide_str(&path);
        let second = globs.decide_str(&path);

        prop_assert_eq!(first, second, "decision must be deterministic");
    }
}

// ── 6. Specific prefix include only matches that prefix ─────────────

proptest! {
    #[test]
    fn prefix_include_denies_outside_prefix(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let inside = format!("src/{seg}/{rest}");
        let outside = format!("other/{seg}/{rest}");

        let globs = IncludeExcludeGlobs::new(
            &["src/**".to_string()],
            &Vec::new(),
        ).unwrap();

        prop_assert_eq!(globs.decide_str(&inside), MatchDecision::Allowed);
        prop_assert_eq!(globs.decide_str(&outside), MatchDecision::DeniedByMissingInclude);
    }
}

// ── 7. Specific prefix exclude denies inside, allows outside ────────

proptest! {
    #[test]
    fn prefix_exclude_denies_inside_allows_outside(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let inside = format!("secret/{seg}/{rest}");
        let outside = format!("public/{seg}/{rest}");

        let globs = IncludeExcludeGlobs::new(
            &Vec::new(),
            &["secret/**".to_string()],
        ).unwrap();

        prop_assert_eq!(globs.decide_str(&inside), MatchDecision::DeniedByExclude);
        prop_assert_eq!(globs.decide_str(&outside), MatchDecision::Allowed);
    }
}

// ── 8. decide_str and decide_path are consistent ────────────────────

proptest! {
    #[test]
    fn decide_str_matches_decide_path(
        path in relative_path(),
        exclude_all in any::<bool>(),
    ) {
        let exc = if exclude_all { vec!["**".to_string()] } else { Vec::new() };
        let globs = IncludeExcludeGlobs::new(&Vec::new(), &exc).unwrap();

        let via_str  = globs.decide_str(&path);
        let via_path = globs.decide_path(std::path::Path::new(&path));

        prop_assert_eq!(via_str, via_path, "decide_str and decide_path must agree");
    }
}
