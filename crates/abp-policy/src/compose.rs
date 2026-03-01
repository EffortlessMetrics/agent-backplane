// SPDX-License-Identifier: MIT OR Apache-2.0
//! Composable policy features: sets, precedence strategies, and validation.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// PolicyDecision
// ---------------------------------------------------------------------------

/// Outcome of a composed policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyDecision {
    /// The action is explicitly permitted.
    Allow {
        /// Explanation of why it was allowed.
        reason: String,
    },
    /// The action is explicitly denied.
    Deny {
        /// Explanation of why it was denied.
        reason: String,
    },
    /// No applicable rule matched.
    Abstain,
}

impl PolicyDecision {
    /// Returns `true` when the decision is [`PolicyDecision::Allow`].
    #[must_use]
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    /// Returns `true` when the decision is [`PolicyDecision::Deny`].
    #[must_use]
    pub fn is_deny(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }

    /// Returns `true` when the decision is [`PolicyDecision::Abstain`].
    #[must_use]
    pub fn is_abstain(&self) -> bool {
        matches!(self, Self::Abstain)
    }
}

// ---------------------------------------------------------------------------
// PolicyPrecedence
// ---------------------------------------------------------------------------

/// Strategy for combining decisions from multiple policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyPrecedence {
    /// Any deny wins — the most restrictive (and default) strategy.
    #[default]
    DenyOverrides,
    /// Any allow wins — the most permissive strategy.
    AllowOverrides,
    /// First non-abstain decision wins.
    FirstApplicable,
}

// ---------------------------------------------------------------------------
// PolicySet
// ---------------------------------------------------------------------------

/// Named collection of [`PolicyProfile`] entries that can be merged.
#[derive(Debug, Clone)]
pub struct PolicySet {
    name: String,
    profiles: Vec<PolicyProfile>,
}

impl PolicySet {
    /// Create an empty policy set with the given name.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            profiles: Vec::new(),
        }
    }

    /// Append a profile to this set.
    pub fn add(&mut self, profile: PolicyProfile) {
        self.profiles.push(profile);
    }

    /// Merge all profiles into one using deny-wins semantics.
    ///
    /// All deny/disallow lists are unioned; all allow lists are unioned.
    #[must_use]
    pub fn merge(&self) -> PolicyProfile {
        let mut merged = PolicyProfile::default();
        for p in &self.profiles {
            merged.allowed_tools.extend(p.allowed_tools.iter().cloned());
            merged
                .disallowed_tools
                .extend(p.disallowed_tools.iter().cloned());
            merged.deny_read.extend(p.deny_read.iter().cloned());
            merged.deny_write.extend(p.deny_write.iter().cloned());
            merged.allow_network.extend(p.allow_network.iter().cloned());
            merged.deny_network.extend(p.deny_network.iter().cloned());
            merged
                .require_approval_for
                .extend(p.require_approval_for.iter().cloned());
        }
        // Deduplicate
        merged.allowed_tools.sort();
        merged.allowed_tools.dedup();
        merged.disallowed_tools.sort();
        merged.disallowed_tools.dedup();
        merged.deny_read.sort();
        merged.deny_read.dedup();
        merged.deny_write.sort();
        merged.deny_write.dedup();
        merged.allow_network.sort();
        merged.allow_network.dedup();
        merged.deny_network.sort();
        merged.deny_network.dedup();
        merged.require_approval_for.sort();
        merged.require_approval_for.dedup();
        merged
    }

    /// The name of this policy set.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

// ---------------------------------------------------------------------------
// ComposedEngine (internal per-profile engine)
// ---------------------------------------------------------------------------

/// Per-profile compiled globs used inside [`ComposedEngine`].
#[derive(Debug, Clone)]
struct CompiledProfile {
    tool_rules: IncludeExcludeGlobs,
    deny_read: IncludeExcludeGlobs,
    deny_write: IncludeExcludeGlobs,
}

impl CompiledProfile {
    fn compile(p: &PolicyProfile) -> Result<Self> {
        let no_include: &[String] = &[];
        Ok(Self {
            tool_rules: IncludeExcludeGlobs::new(&p.allowed_tools, &p.disallowed_tools)
                .context("compile tool globs")?,
            deny_read: IncludeExcludeGlobs::new(no_include, &p.deny_read)
                .context("compile deny_read globs")?,
            deny_write: IncludeExcludeGlobs::new(no_include, &p.deny_write)
                .context("compile deny_write globs")?,
        })
    }

    fn check_tool(&self, tool: &str) -> PolicyDecision {
        use abp_glob::MatchDecision;
        match self.tool_rules.decide_str(tool) {
            MatchDecision::Allowed => PolicyDecision::Allow {
                reason: format!("tool '{tool}' permitted"),
            },
            MatchDecision::DeniedByExclude => PolicyDecision::Deny {
                reason: format!("tool '{tool}' is disallowed"),
            },
            MatchDecision::DeniedByMissingInclude => PolicyDecision::Deny {
                reason: format!("tool '{tool}' not in allowlist"),
            },
        }
    }

    fn check_read(&self, path: &str) -> PolicyDecision {
        if !self.deny_read.decide_path(Path::new(path)).is_allowed() {
            return PolicyDecision::Deny {
                reason: format!("read denied for '{path}'"),
            };
        }
        PolicyDecision::Allow {
            reason: format!("read permitted for '{path}'"),
        }
    }

    fn check_write(&self, path: &str) -> PolicyDecision {
        if !self.deny_write.decide_path(Path::new(path)).is_allowed() {
            return PolicyDecision::Deny {
                reason: format!("write denied for '{path}'"),
            };
        }
        PolicyDecision::Allow {
            reason: format!("write permitted for '{path}'"),
        }
    }
}

/// Policy engine that evaluates multiple [`PolicyProfile`]s according to a
/// [`PolicyPrecedence`] strategy.
#[derive(Debug, Clone)]
pub struct ComposedEngine {
    engines: Vec<CompiledProfile>,
    precedence: PolicyPrecedence,
}

impl ComposedEngine {
    /// Compile multiple profiles into a composed engine.
    ///
    /// # Errors
    ///
    /// Returns an error if any glob pattern is invalid.
    pub fn new(policies: Vec<PolicyProfile>, precedence: PolicyPrecedence) -> Result<Self> {
        let engines = policies
            .iter()
            .map(CompiledProfile::compile)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            engines,
            precedence,
        })
    }

    /// Check whether `tool` is permitted under the composed policy.
    #[must_use]
    pub fn check_tool(&self, tool: &str) -> PolicyDecision {
        self.combine(|e| e.check_tool(tool))
    }

    /// Check whether reading `path` is permitted under the composed policy.
    #[must_use]
    pub fn check_read(&self, path: &str) -> PolicyDecision {
        self.combine(|e| e.check_read(path))
    }

    /// Check whether writing `path` is permitted under the composed policy.
    #[must_use]
    pub fn check_write(&self, path: &str) -> PolicyDecision {
        self.combine(|e| e.check_write(path))
    }

    fn combine<F>(&self, mut f: F) -> PolicyDecision
    where
        F: FnMut(&CompiledProfile) -> PolicyDecision,
    {
        if self.engines.is_empty() {
            return PolicyDecision::Abstain;
        }

        let decisions: Vec<PolicyDecision> = self.engines.iter().map(&mut f).collect();

        match self.precedence {
            PolicyPrecedence::DenyOverrides => {
                // Any deny wins
                for d in &decisions {
                    if d.is_deny() {
                        return d.clone();
                    }
                }
                // Return first allow, or abstain
                for d in decisions {
                    if d.is_allow() {
                        return d;
                    }
                }
                PolicyDecision::Abstain
            }
            PolicyPrecedence::AllowOverrides => {
                // Any allow wins
                for d in &decisions {
                    if d.is_allow() {
                        return d.clone();
                    }
                }
                // Return first deny, or abstain
                for d in decisions {
                    if d.is_deny() {
                        return d;
                    }
                }
                PolicyDecision::Abstain
            }
            PolicyPrecedence::FirstApplicable => {
                // First non-abstain wins
                for d in decisions {
                    if !d.is_abstain() {
                        return d;
                    }
                }
                PolicyDecision::Abstain
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PolicyWarning / PolicyValidator
// ---------------------------------------------------------------------------

/// A warning produced by [`PolicyValidator`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyWarning {
    /// Machine-readable warning kind.
    pub kind: WarningKind,
    /// Human-readable description.
    pub message: String,
}

/// Categories of policy validation warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// A pattern appears in both allow and deny lists.
    OverlappingAllowDeny,
    /// A rule can never match because a broader deny rule shadows it.
    UnreachableRule,
    /// A glob pattern is empty (zero-length string).
    EmptyGlob,
}

/// Validates a [`PolicyProfile`] and reports potential issues.
pub struct PolicyValidator;

impl PolicyValidator {
    /// Analyse `profile` and return any warnings found.
    #[must_use]
    pub fn validate(profile: &PolicyProfile) -> Vec<PolicyWarning> {
        let mut warnings = Vec::new();

        // Check for empty globs in every list.
        Self::check_empty_globs(&profile.allowed_tools, "allowed_tools", &mut warnings);
        Self::check_empty_globs(&profile.disallowed_tools, "disallowed_tools", &mut warnings);
        Self::check_empty_globs(&profile.deny_read, "deny_read", &mut warnings);
        Self::check_empty_globs(&profile.deny_write, "deny_write", &mut warnings);
        Self::check_empty_globs(&profile.allow_network, "allow_network", &mut warnings);
        Self::check_empty_globs(&profile.deny_network, "deny_network", &mut warnings);

        // Overlapping allow/deny in tools.
        Self::check_overlaps(
            &profile.allowed_tools,
            &profile.disallowed_tools,
            "tool",
            &mut warnings,
        );

        // Overlapping allow/deny in network.
        Self::check_overlaps(
            &profile.allow_network,
            &profile.deny_network,
            "network",
            &mut warnings,
        );

        // Unreachable: allowed_tools entries that are also disallowed.
        // A wildcard deny makes specific allows unreachable.
        if profile.disallowed_tools.contains(&"*".to_string()) {
            for tool in &profile.allowed_tools {
                if tool != "*" {
                    warnings.push(PolicyWarning {
                        kind: WarningKind::UnreachableRule,
                        message: format!(
                            "allowed tool '{tool}' is unreachable because disallowed_tools contains '*'"
                        ),
                    });
                }
            }
        }

        // Unreachable: deny_read contains '**' making specific allows pointless.
        if profile.deny_read.iter().any(|p| p == "**" || p == "**/*") {
            warnings.push(PolicyWarning {
                kind: WarningKind::UnreachableRule,
                message: "deny_read contains a catch-all glob; all reads will be denied"
                    .to_string(),
            });
        }
        if profile.deny_write.iter().any(|p| p == "**" || p == "**/*") {
            warnings.push(PolicyWarning {
                kind: WarningKind::UnreachableRule,
                message: "deny_write contains a catch-all glob; all writes will be denied"
                    .to_string(),
            });
        }

        warnings
    }

    fn check_empty_globs(patterns: &[String], field: &str, warnings: &mut Vec<PolicyWarning>) {
        for p in patterns {
            if p.is_empty() {
                warnings.push(PolicyWarning {
                    kind: WarningKind::EmptyGlob,
                    message: format!("empty glob in '{field}'"),
                });
            }
        }
    }

    fn check_overlaps(
        allow: &[String],
        deny: &[String],
        category: &str,
        warnings: &mut Vec<PolicyWarning>,
    ) {
        for a in allow {
            for d in deny {
                if a == d {
                    warnings.push(PolicyWarning {
                        kind: WarningKind::OverlappingAllowDeny,
                        message: format!(
                            "{category} pattern '{a}' appears in both allow and deny lists"
                        ),
                    });
                }
            }
        }
    }
}
