// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for kimi-bridge types.

use kimi_bridge::kimi_types::*;
use proptest::prelude::*;

// ── Strategies ──────────────────────────────────────────────────────────

fn arb_role() -> impl Strategy<Value = Role> {
    prop_oneof![
        Just(Role::System),
        Just(Role::User),
        Just(Role::Assistant),
        Just(Role::Tool),
    ]
}

fn arb_usage() -> impl Strategy<Value = Usage> {
    (0..10_000u64, 0..10_000u64).prop_map(|(p, c)| Usage {
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: p + c,
    })
}

fn arb_kimi_ref() -> impl Strategy<Value = KimiRef> {
    (1..100u32, "https://[a-z]{5,10}\\.com", any::<bool>()).prop_map(|(idx, url, has_title)| {
        KimiRef {
            index: idx,
            url,
            title: if has_title {
                Some("Title".into())
            } else {
                None
            },
        }
    })
}

fn arb_function_call() -> impl Strategy<Value = FunctionCall> {
    ("[a-z_]{3,12}", "\\{[a-z:\" ]{0,30}\\}").prop_map(|(name, args)| FunctionCall {
        name,
        arguments: args,
    })
}

fn arb_tool_call() -> impl Strategy<Value = ToolCall> {
    ("call_[a-z0-9]{4,8}", arb_function_call()).prop_map(|(id, function)| ToolCall {
        id,
        call_type: "function".into(),
        function,
    })
}

// ── Properties ──────────────────────────────────────────────────────────

proptest! {
    #[test]
    fn role_serde_roundtrip(role in arb_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let back: Role = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, role);
    }

    #[test]
    fn usage_serde_roundtrip(usage in arb_usage()) {
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, usage);
    }

    #[test]
    fn kimi_ref_serde_roundtrip(r in arb_kimi_ref()) {
        let json = serde_json::to_string(&r).unwrap();
        let back: KimiRef = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, r);
    }

    #[test]
    fn tool_call_serde_roundtrip(tc in arb_tool_call()) {
        let json = serde_json::to_string(&tc).unwrap();
        let back: ToolCall = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, tc);
    }

    #[test]
    fn builtin_is_builtin_deterministic(name in "[a-z$_]{1,15}") {
        let result1 = builtin::is_builtin(&name);
        let result2 = builtin::is_builtin(&name);
        prop_assert_eq!(result1, result2);
    }

    #[test]
    fn usage_total_tokens_consistency(prompt in 0..100_000u64, completion in 0..100_000u64) {
        let u = Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        };
        let json = serde_json::to_string(&u).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.total_tokens, back.prompt_tokens + back.completion_tokens);
    }
}
