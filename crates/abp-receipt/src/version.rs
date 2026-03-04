// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt format versioning and compatibility checking.

use abp_core::CONTRACT_VERSION;
use std::fmt;

/// The current receipt format version.
///
/// This is distinct from `CONTRACT_VERSION` — the contract version governs the
/// wire protocol while the format version tracks the receipt serialization
/// schema. They are currently aligned but may diverge in the future.
pub const RECEIPT_FORMAT_VERSION: &str = "receipt/v0.1";

/// Parsed representation of a receipt format version string.
///
/// Format strings follow the pattern `"receipt/vMAJOR.MINOR"`.
///
/// # Examples
///
/// ```
/// use abp_receipt::version::FormatVersion;
///
/// let v = FormatVersion::parse("receipt/v0.1").unwrap();
/// assert_eq!(v.major, 0);
/// assert_eq!(v.minor, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatVersion {
    /// Major version — breaking changes increment this.
    pub major: u32,
    /// Minor version — backwards-compatible additions increment this.
    pub minor: u32,
}

/// Errors from version parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The version string does not match the expected format.
    InvalidFormat(String),
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(s) => write!(f, "invalid version format: \"{s}\""),
        }
    }
}

impl std::error::Error for VersionError {}

impl FormatVersion {
    /// Parse a version string like `"receipt/v0.1"`.
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::InvalidFormat`] if the string doesn't match.
    pub fn parse(s: &str) -> Result<Self, VersionError> {
        let rest = s
            .strip_prefix("receipt/v")
            .ok_or_else(|| VersionError::InvalidFormat(s.to_string()))?;
        let parts: Vec<&str> = rest.split('.').collect();
        if parts.len() != 2 {
            return Err(VersionError::InvalidFormat(s.to_string()));
        }
        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| VersionError::InvalidFormat(s.to_string()))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| VersionError::InvalidFormat(s.to_string()))?;
        Ok(Self { major, minor })
    }

    /// Return the current receipt format version.
    #[must_use]
    pub fn current() -> Self {
        Self::parse(RECEIPT_FORMAT_VERSION).expect("built-in version is always valid")
    }

    /// Check whether `other` is compatible with `self`.
    ///
    /// Compatibility rules:
    /// - Same major version: compatible (minor additions are non-breaking).
    /// - Different major version: incompatible.
    #[must_use]
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

impl fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "receipt/v{}.{}", self.major, self.minor)
    }
}

/// Check whether the contract version embedded in a receipt matches the
/// current [`CONTRACT_VERSION`].
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::version::check_contract_version;
///
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// assert!(check_contract_version(&r.meta.contract_version));
/// ```
#[must_use]
pub fn check_contract_version(version: &str) -> bool {
    version == CONTRACT_VERSION
}
