// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended version negotiation for the sidecar protocol.
//!
//! The host sends a set of supported versions; the sidecar selects the best
//! match. This module provides [`VersionOffer`], [`VersionSelection`], and
//! the [`negotiate`] function that drives the negotiation.

use crate::version::{ProtocolVersion, VersionError, VersionRange};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// VersionOffer
// ---------------------------------------------------------------------------

/// An offer from one peer listing the protocol versions it supports.
///
/// # Examples
///
/// ```
/// use abp_protocol::version::ProtocolVersion;
/// use abp_protocol::version_negotiation::VersionOffer;
///
/// let offer = VersionOffer::new(vec![
///     ProtocolVersion { major: 0, minor: 1 },
///     ProtocolVersion { major: 0, minor: 2 },
/// ]);
/// assert_eq!(offer.versions().len(), 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionOffer {
    /// Supported versions in preference order (most preferred first).
    versions: Vec<ProtocolVersion>,
}

impl VersionOffer {
    /// Create an offer from an explicit list of versions.
    #[must_use]
    pub fn new(versions: Vec<ProtocolVersion>) -> Self {
        Self { versions }
    }

    /// Create an offer from a [`VersionRange`] by enumerating all minor
    /// versions from min to max (inclusive). Returns `None` if the range
    /// has different major versions.
    #[must_use]
    pub fn from_range(range: &VersionRange) -> Option<Self> {
        if range.min.major != range.max.major {
            return None;
        }
        let versions: Vec<ProtocolVersion> = (range.min.minor..=range.max.minor)
            .rev()
            .map(|minor| ProtocolVersion {
                major: range.min.major,
                minor,
            })
            .collect();
        Some(Self { versions })
    }

    /// The offered versions.
    #[must_use]
    pub fn versions(&self) -> &[ProtocolVersion] {
        &self.versions
    }

    /// Returns `true` if the offer contains the given version.
    #[must_use]
    pub fn contains(&self, v: &ProtocolVersion) -> bool {
        self.versions.contains(v)
    }

    /// The highest (most recent) version in the offer.
    #[must_use]
    pub fn highest(&self) -> Option<&ProtocolVersion> {
        self.versions.iter().max()
    }

    /// The lowest (oldest) version in the offer.
    #[must_use]
    pub fn lowest(&self) -> Option<&ProtocolVersion> {
        self.versions.iter().min()
    }
}

// ---------------------------------------------------------------------------
// VersionSelection
// ---------------------------------------------------------------------------

/// The result of a successful negotiation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionSelection {
    /// The agreed-upon version.
    pub selected: ProtocolVersion,
    /// The host's offer that was considered.
    pub host_offer: VersionOffer,
    /// The sidecar's offer that was considered.
    pub sidecar_offer: VersionOffer,
}

// ---------------------------------------------------------------------------
// NegotiationError
// ---------------------------------------------------------------------------

/// Errors specific to extended version negotiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NegotiationError {
    /// No overlapping versions between host and sidecar.
    NoOverlap {
        /// What the host offered.
        host: VersionOffer,
        /// What the sidecar offered.
        sidecar: VersionOffer,
    },
    /// One or both offers were empty.
    EmptyOffer,
    /// Underlying version error.
    Version(VersionError),
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOverlap { .. } => write!(f, "no overlapping protocol versions"),
            Self::EmptyOffer => write!(f, "one or both version offers are empty"),
            Self::Version(e) => write!(f, "version error: {e}"),
        }
    }
}

impl std::error::Error for NegotiationError {}

impl From<VersionError> for NegotiationError {
    fn from(e: VersionError) -> Self {
        Self::Version(e)
    }
}

// ---------------------------------------------------------------------------
// negotiate()
// ---------------------------------------------------------------------------

/// Negotiate the best protocol version from two offers.
///
/// Selects the highest version present in both offers. When multiple versions
/// overlap, the host's preference order is used as the tiebreaker.
///
/// # Errors
///
/// Returns [`NegotiationError`] if either offer is empty or if there is no
/// overlapping version.
///
/// # Examples
///
/// ```
/// use abp_protocol::version::ProtocolVersion;
/// use abp_protocol::version_negotiation::{VersionOffer, negotiate};
///
/// let host = VersionOffer::new(vec![
///     ProtocolVersion { major: 0, minor: 2 },
///     ProtocolVersion { major: 0, minor: 1 },
/// ]);
/// let sidecar = VersionOffer::new(vec![
///     ProtocolVersion { major: 0, minor: 1 },
///     ProtocolVersion { major: 0, minor: 3 },
/// ]);
/// let sel = negotiate(&host, &sidecar).unwrap();
/// assert_eq!(sel.selected, ProtocolVersion { major: 0, minor: 1 });
/// ```
pub fn negotiate(
    host_offer: &VersionOffer,
    sidecar_offer: &VersionOffer,
) -> Result<VersionSelection, NegotiationError> {
    if host_offer.versions.is_empty() || sidecar_offer.versions.is_empty() {
        return Err(NegotiationError::EmptyOffer);
    }

    // Find the highest version present in both offers.
    let mut common: Vec<&ProtocolVersion> = host_offer
        .versions
        .iter()
        .filter(|v| sidecar_offer.versions.contains(v))
        .collect();

    if common.is_empty() {
        return Err(NegotiationError::NoOverlap {
            host: host_offer.clone(),
            sidecar: sidecar_offer.clone(),
        });
    }

    common.sort();
    let selected = (*common.last().unwrap()).clone();

    Ok(VersionSelection {
        selected,
        host_offer: host_offer.clone(),
        sidecar_offer: sidecar_offer.clone(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::ProtocolVersion;

    fn v(major: u32, minor: u32) -> ProtocolVersion {
        ProtocolVersion { major, minor }
    }

    #[test]
    fn negotiate_exact_match() {
        let host = VersionOffer::new(vec![v(0, 1)]);
        let sidecar = VersionOffer::new(vec![v(0, 1)]);
        let sel = negotiate(&host, &sidecar).unwrap();
        assert_eq!(sel.selected, v(0, 1));
    }

    #[test]
    fn negotiate_picks_highest_common() {
        let host = VersionOffer::new(vec![v(0, 1), v(0, 2), v(0, 3)]);
        let sidecar = VersionOffer::new(vec![v(0, 2), v(0, 3), v(0, 4)]);
        let sel = negotiate(&host, &sidecar).unwrap();
        assert_eq!(sel.selected, v(0, 3));
    }

    #[test]
    fn negotiate_no_overlap() {
        let host = VersionOffer::new(vec![v(0, 1)]);
        let sidecar = VersionOffer::new(vec![v(1, 0)]);
        let err = negotiate(&host, &sidecar).unwrap_err();
        assert!(matches!(err, NegotiationError::NoOverlap { .. }));
    }

    #[test]
    fn negotiate_empty_host() {
        let host = VersionOffer::new(vec![]);
        let sidecar = VersionOffer::new(vec![v(0, 1)]);
        let err = negotiate(&host, &sidecar).unwrap_err();
        assert!(matches!(err, NegotiationError::EmptyOffer));
    }

    #[test]
    fn negotiate_empty_sidecar() {
        let host = VersionOffer::new(vec![v(0, 1)]);
        let sidecar = VersionOffer::new(vec![]);
        let err = negotiate(&host, &sidecar).unwrap_err();
        assert!(matches!(err, NegotiationError::EmptyOffer));
    }

    #[test]
    fn offer_from_range() {
        let range = VersionRange {
            min: v(0, 1),
            max: v(0, 3),
        };
        let offer = VersionOffer::from_range(&range).unwrap();
        assert_eq!(offer.versions().len(), 3);
        assert!(offer.contains(&v(0, 1)));
        assert!(offer.contains(&v(0, 2)));
        assert!(offer.contains(&v(0, 3)));
    }

    #[test]
    fn offer_from_range_different_majors() {
        let range = VersionRange {
            min: v(0, 1),
            max: v(1, 0),
        };
        assert!(VersionOffer::from_range(&range).is_none());
    }

    #[test]
    fn offer_highest_lowest() {
        let offer = VersionOffer::new(vec![v(0, 3), v(0, 1), v(0, 2)]);
        assert_eq!(offer.highest(), Some(&v(0, 3)));
        assert_eq!(offer.lowest(), Some(&v(0, 1)));
    }

    #[test]
    fn offer_empty_highest_lowest() {
        let offer = VersionOffer::new(vec![]);
        assert_eq!(offer.highest(), None);
        assert_eq!(offer.lowest(), None);
    }

    #[test]
    fn serde_offer_round_trip() {
        let offer = VersionOffer::new(vec![v(0, 1), v(0, 2)]);
        let json = serde_json::to_string(&offer).unwrap();
        let decoded: VersionOffer = serde_json::from_str(&json).unwrap();
        assert_eq!(offer, decoded);
    }

    #[test]
    fn serde_selection_round_trip() {
        let host = VersionOffer::new(vec![v(0, 1), v(0, 2)]);
        let sidecar = VersionOffer::new(vec![v(0, 2)]);
        let sel = negotiate(&host, &sidecar).unwrap();
        let json = serde_json::to_string(&sel).unwrap();
        let decoded: VersionSelection = serde_json::from_str(&json).unwrap();
        assert_eq!(sel, decoded);
    }

    #[test]
    fn negotiation_error_display() {
        let err = NegotiationError::EmptyOffer;
        assert_eq!(format!("{err}"), "one or both version offers are empty");

        let err = NegotiationError::NoOverlap {
            host: VersionOffer::new(vec![v(0, 1)]),
            sidecar: VersionOffer::new(vec![v(1, 0)]),
        };
        assert_eq!(format!("{err}"), "no overlapping protocol versions");
    }

    #[test]
    fn offer_contains() {
        let offer = VersionOffer::new(vec![v(0, 1), v(0, 2)]);
        assert!(offer.contains(&v(0, 1)));
        assert!(!offer.contains(&v(0, 3)));
    }
}
