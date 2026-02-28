// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_gemini_sdk::dialect::*;
use abp_core::WorkOrderBuilder;
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
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 90,
            candidates_token_count: 42,
            total_token_count: 132,
        }),
    };
    let events: Vec<_> = map_response(&resp)
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_json_snapshot!("gemini_mapped_response_events", events);
}
