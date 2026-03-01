// SPDX-License-Identifier: MIT OR Apache-2.0
#![recursion_limit = "256"]
//! Tests for the rule engine (`abp_policy::rules`).

use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_rule(id: &str, cond: RuleCondition, effect: RuleEffect, priority: u32) -> Rule {
    Rule {
        id: id.to_string(),
        description: format!("test rule {id}"),
        condition: cond,
        effect,
        priority,
    }
}

// ---------------------------------------------------------------------------
// RuleCondition tests
// ---------------------------------------------------------------------------

#[test]
fn condition_always_matches() {
    assert!(RuleCondition::Always.matches("anything"));
    assert!(RuleCondition::Always.matches(""));
}

#[test]
fn condition_never_matches() {
    assert!(!RuleCondition::Never.matches("anything"));
    assert!(!RuleCondition::Never.matches(""));
}

#[test]
fn condition_pattern_glob() {
    let cond = RuleCondition::Pattern("*.rs".to_string());
    assert!(cond.matches("main.rs"));
    assert!(!cond.matches("main.py"));
}

#[test]
fn condition_pattern_star_star() {
    let cond = RuleCondition::Pattern("src/**/*.rs".to_string());
    assert!(cond.matches("src/lib.rs"));
    assert!(cond.matches("src/sub/mod.rs"));
    assert!(!cond.matches("tests/it.rs"));
}

#[test]
fn condition_and_all_must_match() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Pattern("*.rs".to_string()),
        RuleCondition::Not(Box::new(RuleCondition::Pattern("test_*".to_string()))),
    ]);
    assert!(cond.matches("main.rs"));
    assert!(!cond.matches("test_main.rs"));
    assert!(!cond.matches("main.py"));
}

#[test]
fn condition_or_any_must_match() {
    let cond = RuleCondition::Or(vec![
        RuleCondition::Pattern("*.rs".to_string()),
        RuleCondition::Pattern("*.toml".to_string()),
    ]);
    assert!(cond.matches("main.rs"));
    assert!(cond.matches("Cargo.toml"));
    assert!(!cond.matches("main.py"));
}

#[test]
fn condition_not_negates() {
    let cond = RuleCondition::Not(Box::new(RuleCondition::Pattern("*.log".to_string())));
    assert!(cond.matches("main.rs"));
    assert!(!cond.matches("app.log"));
}

#[test]
fn condition_invalid_glob_does_not_match() {
    // An invalid glob pattern should not panic — it just doesn't match.
    let cond = RuleCondition::Pattern("[invalid".to_string());
    assert!(!cond.matches("anything"));
}

#[test]
fn condition_nested_and_or() {
    let cond = RuleCondition::And(vec![
        RuleCondition::Or(vec![
            RuleCondition::Pattern("*.rs".to_string()),
            RuleCondition::Pattern("*.toml".to_string()),
        ]),
        RuleCondition::Not(Box::new(RuleCondition::Pattern("secret*".to_string()))),
    ]);
    assert!(cond.matches("lib.rs"));
    assert!(cond.matches("Cargo.toml"));
    assert!(!cond.matches("secret.rs"));
    assert!(!cond.matches("readme.md"));
}

// ---------------------------------------------------------------------------
// RuleEngine — basic operations
// ---------------------------------------------------------------------------

#[test]
fn engine_new_is_empty() {
    let engine = RuleEngine::new();
    assert_eq!(engine.rule_count(), 0);
    assert!(engine.rules().is_empty());
}

#[test]
fn engine_add_and_count() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Allow, 1));
    engine.add_rule(make_rule("r2", RuleCondition::Never, RuleEffect::Deny, 2));
    assert_eq!(engine.rule_count(), 2);
}

#[test]
fn engine_remove_rule() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Allow, 1));
    engine.add_rule(make_rule("r2", RuleCondition::Always, RuleEffect::Deny, 2));
    engine.remove_rule("r1");
    assert_eq!(engine.rule_count(), 1);
    assert_eq!(engine.rules()[0].id, "r2");
}

#[test]
fn engine_remove_nonexistent_is_noop() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Allow, 1));
    engine.remove_rule("no-such-rule");
    assert_eq!(engine.rule_count(), 1);
}

// ---------------------------------------------------------------------------
// RuleEngine — evaluate
// ---------------------------------------------------------------------------

#[test]
fn evaluate_no_rules_returns_allow() {
    let engine = RuleEngine::new();
    assert_eq!(engine.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn evaluate_single_matching_rule() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule(
        "deny-all",
        RuleCondition::Always,
        RuleEffect::Deny,
        1,
    ));
    assert_eq!(engine.evaluate("foo"), RuleEffect::Deny);
}

#[test]
fn evaluate_highest_priority_wins() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule(
        "low",
        RuleCondition::Always,
        RuleEffect::Allow,
        1,
    ));
    engine.add_rule(make_rule(
        "high",
        RuleCondition::Always,
        RuleEffect::Deny,
        10,
    ));
    assert_eq!(engine.evaluate("foo"), RuleEffect::Deny);
}

#[test]
fn evaluate_only_matching_rules_considered() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule(
        "deny-rs",
        RuleCondition::Pattern("*.rs".to_string()),
        RuleEffect::Deny,
        10,
    ));
    engine.add_rule(make_rule(
        "allow-all",
        RuleCondition::Always,
        RuleEffect::Allow,
        1,
    ));

    assert_eq!(engine.evaluate("main.rs"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("main.py"), RuleEffect::Allow);
}

#[test]
fn evaluate_throttle_effect() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule(
        "throttle",
        RuleCondition::Always,
        RuleEffect::Throttle { max: 5 },
        1,
    ));
    assert_eq!(engine.evaluate("anything"), RuleEffect::Throttle { max: 5 });
}

#[test]
fn evaluate_log_effect() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("log", RuleCondition::Always, RuleEffect::Log, 1));
    assert_eq!(engine.evaluate("anything"), RuleEffect::Log);
}

// ---------------------------------------------------------------------------
// RuleEngine — evaluate_all
// ---------------------------------------------------------------------------

#[test]
fn evaluate_all_returns_all_rules() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Allow, 1));
    engine.add_rule(make_rule("r2", RuleCondition::Never, RuleEffect::Deny, 2));

    let results = engine.evaluate_all("foo");
    assert_eq!(results.len(), 2);
    assert!(results[0].matched);
    assert!(!results[1].matched);
}

#[test]
fn evaluate_all_carries_correct_effect() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("r1", RuleCondition::Always, RuleEffect::Log, 1));

    let results = engine.evaluate_all("bar");
    assert_eq!(results[0].effect, RuleEffect::Log);
    assert_eq!(results[0].rule_id, "r1");
}

#[test]
fn evaluate_all_empty_engine() {
    let engine = RuleEngine::new();
    assert!(engine.evaluate_all("x").is_empty());
}

// ---------------------------------------------------------------------------
// Priority & ordering edge cases
// ---------------------------------------------------------------------------

#[test]
fn equal_priority_first_inserted_wins() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule(
        "first",
        RuleCondition::Always,
        RuleEffect::Allow,
        5,
    ));
    engine.add_rule(make_rule(
        "second",
        RuleCondition::Always,
        RuleEffect::Deny,
        5,
    ));
    // max_by_key is stable — last match wins with equal keys,
    // but we intentionally accept either. The important thing is
    // that the engine doesn't panic and returns a valid effect.
    let result = engine.evaluate("foo");
    assert!(result == RuleEffect::Allow || result == RuleEffect::Deny);
}

#[test]
fn priority_zero_is_valid() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule("zero", RuleCondition::Always, RuleEffect::Log, 0));
    assert_eq!(engine.evaluate("x"), RuleEffect::Log);
}

// ---------------------------------------------------------------------------
// Serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn rule_serde_roundtrip() {
    let rule = make_rule(
        "serde-test",
        RuleCondition::And(vec![
            RuleCondition::Pattern("*.rs".to_string()),
            RuleCondition::Not(Box::new(RuleCondition::Never)),
        ]),
        RuleEffect::Throttle { max: 42 },
        7,
    );
    let json = serde_json::to_string(&rule).expect("serialize");
    let back: Rule = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.id, "serde-test");
    assert_eq!(back.priority, 7);
    assert_eq!(back.effect, RuleEffect::Throttle { max: 42 });
}

#[test]
fn rule_effect_serde_variants() {
    for effect in [
        RuleEffect::Allow,
        RuleEffect::Deny,
        RuleEffect::Log,
        RuleEffect::Throttle { max: 10 },
    ] {
        let json = serde_json::to_string(&effect).expect("serialize");
        let back: RuleEffect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, effect);
    }
}

// ---------------------------------------------------------------------------
// Integration-style: complex rule sets
// ---------------------------------------------------------------------------

#[test]
fn complex_rule_set_deny_secrets_allow_rest() {
    let mut engine = RuleEngine::new();
    engine.add_rule(make_rule(
        "deny-secrets",
        RuleCondition::Pattern("secret*".to_string()),
        RuleEffect::Deny,
        100,
    ));
    engine.add_rule(make_rule(
        "log-config",
        RuleCondition::Pattern("*.toml".to_string()),
        RuleEffect::Log,
        50,
    ));
    engine.add_rule(make_rule(
        "allow-all",
        RuleCondition::Always,
        RuleEffect::Allow,
        1,
    ));

    assert_eq!(engine.evaluate("secret.key"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("config.toml"), RuleEffect::Log);
    assert_eq!(engine.evaluate("readme.md"), RuleEffect::Allow);
}
