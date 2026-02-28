// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `selector` module.

use abp_core::Capability;
use abp_integrations::selector::{
    BackendCandidate, BackendSelector, SelectionResult, SelectionStrategy,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn candidate(name: &str, caps: &[Capability], priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.to_string(),
        capabilities: caps.to_vec(),
        priority,
        enabled: true,
        metadata: BTreeMap::new(),
    }
}

fn disabled_candidate(name: &str, caps: &[Capability], priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.to_string(),
        capabilities: caps.to_vec(),
        priority,
        enabled: false,
        metadata: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// SelectionStrategy basics
// ---------------------------------------------------------------------------

#[test]
fn strategy_serialization_roundtrip() {
    let strategies = vec![
        SelectionStrategy::FirstMatch,
        SelectionStrategy::BestFit,
        SelectionStrategy::LeastLoaded,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::Priority,
    ];
    for s in &strategies {
        let json = serde_json::to_string(s).unwrap();
        let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{back:?}"), format!("{s:?}"));
    }
}

// ---------------------------------------------------------------------------
// BackendCandidate
// ---------------------------------------------------------------------------

#[test]
fn candidate_serde_roundtrip() {
    let c = candidate("mock", &[Capability::Streaming, Capability::ToolRead], 1);
    let json = serde_json::to_string(&c).unwrap();
    let back: BackendCandidate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "mock");
    assert_eq!(back.capabilities.len(), 2);
}

#[test]
fn candidate_metadata() {
    let mut c = candidate("meta", &[], 0);
    c.metadata.insert("region".into(), "us-east-1".into());
    assert_eq!(c.metadata.get("region").unwrap(), "us-east-1");
}

// ---------------------------------------------------------------------------
// BackendSelector — empty
// ---------------------------------------------------------------------------

#[test]
fn empty_selector_counts() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert_eq!(sel.candidate_count(), 0);
    assert_eq!(sel.enabled_count(), 0);
}

#[test]
fn empty_selector_select_returns_none() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn empty_selector_select_all_returns_empty() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert!(sel.select_all(&[Capability::Streaming]).is_empty());
}

// ---------------------------------------------------------------------------
// FirstMatch
// ---------------------------------------------------------------------------

#[test]
fn first_match_picks_first_capable() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming, Capability::ToolRead], 2));
    let picked = sel.select(&[Capability::ToolRead]).unwrap();
    assert_eq!(picked.name, "a");
}

#[test]
fn first_match_skips_incapable() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "b");
}

// ---------------------------------------------------------------------------
// BestFit
// ---------------------------------------------------------------------------

#[test]
fn best_fit_picks_most_matching() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate(
        "b",
        &[Capability::ToolRead, Capability::ToolWrite, Capability::Streaming],
        2,
    ));
    let picked = sel.select(&[Capability::ToolRead, Capability::ToolWrite]).unwrap();
    assert_eq!(picked.name, "b");
}

// ---------------------------------------------------------------------------
// LeastLoaded
// ---------------------------------------------------------------------------

#[test]
fn least_loaded_picks_lowest_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(candidate("heavy", &[Capability::Streaming], 100));
    sel.add_candidate(candidate("light", &[Capability::Streaming], 1));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "light");
}

// ---------------------------------------------------------------------------
// RoundRobin
// ---------------------------------------------------------------------------

#[test]
fn round_robin_rotates() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("c", &[Capability::Streaming], 1));

    let names: Vec<String> = (0..6)
        .map(|_| sel.select(&[Capability::Streaming]).unwrap().name.clone())
        .collect();
    assert_eq!(names, vec!["a", "b", "c", "a", "b", "c"]);
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

#[test]
fn priority_picks_lowest_value() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("low", &[Capability::Streaming], 10));
    sel.add_candidate(candidate("high", &[Capability::Streaming], 1));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "high");
}

#[test]
fn priority_respects_capability_filter() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "b");
}

// ---------------------------------------------------------------------------
// Disabled candidates
// ---------------------------------------------------------------------------

#[test]
fn disabled_candidate_is_skipped() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(disabled_candidate("off", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("on", &[Capability::Streaming], 2));
    assert_eq!(sel.enabled_count(), 1);
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "on");
}

#[test]
fn all_disabled_returns_none() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(disabled_candidate("a", &[Capability::Streaming], 1));
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

// ---------------------------------------------------------------------------
// select_all
// ---------------------------------------------------------------------------

#[test]
fn select_all_returns_all_matching() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::Streaming, Capability::ToolRead], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    sel.add_candidate(candidate("c", &[Capability::Streaming, Capability::ToolRead], 3));
    let all = sel.select_all(&[Capability::Streaming, Capability::ToolRead]);
    let names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["a", "c"]);
}

// ---------------------------------------------------------------------------
// Empty requirements
// ---------------------------------------------------------------------------

#[test]
fn empty_requirements_matches_all_enabled() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    let all = sel.select_all(&[]);
    assert_eq!(all.len(), 2);
}

// ---------------------------------------------------------------------------
// SelectionResult
// ---------------------------------------------------------------------------

#[test]
fn selection_result_with_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.unmet_capabilities.is_empty());
    assert_eq!(result.alternatives, vec!["b"]);
}

#[test]
fn selection_result_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(result.selected.is_empty());
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn selection_result_serde_roundtrip() {
    let result = SelectionResult {
        selected: "mock".into(),
        reason: "test".into(),
        alternatives: vec!["other".into()],
        unmet_capabilities: vec![],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: SelectionResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.selected, "mock");
}

// ---------------------------------------------------------------------------
// Candidate counts
// ---------------------------------------------------------------------------

#[test]
fn candidate_and_enabled_counts() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[], 1));
    sel.add_candidate(disabled_candidate("b", &[], 2));
    sel.add_candidate(candidate("c", &[], 3));
    assert_eq!(sel.candidate_count(), 3);
    assert_eq!(sel.enabled_count(), 2);
}

// ---------------------------------------------------------------------------
// Multiple required capabilities
// ---------------------------------------------------------------------------

#[test]
fn multiple_required_caps_must_all_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("partial", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate(
        "full",
        &[Capability::ToolRead, Capability::ToolWrite],
        2,
    ));
    let picked = sel
        .select(&[Capability::ToolRead, Capability::ToolWrite])
        .unwrap();
    assert_eq!(picked.name, "full");
}

#[test]
fn no_candidate_has_all_caps() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate("b", &[Capability::ToolWrite], 2));
    assert!(sel
        .select(&[Capability::ToolRead, Capability::ToolWrite])
        .is_none());
}
