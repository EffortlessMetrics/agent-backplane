// SPDX-License-Identifier: MIT OR Apache-2.0
//! Structured validation report with severity levels.

use std::fmt;

/// Severity level for a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Informational note — not a problem.
    Info,
    /// Something likely unintended but non-blocking.
    Warning,
    /// A hard error that must be fixed.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => f.write_str("info"),
            Severity::Warning => f.write_str("warning"),
            Severity::Error => f.write_str("error"),
        }
    }
}

/// A single issue within a [`ValidationReport`].
#[derive(Debug, Clone)]
pub struct ReportIssue {
    /// Dot-separated path to the offending field.
    pub path: String,
    /// Severity of the issue.
    pub severity: Severity,
    /// Human-readable description.
    pub message: String,
    /// Optional rule name that generated this issue.
    pub rule: Option<String>,
}

impl fmt::Display for ReportIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref rule) = self.rule {
            write!(
                f,
                "[{}] {} ({}): {}",
                self.severity, self.path, rule, self.message
            )
        } else {
            write!(f, "[{}] {}: {}", self.severity, self.path, self.message)
        }
    }
}

/// Structured report collecting all validation issues with severity levels.
///
/// A report is considered valid when it contains zero [`Severity::Error`] issues.
///
/// # Examples
///
/// ```
/// use abp_validate::report::{ValidationReport, Severity};
///
/// let mut report = ValidationReport::new();
/// report.info("config.log_level", "using default log level");
/// report.warn("config.receipts_dir", "receipts directory not configured");
/// report.error("config.port", "port must be between 1 and 65535");
///
/// assert!(!report.is_valid());
/// assert_eq!(report.error_count(), 1);
/// assert_eq!(report.warning_count(), 1);
/// assert_eq!(report.info_count(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    issues: Vec<ReportIssue>,
}

impl ValidationReport {
    /// Create an empty report.
    #[must_use]
    pub fn new() -> Self {
        Self { issues: Vec::new() }
    }

    /// Add an issue at the given severity.
    pub fn add(&mut self, path: impl Into<String>, severity: Severity, message: impl Into<String>) {
        self.issues.push(ReportIssue {
            path: path.into(),
            severity,
            message: message.into(),
            rule: None,
        });
    }

    /// Add an issue attributed to a specific rule.
    pub fn add_with_rule(
        &mut self,
        path: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
        rule: impl Into<String>,
    ) {
        self.issues.push(ReportIssue {
            path: path.into(),
            severity,
            message: message.into(),
            rule: Some(rule.into()),
        });
    }

    /// Add an informational issue.
    pub fn info(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.add(path, Severity::Info, message);
    }

    /// Add a warning issue.
    pub fn warn(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.add(path, Severity::Warning, message);
    }

    /// Add an error issue.
    pub fn error(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.add(path, Severity::Error, message);
    }

    /// Whether the report has no error-level issues (warnings/info are ok).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    /// Whether the report contains no issues at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Total number of issues.
    #[must_use]
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// Number of error-level issues.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    /// Number of warning-level issues.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }

    /// Number of info-level issues.
    #[must_use]
    pub fn info_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Info)
            .count()
    }

    /// Whether the report contains at least one error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// Whether the report contains at least one warning.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.warning_count() > 0
    }

    /// Iterate over all issues.
    pub fn iter(&self) -> impl Iterator<Item = &ReportIssue> {
        self.issues.iter()
    }

    /// Return only issues at the given severity.
    #[must_use]
    pub fn filter_by_severity(&self, severity: Severity) -> Vec<&ReportIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .collect()
    }

    /// Return only issues at or above the given severity.
    #[must_use]
    pub fn filter_at_or_above(&self, min: Severity) -> Vec<&ReportIssue> {
        self.issues.iter().filter(|i| i.severity >= min).collect()
    }

    /// Return only issues whose path starts with the given prefix.
    #[must_use]
    pub fn filter_by_path(&self, prefix: &str) -> Vec<&ReportIssue> {
        self.issues
            .iter()
            .filter(|i| i.path.starts_with(prefix))
            .collect()
    }

    /// Merge all issues from another report into this one.
    pub fn merge(&mut self, other: ValidationReport) {
        self.issues.extend(other.issues);
    }

    /// Consume into the inner vec of issues.
    #[must_use]
    pub fn into_issues(self) -> Vec<ReportIssue> {
        self.issues
    }

    /// Format as a multi-line human-readable report.
    #[must_use]
    pub fn format(&self) -> String {
        if self.issues.is_empty() {
            return "Validation passed: no issues found.".to_string();
        }
        let mut out = format!(
            "Validation report: {} issue(s) ({} error, {} warning, {} info)\n",
            self.len(),
            self.error_count(),
            self.warning_count(),
            self.info_count(),
        );
        for (i, issue) in self.issues.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, issue));
        }
        if self.is_valid() {
            out.push_str("Result: PASS (no errors)\n");
        } else {
            out.push_str("Result: FAIL\n");
        }
        out
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}
