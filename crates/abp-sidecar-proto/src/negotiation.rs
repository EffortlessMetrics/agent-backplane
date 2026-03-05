// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol version negotiation and capability advertisement types.
//!
//! Provides structured types for version negotiation during the sidecar
//! handshake phase, plus a [`HandshakeValidator`] that checks contract
//! compatibility and capability requirements.

use std::fmt;

use abp_core::{
    BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest, CapabilityRequirements,
    SupportLevel,
};
use abp_protocol::{Envelope, ProtocolError, is_compatible_version, parse_version};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ProtocolVersion
// ---------------------------------------------------------------------------

/// Parsed representation of an ABP protocol version string (`"abp/v<major>.<minor>"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Major version component.
    pub major: u32,
    /// Minor version component.
    pub minor: u32,
}

impl ProtocolVersion {
    /// The current protocol version compiled into this binary.
    #[must_use]
    pub fn current() -> Self {
        Self::parse(CONTRACT_VERSION).expect("CONTRACT_VERSION must be valid")
    }

    /// Parse a version string such as `"abp/v0.1"`.
    ///
    /// Returns `None` if the string is not in the expected format.
    #[must_use]
    pub fn parse(version: &str) -> Option<Self> {
        let (major, minor) = parse_version(version)?;
        Some(Self { major, minor })
    }

    /// Two versions are compatible when they share the same major component.
    #[must_use]
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }

    /// Format back to the canonical `"abp/v<major>.<minor>"` string.
    #[must_use]
    pub fn to_version_string(&self) -> String {
        format!("abp/v{}.{}", self.major, self.minor)
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

/// A range of protocol versions a peer is willing to accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    /// Minimum version (inclusive).
    pub min: ProtocolVersion,
    /// Maximum version (inclusive).
    pub max: ProtocolVersion,
}

impl VersionRange {
    /// Create a range spanning a single version.
    #[must_use]
    pub fn exact(version: ProtocolVersion) -> Self {
        Self {
            min: version,
            max: version,
        }
    }

    /// Create a range from two versions. Returns `None` if min > max by
    /// major then minor ordering.
    #[must_use]
    pub fn new(min: ProtocolVersion, max: ProtocolVersion) -> Option<Self> {
        if (min.major, min.minor) > (max.major, max.minor) {
            return None;
        }
        Some(Self { min, max })
    }

    /// Whether `version` falls within this range (inclusive on both ends).
    #[must_use]
    pub fn contains(&self, version: &ProtocolVersion) -> bool {
        let v = (version.major, version.minor);
        v >= (self.min.major, self.min.minor) && v <= (self.max.major, self.max.minor)
    }
}

// ---------------------------------------------------------------------------
// NegotiationResult
// ---------------------------------------------------------------------------

/// Outcome of protocol version negotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationResult {
    /// The two sides agreed on this version.
    Agreed(ProtocolVersion),
    /// No compatible version could be found.
    Incompatible {
        /// Our version.
        ours: ProtocolVersion,
        /// Their version.
        theirs: ProtocolVersion,
    },
}

impl NegotiationResult {
    /// Returns `true` if negotiation succeeded.
    #[must_use]
    pub fn is_agreed(&self) -> bool {
        matches!(self, Self::Agreed(_))
    }
}

/// Negotiate a protocol version between two peers.
///
/// The negotiation picks the peer's version when it is compatible (same
/// major) with ours, preferring the lower minor to ensure both sides
/// understand the exchanged messages.
#[must_use]
pub fn negotiate_version(ours: &ProtocolVersion, theirs: &ProtocolVersion) -> NegotiationResult {
    if ours.is_compatible_with(theirs) {
        let agreed_minor = std::cmp::min(ours.minor, theirs.minor);
        NegotiationResult::Agreed(ProtocolVersion {
            major: ours.major,
            minor: agreed_minor,
        })
    } else {
        NegotiationResult::Incompatible {
            ours: *ours,
            theirs: *theirs,
        }
    }
}

// ---------------------------------------------------------------------------
// CapabilityAdvertisement
// ---------------------------------------------------------------------------

/// Structured capability advertisement exchanged during handshake.
///
/// Wraps a [`CapabilityManifest`] together with the peer's identity and
/// protocol version so that the receiving side can make informed routing
/// decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityAdvertisement {
    /// Protocol version this advertisement targets.
    pub protocol_version: String,
    /// Identity of the backend behind the sidecar.
    pub backend: BackendIdentity,
    /// Full capability manifest.
    pub capabilities: CapabilityManifest,
}

impl CapabilityAdvertisement {
    /// Build an advertisement from parts.
    #[must_use]
    pub fn new(backend: BackendIdentity, capabilities: CapabilityManifest) -> Self {
        Self {
            protocol_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities,
        }
    }

    /// Check if the advertisement includes a specific capability at *any*
    /// support level.
    #[must_use]
    pub fn has_capability(&self, cap: &Capability) -> bool {
        self.capabilities.contains_key(cap)
    }

    /// Check if the advertisement satisfies a set of capability requirements.
    #[must_use]
    pub fn satisfies(&self, requirements: &CapabilityRequirements) -> bool {
        requirements.required.iter().all(|req| {
            self.capabilities
                .get(&req.capability)
                .is_some_and(|level| meets_min_support(level, &req.min_support))
        })
    }
}

/// Check if a given [`SupportLevel`] meets the minimum.
fn meets_min_support(level: &SupportLevel, min: &abp_core::MinSupport) -> bool {
    match min {
        abp_core::MinSupport::Any => true,
        abp_core::MinSupport::Emulated => {
            matches!(level, SupportLevel::Emulated | SupportLevel::Native)
        }
        abp_core::MinSupport::Native => matches!(level, SupportLevel::Native),
    }
}

// ---------------------------------------------------------------------------
// HandshakeValidator
// ---------------------------------------------------------------------------

/// Validates the hello handshake envelope against local expectations.
pub struct HandshakeValidator {
    our_version: ProtocolVersion,
    required_capabilities: Vec<Capability>,
}

impl HandshakeValidator {
    /// Create a validator requiring the current contract version and no
    /// specific capabilities.
    #[must_use]
    pub fn new() -> Self {
        Self {
            our_version: ProtocolVersion::current(),
            required_capabilities: Vec::new(),
        }
    }

    /// Require specific capabilities in the peer's hello.
    #[must_use]
    pub fn require_capabilities(mut self, caps: Vec<Capability>) -> Self {
        self.required_capabilities = caps;
        self
    }

    /// Validate a hello [`Envelope`].
    ///
    /// Checks contract-version compatibility and that all required
    /// capabilities are present.
    pub fn validate_hello(
        &self,
        envelope: &Envelope,
    ) -> Result<CapabilityAdvertisement, ProtocolError> {
        match envelope {
            Envelope::Hello {
                contract_version,
                backend,
                capabilities,
                ..
            } => {
                if !is_compatible_version(contract_version, &self.our_version.to_version_string()) {
                    return Err(ProtocolError::Violation(format!(
                        "incompatible contract version: got \"{contract_version}\", \
                         expected compatible with \"{}\"",
                        self.our_version
                    )));
                }

                for cap in &self.required_capabilities {
                    if !capabilities.contains_key(cap) {
                        return Err(ProtocolError::Violation(format!(
                            "missing required capability: {cap:?}"
                        )));
                    }
                }

                Ok(CapabilityAdvertisement {
                    protocol_version: contract_version.clone(),
                    backend: backend.clone(),
                    capabilities: capabilities.clone(),
                })
            }
            _ => Err(ProtocolError::UnexpectedMessage {
                expected: "hello".into(),
                got: format!("{:?}", std::mem::discriminant(envelope)),
            }),
        }
    }
}

impl Default for HandshakeValidator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement,
        CapabilityRequirements, MinSupport, SupportLevel,
    };

    fn test_identity() -> BackendIdentity {
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }

    // -- ProtocolVersion ---------------------------------------------------

    #[test]
    fn parse_current_version() {
        let v = ProtocolVersion::current();
        assert_eq!(v.to_version_string(), CONTRACT_VERSION);
    }

    #[test]
    fn parse_valid_version() {
        let v = ProtocolVersion::parse("abp/v2.5").unwrap();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 5);
        assert_eq!(v.to_string(), "abp/v2.5");
    }

    #[test]
    fn parse_invalid_version() {
        assert!(ProtocolVersion::parse("invalid").is_none());
        assert!(ProtocolVersion::parse("abp/v").is_none());
        assert!(ProtocolVersion::parse("").is_none());
    }

    #[test]
    fn version_compatibility() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        let v10 = ProtocolVersion { major: 1, minor: 0 };
        assert!(v01.is_compatible_with(&v02));
        assert!(!v01.is_compatible_with(&v10));
    }

    #[test]
    fn version_display() {
        let v = ProtocolVersion { major: 3, minor: 7 };
        assert_eq!(format!("{v}"), "abp/v3.7");
    }

    // -- VersionRange ------------------------------------------------------

    #[test]
    fn version_range_exact() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let range = VersionRange::exact(v);
        assert!(range.contains(&v));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    }

    #[test]
    fn version_range_new_valid() {
        let min = ProtocolVersion { major: 0, minor: 1 };
        let max = ProtocolVersion { major: 0, minor: 3 };
        let range = VersionRange::new(min, max).unwrap();
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
    }

    #[test]
    fn version_range_new_invalid() {
        let min = ProtocolVersion { major: 1, minor: 0 };
        let max = ProtocolVersion { major: 0, minor: 5 };
        assert!(VersionRange::new(min, max).is_none());
    }

    #[test]
    fn version_range_boundaries_inclusive() {
        let min = ProtocolVersion { major: 0, minor: 1 };
        let max = ProtocolVersion { major: 0, minor: 3 };
        let range = VersionRange::new(min, max).unwrap();
        assert!(range.contains(&min));
        assert!(range.contains(&max));
    }

    // -- NegotiationResult -------------------------------------------------

    #[test]
    fn negotiate_compatible_same_version() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&v, &v);
        assert!(result.is_agreed());
        assert_eq!(result, NegotiationResult::Agreed(v));
    }

    #[test]
    fn negotiate_compatible_different_minor() {
        let ours = ProtocolVersion { major: 0, minor: 3 };
        let theirs = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&ours, &theirs);
        assert_eq!(
            result,
            NegotiationResult::Agreed(ProtocolVersion { major: 0, minor: 1 })
        );
    }

    #[test]
    fn negotiate_incompatible() {
        let ours = ProtocolVersion { major: 0, minor: 1 };
        let theirs = ProtocolVersion { major: 1, minor: 0 };
        let result = negotiate_version(&ours, &theirs);
        assert!(!result.is_agreed());
        assert!(matches!(result, NegotiationResult::Incompatible { .. }));
    }

    // -- CapabilityAdvertisement -------------------------------------------

    #[test]
    fn advertisement_has_capability() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);
        assert!(ad.has_capability(&Capability::Streaming));
        assert!(!ad.has_capability(&Capability::ToolRead));
    }

    #[test]
    fn advertisement_satisfies_empty_requirements() {
        let ad = CapabilityAdvertisement::new(test_identity(), CapabilityManifest::new());
        let reqs = CapabilityRequirements::default();
        assert!(ad.satisfies(&reqs));
    }

    #[test]
    fn advertisement_satisfies_requirements() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ad.satisfies(&reqs));
    }

    #[test]
    fn advertisement_fails_insufficient_support_level() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Emulated);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(!ad.satisfies(&reqs));
    }

    #[test]
    fn advertisement_fails_missing_capability() {
        let ad = CapabilityAdvertisement::new(test_identity(), CapabilityManifest::new());
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Any,
            }],
        };
        assert!(!ad.satisfies(&reqs));
    }

    #[test]
    fn advertisement_protocol_version_is_current() {
        let ad = CapabilityAdvertisement::new(test_identity(), CapabilityManifest::new());
        assert_eq!(ad.protocol_version, CONTRACT_VERSION);
    }

    // -- HandshakeValidator ------------------------------------------------

    #[test]
    fn validator_accepts_valid_hello() {
        let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
        let validator = HandshakeValidator::new();
        let ad = validator.validate_hello(&hello).unwrap();
        assert_eq!(ad.backend.id, "test");
    }

    #[test]
    fn validator_rejects_non_hello() {
        let event = Envelope::Event {
            ref_id: "r".into(),
            event: abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: abp_core::AgentEventKind::RunStarted {
                    message: "hi".into(),
                },
                ext: None,
            },
        };
        let validator = HandshakeValidator::new();
        assert!(validator.validate_hello(&event).is_err());
    }

    #[test]
    fn validator_rejects_incompatible_version() {
        let hello = Envelope::Hello {
            contract_version: "abp/v99.0".into(),
            backend: test_identity(),
            capabilities: CapabilityManifest::new(),
            mode: abp_core::ExecutionMode::default(),
        };
        let validator = HandshakeValidator::new();
        let err = validator.validate_hello(&hello).unwrap_err();
        assert!(err.to_string().contains("incompatible"));
    }

    #[test]
    fn validator_checks_required_capabilities() {
        let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
        let validator = HandshakeValidator::new().require_capabilities(vec![Capability::Streaming]);
        let err = validator.validate_hello(&hello).unwrap_err();
        assert!(err.to_string().contains("missing required capability"));
    }

    #[test]
    fn validator_passes_with_required_capabilities_present() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let hello = Envelope::hello(test_identity(), caps);
        let validator = HandshakeValidator::new().require_capabilities(vec![Capability::Streaming]);
        assert!(validator.validate_hello(&hello).is_ok());
    }

    #[test]
    fn meets_min_support_any() {
        assert!(meets_min_support(
            &SupportLevel::Unsupported,
            &MinSupport::Any
        ));
        assert!(meets_min_support(&SupportLevel::Native, &MinSupport::Any));
    }

    #[test]
    fn meets_min_support_emulated() {
        assert!(!meets_min_support(
            &SupportLevel::Unsupported,
            &MinSupport::Emulated
        ));
        assert!(meets_min_support(
            &SupportLevel::Emulated,
            &MinSupport::Emulated
        ));
        assert!(meets_min_support(
            &SupportLevel::Native,
            &MinSupport::Emulated
        ));
    }

    #[test]
    fn meets_min_support_native() {
        assert!(!meets_min_support(
            &SupportLevel::Unsupported,
            &MinSupport::Native
        ));
        assert!(!meets_min_support(
            &SupportLevel::Emulated,
            &MinSupport::Native
        ));
        assert!(meets_min_support(
            &SupportLevel::Native,
            &MinSupport::Native
        ));
    }
}
