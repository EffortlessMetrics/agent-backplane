// SPDX-License-Identifier: MIT OR Apache-2.0
//! Advanced capability negotiation between work-order requirements and backend manifests.

use crate::{Capability, CapabilityManifest, SupportLevel};

/// Numeric rank for [`SupportLevel`] used during negotiation comparisons.
fn support_rank(level: &SupportLevel) -> u8 {
    match level {
        SupportLevel::Native => 3,
        SupportLevel::Emulated => 2,
        SupportLevel::Restricted { .. } => 1,
        SupportLevel::Unsupported => 0,
    }
}

/// A request describing what capabilities a caller needs from a backend.
#[derive(Debug, Clone)]
pub struct NegotiationRequest {
    /// Capabilities that **must** be present at or above `minimum_support`.
    pub required: Vec<Capability>,
    /// Capabilities that are nice-to-have but not mandatory.
    pub preferred: Vec<Capability>,
    /// The lowest [`SupportLevel`] that counts as "satisfied".
    pub minimum_support: SupportLevel,
}

/// The outcome of negotiating a [`NegotiationRequest`] against a [`CapabilityManifest`].
#[derive(Debug, Clone)]
pub struct NegotiationResult {
    /// Required capabilities that the manifest satisfies.
    pub satisfied: Vec<Capability>,
    /// Required capabilities the manifest does **not** satisfy.
    pub unsatisfied: Vec<Capability>,
    /// Preferred capabilities the manifest satisfies (extra value).
    pub bonus: Vec<Capability>,
    /// `true` when every required capability is satisfied.
    pub is_compatible: bool,
}

/// Stateless negotiator that matches requests against manifests.
pub struct CapabilityNegotiator;

impl CapabilityNegotiator {
    /// Check a single manifest against a request.
    #[must_use]
    pub fn negotiate(
        request: &NegotiationRequest,
        manifest: &CapabilityManifest,
    ) -> NegotiationResult {
        let min_rank = support_rank(&request.minimum_support);

        let mut satisfied = Vec::new();
        let mut unsatisfied = Vec::new();

        for cap in &request.required {
            let meets = manifest
                .get(cap)
                .map_or(false, |level| support_rank(level) >= min_rank);
            if meets {
                satisfied.push(cap.clone());
            } else {
                unsatisfied.push(cap.clone());
            }
        }

        let bonus: Vec<Capability> = request
            .preferred
            .iter()
            .filter(|cap| {
                manifest
                    .get(cap)
                    .map_or(false, |level| support_rank(level) >= min_rank)
            })
            .cloned()
            .collect();

        let is_compatible = unsatisfied.is_empty();

        NegotiationResult {
            satisfied,
            unsatisfied,
            bonus,
            is_compatible,
        }
    }

    /// Pick the best manifest from a named set.
    ///
    /// Returns `None` when no manifest is compatible. Among compatible
    /// manifests the one with the highest total score
    /// (`satisfied.len() + bonus.len()`) wins; ties are broken by name order.
    #[must_use]
    pub fn best_match(
        request: &NegotiationRequest,
        manifests: &[(&str, CapabilityManifest)],
    ) -> Option<(String, NegotiationResult)> {
        manifests
            .iter()
            .map(|(name, manifest)| {
                let result = Self::negotiate(request, manifest);
                (name.to_string(), result)
            })
            .filter(|(_, result)| result.is_compatible)
            .max_by(|(name_a, a), (name_b, b)| {
                let score_a = a.satisfied.len() + a.bonus.len();
                let score_b = b.satisfied.len() + b.bonus.len();
                score_a
                    .cmp(&score_b)
                    .then_with(|| name_b.cmp(name_a)) // deterministic tie-break
            })
    }
}

/// Describes the difference between two [`CapabilityManifest`]s.
#[derive(Debug, Clone)]
pub struct CapabilityDiff {
    /// Capabilities present in `new` but absent from `old`.
    pub added: Vec<Capability>,
    /// Capabilities present in `old` but absent from `new`.
    pub removed: Vec<Capability>,
    /// Capabilities whose support level increased (`old_level`, `new_level`).
    pub upgraded: Vec<(Capability, SupportLevel, SupportLevel)>,
    /// Capabilities whose support level decreased (`old_level`, `new_level`).
    pub downgraded: Vec<(Capability, SupportLevel, SupportLevel)>,
}

impl CapabilityDiff {
    /// Compute the diff from `old` to `new`.
    #[must_use]
    pub fn diff(old: &CapabilityManifest, new: &CapabilityManifest) -> Self {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut upgraded = Vec::new();
        let mut downgraded = Vec::new();

        for (cap, new_level) in new {
            match old.get(cap) {
                None => added.push(cap.clone()),
                Some(old_level) => {
                    let old_r = support_rank(old_level);
                    let new_r = support_rank(new_level);
                    if new_r > old_r {
                        upgraded.push((cap.clone(), old_level.clone(), new_level.clone()));
                    } else if new_r < old_r {
                        downgraded.push((cap.clone(), old_level.clone(), new_level.clone()));
                    }
                }
            }
        }

        for cap in old.keys() {
            if !new.contains_key(cap) {
                removed.push(cap.clone());
            }
        }

        Self {
            added,
            removed,
            upgraded,
            downgraded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(pairs: &[(Capability, SupportLevel)]) -> CapabilityManifest {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn support_rank_ordering() {
        assert!(support_rank(&SupportLevel::Native) > support_rank(&SupportLevel::Emulated));
        assert!(support_rank(&SupportLevel::Emulated) > support_rank(&SupportLevel::Restricted { reason: String::new() }));
        assert!(support_rank(&SupportLevel::Restricted { reason: String::new() }) > support_rank(&SupportLevel::Unsupported));
    }
}
