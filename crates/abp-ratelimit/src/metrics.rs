#![allow(dead_code, unused_imports)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Snapshot of rate limit metrics at a point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSnapshot {
    /// Total requests recorded.
    pub total_requests: u64,
    /// Total throttled (rejected) requests.
    pub total_throttled: u64,
    /// Requests per second over the observation window.
    pub requests_per_sec: f64,
    /// Current queue depth (pending requests).
    pub queue_depth: u64,
    /// Average wait time for queued requests.
    pub avg_wait: Duration,
    /// Maximum wait time observed.
    pub max_wait: Duration,
}

/// Rate limit metrics collector.
///
/// Tracks request throughput, throttle events, queue depth, and wait times
/// over a configurable observation window.
#[derive(Debug, Clone)]
pub struct RateLimitMetrics {
    inner: Arc<Mutex<MetricsInner>>,
}

#[derive(Debug)]
struct MetricsInner {
    /// Observation window for per-second calculations.
    window: Duration,
    /// Timestamps of recent requests within the window.
    request_times: VecDeque<Instant>,
    /// Total requests ever recorded.
    total_requests: u64,
    /// Total throttled requests.
    total_throttled: u64,
    /// Current queue depth.
    queue_depth: u64,
    /// Recorded wait times for averaging.
    wait_times: VecDeque<Duration>,
    /// Max wait time observed.
    max_wait: Duration,
}

impl RateLimitMetrics {
    /// Create a new metrics collector with the given observation `window`.
    pub fn new(window: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MetricsInner {
                window,
                request_times: VecDeque::new(),
                total_requests: 0,
                total_throttled: 0,
                queue_depth: 0,
                wait_times: VecDeque::new(),
                max_wait: Duration::ZERO,
            })),
        }
    }

    /// Record a successful request.
    pub fn record_request(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.total_requests += 1;
        inner.request_times.push_back(Instant::now());
        inner.evict_old();
    }

    /// Record a throttled (rejected) request.
    pub fn record_throttle(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.total_throttled += 1;
    }

    /// Record the wait time a request spent queued before being dispatched.
    pub fn record_wait(&self, wait: Duration) {
        let mut inner = self.inner.lock().unwrap();
        if wait > inner.max_wait {
            inner.max_wait = wait;
        }
        inner.wait_times.push_back(wait);
        // Keep wait buffer bounded (last 1000 entries)
        while inner.wait_times.len() > 1000 {
            inner.wait_times.pop_front();
        }
    }

    /// Increment the queue depth by one.
    pub fn inc_queue_depth(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.queue_depth += 1;
    }

    /// Decrement the queue depth by one.
    pub fn dec_queue_depth(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.queue_depth = inner.queue_depth.saturating_sub(1);
    }

    /// Take a snapshot of the current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let mut inner = self.inner.lock().unwrap();
        inner.evict_old();

        let window_secs = inner.window.as_secs_f64().max(1.0);
        let rps = inner.request_times.len() as f64 / window_secs;

        let avg_wait = if inner.wait_times.is_empty() {
            Duration::ZERO
        } else {
            let total: Duration = inner.wait_times.iter().sum();
            total / inner.wait_times.len() as u32
        };

        MetricsSnapshot {
            total_requests: inner.total_requests,
            total_throttled: inner.total_throttled,
            requests_per_sec: rps,
            queue_depth: inner.queue_depth,
            avg_wait,
            max_wait: inner.max_wait,
        }
    }

    /// Reset all metrics to zero.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.total_requests = 0;
        inner.total_throttled = 0;
        inner.queue_depth = 0;
        inner.request_times.clear();
        inner.wait_times.clear();
        inner.max_wait = Duration::ZERO;
    }
}

impl MetricsInner {
    fn evict_old(&mut self) {
        let cutoff = Instant::now() - self.window;
        while let Some(&front) = self.request_times.front() {
            if front < cutoff {
                self.request_times.pop_front();
            } else {
                break;
            }
        }
    }
}

impl Default for RateLimitMetrics {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_snapshot_is_zero() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        let snap = metrics.snapshot();
        assert_eq!(snap.total_requests, 0);
        assert_eq!(snap.total_throttled, 0);
        assert_eq!(snap.queue_depth, 0);
        assert_eq!(snap.avg_wait, Duration::ZERO);
        assert_eq!(snap.max_wait, Duration::ZERO);
    }

    #[test]
    fn record_request_increments() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.record_request();
        metrics.record_request();
        metrics.record_request();
        let snap = metrics.snapshot();
        assert_eq!(snap.total_requests, 3);
    }

    #[test]
    fn record_throttle_increments() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.record_throttle();
        metrics.record_throttle();
        let snap = metrics.snapshot();
        assert_eq!(snap.total_throttled, 2);
    }

    #[test]
    fn requests_per_sec_calculation() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        for _ in 0..20 {
            metrics.record_request();
        }
        let snap = metrics.snapshot();
        // 20 requests in a 10-second window ≈ 2.0 rps
        assert!((snap.requests_per_sec - 2.0).abs() < 0.5);
    }

    #[test]
    fn queue_depth_tracking() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.inc_queue_depth();
        metrics.inc_queue_depth();
        assert_eq!(metrics.snapshot().queue_depth, 2);
        metrics.dec_queue_depth();
        assert_eq!(metrics.snapshot().queue_depth, 1);
    }

    #[test]
    fn queue_depth_does_not_underflow() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.dec_queue_depth();
        assert_eq!(metrics.snapshot().queue_depth, 0);
    }

    #[test]
    fn wait_time_tracking() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.record_wait(Duration::from_millis(100));
        metrics.record_wait(Duration::from_millis(300));
        let snap = metrics.snapshot();
        assert_eq!(snap.avg_wait, Duration::from_millis(200));
        assert_eq!(snap.max_wait, Duration::from_millis(300));
    }

    #[test]
    fn max_wait_is_tracked() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.record_wait(Duration::from_millis(10));
        metrics.record_wait(Duration::from_millis(500));
        metrics.record_wait(Duration::from_millis(50));
        assert_eq!(metrics.snapshot().max_wait, Duration::from_millis(500));
    }

    #[test]
    fn reset_clears_all() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.record_request();
        metrics.record_throttle();
        metrics.inc_queue_depth();
        metrics.record_wait(Duration::from_millis(100));
        metrics.reset();
        let snap = metrics.snapshot();
        assert_eq!(snap.total_requests, 0);
        assert_eq!(snap.total_throttled, 0);
        assert_eq!(snap.queue_depth, 0);
        assert_eq!(snap.max_wait, Duration::ZERO);
    }

    #[test]
    fn default_uses_60s_window() {
        let metrics = RateLimitMetrics::default();
        // Just verify it works
        metrics.record_request();
        assert_eq!(metrics.snapshot().total_requests, 1);
    }

    #[test]
    fn clone_shares_state() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        let clone = metrics.clone();
        metrics.record_request();
        metrics.record_request();
        assert_eq!(clone.snapshot().total_requests, 2);
    }

    #[test]
    fn snapshot_is_consistent() {
        let metrics = RateLimitMetrics::new(Duration::from_secs(10));
        metrics.record_request();
        metrics.record_throttle();
        metrics.inc_queue_depth();
        metrics.record_wait(Duration::from_millis(50));
        let snap = metrics.snapshot();
        assert_eq!(snap.total_requests, 1);
        assert_eq!(snap.total_throttled, 1);
        assert_eq!(snap.queue_depth, 1);
        assert_eq!(snap.avg_wait, Duration::from_millis(50));
        assert_eq!(snap.max_wait, Duration::from_millis(50));
    }
}
