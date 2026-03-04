#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::WorkOrderBuilder;
use abp_gemini_sdk::dialect::*;
use insta::assert_json_snapshot;

#[test]
fn snapshot_default_config() {
    assert_json_snapshot!("gemini_default_config", GeminiConfig::default());
}

#[test]
fn snapshot_mapped_request() {
    let wo = WorkOrderBuilder::new("Migrate to async").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_json_snapshot!("gemini_mapped_request", req);
}

#[test]
fn snapshot_mapped_response_events() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![
                    GeminiPart::Text("I'll migrate the code to async.".into()),
                    GeminiPart::FunctionCall {
                        name: "search".into(),
                        args: serde_json::json!({"query": "rust async patterns"}),
                    },
                ],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 90,
            candidates_token_count: 42,
            total_token_count: 132,
        }),
    };
    let events: Vec<_> = map_response(&resp).into_iter().map(|e| e.kind).collect();
    assert_json_snapshot!("gemini_mapped_response_events", events);
}
