// SPDX-License-Identifier: MIT OR Apache-2.0
//! Composed policy evaluation over multiple [`PolicyEngine`] instances.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::PolicyEngine;

/// Strategy for combining decisions from multiple named policy engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompositionStrategy {
    /// Every engine must allow — a single deny vetoes the action.
    #[default]
    AllMustAllow,
    /// At least one engine must allow — a single allow is sufficient.
    AnyMustAllow,
    /// The first engine that produces a definitive answer wins.
    FirstMatch,
}

/// Outcome of a composed policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComposedResult {
    /// The action is allowed.
    Allowed {
        /// Name of the engine that permitted the action.
        by: String,
    },
    /// The action is denied.
    Denied {
        /// Name of the engine that denied the action.
        by: String,
        /// Human-readable explanation.
        reason: String,
    },
}

impl ComposedResult {
    /// Returns `true` when the result is [`ComposedResult::Allowed`].
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }

    /// Returns `true` when the result is [`ComposedResult::Denied`].
    #[must_use]
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }
}

/// A named policy engine entry inside a [`ComposedPolicy`].
#[derive(Debug, Clone)]
struct NamedEngine {
    name: String,
    engine: PolicyEngine,
}

/// Chains multiple [`PolicyEngine`]s under a single [`CompositionStrategy`].
///
/// Each engine is registered with a human-readable name so that the
/// [`ComposedResult`] can attribute the decision.
#[derive(Debug, Clone)]
pub struct ComposedPolicy {
    engines: Vec<NamedEngine>,
    strategy: CompositionStrategy,
}

impl ComposedPolicy {
    /// Create a new composed policy with the given strategy and no engines.
    #[must_use]
    pub fn new(strategy: CompositionStrategy) -> Self {
        Self {
            engines: Vec::new(),
            strategy,
        }
    }

    /// Register a named policy engine.
    pub fn add_policy(&mut self, name: &str, engine: PolicyEngine) {
        self.engines.push(NamedEngine {
            name: name.to_string(),
            engine,
        });
    }

    /// The composition strategy in use.
    #[must_use]
    pub fn strategy(&self) -> CompositionStrategy {
        self.strategy
    }

    /// Number of registered engines.
    #[must_use]
    pub fn policy_count(&self) -> usize {
        self.engines.len()
    }

    /// Evaluate whether `tool` is permitted across all engines.
    #[must_use]
    pub fn evaluate_tool(&self, tool: &str) -> ComposedResult {
        self.combine(|e| {
            let d = e.can_use_tool(tool);
            (d.allowed, d.reason.unwrap_or_default())
        })
    }

    /// Evaluate whether reading `path` is permitted across all engines.
    #[must_use]
    pub fn evaluate_read(&self, path: &str) -> ComposedResult {
        self.combine(|e| {
            let d = e.can_read_path(Path::new(path));
            (d.allowed, d.reason.unwrap_or_default())
        })
    }

    /// Evaluate whether writing `path` is permitted across all engines.
    #[must_use]
    pub fn evaluate_write(&self, path: &str) -> ComposedResult {
        self.combine(|e| {
            let d = e.can_write_path(Path::new(path));
            (d.allowed, d.reason.unwrap_or_default())
        })
    }

    /// Internal combiner that applies the current strategy.
    fn combine<F>(&self, mut check: F) -> ComposedResult
    where
        F: FnMut(&PolicyEngine) -> (bool, String),
    {
        if self.engines.is_empty() {
            return ComposedResult::Allowed {
                by: "<empty>".to_string(),
            };
        }

        match self.strategy {
            CompositionStrategy::AllMustAllow => {
                // Every engine must allow; first deny wins.
                let mut last_allow_name = String::new();
                for ne in &self.engines {
                    let (allowed, reason) = check(&ne.engine);
                    if !allowed {
                        return ComposedResult::Denied {
                            by: ne.name.clone(),
                            reason,
                        };
                    }
                    last_allow_name.clone_from(&ne.name);
                }
                ComposedResult::Allowed {
                    by: last_allow_name,
                }
            }
            CompositionStrategy::AnyMustAllow => {
                // At least one allow suffices; collect denials in case all deny.
                let mut last_deny = None;
                for ne in &self.engines {
                    let (allowed, reason) = check(&ne.engine);
                    if allowed {
                        return ComposedResult::Allowed {
                            by: ne.name.clone(),
                        };
                    }
                    last_deny = Some((ne.name.clone(), reason));
                }
                match last_deny {
                    Some((by, reason)) => ComposedResult::Denied { by, reason },
                    None => ComposedResult::Allowed {
                        by: "<empty>".to_string(),
                    },
                }
            }
            CompositionStrategy::FirstMatch => {
                // First engine with a definitive result wins.
                let (allowed, reason) = check(&self.engines[0].engine);
                if allowed {
                    ComposedResult::Allowed {
                        by: self.engines[0].name.clone(),
                    }
                } else {
                    ComposedResult::Denied {
                        by: self.engines[0].name.clone(),
                        reason,
                    }
                }
            }
        }
    }
}
