// SPDX-License-Identifier: MIT OR Apache-2.0
//! Event filtering for [`AgentEvent`] streams.
//!
//! Supports include-list and exclude-list modes, matching against the serde
//! name of [`AgentEventKind`] variants (e.g. `"run_started"`,
//! `"assistant_message"`). Matching is case-insensitive.

use crate::{AgentEvent, AgentEventKind};

/// Configurable filter for [`AgentEvent`]s by their [`AgentEventKind`].
#[derive(Debug, Clone)]
pub struct EventFilter {
    mode: FilterMode,
    /// Stored in lowercase for case-insensitive comparison.
    kinds: Vec<String>,
}

#[derive(Debug, Clone)]
enum FilterMode {
    Include,
    Exclude,
}

impl EventFilter {
    /// Create a filter that only passes events whose kind is in `kinds`.
    ///
    /// An empty list means nothing passes.
    ///
    /// # Examples
    ///
    /// ```
    /// use abp_core::filter::EventFilter;
    /// use abp_core::{AgentEvent, AgentEventKind};
    /// use chrono::Utc;
    ///
    /// let filter = EventFilter::include_kinds(&["assistant_message", "error"]);
    ///
    /// let msg = AgentEvent { ts: Utc::now(), kind: AgentEventKind::AssistantMessage { text: "hi".into() }, ext: None };
    /// assert!(filter.matches(&msg));
    ///
    /// let started = AgentEvent { ts: Utc::now(), kind: AgentEventKind::RunStarted { message: "go".into() }, ext: None };
    /// assert!(!filter.matches(&started));
    /// ```
    #[must_use]
    pub fn include_kinds(kinds: &[&str]) -> Self {
        Self {
            mode: FilterMode::Include,
            kinds: kinds.iter().map(|k| k.to_ascii_lowercase()).collect(),
        }
    }

    /// Create a filter that passes everything *except* events whose kind is
    /// in `kinds`.
    ///
    /// An empty list means everything passes.
    ///
    /// # Examples
    ///
    /// ```
    /// use abp_core::filter::EventFilter;
    /// use abp_core::{AgentEvent, AgentEventKind};
    /// use chrono::Utc;
    ///
    /// let filter = EventFilter::exclude_kinds(&["assistant_delta"]);
    ///
    /// let delta = AgentEvent { ts: Utc::now(), kind: AgentEventKind::AssistantDelta { text: "…".into() }, ext: None };
    /// assert!(!filter.matches(&delta));
    ///
    /// let msg = AgentEvent { ts: Utc::now(), kind: AgentEventKind::AssistantMessage { text: "done".into() }, ext: None };
    /// assert!(filter.matches(&msg));
    /// ```
    #[must_use]
    pub fn exclude_kinds(kinds: &[&str]) -> Self {
        Self {
            mode: FilterMode::Exclude,
            kinds: kinds.iter().map(|k| k.to_ascii_lowercase()).collect(),
        }
    }

    /// Returns `true` if `event` passes this filter.
    #[must_use]
    pub fn matches(&self, event: &AgentEvent) -> bool {
        let name = kind_name(&event.kind);
        let in_set = self.kinds.iter().any(|k| k == &name);
        match self.mode {
            FilterMode::Include => in_set,
            FilterMode::Exclude => !in_set,
        }
    }
}

/// Extract the serde tag name (lowercase) for an [`AgentEventKind`] variant.
pub(crate) fn kind_name(kind: &AgentEventKind) -> String {
    // AgentEventKind uses `#[serde(tag = "type", rename_all = "snake_case")]`,
    // so serializing to a JSON Value and reading the "type" field gives us the
    // canonical serde name.
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| {
            v.get("type")
                .and_then(|t| t.as_str().map(str::to_ascii_lowercase))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    #[test]
    fn kind_name_matches_serde_rename() {
        assert_eq!(
            kind_name(&AgentEventKind::RunStarted {
                message: String::new()
            }),
            "run_started"
        );
        assert_eq!(
            kind_name(&AgentEventKind::AssistantMessage {
                text: String::new()
            }),
            "assistant_message"
        );
    }

    #[test]
    fn include_passes_matching() {
        let f = EventFilter::include_kinds(&["run_started"]);
        let e = event(AgentEventKind::RunStarted {
            message: "hi".into(),
        });
        assert!(f.matches(&e));
    }

    #[test]
    fn include_rejects_non_matching() {
        let f = EventFilter::include_kinds(&["run_started"]);
        let e = event(AgentEventKind::Warning {
            message: "oops".into(),
        });
        assert!(!f.matches(&e));
    }

    #[test]
    fn exclude_passes_non_matching() {
        let f = EventFilter::exclude_kinds(&["error"]);
        let e = event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        assert!(f.matches(&e));
    }

    #[test]
    fn exclude_rejects_matching() {
        let f = EventFilter::exclude_kinds(&["error"]);
        let e = event(AgentEventKind::Error {
            message: "bad".into(),
            error_code: None,
        });
        assert!(!f.matches(&e));
    }
}
