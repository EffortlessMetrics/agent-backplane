// SPDX-License-Identifier: MIT OR Apache-2.0
//! API versioning support for the ABP daemon HTTP API.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// ApiVersion
// ---------------------------------------------------------------------------

/// A semantic API version consisting of a major and minor component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl ApiVersion {
    /// Parse a version string such as `"v1"`, `"v1.0"`, or `"1.2"`.
    pub fn parse(s: &str) -> Result<Self, ApiVersionError> {
        let s = s.strip_prefix('v').unwrap_or(s);
        if s.is_empty() {
            return Err(ApiVersionError::InvalidFormat(
                "empty version string".to_string(),
            ));
        }

        let parts: Vec<&str> = s.splitn(2, '.').collect();
        let major = parts[0].parse::<u32>().map_err(|_| {
            ApiVersionError::InvalidFormat(format!("invalid major version: {}", parts[0]))
        })?;

        let minor = if parts.len() > 1 {
            parts[1].parse::<u32>().map_err(|_| {
                ApiVersionError::InvalidFormat(format!("invalid minor version: {}", parts[1]))
            })?
        } else {
            0
        };

        Ok(Self { major, minor })
    }

    /// Two versions are compatible if they share the same major version.
    pub fn is_compatible(&self, other: &ApiVersion) -> bool {
        self.major == other.major
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}", self.major, self.minor)
    }
}

impl Ord for ApiVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
    }
}

impl PartialOrd for ApiVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// ApiVersionError
// ---------------------------------------------------------------------------

/// Errors that can occur during version parsing or negotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiVersionError {
    /// The version string could not be parsed.
    InvalidFormat(String),
    /// The parsed version is not supported by this server.
    UnsupportedVersion(ApiVersion),
}

impl fmt::Display for ApiVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(msg) => write!(f, "invalid version format: {msg}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported API version: {v}"),
        }
    }
}

impl std::error::Error for ApiVersionError {}

// ---------------------------------------------------------------------------
// VersionedEndpoint
// ---------------------------------------------------------------------------

/// Metadata describing the version range for a single API endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedEndpoint {
    /// The URL path pattern (e.g. `/health`).
    pub path: String,
    /// Minimum API version that supports this endpoint (inclusive).
    pub min_version: ApiVersion,
    /// Maximum API version that supports this endpoint (inclusive). `None`
    /// means the endpoint is available in all versions from `min_version`
    /// onward.
    pub max_version: Option<ApiVersion>,
    /// Whether this endpoint is deprecated.
    pub deprecated: bool,
    /// Optional human-readable deprecation message.
    pub deprecated_message: Option<String>,
}

// ---------------------------------------------------------------------------
// ApiVersionRegistry
// ---------------------------------------------------------------------------

/// Registry that tracks which endpoints are available in which API versions.
#[derive(Debug, Clone)]
pub struct ApiVersionRegistry {
    current: ApiVersion,
    endpoints: Vec<VersionedEndpoint>,
}

impl ApiVersionRegistry {
    /// Create a new registry whose current (latest) version is `current`.
    pub fn new(current: ApiVersion) -> Self {
        Self {
            current,
            endpoints: Vec::new(),
        }
    }

    /// Register a versioned endpoint.
    pub fn register(&mut self, endpoint: VersionedEndpoint) {
        self.endpoints.push(endpoint);
    }

    /// Returns `true` if `path` is available for the given `version`.
    pub fn is_supported(&self, path: &str, version: &ApiVersion) -> bool {
        self.endpoints.iter().any(|ep| {
            ep.path == path
                && *version >= ep.min_version
                && ep.max_version.is_none_or(|max| *version <= max)
        })
    }

    /// Return all endpoints that are marked as deprecated.
    pub fn deprecated_endpoints(&self) -> Vec<&VersionedEndpoint> {
        self.endpoints.iter().filter(|ep| ep.deprecated).collect()
    }

    /// The current (latest) API version tracked by this registry.
    pub fn current_version(&self) -> &ApiVersion {
        &self.current
    }

    /// Collect the distinct sorted set of API versions referenced by all
    /// registered endpoints (from `min_version` through the current version
    /// for each major line).
    pub fn supported_versions(&self) -> Vec<ApiVersion> {
        let mut versions = std::collections::BTreeSet::new();
        versions.insert(self.current);
        for ep in &self.endpoints {
            versions.insert(ep.min_version);
            if let Some(max) = ep.max_version {
                versions.insert(max);
            }
        }
        versions.into_iter().collect()
    }

    /// Return every endpoint available for the given `version`.
    pub fn endpoints_for_version(&self, version: &ApiVersion) -> Vec<&VersionedEndpoint> {
        self.endpoints
            .iter()
            .filter(|ep| {
                *version >= ep.min_version
                    && ep.max_version.is_none_or(|max| *version <= max)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// VersionNegotiator
// ---------------------------------------------------------------------------

/// Picks the best supported version given a client request.
pub struct VersionNegotiator;

impl VersionNegotiator {
    /// Given the `requested` version and a list of `supported` versions,
    /// return the highest compatible version (same major) that does not
    /// exceed the requested version. Returns `None` if no compatible version
    /// exists.
    pub fn negotiate(requested: &ApiVersion, supported: &[ApiVersion]) -> Option<ApiVersion> {
        supported
            .iter()
            .filter(|v| v.is_compatible(requested) && **v <= *requested)
            .max()
            .copied()
    }
}
