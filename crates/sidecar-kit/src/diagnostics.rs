// SPDX-License-Identifier: MIT OR Apache-2.0
//! Diagnostics reporting for sidecar runs.
//!
//! Provides structured diagnostic collection with severity levels,
//! summary statistics, and per-run diagnostic snapshots.

use serde::{Deserialize, Serialize};

// ── DiagnosticLevel ─────────────────────────────────────────────────

/// Severity level for a diagnostic message.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    /// Verbose debugging information.
    Debug,
    /// Informational message.
    Info,
    /// Non-fatal warning.
    Warning,
    /// Error condition.
    Error,
}

// ── Diagnostic ──────────────────────────────────────────────────────

/// A single diagnostic message emitted during sidecar processing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Severity level.
    pub level: DiagnosticLevel,
    /// Short machine-readable code (e.g. `"SK001"`).
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Component that generated this diagnostic.
    pub source: Option<String>,
    /// ISO-8601 timestamp of when this diagnostic was created.
    pub timestamp: String,
}

// ── DiagnosticSummary ───────────────────────────────────────────────

/// Aggregate counts of diagnostics by level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSummary {
    /// Number of debug-level diagnostics.
    pub debug_count: usize,
    /// Number of info-level diagnostics.
    pub info_count: usize,
    /// Number of warning-level diagnostics.
    pub warning_count: usize,
    /// Number of error-level diagnostics.
    pub error_count: usize,
    /// Total number of diagnostics across all levels.
    pub total: usize,
}

// ── DiagnosticCollector ─────────────────────────────────────────────

/// Collects diagnostics during a sidecar run.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticCollector {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticCollector {
    /// Create an empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a pre-built diagnostic.
    pub fn add(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Convenience: add an `Info`-level diagnostic with a timestamp of now.
    pub fn add_info(&mut self, code: &str, message: &str) {
        self.add(Diagnostic {
            level: DiagnosticLevel::Info,
            code: code.to_string(),
            message: message.to_string(),
            source: None,
            timestamp: now_iso8601(),
        });
    }

    /// Convenience: add a `Warning`-level diagnostic.
    pub fn add_warning(&mut self, code: &str, message: &str) {
        self.add(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: code.to_string(),
            message: message.to_string(),
            source: None,
            timestamp: now_iso8601(),
        });
    }

    /// Convenience: add an `Error`-level diagnostic.
    pub fn add_error(&mut self, code: &str, message: &str) {
        self.add(Diagnostic {
            level: DiagnosticLevel::Error,
            code: code.to_string(),
            message: message.to_string(),
            source: None,
            timestamp: now_iso8601(),
        });
    }

    /// Borrow the full list of collected diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Return references to diagnostics matching the given level.
    #[must_use]
    pub fn by_level(&self, level: DiagnosticLevel) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.level == level)
            .collect()
    }

    /// `true` if any diagnostic has level `Error`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Error)
    }

    /// Number of `Error`-level diagnostics.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == DiagnosticLevel::Error)
            .count()
    }

    /// Remove all collected diagnostics.
    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }

    /// Compute a summary of the collected diagnostics.
    #[must_use]
    pub fn summary(&self) -> DiagnosticSummary {
        let mut s = DiagnosticSummary::default();
        for d in &self.diagnostics {
            match d.level {
                DiagnosticLevel::Debug => s.debug_count += 1,
                DiagnosticLevel::Info => s.info_count += 1,
                DiagnosticLevel::Warning => s.warning_count += 1,
                DiagnosticLevel::Error => s.error_count += 1,
            }
        }
        s.total = self.diagnostics.len();
        s
    }
}

// ── SidecarDiagnostics ──────────────────────────────────────────────

/// Collected diagnostics for a single sidecar run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SidecarDiagnostics {
    /// Identifier of the run these diagnostics belong to.
    pub run_id: String,
    /// All diagnostics collected during the run.
    pub diagnostics: Vec<Diagnostic>,
    /// Names of the pipeline stages that were active.
    pub pipeline_stages: Vec<String>,
    /// Number of transforms that were applied.
    pub transform_count: usize,
}

// ── helpers ─────────────────────────────────────────────────────────

fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}
