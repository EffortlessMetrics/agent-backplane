// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-rule-engine
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Rule-based access control engine with prioritised, composable conditions.

use globset::Glob;
use serde::{Deserialize, Serialize};

/// A composable predicate that decides whether a rule applies to a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCondition {
    /// Matches every resource.
    Always,
    /// Matches no resource.
    Never,
    /// Matches resources whose name satisfies the glob pattern.
    Pattern(String),
    /// All child conditions must match.
    And(Vec<RuleCondition>),
    /// At least one child condition must match.
    Or(Vec<RuleCondition>),
    /// Negates the inner condition.
    Not(Box<RuleCondition>),
}

impl RuleCondition {
    /// Evaluate this condition against `resource`.
    #[must_use]
    pub fn matches(&self, resource: &str) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Pattern(pat) => Glob::new(pat)
                .ok()
                .is_some_and(|g| g.compile_matcher().is_match(resource)),
            Self::And(conds) => conds.iter().all(|c| c.matches(resource)),
            Self::Or(conds) => conds.iter().any(|c| c.matches(resource)),
            Self::Not(inner) => !inner.matches(resource),
        }
    }
}

/// The action taken when a rule matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleEffect {
    /// Permit the action.
    Allow,
    /// Deny the action.
    Deny,
    /// Allow but emit a log entry.
    Log,
    /// Allow but apply a rate limit.
    Throttle {
        /// Maximum number of allowed invocations.
        max: u32,
    },
}

/// A single access-control rule with a condition, effect, and priority.
///
/// Rules are evaluated in **descending** priority order (higher number wins).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Unique identifier for the rule.
    pub id: String,
    /// Human-readable description of what the rule does.
    pub description: String,
    /// Condition that must be met for this rule to fire.
    pub condition: RuleCondition,
    /// Effect applied when the condition matches.
    pub effect: RuleEffect,
    /// Higher priority rules are evaluated first and take precedence.
    pub priority: u32,
}

/// Result of evaluating a single rule against a resource.
#[derive(Debug, Clone)]
pub struct RuleEvaluation {
    /// The id of the rule that was evaluated.
    pub rule_id: String,
    /// Whether the rule's condition matched the resource.
    pub matched: bool,
    /// The effect that the rule would apply (regardless of match).
    pub effect: RuleEffect,
}

/// Engine that evaluates an ordered set of [`Rule`]s against a resource.
///
/// When multiple rules match, the one with the **highest priority** wins.
/// Ties are broken by insertion order (earlier rule wins).
#[derive(Debug, Clone, Default)]
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    /// Create an empty rule engine.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a rule to the engine.
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Evaluate `resource` and return the effect of the highest-priority
    /// matching rule. Returns [`RuleEffect::Allow`] when no rule matches.
    #[must_use]
    pub fn evaluate(&self, resource: &str) -> RuleEffect {
        self.rules
            .iter()
            .filter(|r| r.condition.matches(resource))
            .max_by_key(|r| r.priority)
            .map_or(RuleEffect::Allow, |r| r.effect.clone())
    }

    /// Evaluate every rule against `resource` and return all results.
    #[must_use]
    pub fn evaluate_all(&self, resource: &str) -> Vec<RuleEvaluation> {
        self.rules
            .iter()
            .map(|r| RuleEvaluation {
                rule_id: r.id.clone(),
                matched: r.condition.matches(resource),
                effect: r.effect.clone(),
            })
            .collect()
    }

    /// Borrow the current rule list.
    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Remove a rule by its id. Does nothing if no such rule exists.
    pub fn remove_rule(&mut self, id: &str) {
        self.rules.retain(|r| r.id != id);
    }

    /// Number of rules currently registered.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}
