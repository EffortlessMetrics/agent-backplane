// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-policy` using proptest.

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use proptest::prelude::*;
use std::path::Path;

/// Strategy producing simple tool names (alphabetic, 1-12 chars).
fn tool_name() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9]{0,11}".prop_map(|s| s.to_string())
}

/// Strategy producing simple safe path segments.
fn path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(|s| s.to_string())
}

/// Strategy producing a relative file path like `dir/subdir/file.ext`.
fn relative_path() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(path_segment(), 1..=4),
        "[a-z]{1,4}",
    )
        .prop_map(|(segs, ext)| {
            let joined = segs.join("/");
            format!("{joined}.{ext}")
        })
}

// ── 1. Allowing a tool ⇒ can_use_tool returns allowed ──────────────

proptest! {
    #[test]
    fn allowed_tool_is_permitted(name in tool_name()) {
        // Wildcard allowlist, no deny → every tool is permitted.
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_use_tool(&name).allowed);
    }
}

// ── 2. Denying read path ⇒ can_read_path returns denied ────────────

proptest! {
    #[test]
    fn denied_read_path_is_blocked(path in relative_path()) {
        let policy = PolicyProfile {
            deny_read: vec!["**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_read_path(Path::new(&path)).allowed);
    }
}

// ── 3. Denying write path ⇒ can_write_path returns denied ──────────

proptest! {
    #[test]
    fn denied_write_path_is_blocked(path in relative_path()) {
        let policy = PolicyProfile {
            deny_write: vec!["**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_write_path(Path::new(&path)).allowed);
    }
}

// ── 4. Default (empty) policy allows everything ─────────────────────

proptest! {
    #[test]
    fn default_policy_allows_all(
        tool in tool_name(),
        path in relative_path(),
    ) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }
}

// ── 5. Deny always overrides allow for tools ────────────────────────

proptest! {
    #[test]
    fn deny_overrides_allow_for_tools(name in tool_name()) {
        // Allow everything, but also deny everything.
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec!["*".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&name).allowed);
    }
}

// ── 6. Specific deny_read prefix blocks inside, allows outside ──────

proptest! {
    #[test]
    fn deny_read_prefix(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let inside  = format!("secret/{seg}/{rest}");
        let outside = format!("public/{seg}/{rest}");

        let policy = PolicyProfile {
            deny_read: vec!["secret/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        prop_assert!(!engine.can_read_path(Path::new(&inside)).allowed);
        prop_assert!(engine.can_read_path(Path::new(&outside)).allowed);
    }
}

// ── 7. Specific deny_write prefix blocks inside, allows outside ─────

proptest! {
    #[test]
    fn deny_write_prefix(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let inside  = format!("locked/{seg}/{rest}");
        let outside = format!("open/{seg}/{rest}");

        let policy = PolicyProfile {
            deny_write: vec!["locked/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        prop_assert!(!engine.can_write_path(Path::new(&inside)).allowed);
        prop_assert!(engine.can_write_path(Path::new(&outside)).allowed);
    }
}

// ── 8. Determinism — same input always gives same output ────────────

proptest! {
    #[test]
    fn decisions_are_deterministic(
        tool in tool_name(),
        path in relative_path(),
        deny_all in any::<bool>(),
    ) {
        let deny = if deny_all { vec!["**".to_string()] } else { Vec::new() };
        let policy = PolicyProfile {
            deny_read: deny.clone(),
            deny_write: deny,
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        let t1 = engine.can_use_tool(&tool).allowed;
        let t2 = engine.can_use_tool(&tool).allowed;
        prop_assert_eq!(t1, t2);

        let r1 = engine.can_read_path(Path::new(&path)).allowed;
        let r2 = engine.can_read_path(Path::new(&path)).allowed;
        prop_assert_eq!(r1, r2);

        let w1 = engine.can_write_path(Path::new(&path)).allowed;
        let w2 = engine.can_write_path(Path::new(&path)).allowed;
        prop_assert_eq!(w1, w2);
    }
}
