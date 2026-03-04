#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the backend selection engine (health, dialect, criteria, work-order).

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, MinSupport, RuntimeConfig,
    WorkOrderBuilder,
};
use abp_integrations::projection::Dialect;
use abp_integrations::selector::{
    BackendCandidate, BackendHealth, BackendSelector, DialectMatch, FallbackStrategy,
    SelectionCriteria, SelectionError, SelectionStrategy,
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

fn criteria(caps: &[Capability]) -> SelectionCriteria {
    SelectionCriteria {
        preferred_backend: None,
        required_capabilities: caps.to_vec(),
        fallback_strategy: FallbackStrategy::None,
        target_dialect: None,
    }
}

// ---------------------------------------------------------------------------
// BackendHealth serialization
// ---------------------------------------------------------------------------

#[test]
fn health_up_serde() {
    let h = BackendHealth::Up;
    let json = serde_json::to_string(&h).unwrap();
    let back: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(back, BackendHealth::Up);
}

#[test]
fn health_degraded_serde() {
    let h = BackendHealth::Degraded {
        reason: "slow".into(),
    };
    let json = serde_json::to_string(&h).unwrap();
    let back: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back,
        BackendHealth::Degraded {
            reason: "slow".into()
        }
    );
}

#[test]
fn health_down_serde() {
    let h = BackendHealth::Down {
        reason: "timeout".into(),
    };
    let json = serde_json::to_string(&h).unwrap();
    let back: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back,
        BackendHealth::Down {
            reason: "timeout".into()
        }
    );
}

// ---------------------------------------------------------------------------
// DialectMatch
// ---------------------------------------------------------------------------

#[test]
fn dialect_match_ordering() {
    assert!(DialectMatch::Native > DialectMatch::Emulated);
    assert!(DialectMatch::Emulated > DialectMatch::Unsupported);
    assert!(DialectMatch::Native > DialectMatch::Unsupported);
}

#[test]
fn dialect_match_serde_roundtrip() {
    for dm in [
        DialectMatch::Native,
        DialectMatch::Emulated,
        DialectMatch::Unsupported,
    ] {
        let json = serde_json::to_string(&dm).unwrap();
        let back: DialectMatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back, dm);
    }
}

// ---------------------------------------------------------------------------
// SelectionError Display
// ---------------------------------------------------------------------------

#[test]
fn error_display_no_matching() {
    let e = SelectionError::NoMatchingBackend {
        reason: "none found".into(),
    };
    assert!(e.to_string().contains("no matching backend"));
}

#[test]
fn error_display_all_unhealthy() {
    let e = SelectionError::AllBackendsUnhealthy {
        backends: vec!["a".into(), "b".into()],
    };
    let s = e.to_string();
    assert!(s.contains("a"));
    assert!(s.contains("b"));
}

#[test]
fn error_display_capability_mismatch() {
    let e = SelectionError::CapabilityMismatch {
        backend: "mock".into(),
        missing: vec![Capability::Streaming],
    };
    assert!(e.to_string().contains("mock"));
}

#[test]
fn error_display_empty_registry() {
    let e = SelectionError::EmptyRegistry;
    assert!(e.to_string().contains("no backends registered"));
}

// ---------------------------------------------------------------------------
// FallbackStrategy
// ---------------------------------------------------------------------------

#[test]
fn fallback_strategy_default_is_none() {
    assert_eq!(FallbackStrategy::default(), FallbackStrategy::None);
}

// ---------------------------------------------------------------------------
// SelectionCriteria from WorkOrder
// ---------------------------------------------------------------------------

#[test]
fn criteria_from_work_order_no_requirements() {
    let wo = WorkOrderBuilder::new("test task").build();
    let c = SelectionCriteria::from_work_order(&wo);
    assert!(c.required_capabilities.is_empty());
    assert!(c.preferred_backend.is_none());
    assert_eq!(c.fallback_strategy, FallbackStrategy::None);
}

#[test]
fn criteria_from_work_order_with_model() {
    let wo = WorkOrderBuilder::new("test")
        .config(RuntimeConfig {
            model: Some("claude".into()),
            ..Default::default()
        })
        .build();
    let c = SelectionCriteria::from_work_order(&wo);
    assert_eq!(c.preferred_backend.as_deref(), Some("claude"));
}

#[test]
fn criteria_from_work_order_with_requirements() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();
    let c = SelectionCriteria::from_work_order(&wo);
    assert_eq!(c.required_capabilities.len(), 2);
    assert!(c.required_capabilities.contains(&Capability::Streaming));
    assert!(c.required_capabilities.contains(&Capability::ToolRead));
}

// ---------------------------------------------------------------------------
// Single backend selection
// ---------------------------------------------------------------------------

#[test]
fn single_backend_selection_basic() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("only", &[Capability::Streaming], 1));
    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "only");
    assert!(report.alternatives.is_empty());
}

#[test]
fn single_backend_no_caps_required() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("any", &[], 1));
    let report = sel.select_with_criteria(&criteria(&[])).unwrap();
    assert_eq!(report.selected_backend, "any");
}

// ---------------------------------------------------------------------------
// Empty registry error
// ---------------------------------------------------------------------------

#[test]
fn empty_registry_returns_error() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let err = sel.select_with_criteria(&criteria(&[])).unwrap_err();
    assert_eq!(err, SelectionError::EmptyRegistry);
}

// ---------------------------------------------------------------------------
// select_backend with WorkOrder
// ---------------------------------------------------------------------------

#[test]
fn select_backend_simple_work_order() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("mock", &[Capability::Streaming], 1));
    let wo = WorkOrderBuilder::new("hello").build();
    let name = sel.select_backend(&wo).unwrap();
    assert_eq!(name, "mock");
}

#[test]
fn select_backend_with_requirements() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate(
        "b",
        &[Capability::ToolRead, Capability::ToolWrite],
        2,
    ));
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Any,
                },
                CapabilityRequirement {
                    capability: Capability::ToolWrite,
                    min_support: MinSupport::Any,
                },
            ],
        })
        .build();
    let name = sel.select_backend(&wo).unwrap();
    assert_eq!(name, "b");
}

#[test]
fn select_backend_empty_registry() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let wo = WorkOrderBuilder::new("test").build();
    let err = sel.select_backend(&wo).unwrap_err();
    assert_eq!(err, SelectionError::EmptyRegistry);
}

#[test]
fn select_backend_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Any,
            }],
        })
        .build();
    assert!(sel.select_backend(&wo).is_err());
}

#[test]
fn select_backend_preferred_model() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    let wo = WorkOrderBuilder::new("test").model("b").build();
    let name = sel.select_backend(&wo).unwrap();
    assert_eq!(name, "b");
}

// ---------------------------------------------------------------------------
// Health-aware selection
// ---------------------------------------------------------------------------

#[test]
fn healthy_backend_preferred_over_degraded() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("degraded_one", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("healthy_one", &[Capability::Streaming], 1));
    sel.set_health(
        "degraded_one",
        BackendHealth::Degraded {
            reason: "slow".into(),
        },
    );
    // healthy_one has no health record → defaults to Up
    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "healthy_one");
}

#[test]
fn down_backend_skipped() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("down_one", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("up_one", &[Capability::Streaming], 2));
    sel.set_health(
        "down_one",
        BackendHealth::Down {
            reason: "offline".into(),
        },
    );
    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "up_one");
}

#[test]
fn all_backends_down_returns_error() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    sel.set_health("a", BackendHealth::Down { reason: "x".into() });
    sel.set_health("b", BackendHealth::Down { reason: "y".into() });
    let err = sel
        .select_with_criteria(&criteria(&[Capability::Streaming]))
        .unwrap_err();
    match err {
        SelectionError::AllBackendsUnhealthy { backends } => {
            assert_eq!(backends.len(), 2);
        }
        other => panic!("expected AllBackendsUnhealthy, got {other:?}"),
    }
}

#[test]
fn degraded_backend_still_selectable() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("deg", &[Capability::Streaming], 1));
    sel.set_health(
        "deg",
        BackendHealth::Degraded {
            reason: "slow".into(),
        },
    );
    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "deg");
}

#[test]
fn health_not_set_defaults_to_up() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("fresh", &[Capability::Streaming], 1));
    assert!(sel.get_health("fresh").is_none());
    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "fresh");
}

#[test]
fn set_health_updates_record() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("x", &[], 1));
    sel.set_health("x", BackendHealth::Up);
    assert!(sel.get_health("x").is_some());
    assert_eq!(sel.get_health("x").unwrap().health, BackendHealth::Up);
}

// ---------------------------------------------------------------------------
// Dialect-aware selection
// ---------------------------------------------------------------------------

#[test]
fn native_dialect_preferred_over_emulated() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("claude_be", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("openai_be", &[Capability::Streaming], 1));
    sel.set_dialect("claude_be", Dialect::Claude);
    sel.set_dialect("openai_be", Dialect::OpenAi);

    let c = SelectionCriteria {
        target_dialect: Some(Dialect::Claude),
        required_capabilities: vec![Capability::Streaming],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "claude_be");
}

#[test]
fn emulated_dialect_preferred_over_unsupported() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("has_dialect", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("no_dialect", &[Capability::Streaming], 1));
    sel.set_dialect("has_dialect", Dialect::OpenAi);
    // no_dialect has no dialect set → Unsupported when target is Claude

    let c = SelectionCriteria {
        target_dialect: Some(Dialect::Claude),
        required_capabilities: vec![Capability::Streaming],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "has_dialect");
}

#[test]
fn no_target_dialect_all_treated_native() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));
    sel.set_dialect("a", Dialect::Claude);
    // No target dialect → both treated as Native
    let c = SelectionCriteria {
        required_capabilities: vec![Capability::Streaming],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    // Both have same dialect score; "a" wins on priority
    assert_eq!(report.selected_backend, "a");
}

#[test]
fn dialect_and_capability_combined() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate(
        "native_full",
        &[Capability::Streaming, Capability::ToolRead],
        2,
    ));
    sel.add_candidate(candidate("emulated_full", &[Capability::Streaming, Capability::ToolRead], 1));
    sel.set_dialect("native_full", Dialect::Claude);
    sel.set_dialect("emulated_full", Dialect::OpenAi);

    let c = SelectionCriteria {
        target_dialect: Some(Dialect::Claude),
        required_capabilities: vec![Capability::Streaming, Capability::ToolRead],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    // native_full has native dialect (+5 over emulated) but higher priority number;
    // dialect bonus (5.0) outweighs priority difference.
    assert_eq!(report.selected_backend, "native_full");
}

// ---------------------------------------------------------------------------
// Preferred backend in criteria
// ---------------------------------------------------------------------------

#[test]
fn criteria_preferred_backend_honored() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 10));

    let c = SelectionCriteria {
        preferred_backend: Some("b".into()),
        required_capabilities: vec![Capability::Streaming],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "b");
    assert_eq!(report.reason, "preferred backend matched");
}

#[test]
fn criteria_preferred_backend_down_falls_back() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("preferred", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("fallback", &[Capability::Streaming], 2));
    sel.set_health(
        "preferred",
        BackendHealth::Down {
            reason: "dead".into(),
        },
    );

    let c = SelectionCriteria {
        preferred_backend: Some("preferred".into()),
        required_capabilities: vec![Capability::Streaming],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "fallback");
}

#[test]
fn criteria_preferred_missing_caps_error() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("pref", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate("other", &[Capability::Streaming], 2));

    let c = SelectionCriteria {
        preferred_backend: Some("pref".into()),
        required_capabilities: vec![Capability::Streaming],
        fallback_strategy: FallbackStrategy::None,
        ..Default::default()
    };
    let err = sel.select_with_criteria(&c).unwrap_err();
    match err {
        SelectionError::CapabilityMismatch { backend, missing } => {
            assert_eq!(backend, "pref");
            assert!(missing.contains(&Capability::Streaming));
        }
        other => panic!("expected CapabilityMismatch, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Fallback strategies
// ---------------------------------------------------------------------------

#[test]
fn fallback_none_rejects_partial() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("partial", &[Capability::ToolRead], 1));

    let c = SelectionCriteria {
        required_capabilities: vec![Capability::ToolRead, Capability::Streaming],
        fallback_strategy: FallbackStrategy::None,
        ..Default::default()
    };
    assert!(sel.select_with_criteria(&c).is_err());
}

#[test]
fn fallback_best_effort_accepts_partial() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("partial", &[Capability::ToolRead], 1));

    let c = SelectionCriteria {
        required_capabilities: vec![Capability::ToolRead, Capability::Streaming],
        fallback_strategy: FallbackStrategy::BestEffort,
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "partial");
}

#[test]
fn fallback_any_healthy_ignores_caps() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("irrelevant", &[], 1));

    let c = SelectionCriteria {
        required_capabilities: vec![Capability::Streaming, Capability::ToolWrite],
        fallback_strategy: FallbackStrategy::AnyHealthy,
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "irrelevant");
}

#[test]
fn fallback_best_effort_ranks_by_coverage() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("one_cap", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate(
        "two_caps",
        &[Capability::ToolRead, Capability::Streaming],
        1,
    ));

    let c = SelectionCriteria {
        required_capabilities: vec![Capability::ToolRead, Capability::Streaming],
        fallback_strategy: FallbackStrategy::BestEffort,
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "two_caps");
}

// ---------------------------------------------------------------------------
// SelectionReport
// ---------------------------------------------------------------------------

#[test]
fn report_contains_evaluations() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("b", &[Capability::Streaming], 2));

    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.considered.len(), 2);
}

#[test]
fn report_alternatives_listed() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("winner", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("alt1", &[Capability::Streaming], 2));
    sel.add_candidate(candidate("alt2", &[Capability::Streaming], 3));

    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "winner");
    assert_eq!(report.alternatives.len(), 2);
    assert!(report.alternatives.contains(&"alt1".to_string()));
    assert!(report.alternatives.contains(&"alt2".to_string()));
}

#[test]
fn report_rejected_candidates_have_reason() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("good", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("bad", &[Capability::ToolRead], 2));

    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    let bad_eval = report.considered.iter().find(|e| e.backend == "bad").unwrap();
    assert!(bad_eval.rejected_reason.is_some());
    assert_eq!(bad_eval.composite_score, 0.0);
}

// ---------------------------------------------------------------------------
// Multiple backend ranking
// ---------------------------------------------------------------------------

#[test]
fn multiple_backends_ranked_by_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("low_pri", &[Capability::Streaming], 10));
    sel.add_candidate(candidate("high_pri", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("med_pri", &[Capability::Streaming], 5));

    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "high_pri");
}

#[test]
fn multiple_backends_ranked_by_health_then_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("degraded", &[Capability::Streaming], 1));
    sel.add_candidate(candidate("healthy", &[Capability::Streaming], 2));
    sel.set_health(
        "degraded",
        BackendHealth::Degraded {
            reason: "slow".into(),
        },
    );

    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    // Health difference (Up=30 vs Degraded=10) outweighs priority difference
    assert_eq!(report.selected_backend, "healthy");
}

// ---------------------------------------------------------------------------
// Capability-based filtering
// ---------------------------------------------------------------------------

#[test]
fn capability_filter_strict_no_partial() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("partial", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate(
        "full",
        &[Capability::ToolRead, Capability::ToolWrite],
        2,
    ));

    let c = criteria(&[Capability::ToolRead, Capability::ToolWrite]);
    let report = sel.select_with_criteria(&c).unwrap();
    assert_eq!(report.selected_backend, "full");
}

#[test]
fn capability_filter_no_backend_has_all() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", &[Capability::ToolRead], 1));
    sel.add_candidate(candidate("b", &[Capability::ToolWrite], 2));

    let c = criteria(&[Capability::ToolRead, Capability::ToolWrite]);
    assert!(sel.select_with_criteria(&c).is_err());
}

#[test]
fn empty_requirements_matches_any_backend() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("any", &[], 1));
    let report = sel.select_with_criteria(&criteria(&[])).unwrap();
    assert_eq!(report.selected_backend, "any");
}

// ---------------------------------------------------------------------------
// set_dialect
// ---------------------------------------------------------------------------

#[test]
fn set_dialect_updates_map() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("x", &[Capability::Streaming], 1));
    sel.set_dialect("x", Dialect::Claude);

    let c = SelectionCriteria {
        target_dialect: Some(Dialect::Claude),
        required_capabilities: vec![Capability::Streaming],
        ..Default::default()
    };
    let report = sel.select_with_criteria(&c).unwrap();
    let eval = report.considered.iter().find(|e| e.backend == "x").unwrap();
    assert_eq!(eval.dialect_match, DialectMatch::Native);
}

// ---------------------------------------------------------------------------
// Edge: disabled candidates in criteria-based selection
// ---------------------------------------------------------------------------

#[test]
fn disabled_candidates_excluded_from_criteria_selection() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut off = candidate("off", &[Capability::Streaming], 1);
    off.enabled = false;
    sel.add_candidate(off);
    sel.add_candidate(candidate("on", &[Capability::Streaming], 2));

    let report = sel.select_with_criteria(&criteria(&[Capability::Streaming])).unwrap();
    assert_eq!(report.selected_backend, "on");
    // Only enabled candidates appear in evaluations
    assert_eq!(report.considered.len(), 1);
}
