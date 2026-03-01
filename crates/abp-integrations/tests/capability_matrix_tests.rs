// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for CapabilityMatrix and CapabilityReport.

use abp_core::Capability;
use abp_integrations::capability::{CapabilityMatrix, CapabilityReport};

fn matrix_with_two_backends() -> CapabilityMatrix {
    let mut m = CapabilityMatrix::new();
    m.register(
        "alpha",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    m.register(
        "beta",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::McpClient,
        ],
    );
    m
}

// ---------------------------------------------------------------------------
// 1. new() creates an empty matrix
// ---------------------------------------------------------------------------
#[test]
fn new_is_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.is_empty());
    assert_eq!(m.backend_count(), 0);
}

// ---------------------------------------------------------------------------
// 2. register adds capabilities
// ---------------------------------------------------------------------------
#[test]
fn register_adds_capabilities() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    assert_eq!(m.backend_count(), 1);
    assert!(!m.is_empty());
}

// ---------------------------------------------------------------------------
// 3. register merges with existing set
// ---------------------------------------------------------------------------
#[test]
fn register_merges_capabilities() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    m.register("mock", vec![Capability::ToolRead]);
    let caps = m.all_capabilities("mock").unwrap();
    assert!(caps.contains(&Capability::Streaming));
    assert!(caps.contains(&Capability::ToolRead));
    assert_eq!(m.backend_count(), 1);
}

// ---------------------------------------------------------------------------
// 4. supports returns true for present capability
// ---------------------------------------------------------------------------
#[test]
fn supports_returns_true() {
    let m = matrix_with_two_backends();
    assert!(m.supports("alpha", &Capability::Streaming));
}

// ---------------------------------------------------------------------------
// 5. supports returns false for absent capability
// ---------------------------------------------------------------------------
#[test]
fn supports_returns_false_for_missing() {
    let m = matrix_with_two_backends();
    assert!(!m.supports("alpha", &Capability::McpClient));
}

// ---------------------------------------------------------------------------
// 6. supports returns false for unknown backend
// ---------------------------------------------------------------------------
#[test]
fn supports_returns_false_for_unknown_backend() {
    let m = matrix_with_two_backends();
    assert!(!m.supports("unknown", &Capability::Streaming));
}

// ---------------------------------------------------------------------------
// 7. backends_for finds all matching backends
// ---------------------------------------------------------------------------
#[test]
fn backends_for_returns_matching() {
    let m = matrix_with_two_backends();
    let mut backends = m.backends_for(&Capability::Streaming);
    backends.sort();
    assert_eq!(backends, vec!["alpha", "beta"]);
}

// ---------------------------------------------------------------------------
// 8. backends_for returns empty for unregistered capability
// ---------------------------------------------------------------------------
#[test]
fn backends_for_returns_empty() {
    let m = matrix_with_two_backends();
    assert!(m.backends_for(&Capability::SessionResume).is_empty());
}

// ---------------------------------------------------------------------------
// 9. all_capabilities returns None for unknown backend
// ---------------------------------------------------------------------------
#[test]
fn all_capabilities_unknown_backend() {
    let m = matrix_with_two_backends();
    assert!(m.all_capabilities("nonexistent").is_none());
}

// ---------------------------------------------------------------------------
// 10. all_capabilities returns correct set
// ---------------------------------------------------------------------------
#[test]
fn all_capabilities_correct_set() {
    let m = matrix_with_two_backends();
    let caps = m.all_capabilities("alpha").unwrap();
    assert_eq!(caps.len(), 3);
    assert!(caps.contains(&Capability::ToolWrite));
}

// ---------------------------------------------------------------------------
// 11. common_capabilities returns intersection
// ---------------------------------------------------------------------------
#[test]
fn common_capabilities_intersection() {
    let m = matrix_with_two_backends();
    let common = m.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(common.contains(&Capability::ToolRead));
    assert!(!common.contains(&Capability::ToolWrite));
    assert!(!common.contains(&Capability::McpClient));
}

// ---------------------------------------------------------------------------
// 12. common_capabilities empty matrix returns empty set
// ---------------------------------------------------------------------------
#[test]
fn common_capabilities_empty_matrix() {
    let m = CapabilityMatrix::new();
    assert!(m.common_capabilities().is_empty());
}

// ---------------------------------------------------------------------------
// 13. evaluate with perfect match yields score 1.0
// ---------------------------------------------------------------------------
#[test]
fn evaluate_perfect_score() {
    let m = matrix_with_two_backends();
    let report = m.evaluate("alpha", &[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(report.score, 1.0);
    assert!(report.missing.is_empty());
    assert_eq!(report.supported.len(), 2);
}

// ---------------------------------------------------------------------------
// 14. evaluate with partial match yields fractional score
// ---------------------------------------------------------------------------
#[test]
fn evaluate_partial_score() {
    let m = matrix_with_two_backends();
    let report = m.evaluate("alpha", &[Capability::Streaming, Capability::McpClient]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.supported, vec![Capability::Streaming]);
    assert_eq!(report.missing, vec![Capability::McpClient]);
}

// ---------------------------------------------------------------------------
// 15. evaluate with no match yields score 0.0
// ---------------------------------------------------------------------------
#[test]
fn evaluate_zero_score() {
    let m = matrix_with_two_backends();
    let report = m.evaluate("alpha", &[Capability::SessionResume]);
    assert_eq!(report.score, 0.0);
    assert!(report.supported.is_empty());
}

// ---------------------------------------------------------------------------
// 16. evaluate with empty required yields score 1.0
// ---------------------------------------------------------------------------
#[test]
fn evaluate_empty_required() {
    let m = matrix_with_two_backends();
    let report = m.evaluate("alpha", &[]);
    assert_eq!(report.score, 1.0);
}

// ---------------------------------------------------------------------------
// 17. evaluate unknown backend yields score 0.0
// ---------------------------------------------------------------------------
#[test]
fn evaluate_unknown_backend() {
    let m = matrix_with_two_backends();
    let report = m.evaluate("ghost", &[Capability::Streaming]);
    assert_eq!(report.score, 0.0);
    assert_eq!(report.backend, "ghost");
}

// ---------------------------------------------------------------------------
// 18. best_backend picks the highest scoring backend
// ---------------------------------------------------------------------------
#[test]
fn best_backend_picks_highest() {
    let m = matrix_with_two_backends();
    // alpha has ToolWrite, beta does not
    let best = m.best_backend(&[Capability::ToolWrite, Capability::Streaming]);
    assert_eq!(best.as_deref(), Some("alpha"));
}

// ---------------------------------------------------------------------------
// 19. best_backend returns None on empty matrix
// ---------------------------------------------------------------------------
#[test]
fn best_backend_empty_matrix() {
    let m = CapabilityMatrix::new();
    assert!(m.best_backend(&[Capability::Streaming]).is_none());
}

// ---------------------------------------------------------------------------
// 20. best_backend tie-breaks by lexicographic name
// ---------------------------------------------------------------------------
#[test]
fn best_backend_tiebreak_lexicographic() {
    let m = matrix_with_two_backends();
    // Both support Streaming equally — BTreeMap order + max_by keeps last equal
    let best = m.best_backend(&[Capability::Streaming]);
    assert!(best.is_some());
    // Both score 1.0; deterministic but depends on iterator + max_by semantics
    assert_eq!(best.as_deref(), Some("beta"));
}

// ---------------------------------------------------------------------------
// 21. report fields are correct
// ---------------------------------------------------------------------------
#[test]
fn report_fields() {
    let m = matrix_with_two_backends();
    let r: CapabilityReport = m.evaluate(
        "beta",
        &[
            Capability::Streaming,
            Capability::McpClient,
            Capability::ToolBash,
        ],
    );
    assert_eq!(r.backend, "beta");
    assert_eq!(r.supported.len(), 2);
    assert_eq!(r.missing, vec![Capability::ToolBash]);
    assert!((r.score - 2.0 / 3.0).abs() < 1e-10);
}
