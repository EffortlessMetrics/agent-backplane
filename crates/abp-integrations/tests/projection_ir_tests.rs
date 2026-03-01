// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for IR-based projection matrix translation.

use abp_integrations::projection::{
    Dialect, MODEL_EQUIVALENCE_TABLE, TranslationFidelity, detect_dialect, map_via_ir,
    translate_model_name,
};
use serde_json::json;

// ── OpenAI → Claude ─────────────────────────────────────────────────────

#[test]
fn openai_to_claude_text() {
    let msgs = json!([
        {"role": "user", "content": "Hello"}
    ]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["role"], "user");
    assert_eq!(arr[0]["content"], "Hello");
    assert_eq!(report.messages_mapped, 1);
    assert_eq!(report.source_dialect, Dialect::OpenAi);
    assert_eq!(report.target_dialect, Dialect::Claude);
}

#[test]
fn openai_to_claude_system_excluded() {
    let msgs = json!([
        {"role": "system", "content": "Be helpful"},
        {"role": "user", "content": "Hi"}
    ]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["role"], "user");
    assert!(!report.losses.is_empty());
    assert_eq!(report.fidelity, TranslationFidelity::Degraded);
}

#[test]
fn openai_to_claude_tool_call() {
    let msgs = json!([
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "read_file",
                    "arguments": "{\"path\":\"main.rs\"}"
                }
            }]
        }
    ]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "assistant");
    // Content is a serialized JSON array of Claude blocks
    let blocks: Vec<serde_json::Value> =
        serde_json::from_str(arr[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(blocks[0]["type"], "tool_use");
    assert_eq!(blocks[0]["id"], "call_1");
    assert_eq!(blocks[0]["name"], "read_file");
    assert_eq!(report.messages_mapped, 1);
}

#[test]
fn openai_to_claude_tool_result() {
    let msgs = json!([
        {"role": "tool", "content": "file contents", "tool_call_id": "call_1"}
    ]);
    let (result, _report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "user");
    let blocks: Vec<serde_json::Value> =
        serde_json::from_str(arr[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "call_1");
    assert_eq!(blocks[0]["content"], "file contents");
}

// ── OpenAI → Gemini ─────────────────────────────────────────────────────

#[test]
fn openai_to_gemini_text() {
    let msgs = json!([
        {"role": "user", "content": "Hello"},
        {"role": "assistant", "content": "Hi there!"}
    ]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::Gemini, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["role"], "user");
    assert_eq!(arr[0]["parts"][0]["text"], "Hello");
    assert_eq!(arr[1]["role"], "model");
    assert_eq!(arr[1]["parts"][0]["text"], "Hi there!");
    assert_eq!(report.messages_mapped, 2);
}

#[test]
fn openai_to_gemini_tool_call() {
    let msgs = json!([
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "search",
                    "arguments": "{\"q\":\"rust\"}"
                }
            }]
        }
    ]);
    let (result, _report) = map_via_ir(Dialect::OpenAi, Dialect::Gemini, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "model");
    assert_eq!(arr[0]["parts"][0]["functionCall"]["name"], "search");
    assert_eq!(arr[0]["parts"][0]["functionCall"]["args"]["q"], "rust");
}

// ── Claude → OpenAI ─────────────────────────────────────────────────────

#[test]
fn claude_to_openai_text() {
    let msgs = json!([
        {"role": "user", "content": "Hello"},
        {"role": "assistant", "content": "Sure!"}
    ]);
    let (result, report) = map_via_ir(Dialect::Claude, Dialect::OpenAi, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["role"], "user");
    assert_eq!(arr[0]["content"], "Hello");
    assert_eq!(arr[1]["role"], "assistant");
    assert_eq!(arr[1]["content"], "Sure!");
    assert_eq!(report.fidelity, TranslationFidelity::LossySupported);
}

#[test]
fn claude_to_openai_tool_use() {
    let blocks =
        json!([{"type": "tool_use", "id": "tu_1", "name": "read", "input": {"path": "a.rs"}}]);
    let msgs = json!([
        {"role": "assistant", "content": serde_json::to_string(&blocks).unwrap()}
    ]);
    let (result, _report) = map_via_ir(Dialect::Claude, Dialect::OpenAi, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "assistant");
    let tc = &arr[0]["tool_calls"][0];
    assert_eq!(tc["id"], "tu_1");
    assert_eq!(tc["function"]["name"], "read");
}

// ── Claude → Gemini ─────────────────────────────────────────────────────

#[test]
fn claude_to_gemini_text() {
    let msgs = json!([
        {"role": "user", "content": "Hi"},
        {"role": "assistant", "content": "Hello!"}
    ]);
    let (result, _report) = map_via_ir(Dialect::Claude, Dialect::Gemini, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "user");
    assert_eq!(arr[0]["parts"][0]["text"], "Hi");
    assert_eq!(arr[1]["role"], "model");
    assert_eq!(arr[1]["parts"][0]["text"], "Hello!");
}

// ── Gemini → OpenAI ─────────────────────────────────────────────────────

#[test]
fn gemini_to_openai_text() {
    let msgs = json!([
        {"role": "user", "parts": [{"text": "Hello"}]},
        {"role": "model", "parts": [{"text": "Hi!"}]}
    ]);
    let (result, report) = map_via_ir(Dialect::Gemini, Dialect::OpenAi, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["role"], "user");
    assert_eq!(arr[0]["content"], "Hello");
    assert_eq!(arr[1]["role"], "assistant");
    assert_eq!(arr[1]["content"], "Hi!");
    assert_eq!(report.fidelity, TranslationFidelity::LossySupported);
}

#[test]
fn gemini_to_openai_function_call() {
    let msgs = json!([
        {"role": "model", "parts": [{"functionCall": {"name": "search", "args": {"q": "rust"}}}]}
    ]);
    let (result, _report) = map_via_ir(Dialect::Gemini, Dialect::OpenAi, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "assistant");
    let tc = &arr[0]["tool_calls"][0];
    assert_eq!(tc["function"]["name"], "search");
}

// ── Gemini → Claude ─────────────────────────────────────────────────────

#[test]
fn gemini_to_claude_text() {
    let msgs = json!([
        {"role": "user", "parts": [{"text": "Hi"}]},
        {"role": "model", "parts": [{"text": "Hello!"}]}
    ]);
    let (result, _report) = map_via_ir(Dialect::Gemini, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["role"], "user");
    assert_eq!(arr[0]["content"], "Hi");
    assert_eq!(arr[1]["role"], "assistant");
    assert_eq!(arr[1]["content"], "Hello!");
}

// ── Identity translation ────────────────────────────────────────────────

#[test]
fn identity_translation_returns_clone() {
    let msgs = json!([
        {"role": "user", "content": "Hello"},
        {"role": "assistant", "content": "Hi!"}
    ]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::OpenAi, &msgs).unwrap();
    assert_eq!(result, msgs);
    assert_eq!(report.fidelity, TranslationFidelity::Lossless);
    assert!(report.losses.is_empty());
    assert_eq!(report.messages_mapped, 2);
}

// ── Multi-turn conversations ────────────────────────────────────────────

#[test]
fn multi_turn_openai_to_claude() {
    let msgs = json!([
        {"role": "system", "content": "Be concise"},
        {"role": "user", "content": "Read main.rs"},
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "read_file", "arguments": "{\"path\":\"main.rs\"}"}
            }]
        },
        {"role": "tool", "content": "fn main() {}", "tool_call_id": "c1"},
        {"role": "assistant", "content": "Done."}
    ]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    // System message excluded, 4 remaining messages
    assert_eq!(arr.len(), 4);
    assert_eq!(report.messages_mapped, 5);
    assert!(report.losses.iter().any(|l| l.contains("system")));
}

#[test]
fn multi_turn_gemini_to_openai() {
    let msgs = json!([
        {"role": "user", "parts": [{"text": "Hi"}]},
        {"role": "model", "parts": [{"text": "Hello!"}]},
        {"role": "user", "parts": [{"text": "Bye"}]}
    ]);
    let (result, report) = map_via_ir(Dialect::Gemini, Dialect::OpenAi, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[2]["content"], "Bye");
    assert_eq!(report.messages_mapped, 3);
}

// ── detect_dialect ──────────────────────────────────────────────────────

#[test]
fn detect_dialect_openai_system_role() {
    let msgs = json!([
        {"role": "system", "content": "Be helpful"},
        {"role": "user", "content": "Hi"}
    ]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::OpenAi));
}

#[test]
fn detect_dialect_openai_tool_calls() {
    let msgs = json!([
        {"role": "assistant", "content": null, "tool_calls": []}
    ]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::OpenAi));
}

#[test]
fn detect_dialect_gemini_parts() {
    let msgs = json!([
        {"role": "user", "parts": [{"text": "Hello"}]}
    ]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::Gemini));
}

#[test]
fn detect_dialect_claude_default() {
    let msgs = json!([
        {"role": "user", "content": "Hello"},
        {"role": "assistant", "content": "Hi"}
    ]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::Claude));
}

#[test]
fn detect_dialect_empty_returns_none() {
    let msgs = json!([]);
    assert_eq!(detect_dialect(&msgs), None);
}

#[test]
fn detect_dialect_non_array_returns_none() {
    let msgs = json!({"role": "user"});
    assert_eq!(detect_dialect(&msgs), None);
}

// ── Model name translation ──────────────────────────────────────────────

#[test]
fn model_name_gpt4o_to_claude() {
    let result = translate_model_name("gpt-4o", Dialect::Claude);
    assert_eq!(result.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn model_name_claude_to_gemini() {
    let result = translate_model_name("claude-sonnet-4-20250514", Dialect::Gemini);
    assert_eq!(result.as_deref(), Some("gemini-2.5-flash"));
}

#[test]
fn model_name_gemini_to_openai() {
    let result = translate_model_name("gemini-2.5-flash", Dialect::OpenAi);
    assert_eq!(result.as_deref(), Some("gpt-4o"));
}

#[test]
fn model_name_unknown_returns_none() {
    let result = translate_model_name("unknown-model-xyz", Dialect::Claude);
    assert!(result.is_none());
}

#[test]
fn model_name_to_abp_passthrough() {
    let result = translate_model_name("gpt-4o", Dialect::Abp);
    assert_eq!(result.as_deref(), Some("gpt-4o"));
}

#[test]
fn model_name_to_mock_passthrough() {
    let result = translate_model_name("anything", Dialect::Mock);
    assert_eq!(result.as_deref(), Some("anything"));
}

#[test]
fn model_name_no_equivalent_returns_none() {
    // gpt-4o-mini has no kimi equivalent
    let result = translate_model_name("gpt-4o-mini", Dialect::Kimi);
    assert!(result.is_none());
}

#[test]
fn model_equivalence_table_has_entries() {
    assert!(!MODEL_EQUIVALENCE_TABLE.is_empty());
    // Each row should have at least openai and claude filled in
    for &(openai, claude, _gemini, _codex, _kimi) in MODEL_EQUIVALENCE_TABLE {
        assert!(!openai.is_empty(), "openai model should not be empty");
        assert!(!claude.is_empty(), "claude model should not be empty");
    }
}

// ── Error cases ─────────────────────────────────────────────────────────

#[test]
fn map_via_ir_non_array_returns_error() {
    let msgs = json!({"role": "user", "content": "Hello"});
    let result = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs);
    assert!(result.is_err());
}

#[test]
fn map_via_ir_empty_array() {
    let msgs = json!([]);
    let (result, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    assert_eq!(result.as_array().unwrap().len(), 0);
    assert_eq!(report.messages_mapped, 0);
}

// ── Fidelity reporting ──────────────────────────────────────────────────

#[test]
fn fidelity_lossless_for_identity() {
    let msgs = json!([{"role": "user", "content": "Hi"}]);
    let (_result, report) = map_via_ir(Dialect::Claude, Dialect::Claude, &msgs).unwrap();
    assert_eq!(report.fidelity, TranslationFidelity::Lossless);
}

#[test]
fn fidelity_lossy_supported_no_system() {
    let msgs = json!([
        {"role": "user", "content": "Hi"},
        {"role": "assistant", "content": "Hello"}
    ]);
    let (_result, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    assert_eq!(report.fidelity, TranslationFidelity::LossySupported);
    assert!(report.losses.is_empty());
}

#[test]
fn fidelity_degraded_with_system_loss() {
    let msgs = json!([
        {"role": "system", "content": "Be helpful"},
        {"role": "user", "content": "Hi"}
    ]);
    let (_result, report) = map_via_ir(Dialect::OpenAi, Dialect::Gemini, &msgs).unwrap();
    assert_eq!(report.fidelity, TranslationFidelity::Degraded);
    assert!(report.losses.iter().any(|l| l.contains("system")));
}

// ── Gemini uppercase Text key ───────────────────────────────────────────

#[test]
fn gemini_uppercase_text_key() {
    let msgs = json!([
        {"role": "user", "parts": [{"Text": "Hello"}]}
    ]);
    let (result, _report) = map_via_ir(Dialect::Gemini, Dialect::OpenAi, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["content"], "Hello");
}

// ── Codex/Kimi as OpenAI-style ─────────────────────────────────────────

#[test]
fn codex_to_claude_text() {
    let msgs = json!([
        {"role": "user", "content": "Hello from Codex"}
    ]);
    let (result, report) = map_via_ir(Dialect::Codex, Dialect::Claude, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["content"], "Hello from Codex");
    assert_eq!(report.source_dialect, Dialect::Codex);
}

#[test]
fn kimi_to_gemini_text() {
    let msgs = json!([
        {"role": "user", "content": "Hello from Kimi"}
    ]);
    let (result, report) = map_via_ir(Dialect::Kimi, Dialect::Gemini, &msgs).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0]["parts"][0]["text"], "Hello from Kimi");
    assert_eq!(report.source_dialect, Dialect::Kimi);
}
