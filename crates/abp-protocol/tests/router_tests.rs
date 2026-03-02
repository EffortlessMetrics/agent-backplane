// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the message routing module.

use abp_core::{BackendIdentity, CapabilityManifest};
use abp_protocol::Envelope;
use abp_protocol::router::{MessageRoute, MessageRouter, RouteTable};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fatal(ref_id: Option<&str>, msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(Into::into),
        error: msg.into(),
        error_code: None,
    }
}

fn hello(id: &str) -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: id.into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn event_envelope(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: abp_core::AgentEvent {
            ts: chrono::Utc::now(),
            kind: abp_core::AgentEventKind::Warning {
                message: "test".into(),
            },
            ext: None,
        },
    }
}

fn route(pattern: &str, dest: &str, priority: u32) -> MessageRoute {
    MessageRoute {
        pattern: pattern.into(),
        destination: dest.into(),
        priority,
    }
}

// ---------------------------------------------------------------------------
// MessageRoute serde
// ---------------------------------------------------------------------------

#[test]
fn message_route_roundtrip_serde() {
    let r = route("hello", "handler-a", 10);
    let json = serde_json::to_string(&r).unwrap();
    let decoded: MessageRoute = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.pattern, "hello");
    assert_eq!(decoded.destination, "handler-a");
    assert_eq!(decoded.priority, 10);
}

// ---------------------------------------------------------------------------
// MessageRouter — basic matching
// ---------------------------------------------------------------------------

#[test]
fn router_matches_by_envelope_type() {
    let mut router = MessageRouter::new();
    router.add_route(route("hello", "hello-handler", 1));

    let env = hello("sidecar-1");
    let m = router.route(&env).unwrap();
    assert_eq!(m.destination, "hello-handler");
}

#[test]
fn router_returns_none_when_no_match() {
    let mut router = MessageRouter::new();
    router.add_route(route("run", "run-handler", 1));

    let env = fatal(None, "boom");
    assert!(router.route(&env).is_none());
}

#[test]
fn router_matches_fatal_by_type() {
    let mut router = MessageRouter::new();
    router.add_route(route("fatal", "error-handler", 1));

    let env = fatal(None, "oops");
    let m = router.route(&env).unwrap();
    assert_eq!(m.destination, "error-handler");
}

#[test]
fn router_matches_event_by_type() {
    let mut router = MessageRouter::new();
    router.add_route(route("event", "event-sink", 5));

    let env = event_envelope("run-1");
    let m = router.route(&env).unwrap();
    assert_eq!(m.destination, "event-sink");
}

// ---------------------------------------------------------------------------
// ref_id prefix matching
// ---------------------------------------------------------------------------

#[test]
fn router_matches_by_ref_id_prefix() {
    let mut router = MessageRouter::new();
    router.add_route(route("run-42", "special-handler", 10));

    let env = event_envelope("run-42-abc");
    let m = router.route(&env).unwrap();
    assert_eq!(m.destination, "special-handler");
}

#[test]
fn router_ref_id_prefix_no_match() {
    let mut router = MessageRouter::new();
    router.add_route(route("run-99", "handler-99", 10));

    let env = event_envelope("run-42-abc");
    assert!(router.route(&env).is_none());
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

#[test]
fn router_higher_priority_wins() {
    let mut router = MessageRouter::new();
    router.add_route(route("fatal", "low", 1));
    router.add_route(route("fatal", "high", 100));

    let env = fatal(None, "err");
    let m = router.route(&env).unwrap();
    assert_eq!(m.destination, "high");
}

#[test]
fn router_priority_order_independent_of_insertion() {
    let mut router = MessageRouter::new();
    router.add_route(route("event", "high", 50));
    router.add_route(route("event", "low", 1));
    router.add_route(route("event", "mid", 25));

    let env = event_envelope("x");
    let m = router.route(&env).unwrap();
    assert_eq!(m.destination, "high");
}

// ---------------------------------------------------------------------------
// route_all
// ---------------------------------------------------------------------------

#[test]
fn route_all_routes_matching_envelopes() {
    let mut router = MessageRouter::new();
    router.add_route(route("fatal", "err-handler", 1));
    router.add_route(route("hello", "hello-handler", 1));

    let envelopes = vec![
        fatal(None, "boom"),
        hello("s1"),
        event_envelope("r1"), // no matching route
    ];
    let matches = router.route_all(&envelopes);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].route.destination, "err-handler");
    assert_eq!(matches[1].route.destination, "hello-handler");
}

#[test]
fn route_all_empty_input() {
    let router = MessageRouter::new();
    let matches = router.route_all(&[]);
    assert!(matches.is_empty());
}

// ---------------------------------------------------------------------------
// remove_route / route_count
// ---------------------------------------------------------------------------

#[test]
fn remove_route_by_destination() {
    let mut router = MessageRouter::new();
    router.add_route(route("hello", "handler-a", 1));
    router.add_route(route("fatal", "handler-b", 2));
    router.add_route(route("event", "handler-a", 3));

    assert_eq!(router.route_count(), 3);
    router.remove_route("handler-a");
    assert_eq!(router.route_count(), 1);

    // Only handler-b remains.
    assert!(router.route(&hello("x")).is_none());
    assert!(router.route(&fatal(None, "x")).is_some());
}

#[test]
fn remove_nonexistent_destination_is_noop() {
    let mut router = MessageRouter::new();
    router.add_route(route("hello", "h", 1));
    router.remove_route("does-not-exist");
    assert_eq!(router.route_count(), 1);
}

#[test]
fn route_count_empty() {
    let router = MessageRouter::new();
    assert_eq!(router.route_count(), 0);
}

// ---------------------------------------------------------------------------
// RouteTable
// ---------------------------------------------------------------------------

#[test]
fn route_table_insert_and_lookup() {
    let mut table = RouteTable::new();
    table.insert("hello", "hello-dest");
    table.insert("fatal", "fatal-dest");

    assert_eq!(table.lookup("hello"), Some("hello-dest"));
    assert_eq!(table.lookup("fatal"), Some("fatal-dest"));
    assert_eq!(table.lookup("run"), None);
}

#[test]
fn route_table_overwrite() {
    let mut table = RouteTable::new();
    table.insert("hello", "old");
    table.insert("hello", "new");
    assert_eq!(table.lookup("hello"), Some("new"));
}

#[test]
fn route_table_entries_returns_btreemap() {
    let mut table = RouteTable::new();
    table.insert("b", "dest-b");
    table.insert("a", "dest-a");

    let entries = table.entries();
    assert_eq!(entries.len(), 2);
    // BTreeMap is sorted by key.
    let keys: Vec<&String> = entries.keys().collect();
    assert_eq!(keys, vec!["a", "b"]);
}

#[test]
fn route_table_roundtrip_serde() {
    let mut table = RouteTable::new();
    table.insert("event", "sink");
    let json = serde_json::to_string(&table).unwrap();
    let decoded: RouteTable = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.lookup("event"), Some("sink"));
}
