// SPDX-License-Identifier: MIT OR Apache-2.0
//! Message routing for dispatching envelopes to named destinations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Envelope;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single routing rule that maps a pattern to a destination handler.
///
/// The `pattern` is matched against the envelope type name (`hello`, `run`,
/// `event`, `final`, `fatal`) or, for envelopes that carry a `ref_id`, against
/// a `ref_id` prefix.  Higher `priority` values are evaluated first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRoute {
    /// Matching pattern — envelope type or ref_id prefix.
    pub pattern: String,
    /// Target handler name.
    pub destination: String,
    /// Higher values are evaluated first.
    pub priority: u32,
}

/// The result of successfully routing a single envelope.
#[derive(Debug, Clone)]
pub struct RouteMatch {
    /// The route that matched.
    pub route: MessageRoute,
    /// The envelope that was matched.
    pub envelope: Envelope,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the wire-level type name for an [`Envelope`] variant.
fn envelope_type(env: &Envelope) -> &'static str {
    match env {
        Envelope::Hello { .. } => "hello",
        Envelope::Run { .. } => "run",
        Envelope::Event { .. } => "event",
        Envelope::Final { .. } => "final",
        Envelope::Fatal { .. } => "fatal",
    }
}

/// Return the `ref_id` (or `id` for `Run`) carried by the envelope, if any.
fn envelope_ref_id(env: &Envelope) -> Option<&str> {
    match env {
        Envelope::Run { id, .. } => Some(id.as_str()),
        Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => Some(ref_id.as_str()),
        Envelope::Fatal { ref_id, .. } => ref_id.as_deref(),
        Envelope::Hello { .. } => None,
    }
}

/// Check whether `route` matches `envelope`.
fn matches(route: &MessageRoute, envelope: &Envelope) -> bool {
    let t = envelope_type(envelope);
    if route.pattern == t {
        return true;
    }
    if let Some(rid) = envelope_ref_id(envelope)
        && rid.starts_with(&route.pattern)
    {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// MessageRouter
// ---------------------------------------------------------------------------

/// Dispatches envelopes to destinations based on a prioritised set of routes.
#[derive(Debug, Clone, Default)]
pub struct MessageRouter {
    routes: Vec<MessageRoute>,
}

impl MessageRouter {
    /// Create an empty router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new route.  The route list is re-sorted by descending
    /// priority after each insertion so that [`Self::route`] always returns
    /// the highest-priority match.
    pub fn add_route(&mut self, route: MessageRoute) {
        self.routes.push(route);
        self.routes.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Find the highest-priority route that matches `envelope`, if any.
    #[must_use]
    pub fn route(&self, envelope: &Envelope) -> Option<&MessageRoute> {
        self.routes.iter().find(|r| matches(r, envelope))
    }

    /// Route every envelope, returning a [`RouteMatch`] for each one that
    /// matched at least one route.
    #[must_use]
    pub fn route_all(&self, envelopes: &[Envelope]) -> Vec<RouteMatch> {
        envelopes
            .iter()
            .filter_map(|env| {
                self.route(env).map(|r| RouteMatch {
                    route: r.clone(),
                    envelope: env.clone(),
                })
            })
            .collect()
    }

    /// Remove all routes whose destination equals `destination`.
    pub fn remove_route(&mut self, destination: &str) {
        self.routes.retain(|r| r.destination != destination);
    }

    /// Number of registered routes.
    #[must_use]
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

// ---------------------------------------------------------------------------
// RouteTable
// ---------------------------------------------------------------------------

/// A simple envelope-type → destination lookup table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteTable {
    table: BTreeMap<String, String>,
}

impl RouteTable {
    /// Create an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Map an envelope type to a destination, replacing any previous entry.
    pub fn insert(&mut self, envelope_type: &str, destination: &str) {
        self.table
            .insert(envelope_type.to_owned(), destination.to_owned());
    }

    /// Look up the destination for an envelope type.
    #[must_use]
    pub fn lookup(&self, envelope_type: &str) -> Option<&str> {
        self.table.get(envelope_type).map(String::as_str)
    }

    /// View the underlying map.
    #[must_use]
    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.table
    }
}
