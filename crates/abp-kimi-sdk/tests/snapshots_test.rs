// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_kimi_sdk::dialect::*;
use abp_core::WorkOrderBuilder;
use insta::assert_json_snapshot;

#[test]
fn snapshot_default_config() {
    assert_json_snapshot!("kimi_default_config", KimiConfig::default());
}

#[test]
fn snapshot_mapped_request() {
    let wo = WorkOrderBuilder::new("Optimize database queries").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_json_snapshot!("kimi_mapped_request", req);
}

#[test]
fn snapshot_mapped_response_events() {
    let resp = KimiResponse {
        id: "cmpl_snapshot".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("I'll optimize the queries.".into()),
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"query":"sql optimization tips"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 75,
            completion_tokens: 30,
            total_tokens: 105,
        }),
    };
    let events: Vec<_> = map_response(&resp)
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_json_snapshot!("kimi_mapped_response_events", events);
}
