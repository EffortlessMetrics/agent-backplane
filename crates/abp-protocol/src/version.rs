// SPDX-License-Identifier: MIT OR Apache-2.0
//! Structured protocol version negotiation.
//!
//! Provides [`ProtocolVersion`], [`VersionRange`], and [`negotiate_version`]
//! for type-safe version handling beyond the free functions in the crate root.

use std::fmt;

use abp_core::CONTRACT_VERSION;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when parsing or negotiating protocol versions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VersionError {
    /// The string does not match the `"abp/vMAJOR.MINOR"` format.
    #[error("invalid version format (expected \"abp/vMAJOR.MINOR\")")]
    InvalidFormat,

    /// The major version component is not a valid integer.
    #[error("invalid major version component")]
    InvalidMajor,

    /// The minor version component is not a valid integer.
    #[error("invalid minor version component")]
    InvalidMinor,

    /// The local and remote versions have different major versions.
    #[error("incompatible protocol versions: local {local}, remote {remote}")]
    Incompatible {
        /// The local protocol version.
        local: ProtocolVersion,
        /// The remote protocol version.
        remote: ProtocolVersion,
    },
}

// ---------------------------------------------------------------------------
// ProtocolVersion
// ---------------------------------------------------------------------------

/// A parsed `"abp/vMAJOR.MINOR"` protocol version.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Major version number (breaking changes).
    pub major: u32,
    /// Minor version number (backwards-compatible additions).
    pub minor: u32,
}

impl ProtocolVersion {
    /// Parse a version string of the form `"abp/vMAJOR.MINOR"`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_protocol::version::ProtocolVersion;
    /// let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    /// assert_eq!(v.major, 0);
    /// assert_eq!(v.minor, 1);
    ///
    /// // Invalid format returns an error.
    /// assert!(ProtocolVersion::parse("invalid").is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`VersionError`] if the string does not match the expected format.
    pub fn parse(s: &str) -> Result<Self, VersionError> {
        let rest = s.strip_prefix("abp/v").ok_or(VersionError::InvalidFormat)?;
        let (major_str, minor_str) = rest
            .split_once('.')
            .ok_or(VersionError::InvalidFormat)?;
        let major = major_str.parse::<u32>().map_err(|_| VersionError::InvalidMajor)?;
        let minor = minor_str.parse::<u32>().map_err(|_| VersionError::InvalidMinor)?;
        Ok(Self { major, minor })
    }

    /// Format as `"abp/vMAJOR.MINOR"`.
    #[must_use]
    #[allow(clippy::inherent_to_string_shadow_display)]
    pub fn to_string(&self) -> String {
        format!("abp/v{}.{}", self.major, self.minor)
    }

    /// Two versions are compatible when they share the same major version and
    /// `other.minor >= self.minor` (i.e. the remote side is at least as new).
    #[must_use]
    pub fn is_compatible(&self, other: &ProtocolVersion) -> bool {
        self.major == other.major && other.minor >= self.minor
    }

    /// Returns the [`ProtocolVersion`] corresponding to [`CONTRACT_VERSION`].
    #[must_use]
    pub fn current() -> Self {
        Self::parse(CONTRACT_VERSION).expect("CONTRACT_VERSION must be a valid version string")
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "abp/v{}.{}", self.major, self.minor)
    }
}

// ---------------------------------------------------------------------------
// VersionRange
// ---------------------------------------------------------------------------

/// An inclusive range of protocol versions `[min, max]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    /// Minimum supported version (inclusive).
    pub min: ProtocolVersion,
    /// Maximum supported version (inclusive).
    pub max: ProtocolVersion,
}

impl VersionRange {
    /// Returns `true` if `version` falls within `[min, max]` (inclusive).
    #[must_use]
    pub fn contains(&self, version: &ProtocolVersion) -> bool {
        version >= &self.min && version <= &self.max
    }

    /// Returns `true` if `version` is compatible with the range — i.e. it
    /// shares the same major version as both bounds and falls within them.
    #[must_use]
    pub fn is_compatible(&self, version: &ProtocolVersion) -> bool {
        self.min.major == version.major
            && self.max.major == version.major
            && self.contains(version)
    }
}

// ---------------------------------------------------------------------------
// Negotiation
// ---------------------------------------------------------------------------

/// Negotiate the effective protocol version between a local and remote peer.
///
/// Returns the *minimum* of the two versions when they are mutually
/// compatible (same major, each peer's minor is ≥ the other's minimum
/// requirement — which is trivially true when majors match).
///
/// # Errors
///
/// Returns [`VersionError::Incompatible`] when the major versions differ.
pub fn negotiate_version(
    local: &ProtocolVersion,
    remote: &ProtocolVersion,
) -> Result<ProtocolVersion, VersionError> {
    if local.major != remote.major {
        return Err(VersionError::Incompatible {
            local: local.clone(),
            remote: remote.clone(),
        });
    }
    Ok(std::cmp::min(local, remote).clone())
}
