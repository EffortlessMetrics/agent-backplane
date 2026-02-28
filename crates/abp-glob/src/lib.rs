//! abp-glob
#![deny(unsafe_code)]
//!
//! Focused glob compilation and include/exclude matching utilities.

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Result of evaluating a path against include/exclude glob rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchDecision {
    /// Path passes both include and exclude filters.
    Allowed,
    /// Path matched an exclude pattern.
    DeniedByExclude,
    /// Path did not match any include pattern (when includes are specified).
    DeniedByMissingInclude,
}

impl MatchDecision {
    /// Returns `true` only for [`MatchDecision::Allowed`].
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Compiled include/exclude glob pair for path filtering.
///
/// Exclude patterns take precedence: a path matching an exclude glob is denied
/// even if it also matches an include glob. Empty pattern lists are treated as
/// "no constraint" (all paths pass).
#[derive(Debug, Clone)]
pub struct IncludeExcludeGlobs {
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
}

impl IncludeExcludeGlobs {
    /// Compile include and exclude pattern lists into a reusable matcher.
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self> {
        Ok(Self {
            include: build_globset(include)?,
            exclude: build_globset(exclude)?,
        })
    }

    /// Evaluate a [`Path`] against the compiled glob rules.
    pub fn decide_path(&self, candidate: &Path) -> MatchDecision {
        if self
            .exclude
            .as_ref()
            .is_some_and(|set| set.is_match(candidate))
        {
            return MatchDecision::DeniedByExclude;
        }
        if self
            .include
            .as_ref()
            .is_some_and(|set| !set.is_match(candidate))
        {
            return MatchDecision::DeniedByMissingInclude;
        }
        MatchDecision::Allowed
    }

    /// Convenience wrapper around [`decide_path`](Self::decide_path) for string slices.
    pub fn decide_str(&self, candidate: &str) -> MatchDecision {
        self.decide_path(Path::new(candidate))
    }
}

/// Compile a list of glob patterns into a [`GlobSet`], returning `None` for empty input.
pub fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    Ok(Some(b.build()?))
}

#[cfg(test)]
mod tests {
    use super::{IncludeExcludeGlobs, MatchDecision};

    fn patterns(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn allows_everything_without_patterns() {
        let rules = IncludeExcludeGlobs::new(&Vec::new(), &Vec::new()).expect("compile rules");
        assert_eq!(rules.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(rules.decide_str("README.md"), MatchDecision::Allowed);
    }

    #[test]
    fn include_patterns_gate_matches() {
        let rules = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &Vec::new())
            .expect("compile include rules");
        assert_eq!(rules.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            rules.decide_str("README.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn exclude_patterns_take_precedence() {
        let rules =
            IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["src/private/**"]))
                .expect("compile include/exclude rules");
        assert_eq!(
            rules.decide_str("src/private/secrets.txt"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn invalid_pattern_returns_error() {
        let err = IncludeExcludeGlobs::new(&patterns(&["["]), &Vec::new())
            .expect_err("invalid glob should fail");
        assert!(
            err.to_string().contains("invalid glob"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn multiple_include_patterns() {
        let rules =
            IncludeExcludeGlobs::new(&patterns(&["src/**", "tests/**"]), &Vec::new())
                .expect("compile rules");
        assert_eq!(rules.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(rules.decide_str("tests/it.rs"), MatchDecision::Allowed);
        assert_eq!(
            rules.decide_str("README.md"),
            MatchDecision::DeniedByMissingInclude
        );
        assert_eq!(
            rules.decide_str("docs/guide.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn multiple_exclude_patterns() {
        let rules = IncludeExcludeGlobs::new(
            &Vec::new(),
            &patterns(&["*.log", "target/**", "*.tmp"]),
        )
        .expect("compile rules");
        assert_eq!(rules.decide_str("build.log"), MatchDecision::DeniedByExclude);
        assert_eq!(
            rules.decide_str("target/debug/bin"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(rules.decide_str("data.tmp"), MatchDecision::DeniedByExclude);
        assert_eq!(rules.decide_str("src/main.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn nested_paths() {
        let rules = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &Vec::new())
            .expect("compile rules");
        assert_eq!(
            rules.decide_str("src/a/b/c/d.rs"),
            MatchDecision::Allowed
        );
        assert_eq!(
            rules.decide_str("src/a/b/c/d/e/f/g.txt"),
            MatchDecision::Allowed
        );
    }

    #[test]
    fn unicode_paths() {
        let rules = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &Vec::new())
            .expect("compile rules");
        assert_eq!(
            rules.decide_str("src/données/fichier.rs"),
            MatchDecision::Allowed
        );
        assert_eq!(
            rules.decide_str("données/fichier.rs"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn empty_string_path() {
        let rules = IncludeExcludeGlobs::new(&Vec::new(), &Vec::new()).expect("compile rules");
        assert_eq!(rules.decide_str(""), MatchDecision::Allowed);

        let with_include =
            IncludeExcludeGlobs::new(&patterns(&["src/**"]), &Vec::new()).expect("compile rules");
        assert_eq!(
            with_include.decide_str(""),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn wildcard_only_include() {
        let rules = IncludeExcludeGlobs::new(&patterns(&["*"]), &Vec::new())
            .expect("compile rules");
        assert_eq!(rules.decide_str("README.md"), MatchDecision::Allowed);
        assert_eq!(rules.decide_str("Cargo.toml"), MatchDecision::Allowed);
        // globset default: literal_separator is false, so * crosses /
        assert_eq!(rules.decide_str("src/lib.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn double_star_vs_single_star() {
        let single = IncludeExcludeGlobs::new(&patterns(&["*.rs"]), &Vec::new())
            .expect("compile single star");
        let double = IncludeExcludeGlobs::new(&patterns(&["**/*.rs"]), &Vec::new())
            .expect("compile double star");

        // Top-level .rs file: both match
        assert_eq!(single.decide_str("main.rs"), MatchDecision::Allowed);
        assert_eq!(double.decide_str("main.rs"), MatchDecision::Allowed);

        // globset default: literal_separator is false, so *.rs also matches nested
        assert_eq!(single.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(double.decide_str("src/lib.rs"), MatchDecision::Allowed);

        // Difference: *.rs won't match a file without .rs extension
        assert_eq!(
            single.decide_str("src/lib.txt"),
            MatchDecision::DeniedByMissingInclude
        );
        assert_eq!(
            double.decide_str("src/lib.txt"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn exclude_overrides_include_complex() {
        let rules = IncludeExcludeGlobs::new(
            &patterns(&["src/**", "tests/**"]),
            &patterns(&["src/generated/**", "tests/fixtures/**"]),
        )
        .expect("compile rules");
        assert_eq!(rules.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            rules.decide_str("src/generated/output.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(rules.decide_str("tests/unit.rs"), MatchDecision::Allowed);
        assert_eq!(
            rules.decide_str("tests/fixtures/data.json"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            rules.decide_str("docs/readme.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn decide_path_vs_decide_str_consistency() {
        use std::path::Path;

        let rules =
            IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["src/secret/**"]))
                .expect("compile rules");

        let cases = &["src/lib.rs", "src/secret/key.pem", "README.md"];
        for &c in cases {
            assert_eq!(
                rules.decide_str(c),
                rules.decide_path(Path::new(c)),
                "mismatch for path: {c}"
            );
        }
    }

    #[test]
    fn build_globset_with_empty_returns_none() {
        let result = super::build_globset(&[]).expect("should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn build_globset_with_patterns_returns_some() {
        let result =
            super::build_globset(&patterns(&["*.rs", "src/**"])).expect("should succeed");
        assert!(result.is_some());
        let set = result.unwrap();
        assert!(set.is_match("main.rs"));
        assert!(set.is_match("src/lib.rs"));
        assert!(!set.is_match("README.md"));
    }

    #[test]
    fn match_decision_is_allowed() {
        assert!(MatchDecision::Allowed.is_allowed());
        assert!(!MatchDecision::DeniedByExclude.is_allowed());
        assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
    }
}
