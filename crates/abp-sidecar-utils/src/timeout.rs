// SPDX-License-Identifier: MIT OR Apache-2.0
//! Timeout management for sidecar operations.
//!
//! [`TimeoutManager`] provides configurable per-phase timeouts for the
//! sidecar protocol lifecycle: handshake, run dispatch, and overall
//! execution. It also tracks elapsed time and remaining budgets.

use std::time::{Duration, Instant};

use thiserror::Error;

/// Default handshake timeout (10 seconds).
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Default run dispatch timeout (30 seconds).
pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(30);

/// Default overall execution timeout (10 minutes).
pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(600);

/// Errors from timeout management.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TimeoutError {
    /// The handshake phase timed out.
    #[error("handshake timed out after {0:?}")]
    Handshake(Duration),
    /// The run dispatch phase timed out.
    #[error("run dispatch timed out after {0:?}")]
    RunDispatch(Duration),
    /// The overall execution timed out.
    #[error("execution timed out after {0:?}")]
    Execution(Duration),
}

/// Per-phase timeout configuration.
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Maximum time to wait for the hello handshake.
    pub handshake: Duration,
    /// Maximum time to wait for a run dispatch.
    pub run_dispatch: Duration,
    /// Maximum total execution time.
    pub execution: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            handshake: DEFAULT_HANDSHAKE_TIMEOUT,
            run_dispatch: DEFAULT_RUN_TIMEOUT,
            execution: DEFAULT_EXECUTION_TIMEOUT,
        }
    }
}

/// Protocol phase being tracked by the timeout manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutPhase {
    /// Waiting for the hello handshake.
    Handshake,
    /// Waiting for a run to be dispatched.
    RunDispatch,
    /// Running the work order.
    Execution,
    /// All phases complete.
    Done,
}

/// Manages per-phase timeouts for the sidecar protocol lifecycle.
///
/// # Examples
///
/// ```
/// use abp_sidecar_utils::timeout::TimeoutManager;
/// use std::time::Duration;
///
/// let mut tm = TimeoutManager::new_default();
/// assert!(!tm.is_expired());
///
/// // Advance through phases
/// tm.advance_to_run_dispatch();
/// tm.advance_to_execution();
/// tm.mark_done();
/// ```
#[derive(Debug)]
pub struct TimeoutManager {
    config: TimeoutConfig,
    started_at: Instant,
    phase: TimeoutPhase,
    phase_started_at: Instant,
}

impl TimeoutManager {
    /// Create a manager with the given configuration.
    #[must_use]
    pub fn new(config: TimeoutConfig) -> Self {
        let now = Instant::now();
        Self {
            config,
            started_at: now,
            phase: TimeoutPhase::Handshake,
            phase_started_at: now,
        }
    }

    /// Create a manager with default timeouts.
    #[must_use]
    pub fn new_default() -> Self {
        Self::new(TimeoutConfig::default())
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> TimeoutPhase {
        self.phase
    }

    /// Total elapsed time since creation.
    #[must_use]
    pub fn total_elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Elapsed time in the current phase.
    #[must_use]
    pub fn phase_elapsed(&self) -> Duration {
        self.phase_started_at.elapsed()
    }

    /// Remaining time in the current phase, or zero if expired.
    #[must_use]
    pub fn phase_remaining(&self) -> Duration {
        let limit = self.current_phase_limit();
        let elapsed = self.phase_elapsed();
        limit.saturating_sub(elapsed)
    }

    /// Whether the current phase has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        if self.phase == TimeoutPhase::Done {
            return false;
        }
        self.phase_elapsed() > self.current_phase_limit()
            || self.total_elapsed() > self.config.execution
    }

    /// Check the current phase and return an error if it has expired.
    pub fn check(&self) -> Result<(), TimeoutError> {
        if self.phase == TimeoutPhase::Done {
            return Ok(());
        }

        if self.total_elapsed() > self.config.execution {
            return Err(TimeoutError::Execution(self.config.execution));
        }

        let elapsed = self.phase_elapsed();
        let limit = self.current_phase_limit();
        if elapsed > limit {
            match self.phase {
                TimeoutPhase::Handshake => Err(TimeoutError::Handshake(limit)),
                TimeoutPhase::RunDispatch => Err(TimeoutError::RunDispatch(limit)),
                TimeoutPhase::Execution => Err(TimeoutError::Execution(limit)),
                TimeoutPhase::Done => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    /// Advance to the run-dispatch phase.
    pub fn advance_to_run_dispatch(&mut self) {
        self.phase = TimeoutPhase::RunDispatch;
        self.phase_started_at = Instant::now();
    }

    /// Advance to the execution phase.
    pub fn advance_to_execution(&mut self) {
        self.phase = TimeoutPhase::Execution;
        self.phase_started_at = Instant::now();
    }

    /// Mark all phases as done.
    pub fn mark_done(&mut self) {
        self.phase = TimeoutPhase::Done;
    }

    /// The timeout configuration.
    #[must_use]
    pub fn config(&self) -> &TimeoutConfig {
        &self.config
    }

    fn current_phase_limit(&self) -> Duration {
        match self.phase {
            TimeoutPhase::Handshake => self.config.handshake,
            TimeoutPhase::RunDispatch => self.config.run_dispatch,
            TimeoutPhase::Execution => self.config.execution,
            TimeoutPhase::Done => Duration::MAX,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = TimeoutConfig::default();
        assert_eq!(cfg.handshake, DEFAULT_HANDSHAKE_TIMEOUT);
        assert_eq!(cfg.run_dispatch, DEFAULT_RUN_TIMEOUT);
        assert_eq!(cfg.execution, DEFAULT_EXECUTION_TIMEOUT);
    }

    #[test]
    fn initial_phase_is_handshake() {
        let tm = TimeoutManager::new_default();
        assert_eq!(tm.phase(), TimeoutPhase::Handshake);
    }

    #[test]
    fn not_expired_immediately() {
        let tm = TimeoutManager::new_default();
        assert!(!tm.is_expired());
        assert!(tm.check().is_ok());
    }

    #[test]
    fn phase_transitions() {
        let mut tm = TimeoutManager::new_default();
        assert_eq!(tm.phase(), TimeoutPhase::Handshake);

        tm.advance_to_run_dispatch();
        assert_eq!(tm.phase(), TimeoutPhase::RunDispatch);

        tm.advance_to_execution();
        assert_eq!(tm.phase(), TimeoutPhase::Execution);

        tm.mark_done();
        assert_eq!(tm.phase(), TimeoutPhase::Done);
        assert!(!tm.is_expired());
    }

    #[test]
    fn done_phase_check_always_ok() {
        let mut tm = TimeoutManager::new_default();
        tm.mark_done();
        assert!(tm.check().is_ok());
        assert!(!tm.is_expired());
    }

    #[test]
    fn handshake_timeout_detected() {
        let cfg = TimeoutConfig {
            handshake: Duration::from_millis(1),
            run_dispatch: Duration::from_secs(30),
            execution: Duration::from_secs(600),
        };
        let tm = TimeoutManager::new(cfg);
        std::thread::sleep(Duration::from_millis(5));
        assert!(tm.is_expired());
        let err = tm.check().unwrap_err();
        assert!(matches!(err, TimeoutError::Handshake(_)));
    }

    #[test]
    fn execution_timeout_overrides_phase() {
        let cfg = TimeoutConfig {
            handshake: Duration::from_secs(600),
            run_dispatch: Duration::from_secs(600),
            execution: Duration::from_millis(1),
        };
        let tm = TimeoutManager::new(cfg);
        std::thread::sleep(Duration::from_millis(5));
        let err = tm.check().unwrap_err();
        assert!(matches!(err, TimeoutError::Execution(_)));
    }

    #[test]
    fn phase_remaining_decreases() {
        let cfg = TimeoutConfig {
            handshake: Duration::from_secs(10),
            run_dispatch: Duration::from_secs(30),
            execution: Duration::from_secs(600),
        };
        let tm = TimeoutManager::new(cfg);
        let remaining = tm.phase_remaining();
        assert!(remaining <= Duration::from_secs(10));
        assert!(remaining > Duration::from_secs(9));
    }

    #[test]
    fn total_elapsed_increases() {
        let tm = TimeoutManager::new_default();
        std::thread::sleep(Duration::from_millis(5));
        assert!(tm.total_elapsed() >= Duration::from_millis(1));
    }

    #[test]
    fn error_display_messages() {
        let e = TimeoutError::Handshake(Duration::from_secs(10));
        assert!(e.to_string().contains("handshake"));

        let e = TimeoutError::RunDispatch(Duration::from_secs(30));
        assert!(e.to_string().contains("run dispatch"));

        let e = TimeoutError::Execution(Duration::from_secs(600));
        assert!(e.to_string().contains("execution"));
    }

    #[test]
    fn config_accessor() {
        let cfg = TimeoutConfig {
            handshake: Duration::from_secs(5),
            run_dispatch: Duration::from_secs(15),
            execution: Duration::from_secs(300),
        };
        let tm = TimeoutManager::new(cfg.clone());
        assert_eq!(tm.config().handshake, Duration::from_secs(5));
        assert_eq!(tm.config().run_dispatch, Duration::from_secs(15));
        assert_eq!(tm.config().execution, Duration::from_secs(300));
    }
}
