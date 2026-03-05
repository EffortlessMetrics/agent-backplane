// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar-side protocol version negotiation.
//!
//! During the hello handshake the sidecar proposes a version (or set of
//! versions) it supports. The control plane responds with an accept or
//! reject. This module provides the types and logic for that exchange.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// VersionProposal
// ---------------------------------------------------------------------------

/// A version proposal sent by the sidecar inside (or alongside) the hello
/// envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionProposal {
    /// Preferred version (highest the sidecar supports).
    pub preferred: String,
    /// All versions the sidecar is willing to speak, most preferred first.
    pub supported: Vec<String>,
}

impl VersionProposal {
    /// Create a proposal with a single supported version.
    #[must_use]
    pub fn single(version: impl Into<String>) -> Self {
        let v = version.into();
        Self {
            preferred: v.clone(),
            supported: vec![v],
        }
    }

    /// Create a proposal from a preferred version and a list of alternatives.
    #[must_use]
    pub fn new(preferred: impl Into<String>, supported: Vec<String>) -> Self {
        Self {
            preferred: preferred.into(),
            supported,
        }
    }

    /// Whether the proposal includes the given version string.
    #[must_use]
    pub fn contains(&self, version: &str) -> bool {
        self.supported.iter().any(|v| v == version)
    }
}

// ---------------------------------------------------------------------------
// VersionResponse
// ---------------------------------------------------------------------------

/// The control plane's response to a version proposal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum VersionResponse {
    /// The control plane accepted a specific version.
    Accepted {
        /// The version that was agreed upon.
        version: String,
    },
    /// The control plane rejected all proposed versions.
    Rejected {
        /// Human-readable reason for the rejection.
        reason: String,
        /// Versions the control plane supports (so the sidecar can
        /// potentially retry with a compatible version).
        supported: Vec<String>,
    },
}

impl VersionResponse {
    /// Create an accepted response.
    #[must_use]
    pub fn accepted(version: impl Into<String>) -> Self {
        Self::Accepted {
            version: version.into(),
        }
    }

    /// Create a rejected response.
    #[must_use]
    pub fn rejected(reason: impl Into<String>, supported: Vec<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
            supported,
        }
    }

    /// Returns `true` if the response is an acceptance.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// Returns the accepted version, if any.
    #[must_use]
    pub fn accepted_version(&self) -> Option<&str> {
        match self {
            Self::Accepted { version } => Some(version),
            Self::Rejected { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// negotiate_from_proposal
// ---------------------------------------------------------------------------

/// Negotiate a version from a sidecar's proposal against the host's
/// supported versions.
///
/// Returns an [`VersionResponse::Accepted`] with the highest common version,
/// or [`VersionResponse::Rejected`] if there is no overlap.
#[must_use]
pub fn negotiate_from_proposal(
    proposal: &VersionProposal,
    host_supported: &[String],
) -> VersionResponse {
    // Walk the sidecar's preference order and pick the first one the host
    // also supports.
    for v in &proposal.supported {
        if host_supported.contains(v) {
            return VersionResponse::accepted(v);
        }
    }
    VersionResponse::rejected("no compatible version", host_supported.to_vec())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_proposal() {
        let p = VersionProposal::single("abp/v0.1");
        assert_eq!(p.preferred, "abp/v0.1");
        assert_eq!(p.supported.len(), 1);
        assert!(p.contains("abp/v0.1"));
        assert!(!p.contains("abp/v0.2"));
    }

    #[test]
    fn multi_proposal() {
        let p = VersionProposal::new("abp/v0.2", vec!["abp/v0.2".into(), "abp/v0.1".into()]);
        assert_eq!(p.preferred, "abp/v0.2");
        assert!(p.contains("abp/v0.1"));
        assert!(p.contains("abp/v0.2"));
    }

    #[test]
    fn negotiate_picks_first_common() {
        let proposal = VersionProposal::new(
            "abp/v0.3",
            vec!["abp/v0.3".into(), "abp/v0.2".into(), "abp/v0.1".into()],
        );
        let host = vec!["abp/v0.1".into(), "abp/v0.2".into()];
        let resp = negotiate_from_proposal(&proposal, &host);
        assert!(resp.is_accepted());
        assert_eq!(resp.accepted_version(), Some("abp/v0.2"));
    }

    #[test]
    fn negotiate_no_overlap() {
        let proposal = VersionProposal::single("abp/v1.0");
        let host = vec!["abp/v0.1".into()];
        let resp = negotiate_from_proposal(&proposal, &host);
        assert!(!resp.is_accepted());
        assert!(resp.accepted_version().is_none());
    }

    #[test]
    fn version_response_accepted_serde() {
        let resp = VersionResponse::accepted("abp/v0.1");
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: VersionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn version_response_rejected_serde() {
        let resp = VersionResponse::rejected("no match", vec!["abp/v0.1".into()]);
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: VersionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn proposal_serde_roundtrip() {
        let p = VersionProposal::new("abp/v0.2", vec!["abp/v0.2".into(), "abp/v0.1".into()]);
        let json = serde_json::to_string(&p).unwrap();
        let decoded: VersionProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn negotiate_exact_match() {
        let proposal = VersionProposal::single("abp/v0.1");
        let host = vec!["abp/v0.1".into()];
        let resp = negotiate_from_proposal(&proposal, &host);
        assert_eq!(resp.accepted_version(), Some("abp/v0.1"));
    }
}
