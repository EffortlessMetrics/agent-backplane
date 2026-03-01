// SPDX-License-Identifier: MIT OR Apache-2.0
//! Priority-based run queue for the ABP daemon.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Priority levels for queued runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuePriority {
    /// Lowest priority.
    Low,
    /// Default priority.
    Normal,
    /// Elevated priority.
    High,
    /// Highest priority — processed before all others.
    Critical,
}

/// A work-order run waiting in the queue.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueuedRun {
    /// Unique queue entry identifier.
    pub id: String,
    /// Associated work order identifier.
    pub work_order_id: String,
    /// Priority level for scheduling.
    pub priority: QueuePriority,
    /// ISO-8601 timestamp when the run was enqueued.
    pub queued_at: String,
    /// Target backend name, if specified.
    pub backend: Option<String>,
    /// Arbitrary key-value metadata.
    pub metadata: BTreeMap<String, String>,
}

/// Errors returned by [`RunQueue`] operations.
#[derive(Debug)]
pub enum QueueError {
    /// The queue has reached its maximum capacity.
    Full {
        /// Maximum number of items the queue can hold.
        max: usize,
    },
    /// A run with the given ID is already enqueued.
    DuplicateId(String),
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueueError::Full { max } => write!(f, "queue is full (max {max})"),
            QueueError::DuplicateId(id) => write!(f, "duplicate queue entry: {id}"),
        }
    }
}

impl std::error::Error for QueueError {}

/// Snapshot statistics for a [`RunQueue`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueStats {
    /// Number of items currently in the queue.
    pub total: usize,
    /// Maximum queue capacity.
    pub max: usize,
    /// Breakdown of items per priority level.
    pub by_priority: BTreeMap<String, usize>,
}

/// A bounded, priority-aware run queue.
///
/// [`dequeue`](RunQueue::dequeue) returns the highest-priority item first;
/// among items of equal priority the oldest (FIFO) item is returned.
pub struct RunQueue {
    entries: Vec<QueuedRun>,
    max_size: usize,
}

impl RunQueue {
    /// Create a new queue with the given maximum capacity.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_size,
        }
    }

    /// Add a run to the queue. Returns an error if the queue is full or the ID
    /// already exists.
    pub fn enqueue(&mut self, run: QueuedRun) -> Result<(), QueueError> {
        if self.entries.len() >= self.max_size {
            return Err(QueueError::Full { max: self.max_size });
        }
        if self.entries.iter().any(|r| r.id == run.id) {
            return Err(QueueError::DuplicateId(run.id));
        }
        self.entries.push(run);
        Ok(())
    }

    /// Remove and return the highest-priority run (FIFO within the same
    /// priority level).
    pub fn dequeue(&mut self) -> Option<QueuedRun> {
        if self.entries.is_empty() {
            return None;
        }
        let max_pri = self.entries.iter().map(|r| r.priority).max().unwrap();
        let idx = self
            .entries
            .iter()
            .position(|r| r.priority == max_pri)
            .unwrap();
        Some(self.entries.remove(idx))
    }

    /// Peek at the next run that would be dequeued without removing it.
    pub fn peek(&self) -> Option<&QueuedRun> {
        let max_pri = self.entries.iter().map(|r| r.priority).max()?;
        self.entries.iter().find(|r| r.priority == max_pri)
    }

    /// Return the number of queued runs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if the queue contains no runs.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return `true` if the queue has reached its maximum capacity.
    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.max_size
    }

    /// Remove a specific run by ID, returning it if found.
    pub fn remove(&mut self, id: &str) -> Option<QueuedRun> {
        let pos = self.entries.iter().position(|r| r.id == id)?;
        Some(self.entries.remove(pos))
    }

    /// Remove all entries from the queue.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Return references to all runs matching the given priority.
    pub fn by_priority(&self, priority: QueuePriority) -> Vec<&QueuedRun> {
        self.entries
            .iter()
            .filter(|r| r.priority == priority)
            .collect()
    }

    /// Return a snapshot of queue statistics.
    pub fn stats(&self) -> QueueStats {
        let mut by_priority = BTreeMap::new();
        for entry in &self.entries {
            let key = match entry.priority {
                QueuePriority::Low => "low",
                QueuePriority::Normal => "normal",
                QueuePriority::High => "high",
                QueuePriority::Critical => "critical",
            };
            *by_priority.entry(key.to_string()).or_insert(0usize) += 1;
        }
        QueueStats {
            total: self.entries.len(),
            max: self.max_size,
            by_priority,
        }
    }
}
