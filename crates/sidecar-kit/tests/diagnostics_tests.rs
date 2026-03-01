// SPDX-License-Identifier: MIT OR Apache-2.0

use sidecar_kit::diagnostics::{
    Diagnostic, DiagnosticCollector, DiagnosticLevel, DiagnosticSummary, SidecarDiagnostics,
};

// ── DiagnosticLevel tests ───────────────────────────────────────────

#[test]
fn level_ordering() {
    assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
    assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
    assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
}

#[test]
fn level_equality() {
    assert_eq!(DiagnosticLevel::Warning, DiagnosticLevel::Warning);
    assert_ne!(DiagnosticLevel::Debug, DiagnosticLevel::Error);
}

#[test]
fn level_clone() {
    let level = DiagnosticLevel::Info;
    let cloned = level.clone();
    assert_eq!(level, cloned);
}

#[test]
fn level_serde_roundtrip() {
    let level = DiagnosticLevel::Warning;
    let json = serde_json::to_string(&level).unwrap();
    assert_eq!(json, "\"warning\"");
    let back: DiagnosticLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, level);
}

// ── Diagnostic tests ────────────────────────────────────────────────

#[test]
fn diagnostic_serde_roundtrip() {
    let d = Diagnostic {
        level: DiagnosticLevel::Error,
        code: "SK001".to_string(),
        message: "something broke".to_string(),
        source: Some("pipeline".to_string()),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: Diagnostic = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, "SK001");
    assert_eq!(back.level, DiagnosticLevel::Error);
    assert_eq!(back.source.as_deref(), Some("pipeline"));
}

#[test]
fn diagnostic_source_none() {
    let d = Diagnostic {
        level: DiagnosticLevel::Debug,
        code: "SK000".to_string(),
        message: "trace".to_string(),
        source: None,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
    };
    assert!(d.source.is_none());
}

// ── DiagnosticCollector tests ───────────────────────────────────────

#[test]
fn collector_starts_empty() {
    let c = DiagnosticCollector::new();
    assert!(c.diagnostics().is_empty());
    assert!(!c.has_errors());
    assert_eq!(c.error_count(), 0);
}

#[test]
fn collector_add_info() {
    let mut c = DiagnosticCollector::new();
    c.add_info("SK100", "all good");
    assert_eq!(c.diagnostics().len(), 1);
    assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Info);
    assert_eq!(c.diagnostics()[0].code, "SK100");
}

#[test]
fn collector_add_warning() {
    let mut c = DiagnosticCollector::new();
    c.add_warning("SK200", "watch out");
    assert_eq!(c.diagnostics().len(), 1);
    assert_eq!(c.diagnostics()[0].level, DiagnosticLevel::Warning);
}

#[test]
fn collector_add_error() {
    let mut c = DiagnosticCollector::new();
    c.add_error("SK300", "failed");
    assert!(c.has_errors());
    assert_eq!(c.error_count(), 1);
}

#[test]
fn collector_add_raw_diagnostic() {
    let mut c = DiagnosticCollector::new();
    c.add(Diagnostic {
        level: DiagnosticLevel::Debug,
        code: "SK010".to_string(),
        message: "verbose".to_string(),
        source: Some("test".to_string()),
        timestamp: "2025-06-01T00:00:00Z".to_string(),
    });
    assert_eq!(c.diagnostics().len(), 1);
    assert_eq!(c.diagnostics()[0].source.as_deref(), Some("test"));
}

#[test]
fn collector_by_level() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "info");
    c.add_warning("W1", "warn");
    c.add_error("E1", "err");
    c.add_info("I2", "info2");

    assert_eq!(c.by_level(DiagnosticLevel::Info).len(), 2);
    assert_eq!(c.by_level(DiagnosticLevel::Warning).len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Error).len(), 1);
    assert_eq!(c.by_level(DiagnosticLevel::Debug).len(), 0);
}

#[test]
fn collector_has_errors_false_without_errors() {
    let mut c = DiagnosticCollector::new();
    c.add_info("I1", "ok");
    c.add_warning("W1", "hmm");
    assert!(!c.has_errors());
}

#[test]
fn collector_clear() {
    let mut c = DiagnosticCollector::new();
    c.add_error("E1", "bad");
    c.add_info("I1", "ok");
    assert_eq!(c.diagnostics().len(), 2);
    c.clear();
    assert!(c.diagnostics().is_empty());
    assert!(!c.has_errors());
}

#[test]
fn collector_summary_empty() {
    let c = DiagnosticCollector::new();
    let s = c.summary();
    assert_eq!(
        s,
        DiagnosticSummary {
            debug_count: 0,
            info_count: 0,
            warning_count: 0,
            error_count: 0,
            total: 0,
        }
    );
}

#[test]
fn collector_summary_mixed() {
    let mut c = DiagnosticCollector::new();
    c.add(Diagnostic {
        level: DiagnosticLevel::Debug,
        code: "D1".to_string(),
        message: "dbg".to_string(),
        source: None,
        timestamp: "t".to_string(),
    });
    c.add_info("I1", "info");
    c.add_warning("W1", "warn");
    c.add_warning("W2", "warn2");
    c.add_error("E1", "err");

    let s = c.summary();
    assert_eq!(s.debug_count, 1);
    assert_eq!(s.info_count, 1);
    assert_eq!(s.warning_count, 2);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.total, 5);
}

#[test]
fn collector_multiple_errors_count() {
    let mut c = DiagnosticCollector::new();
    c.add_error("E1", "first");
    c.add_error("E2", "second");
    c.add_error("E3", "third");
    assert_eq!(c.error_count(), 3);
    assert!(c.has_errors());
}

// ── SidecarDiagnostics tests ────────────────────────────────────────

#[test]
fn sidecar_diagnostics_serde_roundtrip() {
    let sd = SidecarDiagnostics {
        run_id: "run-42".to_string(),
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Info,
            code: "SK100".to_string(),
            message: "started".to_string(),
            source: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
        }],
        pipeline_stages: vec!["validate".to_string(), "timestamp".to_string()],
        transform_count: 3,
    };

    let json = serde_json::to_string(&sd).unwrap();
    let back: SidecarDiagnostics = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, "run-42");
    assert_eq!(back.diagnostics.len(), 1);
    assert_eq!(back.pipeline_stages.len(), 2);
    assert_eq!(back.transform_count, 3);
}

#[test]
fn sidecar_diagnostics_empty() {
    let sd = SidecarDiagnostics {
        run_id: "run-0".to_string(),
        diagnostics: vec![],
        pipeline_stages: vec![],
        transform_count: 0,
    };
    assert!(sd.diagnostics.is_empty());
    assert!(sd.pipeline_stages.is_empty());
}

// ── DiagnosticSummary tests ─────────────────────────────────────────

#[test]
fn summary_serde_roundtrip() {
    let s = DiagnosticSummary {
        debug_count: 1,
        info_count: 2,
        warning_count: 3,
        error_count: 4,
        total: 10,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: DiagnosticSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}
