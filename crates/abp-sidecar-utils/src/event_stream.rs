// SPDX-License-Identifier: MIT OR Apache-2.0
//! Event stream processor — validates ref_id correlation, detects
//! out-of-order events, and tracks event counts by type.

use std::collections::BTreeMap;

use abp_core::AgentEvent;
use abp_protocol::{Envelope, ProtocolError};
use thiserror::Error;
use tracing::warn;

/// Errors produced by the event stream processor.
#[derive(Debug, Error)]
pub enum EventStreamError {
    /// An event's `ref_id` does not match the expected run id.
    #[error("ref_id mismatch: expected \"{expected}\", got \"{got}\"")]
    RefIdMismatch {
        /// The expected ref_id.
        expected: String,
        /// The actual ref_id found.
        got: String,
    },
    /// An event arrived after a terminal envelope.
    #[error("event received after terminal envelope")]
    EventAfterTerminal,
    /// The envelope is not an event type.
    #[error("unexpected envelope type: {0}")]
    UnexpectedEnvelope(String),
    /// Protocol-level error.
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
}

/// Cumulative statistics tracked by [`EventStreamProcessor`].
#[derive(Debug, Clone, Default)]
pub struct EventStreamStats {
    /// Number of events successfully processed.
    pub events_processed: u64,
    /// Number of events dropped due to ref_id mismatch.
    pub ref_id_mismatches: u64,
    /// Number of events received after a terminal envelope.
    pub events_after_terminal: u64,
    /// Event counts keyed by the `AgentEventKind` discriminant name.
    pub counts_by_type: BTreeMap<String, u64>,
}

/// Processes a stream of [`Envelope`] values, extracting and validating
/// [`AgentEvent`]s.
///
/// # Examples
///
/// ```
/// use abp_sidecar_utils::event_stream::EventStreamProcessor;
/// use abp_protocol::{Envelope, JsonlCodec};
/// use abp_core::{AgentEvent, AgentEventKind};
/// use chrono::Utc;
///
/// let mut proc = EventStreamProcessor::new("run-1".to_string());
///
/// let event = AgentEvent {
///     ts: Utc::now(),
///     kind: AgentEventKind::AssistantMessage { text: "hi".into() },
///     ext: None,
/// };
/// let env = Envelope::Event {
///     ref_id: "run-1".into(),
///     event: event.clone(),
/// };
/// let result = proc.process_envelope(&env);
/// assert!(result.is_ok());
/// assert_eq!(proc.stats().events_processed, 1);
/// ```
#[derive(Debug)]
pub struct EventStreamProcessor {
    expected_ref_id: String,
    terminal_received: bool,
    stats: EventStreamStats,
}

impl EventStreamProcessor {
    /// Create a new processor expecting events for the given run id.
    #[must_use]
    pub fn new(expected_ref_id: String) -> Self {
        Self {
            expected_ref_id,
            terminal_received: false,
            stats: EventStreamStats::default(),
        }
    }

    /// Current cumulative statistics.
    #[must_use]
    pub fn stats(&self) -> &EventStreamStats {
        &self.stats
    }

    /// Whether a terminal (`Final` or `Fatal`) envelope has been seen.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.terminal_received
    }

    /// Process a single envelope.
    ///
    /// - `Event` envelopes: validates ref_id, checks ordering, returns the
    ///   inner [`AgentEvent`] on success.
    /// - `Final` / `Fatal`: marks the stream as terminated, returns `Ok(None)`.
    /// - Other types: returns an error.
    ///
    /// Out-of-order events (after terminal) are logged and counted but do
    /// **not** halt processing — the processor returns an error for the
    /// caller to decide whether to abort.
    pub fn process_envelope(
        &mut self,
        envelope: &Envelope,
    ) -> Result<Option<AgentEvent>, EventStreamError> {
        match envelope {
            Envelope::Event { ref_id, event } => {
                if self.terminal_received {
                    self.stats.events_after_terminal += 1;
                    warn!(ref_id = %ref_id, "event received after terminal envelope");
                    return Err(EventStreamError::EventAfterTerminal);
                }

                if *ref_id != self.expected_ref_id {
                    self.stats.ref_id_mismatches += 1;
                    warn!(
                        expected = %self.expected_ref_id,
                        got = %ref_id,
                        "ref_id mismatch"
                    );
                    return Err(EventStreamError::RefIdMismatch {
                        expected: self.expected_ref_id.clone(),
                        got: ref_id.clone(),
                    });
                }

                self.stats.events_processed += 1;
                let type_name = event_kind_name(&event.kind);
                *self.stats.counts_by_type.entry(type_name).or_insert(0) += 1;
                Ok(Some(event.clone()))
            }

            Envelope::Final { ref_id, .. } => {
                if *ref_id != self.expected_ref_id {
                    warn!(
                        expected = %self.expected_ref_id,
                        got = %ref_id,
                        "terminal ref_id mismatch"
                    );
                }
                self.terminal_received = true;
                Ok(None)
            }

            Envelope::Fatal { .. } => {
                self.terminal_received = true;
                Ok(None)
            }

            Envelope::Hello { .. } => Err(EventStreamError::UnexpectedEnvelope("hello".into())),
            Envelope::Run { .. } => Err(EventStreamError::UnexpectedEnvelope("run".into())),
        }
    }

    /// Convenience: process multiple envelopes, collecting the extracted events.
    pub fn process_many(
        &mut self,
        envelopes: &[Envelope],
    ) -> Vec<Result<Option<AgentEvent>, EventStreamError>> {
        envelopes
            .iter()
            .map(|env| self.process_envelope(env))
            .collect()
    }
}

/// Extract a human-readable discriminant name from an [`AgentEventKind`].
fn event_kind_name(kind: &abp_core::AgentEventKind) -> String {
    // Serialize to JSON, extract the "type" field.
    if let Ok(val) = serde_json::to_value(kind) {
        if let Some(t) = val.get("type").and_then(|v| v.as_str()) {
            return t.to_string();
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;

    fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
        Envelope::Event {
            ref_id: ref_id.into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind,
                ext: None,
            },
        }
    }

    #[test]
    fn process_valid_events() {
        let mut proc = EventStreamProcessor::new("run-1".into());

        let env = make_event(
            "run-1",
            AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
        );
        let result = proc.process_envelope(&env).unwrap();
        assert!(result.is_some());
        assert_eq!(proc.stats().events_processed, 1);
        assert_eq!(
            proc.stats().counts_by_type.get("assistant_message"),
            Some(&1)
        );
    }

    #[test]
    fn ref_id_mismatch() {
        let mut proc = EventStreamProcessor::new("run-1".into());
        let env = make_event(
            "run-other",
            AgentEventKind::RunStarted {
                message: "hi".into(),
            },
        );
        let err = proc.process_envelope(&env).unwrap_err();
        assert!(matches!(err, EventStreamError::RefIdMismatch { .. }));
        assert_eq!(proc.stats().ref_id_mismatches, 1);
    }

    #[test]
    fn event_after_terminal() {
        let mut proc = EventStreamProcessor::new("run-1".into());

        let fatal = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "done".into(),
            error_code: None,
        };
        proc.process_envelope(&fatal).unwrap();
        assert!(proc.is_terminal());

        let env = make_event(
            "run-1",
            AgentEventKind::RunCompleted {
                message: "late".into(),
            },
        );
        let err = proc.process_envelope(&env).unwrap_err();
        assert!(matches!(err, EventStreamError::EventAfterTerminal));
        assert_eq!(proc.stats().events_after_terminal, 1);
    }

    #[test]
    fn final_marks_terminal() {
        let mut proc = EventStreamProcessor::new("run-1".into());
        let receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .build();
        let final_env = Envelope::Final {
            ref_id: "run-1".into(),
            receipt,
        };
        let result = proc.process_envelope(&final_env).unwrap();
        assert!(result.is_none());
        assert!(proc.is_terminal());
    }

    #[test]
    fn unexpected_envelope_types() {
        let mut proc = EventStreamProcessor::new("run-1".into());
        let hello = Envelope::hello(
            abp_core::BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            abp_core::CapabilityManifest::new(),
        );
        let err = proc.process_envelope(&hello).unwrap_err();
        assert!(matches!(err, EventStreamError::UnexpectedEnvelope(_)));
    }

    #[test]
    fn counts_by_type() {
        let mut proc = EventStreamProcessor::new("run-1".into());

        for _ in 0..3 {
            let env = make_event(
                "run-1",
                AgentEventKind::AssistantDelta { text: "tok".into() },
            );
            proc.process_envelope(&env).unwrap();
        }

        let env = make_event(
            "run-1",
            AgentEventKind::AssistantMessage {
                text: "full".into(),
            },
        );
        proc.process_envelope(&env).unwrap();

        assert_eq!(proc.stats().counts_by_type.get("assistant_delta"), Some(&3));
        assert_eq!(
            proc.stats().counts_by_type.get("assistant_message"),
            Some(&1)
        );
        assert_eq!(proc.stats().events_processed, 4);
    }

    #[test]
    fn process_many() {
        let mut proc = EventStreamProcessor::new("run-1".into());
        let envs = vec![
            make_event(
                "run-1",
                AgentEventKind::RunStarted {
                    message: "go".into(),
                },
            ),
            make_event(
                "run-1",
                AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
            ),
        ];
        let results = proc.process_many(&envs);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
        assert_eq!(proc.stats().events_processed, 2);
    }
}
