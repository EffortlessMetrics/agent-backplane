// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_daemon::queue::{QueuePriority, QueuedRun, RunQueue};
use std::collections::BTreeMap;

#[test]
fn daemon_queue_module_reexports_run_queue_types() {
    let mut q = RunQueue::new(1);
    q.enqueue(QueuedRun {
        id: "q1".to_string(),
        work_order_id: "wo-q1".to_string(),
        priority: QueuePriority::Normal,
        queued_at: "2025-01-01T00:00:00Z".to_string(),
        backend: None,
        metadata: BTreeMap::new(),
    })
    .unwrap();

    assert_eq!(q.dequeue().unwrap().id, "q1");
}
