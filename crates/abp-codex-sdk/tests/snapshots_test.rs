// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_codex_sdk::dialect::*;
use abp_core::WorkOrderBuilder;
use insta::assert_json_snapshot;

#[test]
fn snapshot_default_config() {
    assert_json_snapshot!("codex_default_config", CodexConfig::default());
}

#[test]
fn snapshot_mapped_request() {
    let wo = WorkOrderBuilder::new("Write unit tests").build();
    let cfg = CodexConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_json_snapshot!("codex_mapped_request", req);
}

#[test]
fn snapshot_mapped_response_events() {
    let resp = CodexResponse {
        id: "resp_snapshot".into(),
        model: "codex-mini-latest".into(),
        output: vec![
            CodexOutputItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Here are the unit tests.".into(),
                }],
            },
            CodexOutputItem::FunctionCall {
                id: "fc_1".into(),
                name: "shell".into(),
                arguments: r#"{"command":"cargo test"}"#.into(),
            },
        ],
        usage: Some(CodexUsage {
            input_tokens: 80,
            output_tokens: 45,
            total_tokens: 125,
        }),
    };
    let events: Vec<_> = map_response(&resp).into_iter().map(|e| e.kind).collect();
    assert_json_snapshot!("codex_mapped_response_events", events);
}
