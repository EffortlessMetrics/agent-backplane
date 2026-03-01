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
    (prop::collection::vec(path_segment(), 1..=4), "[a-z]{1,4}").prop_map(|(segs, ext)| {
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

// ── 9. Explicit allow permits a named tool ──────────────────────────

proptest! {
    #[test]
    fn explicit_allow_permits_named_tool(name in tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec![name.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_use_tool(&name).allowed);
    }
}

// ── 10. Explicit deny blocks a named tool ───────────────────────────

proptest! {
    #[test]
    fn explicit_deny_blocks_named_tool(name in tool_name()) {
        let policy = PolicyProfile {
            disallowed_tools: vec![name.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&name).allowed);
    }
}

// ── 11. Non-matching read path is allowed ───────────────────────────

proptest! {
    #[test]
    fn non_matching_read_path_allowed(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let safe = format!("allowed/{seg}/{rest}");
        let policy = PolicyProfile {
            deny_read: vec!["forbidden/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_read_path(Path::new(&safe)).allowed);
    }
}

// ── 12. Non-matching write path is allowed ──────────────────────────

proptest! {
    #[test]
    fn non_matching_write_path_allowed(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let safe = format!("writable/{seg}/{rest}");
        let policy = PolicyProfile {
            deny_write: vec!["readonly/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&safe)).allowed);
    }
}

// ── 13. Serde roundtrip preserves policy behavior ───────────────────

proptest! {
    #[test]
    fn serde_roundtrip_preserves_behavior(
        tool in tool_name(),
        path in relative_path(),
    ) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec!["Dangerous".to_string()],
            deny_read: vec!["secret/**".to_string()],
            deny_write: vec!["locked/**".to_string()],
            ..PolicyProfile::default()
        };
        let json = serde_json::to_string(&policy).unwrap();
        let restored: PolicyProfile = serde_json::from_str(&json).unwrap();

        let engine_a = PolicyEngine::new(&policy).unwrap();
        let engine_b = PolicyEngine::new(&restored).unwrap();

        prop_assert_eq!(
            engine_a.can_use_tool(&tool).allowed,
            engine_b.can_use_tool(&tool).allowed
        );
        prop_assert_eq!(
            engine_a.can_read_path(Path::new(&path)).allowed,
            engine_b.can_read_path(Path::new(&path)).allowed
        );
        prop_assert_eq!(
            engine_a.can_write_path(Path::new(&path)).allowed,
            engine_b.can_write_path(Path::new(&path)).allowed
        );
    }
}

// ── 14. Multiple deny_read patterns combined ────────────────────────

proptest! {
    #[test]
    fn multiple_deny_read_patterns(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let in_secrets = format!("secrets/{seg}/{rest}");
        let in_private = format!("private/{seg}/{rest}");
        let in_safe = format!("public/{seg}/{rest}");

        let policy = PolicyProfile {
            deny_read: vec!["secrets/**".to_string(), "private/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        prop_assert!(!engine.can_read_path(Path::new(&in_secrets)).allowed);
        prop_assert!(!engine.can_read_path(Path::new(&in_private)).allowed);
        prop_assert!(engine.can_read_path(Path::new(&in_safe)).allowed);
    }
}

// ── 15. Multiple deny_write patterns combined ───────────────────────

proptest! {
    #[test]
    fn multiple_deny_write_patterns(
        seg in path_segment(),
        rest in relative_path(),
    ) {
        let in_system = format!("system/{seg}/{rest}");
        let in_config = format!("config/{seg}/{rest}");
        let in_safe = format!("workspace/{seg}/{rest}");

        let policy = PolicyProfile {
            deny_write: vec!["system/**".to_string(), "config/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        prop_assert!(!engine.can_write_path(Path::new(&in_system)).allowed);
        prop_assert!(!engine.can_write_path(Path::new(&in_config)).allowed);
        prop_assert!(engine.can_write_path(Path::new(&in_safe)).allowed);
    }
}

// ── 16. Glob star matches tool name prefix ──────────────────────────

proptest! {
    #[test]
    fn glob_star_matches_tool_prefix(suffix in tool_name()) {
        let full_name = format!("Bash{suffix}");
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash*".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&full_name).allowed);
    }
}

// ── 17. Allowlist blocks unlisted tool ──────────────────────────────

proptest! {
    #[test]
    fn allowlist_blocks_unlisted_tool(name in tool_name()) {
        // Use a fixed allowlist that won't match random generated names.
        let policy = PolicyProfile {
            allowed_tools: vec!["XYZZY_ONLY".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        // Generated names are alphabetic and won't be "XYZZY_ONLY"
        prop_assert!(!engine.can_use_tool(&name).allowed);
    }
}

// ── 18. Deny and allow same tool: deny wins ─────────────────────────

proptest! {
    #[test]
    fn deny_same_tool_as_allow(name in tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec![name.clone()],
            disallowed_tools: vec![name.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&name).allowed);
    }
}

// ── 19. Extension-based deny_read blocks matching files ─────────────

proptest! {
    #[test]
    fn extension_deny_read_blocks(
        seg in path_segment(),
    ) {
        let blocked = format!("{seg}.secret");
        let allowed = format!("{seg}.txt");

        let policy = PolicyProfile {
            deny_read: vec!["*.secret".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        prop_assert!(!engine.can_read_path(Path::new(&blocked)).allowed);
        prop_assert!(engine.can_read_path(Path::new(&allowed)).allowed);
    }
}
