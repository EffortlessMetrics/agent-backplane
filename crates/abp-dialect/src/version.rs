// SPDX-License-Identifier: MIT OR Apache-2.0
//! API version tracking and compatibility constraints per dialect.
//!
//! `DialectVersion` pairs a dialect with a version string (e.g.
//! `"2024-06-01"` for Claude).  `VersionConstraint` expresses minimum /
//! exact / range requirements for compatibility checking.

use serde::{Deserialize, Serialize};

use crate::Dialect;

// ── DialectVersion ──────────────────────────────────────────────────────

/// A concrete API version for a specific dialect.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DialectVersion {
    /// The dialect this version applies to.
    pub dialect: Dialect,
    /// Opaque version string (e.g. `"2024-12-01"`, `"v1"`, `"2024-06-01"`).
    pub version: String,
}

impl DialectVersion {
    /// Create a new dialect version.
    #[must_use]
    pub fn new(dialect: Dialect, version: impl Into<String>) -> Self {
        Self {
            dialect,
            version: version.into(),
        }
    }

    /// Parse a version string in the format `"dialect/version"`.
    ///
    /// Returns `None` if the format is invalid or the dialect is unknown.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (dialect_str, ver) = s.split_once('/')?;
        let dialect = match dialect_str {
            "openai" | "open_ai" => Dialect::OpenAi,
            "claude" => Dialect::Claude,
            "gemini" => Dialect::Gemini,
            "codex" => Dialect::Codex,
            "kimi" => Dialect::Kimi,
            "copilot" => Dialect::Copilot,
            _ => return None,
        };
        if ver.is_empty() {
            return None;
        }
        Some(Self::new(dialect, ver))
    }

    /// Format as `"dialect/version"`.
    #[must_use]
    pub fn to_string_pair(&self) -> String {
        let d = match self.dialect {
            Dialect::OpenAi => "openai",
            Dialect::Claude => "claude",
            Dialect::Gemini => "gemini",
            Dialect::Codex => "codex",
            Dialect::Kimi => "kimi",
            Dialect::Copilot => "copilot",
        };
        format!("{d}/{}", self.version)
    }
}

impl std::fmt::Display for DialectVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.dialect.label(), self.version)
    }
}

// ── Known latest versions ───────────────────────────────────────────────

/// Returns the latest known API version for each dialect.
#[must_use]
pub fn latest_versions() -> &'static [(Dialect, &'static str)] {
    &[
        (Dialect::OpenAi, "2024-12-01"),
        (Dialect::Claude, "2024-06-01"),
        (Dialect::Gemini, "v1beta"),
        (Dialect::Codex, "2025-03-01"),
        (Dialect::Kimi, "v1"),
        (Dialect::Copilot, "2024-12-15"),
    ]
}

/// Look up the latest known version for a dialect.
#[must_use]
pub fn latest_version(dialect: Dialect) -> Option<DialectVersion> {
    latest_versions()
        .iter()
        .find(|(d, _)| *d == dialect)
        .map(|(d, v)| DialectVersion::new(*d, *v))
}

// ── VersionConstraint ───────────────────────────────────────────────────

/// A version constraint for compatibility checking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VersionConstraint {
    /// Any version is acceptable.
    Any,
    /// Exactly this version string.
    Exact {
        /// The required version.
        version: String,
    },
    /// Version must be ≥ this string (lexicographic comparison).
    Minimum {
        /// The minimum version (inclusive).
        version: String,
    },
    /// Version must fall in `[min, max]` (lexicographic, inclusive).
    Range {
        /// Lower bound (inclusive).
        min: String,
        /// Upper bound (inclusive).
        max: String,
    },
}

impl VersionConstraint {
    /// Check whether a version string satisfies this constraint.
    #[must_use]
    pub fn satisfied_by(&self, version: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Exact { version: v } => version == v,
            Self::Minimum { version: min } => version >= min.as_str(),
            Self::Range { min, max } => version >= min.as_str() && version <= max.as_str(),
        }
    }

    /// Convenience constructor for [`Exact`](Self::Exact).
    #[must_use]
    pub fn exact(version: impl Into<String>) -> Self {
        Self::Exact {
            version: version.into(),
        }
    }

    /// Convenience constructor for [`Minimum`](Self::Minimum).
    #[must_use]
    pub fn minimum(version: impl Into<String>) -> Self {
        Self::Minimum {
            version: version.into(),
        }
    }

    /// Convenience constructor for [`Range`](Self::Range).
    #[must_use]
    pub fn range(min: impl Into<String>, max: impl Into<String>) -> Self {
        Self::Range {
            min: min.into(),
            max: max.into(),
        }
    }
}

impl std::fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Any => f.write_str("*"),
            Self::Exact { version } => write!(f, "={version}"),
            Self::Minimum { version } => write!(f, ">={version}"),
            Self::Range { min, max } => write!(f, "[{min}, {max}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_version_new() {
        let v = DialectVersion::new(Dialect::Claude, "2024-06-01");
        assert_eq!(v.dialect, Dialect::Claude);
        assert_eq!(v.version, "2024-06-01");
    }

    #[test]
    fn dialect_version_parse_valid() {
        let v = DialectVersion::parse("claude/2024-06-01").unwrap();
        assert_eq!(v.dialect, Dialect::Claude);
        assert_eq!(v.version, "2024-06-01");
    }

    #[test]
    fn dialect_version_parse_openai() {
        let v = DialectVersion::parse("openai/2024-12-01").unwrap();
        assert_eq!(v.dialect, Dialect::OpenAi);
        assert_eq!(v.version, "2024-12-01");
    }

    #[test]
    fn dialect_version_parse_open_ai_alias() {
        let v = DialectVersion::parse("open_ai/v1").unwrap();
        assert_eq!(v.dialect, Dialect::OpenAi);
        assert_eq!(v.version, "v1");
    }

    #[test]
    fn dialect_version_parse_invalid_no_slash() {
        assert!(DialectVersion::parse("claude2024").is_none());
    }

    #[test]
    fn dialect_version_parse_invalid_unknown_dialect() {
        assert!(DialectVersion::parse("unknown/v1").is_none());
    }

    #[test]
    fn dialect_version_parse_empty_version() {
        assert!(DialectVersion::parse("claude/").is_none());
    }

    #[test]
    fn dialect_version_to_string_pair() {
        let v = DialectVersion::new(Dialect::Gemini, "v1beta");
        assert_eq!(v.to_string_pair(), "gemini/v1beta");
    }

    #[test]
    fn dialect_version_display() {
        let v = DialectVersion::new(Dialect::Claude, "2024-06-01");
        assert_eq!(v.to_string(), "Claude/2024-06-01");
    }

    #[test]
    fn dialect_version_serde_roundtrip() {
        let v = DialectVersion::new(Dialect::OpenAi, "2024-12-01");
        let json = serde_json::to_string(&v).unwrap();
        let back: DialectVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn latest_versions_covers_all_dialects() {
        for d in Dialect::all() {
            assert!(
                latest_version(*d).is_some(),
                "missing latest version for {d:?}"
            );
        }
    }

    #[test]
    fn latest_version_claude() {
        let v = latest_version(Dialect::Claude).unwrap();
        assert_eq!(v.version, "2024-06-01");
    }

    #[test]
    fn constraint_any_always_satisfied() {
        let c = VersionConstraint::Any;
        assert!(c.satisfied_by("anything"));
        assert!(c.satisfied_by(""));
    }

    #[test]
    fn constraint_exact_match() {
        let c = VersionConstraint::exact("2024-06-01");
        assert!(c.satisfied_by("2024-06-01"));
        assert!(!c.satisfied_by("2024-06-02"));
    }

    #[test]
    fn constraint_minimum() {
        let c = VersionConstraint::minimum("2024-06-01");
        assert!(c.satisfied_by("2024-06-01"));
        assert!(c.satisfied_by("2024-12-01"));
        assert!(!c.satisfied_by("2024-01-01"));
    }

    #[test]
    fn constraint_range() {
        let c = VersionConstraint::range("2024-01-01", "2024-06-30");
        assert!(c.satisfied_by("2024-03-15"));
        assert!(c.satisfied_by("2024-01-01"));
        assert!(c.satisfied_by("2024-06-30"));
        assert!(!c.satisfied_by("2024-07-01"));
        assert!(!c.satisfied_by("2023-12-31"));
    }

    #[test]
    fn constraint_display() {
        assert_eq!(VersionConstraint::Any.to_string(), "*");
        assert_eq!(VersionConstraint::exact("v1").to_string(), "=v1");
        assert_eq!(
            VersionConstraint::minimum("2024-06-01").to_string(),
            ">=2024-06-01"
        );
        assert_eq!(VersionConstraint::range("a", "z").to_string(), "[a, z]");
    }

    #[test]
    fn constraint_serde_roundtrip_any() {
        let c = VersionConstraint::Any;
        let json = serde_json::to_string(&c).unwrap();
        let back: VersionConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn constraint_serde_roundtrip_exact() {
        let c = VersionConstraint::exact("2024-06-01");
        let json = serde_json::to_string(&c).unwrap();
        let back: VersionConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn constraint_serde_roundtrip_range() {
        let c = VersionConstraint::range("2024-01-01", "2024-12-31");
        let json = serde_json::to_string(&c).unwrap();
        let back: VersionConstraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn dialect_version_ordering() {
        let a = DialectVersion::new(Dialect::Claude, "2024-01-01");
        let b = DialectVersion::new(Dialect::Claude, "2024-06-01");
        assert!(a < b);
    }
}
