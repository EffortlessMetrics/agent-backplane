// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_projection::selection::{ModelCandidate, ModelSelector, SelectionStrategy};

fn candidate(name: &str, model: &str) -> ModelCandidate {
    ModelCandidate {
        backend_name: name.to_string(),
        model_id: model.to_string(),
        estimated_latency_ms: None,
        estimated_cost_per_1k_tokens: None,
        fidelity_score: None,
        weight: 1.0,
    }
}

fn candidate_with_latency(name: &str, latency: u64) -> ModelCandidate {
    ModelCandidate {
        estimated_latency_ms: Some(latency),
        ..candidate(name, "m")
    }
}

fn candidate_with_cost(name: &str, cost: f64) -> ModelCandidate {
    ModelCandidate {
        estimated_cost_per_1k_tokens: Some(cost),
        ..candidate(name, "m")
    }
}

fn candidate_with_fidelity(name: &str, fidelity: f64) -> ModelCandidate {
    ModelCandidate {
        fidelity_score: Some(fidelity),
        ..candidate(name, "m")
    }
}

fn candidate_with_weight(name: &str, weight: f64) -> ModelCandidate {
    ModelCandidate {
        weight,
        ..candidate(name, "m")
    }
}

// ── LowestLatency ───────────────────────────────────────────────────────

#[test]
fn lowest_latency_selects_min() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            candidate_with_latency("slow", 500),
            candidate_with_latency("fast", 50),
            candidate_with_latency("mid", 200),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "fast");
}

#[test]
fn lowest_latency_empty_returns_none() {
    let sel = ModelSelector::new(SelectionStrategy::LowestLatency, vec![]);
    assert!(sel.select().is_none());
}

#[test]
fn lowest_latency_missing_metadata_sorts_last() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            candidate("no_latency", "m"),
            candidate_with_latency("has_latency", 100),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "has_latency");
}

// ── LowestCost ──────────────────────────────────────────────────────────

#[test]
fn lowest_cost_selects_cheapest() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            candidate_with_cost("expensive", 5.0),
            candidate_with_cost("cheap", 0.5),
            candidate_with_cost("mid", 2.0),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "cheap");
}

#[test]
fn lowest_cost_empty_returns_none() {
    let sel = ModelSelector::new(SelectionStrategy::LowestCost, vec![]);
    assert!(sel.select().is_none());
}

#[test]
fn lowest_cost_missing_metadata_sorts_last() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            candidate("no_cost", "m"),
            candidate_with_cost("has_cost", 1.0),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "has_cost");
}

// ── HighestFidelity ─────────────────────────────────────────────────────

#[test]
fn highest_fidelity_selects_best() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            candidate_with_fidelity("low", 0.3),
            candidate_with_fidelity("high", 0.95),
            candidate_with_fidelity("mid", 0.7),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "high");
}

#[test]
fn highest_fidelity_empty_returns_none() {
    let sel = ModelSelector::new(SelectionStrategy::HighestFidelity, vec![]);
    assert!(sel.select().is_none());
}

#[test]
fn highest_fidelity_missing_score_sorts_last() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            candidate("no_fidelity", "m"),
            candidate_with_fidelity("has_fidelity", 0.8),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "has_fidelity");
}

// ── RoundRobin ──────────────────────────────────────────────────────────

#[test]
fn round_robin_cycles_through_candidates() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![
            candidate("a", "m"),
            candidate("b", "m"),
            candidate("c", "m"),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "a");
    assert_eq!(sel.select().unwrap().backend_name, "b");
    assert_eq!(sel.select().unwrap().backend_name, "c");
}

#[test]
fn round_robin_wraps_around() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![candidate("a", "m"), candidate("b", "m")],
    );
    assert_eq!(sel.select().unwrap().backend_name, "a");
    assert_eq!(sel.select().unwrap().backend_name, "b");
    assert_eq!(sel.select().unwrap().backend_name, "a");
    assert_eq!(sel.select().unwrap().backend_name, "b");
}

#[test]
fn round_robin_empty_returns_none() {
    let sel = ModelSelector::new(SelectionStrategy::RoundRobin, vec![]);
    assert!(sel.select().is_none());
}

#[test]
fn round_robin_single_candidate() {
    let sel = ModelSelector::new(SelectionStrategy::RoundRobin, vec![candidate("only", "m")]);
    assert_eq!(sel.select().unwrap().backend_name, "only");
    assert_eq!(sel.select().unwrap().backend_name, "only");
}

// ── WeightedRandom ──────────────────────────────────────────────────────

#[test]
fn weighted_random_single_nonzero_always_selected() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            candidate_with_weight("zero_a", 0.0),
            candidate_with_weight("nonzero", 1.0),
            candidate_with_weight("zero_b", 0.0),
        ],
    );
    // Run multiple times — the only candidate with positive weight should be selected.
    for _ in 0..20 {
        assert_eq!(sel.select().unwrap().backend_name, "nonzero");
    }
}

#[test]
fn weighted_random_empty_returns_none() {
    let sel = ModelSelector::new(SelectionStrategy::WeightedRandom, vec![]);
    assert!(sel.select().is_none());
}

#[test]
fn weighted_random_all_zero_weight_returns_first() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            candidate_with_weight("a", 0.0),
            candidate_with_weight("b", 0.0),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "a");
}

#[test]
fn weighted_random_negative_weight_treated_as_zero() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            candidate_with_weight("negative", -5.0),
            candidate_with_weight("positive", 1.0),
        ],
    );
    for _ in 0..20 {
        assert_eq!(sel.select().unwrap().backend_name, "positive");
    }
}

// ── FallbackChain ───────────────────────────────────────────────────────

#[test]
fn fallback_chain_returns_first() {
    let sel = ModelSelector::new(
        SelectionStrategy::FallbackChain,
        vec![
            candidate("primary", "m"),
            candidate("secondary", "m"),
            candidate("tertiary", "m"),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "primary");
}

#[test]
fn fallback_chain_empty_returns_none() {
    let sel = ModelSelector::new(SelectionStrategy::FallbackChain, vec![]);
    assert!(sel.select().is_none());
}

// ── select_n ────────────────────────────────────────────────────────────

#[test]
fn select_n_returns_correct_count() {
    let sel = ModelSelector::new(
        SelectionStrategy::FallbackChain,
        vec![
            candidate("a", "m"),
            candidate("b", "m"),
            candidate("c", "m"),
        ],
    );
    assert_eq!(sel.select_n(2).len(), 2);
}

#[test]
fn select_n_zero_returns_empty() {
    let sel = ModelSelector::new(SelectionStrategy::FallbackChain, vec![candidate("a", "m")]);
    assert!(sel.select_n(0).is_empty());
}

#[test]
fn select_n_exceeding_candidates_returns_all() {
    let sel = ModelSelector::new(
        SelectionStrategy::FallbackChain,
        vec![candidate("a", "m"), candidate("b", "m")],
    );
    assert_eq!(sel.select_n(10).len(), 2);
}

#[test]
fn select_n_lowest_latency_ordering() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            candidate_with_latency("slow", 500),
            candidate_with_latency("fast", 50),
            candidate_with_latency("mid", 200),
        ],
    );
    let ranked: Vec<_> = sel
        .select_n(3)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(ranked, vec!["fast", "mid", "slow"]);
}

#[test]
fn select_n_highest_fidelity_ordering() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            candidate_with_fidelity("low", 0.3),
            candidate_with_fidelity("high", 0.95),
            candidate_with_fidelity("mid", 0.7),
        ],
    );
    let ranked: Vec<_> = sel
        .select_n(3)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(ranked, vec!["high", "mid", "low"]);
}

#[test]
fn select_n_lowest_cost_ordering() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            candidate_with_cost("expensive", 5.0),
            candidate_with_cost("cheap", 0.5),
            candidate_with_cost("mid", 2.0),
        ],
    );
    let ranked: Vec<_> = sel
        .select_n(3)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(ranked, vec!["cheap", "mid", "expensive"]);
}

#[test]
fn select_n_round_robin_starts_at_current() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![
            candidate("a", "m"),
            candidate("b", "m"),
            candidate("c", "m"),
        ],
    );
    // Advance counter by selecting once (consumes index 0).
    let _ = sel.select();
    // select_n should start at the current counter position (1).
    let ranked: Vec<_> = sel
        .select_n(3)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(ranked, vec!["b", "c", "a"]);
}

#[test]
fn select_n_weighted_random_orders_by_weight() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            candidate_with_weight("light", 1.0),
            candidate_with_weight("heavy", 10.0),
            candidate_with_weight("mid", 5.0),
        ],
    );
    let ranked: Vec<_> = sel
        .select_n(3)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(ranked, vec!["heavy", "mid", "light"]);
}

#[test]
fn select_n_empty_candidates() {
    let sel = ModelSelector::new(SelectionStrategy::LowestLatency, vec![]);
    assert!(sel.select_n(5).is_empty());
}

// ── Serde roundtrips ────────────────────────────────────────────────────

#[test]
fn serialization_roundtrip_strategy() {
    let strategy = SelectionStrategy::WeightedRandom;
    let json = serde_json::to_string(&strategy).unwrap();
    let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, strategy);
}

#[test]
fn serialization_roundtrip_candidate() {
    let c = candidate_with_fidelity("test_backend", 0.9);
    let json = serde_json::to_string(&c).unwrap();
    let back: ModelCandidate = serde_json::from_str(&json).unwrap();
    assert_eq!(back, c);
}

#[test]
fn serialization_roundtrip_selector() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![candidate_with_cost("a", 1.0)],
    );
    let json = serde_json::to_string(&sel).unwrap();
    let back: ModelSelector = serde_json::from_str(&json).unwrap();
    assert_eq!(back.strategy, sel.strategy);
    assert_eq!(back.candidates, sel.candidates);
}

// ── Clone ───────────────────────────────────────────────────────────────

#[test]
fn clone_preserves_counter_state() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![
            candidate("a", "m"),
            candidate("b", "m"),
            candidate("c", "m"),
        ],
    );
    let _ = sel.select(); // counter -> 1
    let cloned = sel.clone();
    // Clone should continue from where the original left off.
    assert_eq!(cloned.select().unwrap().backend_name, "b");
}

// ── Mixed / partial metadata ────────────────────────────────────────────

#[test]
fn mixed_metadata_lowest_latency_prefers_known() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            candidate("unknown", "m"),
            candidate_with_latency("known_slow", 999),
            candidate_with_latency("known_fast", 10),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "known_fast");
}

#[test]
fn mixed_metadata_highest_fidelity_prefers_known() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            candidate("unknown", "m"),
            candidate_with_fidelity("known", 0.5),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "known");
}

#[test]
fn all_strategies_return_none_on_empty() {
    let strategies = [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ];
    for strategy in strategies {
        let sel = ModelSelector::new(strategy, vec![]);
        assert!(
            sel.select().is_none(),
            "{strategy:?} should return None on empty"
        );
    }
}

#[test]
fn strategy_serde_snake_case() {
    let json = serde_json::to_string(&SelectionStrategy::LowestLatency).unwrap();
    assert_eq!(json, r#""lowest_latency""#);
    let json = serde_json::to_string(&SelectionStrategy::HighestFidelity).unwrap();
    assert_eq!(json, r#""highest_fidelity""#);
    let json = serde_json::to_string(&SelectionStrategy::RoundRobin).unwrap();
    assert_eq!(json, r#""round_robin""#);
    let json = serde_json::to_string(&SelectionStrategy::WeightedRandom).unwrap();
    assert_eq!(json, r#""weighted_random""#);
    let json = serde_json::to_string(&SelectionStrategy::FallbackChain).unwrap();
    assert_eq!(json, r#""fallback_chain""#);
}
