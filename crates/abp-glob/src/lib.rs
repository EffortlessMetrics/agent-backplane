//! abp-glob
//!
//! Focused glob compilation and include/exclude matching utilities.

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchDecision {
    Allowed,
    DeniedByExclude,
    DeniedByMissingInclude,
}

impl MatchDecision {
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

#[derive(Debug, Clone)]
pub struct IncludeExcludeGlobs {
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
}

impl IncludeExcludeGlobs {
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self> {
        Ok(Self {
            include: build_globset(include)?,
            exclude: build_globset(exclude)?,
        })
    }

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

    pub fn decide_str(&self, candidate: &str) -> MatchDecision {
        self.decide_path(Path::new(candidate))
    }
}

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
}
