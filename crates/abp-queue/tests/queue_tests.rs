// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_queue::{QueueError, QueuePriority, QueueStats, QueuedRun, RunQueue};
use std::collections::BTreeMap;

fn make_run(id: &str, priority: QueuePriority) -> QueuedRun {
    QueuedRun {
        id: id.to_string(),
        work_order_id: format!("wo-{id}"),
        priority,
        queued_at: "2025-01-01T00:00:00Z".to_string(),
        backend: None,
        metadata: BTreeMap::new(),
    }
}

#[test]
fn new_queue_is_empty() {
    let q = RunQueue::new(10);
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
}

#[test]
fn enqueue_increments_len() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    assert_eq!(q.len(), 1);
    assert!(!q.is_empty());
}

#[test]
fn dequeue_returns_none_when_empty() {
    let mut q = RunQueue::new(10);
    assert!(q.dequeue().is_none());
}

#[test]
fn dequeue_returns_highest_priority() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("low", QueuePriority::Low)).unwrap();
    q.enqueue(make_run("crit", QueuePriority::Critical))
        .unwrap();
    q.enqueue(make_run("norm", QueuePriority::Normal)).unwrap();
    let got = q.dequeue().unwrap();
    assert_eq!(got.id, "crit");
}

#[test]
fn dequeue_fifo_within_same_priority() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("first", QueuePriority::High)).unwrap();
    q.enqueue(make_run("second", QueuePriority::High)).unwrap();
    let got = q.dequeue().unwrap();
    assert_eq!(got.id, "first");
}

#[test]
fn peek_does_not_remove() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    assert!(q.peek().is_some());
    assert_eq!(q.len(), 1);
}

#[test]
fn peek_returns_highest_priority() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("low", QueuePriority::Low)).unwrap();
    q.enqueue(make_run("high", QueuePriority::High)).unwrap();
    assert_eq!(q.peek().unwrap().id, "high");
}

#[test]
fn is_full_when_at_capacity() {
    let mut q = RunQueue::new(2);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    q.enqueue(make_run("b", QueuePriority::Normal)).unwrap();
    assert!(q.is_full());
}

#[test]
fn enqueue_full_returns_error() {
    let mut q = RunQueue::new(1);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    let err = q.enqueue(make_run("b", QueuePriority::Normal)).unwrap_err();
    match err {
        QueueError::Full { max } => assert_eq!(max, 1),
        other => panic!("expected Full, got {other:?}"),
    }
}

#[test]
fn enqueue_duplicate_id_returns_error() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("dup", QueuePriority::Normal)).unwrap();
    let err = q.enqueue(make_run("dup", QueuePriority::High)).unwrap_err();
    match err {
        QueueError::DuplicateId(id) => assert_eq!(id, "dup"),
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

#[test]
fn remove_by_id() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    q.enqueue(make_run("b", QueuePriority::High)).unwrap();
    let removed = q.remove("a").unwrap();
    assert_eq!(removed.id, "a");
    assert_eq!(q.len(), 1);
}

#[test]
fn remove_missing_returns_none() {
    let mut q = RunQueue::new(10);
    assert!(q.remove("nope").is_none());
}

#[test]
fn clear_empties_the_queue() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    q.enqueue(make_run("b", QueuePriority::High)).unwrap();
    q.clear();
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
}

#[test]
fn by_priority_filters_correctly() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("lo1", QueuePriority::Low)).unwrap();
    q.enqueue(make_run("hi1", QueuePriority::High)).unwrap();
    q.enqueue(make_run("lo2", QueuePriority::Low)).unwrap();
    let lows = q.by_priority(QueuePriority::Low);
    assert_eq!(lows.len(), 2);
    assert!(lows.iter().all(|r| r.priority == QueuePriority::Low));
}

#[test]
fn by_priority_empty_for_missing() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("a", QueuePriority::Low)).unwrap();
    assert!(q.by_priority(QueuePriority::Critical).is_empty());
}

#[test]
fn stats_reports_counts() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("a", QueuePriority::Low)).unwrap();
    q.enqueue(make_run("b", QueuePriority::Low)).unwrap();
    q.enqueue(make_run("c", QueuePriority::High)).unwrap();
    let stats = q.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.max, 10);
    assert_eq!(stats.by_priority.get("low"), Some(&2));
    assert_eq!(stats.by_priority.get("high"), Some(&1));
    assert_eq!(stats.by_priority.get("critical"), None);
}

#[test]
fn full_drain_order() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_run("lo", QueuePriority::Low)).unwrap();
    q.enqueue(make_run("norm", QueuePriority::Normal)).unwrap();
    q.enqueue(make_run("hi", QueuePriority::High)).unwrap();
    q.enqueue(make_run("crit", QueuePriority::Critical))
        .unwrap();
    let order: Vec<String> = std::iter::from_fn(|| q.dequeue()).map(|r| r.id).collect();
    assert_eq!(order, vec!["crit", "hi", "norm", "lo"]);
}

#[test]
fn queue_error_display_full() {
    let err = QueueError::Full { max: 5 };
    assert_eq!(err.to_string(), "queue is full (max 5)");
}

#[test]
fn queue_error_display_duplicate() {
    let err = QueueError::DuplicateId("x".to_string());
    assert_eq!(err.to_string(), "duplicate queue entry: x");
}

#[test]
fn queue_priority_ordering() {
    assert!(QueuePriority::Low < QueuePriority::Normal);
    assert!(QueuePriority::Normal < QueuePriority::High);
    assert!(QueuePriority::High < QueuePriority::Critical);
}

#[test]
fn queued_run_serialization_roundtrip() {
    let run = make_run("rt", QueuePriority::High);
    let json = serde_json::to_string(&run).unwrap();
    let back: QueuedRun = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "rt");
    assert_eq!(back.priority, QueuePriority::High);
}

#[test]
fn stats_serialization_roundtrip() {
    let mut q = RunQueue::new(5);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    let stats = q.stats();
    let json = serde_json::to_string(&stats).unwrap();
    let back: QueueStats = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total, 1);
    assert_eq!(back.max, 5);
}

#[test]
fn enqueue_after_dequeue_respects_capacity() {
    let mut q = RunQueue::new(1);
    q.enqueue(make_run("a", QueuePriority::Normal)).unwrap();
    assert!(q.is_full());
    q.dequeue();
    assert!(!q.is_full());
    q.enqueue(make_run("b", QueuePriority::Normal)).unwrap();
    assert_eq!(q.len(), 1);
}

#[test]
fn backend_and_metadata_preserved() {
    let mut run = make_run("m", QueuePriority::Normal);
    run.backend = Some("openai".to_string());
    run.metadata.insert("key".to_string(), "value".to_string());
    let mut q = RunQueue::new(10);
    q.enqueue(run).unwrap();
    let got = q.dequeue().unwrap();
    assert_eq!(got.backend.as_deref(), Some("openai"));
    assert_eq!(got.metadata.get("key").map(|s| s.as_str()), Some("value"));
}
