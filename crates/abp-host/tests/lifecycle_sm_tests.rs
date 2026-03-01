// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the lifecycle state machine in abp-host.

use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};

// ---------------------------------------------------------------------------
// 1. Initial state
// ---------------------------------------------------------------------------

#[test]
fn initial_state_is_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn default_equals_new() {
    let mgr = LifecycleManager::default();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

// ---------------------------------------------------------------------------
// 2. Valid forward transitions
// ---------------------------------------------------------------------------

#[test]
fn transition_uninitialized_to_starting() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Starting);
}

#[test]
fn transition_starting_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn transition_ready_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Running);
}

#[test]
fn transition_running_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn transition_ready_to_stopping() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopping);
}

#[test]
fn transition_running_to_stopping() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopping);
}

#[test]
fn transition_stopping_to_stopped() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

// ---------------------------------------------------------------------------
// 3. Failed transition from any state
// ---------------------------------------------------------------------------

#[test]
fn transition_uninitialized_to_failed() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Failed, Some("crash".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);
}

#[test]
fn transition_starting_to_failed() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Failed, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);
}

#[test]
fn transition_running_to_failed() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Failed, Some("oom".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);
}

#[test]
fn transition_stopping_to_failed() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Failed, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);
}

// ---------------------------------------------------------------------------
// 4. Invalid transitions
// ---------------------------------------------------------------------------

#[test]
fn invalid_uninitialized_to_running() {
    let mut mgr = LifecycleManager::new();
    let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
    assert_eq!(
        err,
        LifecycleError::InvalidTransition {
            from: LifecycleState::Uninitialized,
            to: LifecycleState::Running,
        }
    );
}

#[test]
fn invalid_ready_to_starting() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let err = mgr.transition(LifecycleState::Starting, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn invalid_stopped_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

// ---------------------------------------------------------------------------
// 5. Already-in-state error
// ---------------------------------------------------------------------------

#[test]
fn already_in_state_error() {
    let mut mgr = LifecycleManager::new();
    let err = mgr
        .transition(LifecycleState::Uninitialized, None)
        .unwrap_err();
    assert_eq!(
        err,
        LifecycleError::AlreadyInState(LifecycleState::Uninitialized)
    );
}

#[test]
fn already_in_state_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let err = mgr.transition(LifecycleState::Ready, None).unwrap_err();
    assert_eq!(err, LifecycleError::AlreadyInState(LifecycleState::Ready));
}

// ---------------------------------------------------------------------------
// 6. can_transition
// ---------------------------------------------------------------------------

#[test]
fn can_transition_reports_correctly() {
    let mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Running));
    assert!(!mgr.can_transition(&LifecycleState::Stopped));
    // Failed is always reachable.
    assert!(mgr.can_transition(&LifecycleState::Failed));
}

// ---------------------------------------------------------------------------
// 7. History tracking
// ---------------------------------------------------------------------------

#[test]
fn history_is_empty_initially() {
    let mgr = LifecycleManager::new();
    assert!(mgr.history().is_empty());
}

#[test]
fn history_records_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();

    let h = mgr.history();
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].from, LifecycleState::Uninitialized);
    assert_eq!(h[0].to, LifecycleState::Starting);
    assert_eq!(h[0].reason.as_deref(), Some("boot"));
    assert_eq!(h[1].from, LifecycleState::Starting);
    assert_eq!(h[1].to, LifecycleState::Ready);
    assert!(h[1].reason.is_none());
}

#[test]
fn history_not_recorded_on_error() {
    let mut mgr = LifecycleManager::new();
    let _ = mgr.transition(LifecycleState::Running, None);
    assert!(mgr.history().is_empty());
}

// ---------------------------------------------------------------------------
// 8. Uptime tracking
// ---------------------------------------------------------------------------

#[test]
fn uptime_none_before_ready() {
    let mgr = LifecycleManager::new();
    assert!(mgr.uptime().is_none());
}

#[test]
fn uptime_available_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let up = mgr.uptime();
    assert!(up.is_some());
}

#[test]
fn uptime_persists_through_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert!(mgr.uptime().is_some());
}

// ---------------------------------------------------------------------------
// 9. Display and Error impls
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_error_display_invalid_transition() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Uninitialized,
        to: LifecycleState::Running,
    };
    let msg = err.to_string();
    assert!(msg.contains("invalid lifecycle transition"));
    assert!(msg.contains("uninitialized"));
    assert!(msg.contains("running"));
}

#[test]
fn lifecycle_error_display_already_in_state() {
    let err = LifecycleError::AlreadyInState(LifecycleState::Ready);
    let msg = err.to_string();
    assert!(msg.contains("already in state"));
    assert!(msg.contains("ready"));
}

#[test]
fn lifecycle_error_is_std_error() {
    let err: Box<dyn std::error::Error> =
        Box::new(LifecycleError::AlreadyInState(LifecycleState::Stopped));
    assert!(err.to_string().contains("already in state"));
}

// ---------------------------------------------------------------------------
// 10. State Display impl
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_state_display() {
    assert_eq!(LifecycleState::Uninitialized.to_string(), "uninitialized");
    assert_eq!(LifecycleState::Starting.to_string(), "starting");
    assert_eq!(LifecycleState::Ready.to_string(), "ready");
    assert_eq!(LifecycleState::Running.to_string(), "running");
    assert_eq!(LifecycleState::Stopping.to_string(), "stopping");
    assert_eq!(LifecycleState::Stopped.to_string(), "stopped");
    assert_eq!(LifecycleState::Failed.to_string(), "failed");
}

// ---------------------------------------------------------------------------
// 11. Serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_state_serde_roundtrip() {
    let states = vec![
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
        LifecycleState::Failed,
    ];
    for s in states {
        let json = serde_json::to_string(&s).unwrap();
        let de: LifecycleState = serde_json::from_str(&json).unwrap();
        assert_eq!(de, s);
    }
}

// ---------------------------------------------------------------------------
// 12. Full lifecycle path
// ---------------------------------------------------------------------------

#[test]
fn full_lifecycle_happy_path() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 6);
}
