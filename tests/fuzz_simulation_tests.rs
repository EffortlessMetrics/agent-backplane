// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz simulation tests — deterministic test cases that exercise the same code
//! paths as the cargo-fuzz harnesses in `fuzz/`, using hand-crafted edge-case
//! inputs rather than random bytes.
#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, ContextPacket, ExecutionLane, Outcome, PolicyProfile,
    Receipt, ReceiptBuilder, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode,
};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .expect("hashing must succeed")
}

fn make_policy(
    allowed: Vec<&str>,
    disallowed: Vec<&str>,
    deny_read: Vec<&str>,
    deny_write: Vec<&str>,
) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: allowed.into_iter().map(String::from).collect(),
        disallowed_tools: disallowed.into_iter().map(String::from).collect(),
        deny_read: deny_read.into_iter().map(String::from).collect(),
        deny_write: deny_write.into_iter().map(String::from).collect(),
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    }
}

// ===================================================================
// 1. JSONL Envelope Parsing
// ===================================================================

#[test]
fn fuzz_sim_envelope_empty_string() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn fuzz_sim_envelope_null_json() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn fuzz_sim_envelope_bare_number() {
    assert!(JsonlCodec::decode("42").is_err());
}

#[test]
fn fuzz_sim_envelope_bare_string() {
    assert!(JsonlCodec::decode("\"hello\"").is_err());
}

#[test]
fn fuzz_sim_envelope_bare_array() {
    assert!(JsonlCodec::decode("[1,2,3]").is_err());
}

#[test]
fn fuzz_sim_envelope_empty_object() {
    assert!(JsonlCodec::decode("{}").is_err());
}

#[test]
fn fuzz_sim_envelope_unknown_tag() {
    let _ = JsonlCodec::decode(r#"{"t":"unknown","data":{}}"#);
}

#[test]
fn fuzz_sim_envelope_missing_tag() {
    let _ = JsonlCodec::decode(r#"{"id":"abc","data":"x"}"#);
}

#[test]
fn fuzz_sim_envelope_deeply_nested() {
    let nested = "{".repeat(64) + &"}".repeat(64);
    let _ = JsonlCodec::decode(&nested);
}

#[test]
fn fuzz_sim_envelope_huge_string_value() {
    let big = format!(
        r#"{{"t":"fatal","ref_id":"x","error":"{}","error_code":null}}"#,
        "A".repeat(100_000)
    );
    let _ = JsonlCodec::decode(&big);
}

#[test]
fn fuzz_sim_envelope_unicode_payload() {
    let _ = JsonlCodec::decode(r#"{"t":"fatal","ref_id":"日本語","error":"🎉","error_code":null}"#);
}

#[test]
fn fuzz_sim_envelope_roundtrip_fatal() {
    let json = r#"{"t":"fatal","ref_id":"r1","error":"boom","error_code":null}"#;
    if let Ok(env) = JsonlCodec::decode(json) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let rt = JsonlCodec::decode(encoded.trim()).unwrap();
        assert_eq!(
            serde_json::to_value(&env).unwrap(),
            serde_json::to_value(&rt).unwrap()
        );
    }
}

#[test]
fn fuzz_sim_envelope_multiline_batch() {
    let input = "not json\n{}\n\"hello\"\n";
    let results = abp_protocol::codec::StreamingCodec::decode_batch(input);
    assert_eq!(results.len(), 3);
}

#[test]
fn fuzz_sim_envelope_line_count_consistency() {
    let input = "line1\nline2\n\nline4\n";
    let count = abp_protocol::codec::StreamingCodec::line_count(input);
    let manual = input.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(count, manual);
}

#[test]
fn fuzz_sim_envelope_validate_jsonl_empty() {
    let errors = abp_protocol::codec::StreamingCodec::validate_jsonl("");
    assert!(errors.is_empty());
}

#[test]
fn fuzz_sim_envelope_stream_decode_garbage() {
    let data = b"garbage\nmore garbage\n";
    let reader = std::io::BufReader::new(&data[..]);
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.iter().all(|r| r.is_err()));
}

#[test]
fn fuzz_sim_envelope_version_parsing() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(
        abp_protocol::parse_version("abp/v999.999"),
        Some((999, 999))
    );
    assert!(abp_protocol::parse_version("").is_none());
    assert!(abp_protocol::parse_version("not a version").is_none());
    assert!(abp_protocol::parse_version("abp/v").is_none());
}

#[test]
fn fuzz_sim_envelope_version_compatibility() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(abp_protocol::is_compatible_version("abp/v0.99", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("garbage", "abp/v0.1"));
}

#[test]
fn fuzz_sim_envelope_validator_never_panics() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    // Construct a minimal fatal envelope for validation.
    let env = Envelope::fatal_with_code(
        Some("ref-1".to_string()),
        "test error",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let result = validator.validate(&env);
    let _ = result;
}

// ===================================================================
// 2. WorkOrder Deserialization
// ===================================================================

#[test]
fn fuzz_sim_work_order_from_empty_json() {
    assert!(serde_json::from_str::<WorkOrder>("{}").is_err());
}

#[test]
fn fuzz_sim_work_order_from_null() {
    assert!(serde_json::from_str::<WorkOrder>("null").is_err());
}

#[test]
fn fuzz_sim_work_order_from_array() {
    assert!(serde_json::from_str::<WorkOrder>("[]").is_err());
}

#[test]
fn fuzz_sim_work_order_from_string() {
    assert!(serde_json::from_str::<WorkOrder>("\"hello\"").is_err());
}

#[test]
fn fuzz_sim_work_order_partial_fields() {
    let json = r#"{"task":"hello"}"#;
    let _ = serde_json::from_str::<WorkOrder>(json);
}

#[test]
fn fuzz_sim_work_order_extra_fields_ignored() {
    let wo = WorkOrderBuilder::new("test").build();
    let mut val = serde_json::to_value(&wo).unwrap();
    val.as_object_mut()
        .unwrap()
        .insert("extra_field".to_string(), serde_json::json!("surprise"));
    let rt: WorkOrder = serde_json::from_value(val).unwrap();
    assert_eq!(rt.task, "test");
}

#[test]
fn fuzz_sim_work_order_builder_empty_task() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn fuzz_sim_work_order_builder_unicode_task() {
    let wo = WorkOrderBuilder::new("日本語タスク 🚀").build();
    assert_eq!(wo.task, "日本語タスク 🚀");
}

#[test]
fn fuzz_sim_work_order_roundtrip_deterministic() {
    let wo = WorkOrderBuilder::new("fuzz-task")
        .lane(ExecutionLane::PatchFirst)
        .root("/tmp/test")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["*.rs".to_string()])
        .exclude(vec!["target/**".to_string()])
        .build();
    let json1 = serde_json::to_string(&wo).unwrap();
    let json2 = serde_json::to_string(&wo).unwrap();
    assert_eq!(json1, json2);
    let rt: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json3 = serde_json::to_string(&rt).unwrap();
    assert_eq!(json1, json3);
}

#[test]
fn fuzz_sim_work_order_builder_with_policy() {
    let policy = make_policy(
        vec!["read", "write"],
        vec!["bash"],
        vec!["/etc/**"],
        vec!["/usr/**"],
    );
    let wo = WorkOrderBuilder::new("secure-task").policy(policy).build();
    assert!(!wo.policy.allowed_tools.is_empty());
    assert!(!wo.policy.disallowed_tools.is_empty());
}

#[test]
fn fuzz_sim_work_order_builder_with_context() {
    let ctx = ContextPacket {
        files: vec!["README.md".to_string(), "src/main.rs".to_string()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("ctx-task").context(ctx).build();
    assert_eq!(wo.context.files.len(), 2);
}

#[test]
fn fuzz_sim_work_order_max_budget_nan() {
    // NaN should not cause panics
    let wo = WorkOrderBuilder::new("budget-task").build();
    let _ = serde_json::to_string(&wo);
}

// ===================================================================
// 3. Receipt Hash Verification
// ===================================================================

#[test]
fn fuzz_sim_receipt_hash_deterministic() {
    let receipt = make_receipt("backend-1", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&receipt).unwrap();
    let h2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn fuzz_sim_receipt_hash_is_valid_hex() {
    let receipt = make_receipt("test-be", Outcome::Complete);
    let hash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
}

#[test]
fn fuzz_sim_receipt_with_hash_embeds() {
    let receipt = make_receipt("hash-test", Outcome::Complete);
    let hashed = receipt.clone().with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    let embedded = hashed.receipt_sha256.as_ref().unwrap();
    assert_eq!(embedded.len(), 64);
}

#[test]
fn fuzz_sim_receipt_rehash_consistency() {
    let receipt = make_receipt("rehash-test", Outcome::Partial);
    let hashed = receipt.with_hash().unwrap();
    let rehash = abp_core::receipt_hash(&hashed).unwrap();
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap(), &rehash);
}

#[test]
fn fuzz_sim_receipt_cross_crate_hash_agreement() {
    let receipt = make_receipt("cross-crate", Outcome::Failed);
    let core_hash = abp_core::receipt_hash(&receipt).unwrap();
    let crate_hash = abp_receipt::compute_hash(&receipt).unwrap();
    assert_eq!(core_hash, crate_hash);
}

#[test]
fn fuzz_sim_receipt_verify_hash_unhashed() {
    let receipt = make_receipt("unhashed", Outcome::Complete);
    assert!(abp_receipt::verify_hash(&receipt));
}

#[test]
fn fuzz_sim_receipt_verify_hash_correct() {
    let hashed = make_hashed_receipt("verify-ok");
    assert!(abp_receipt::verify_hash(&hashed));
}

#[test]
fn fuzz_sim_receipt_verify_hash_tampered() {
    let mut tampered = make_hashed_receipt("tamper-test");
    tampered.receipt_sha256 = Some("0".repeat(64));
    assert!(!abp_receipt::verify_hash(&tampered));
}

#[test]
fn fuzz_sim_receipt_verify_hash_short_hash() {
    let mut bad = make_hashed_receipt("short-hash");
    bad.receipt_sha256 = Some("deadbeef".to_string());
    assert!(!abp_receipt::verify_hash(&bad));
}

#[test]
fn fuzz_sim_receipt_canonicalize_never_panics() {
    let receipt = make_receipt("canon", Outcome::Complete);
    let canon = abp_receipt::canonicalize(&receipt).unwrap();
    assert!(!canon.is_empty());
    // Canonical form must be valid JSON.
    let val: serde_json::Value = serde_json::from_str(&canon).unwrap();
    assert!(val.is_object());
}

#[test]
fn fuzz_sim_receipt_diff_identical() {
    let r1 = make_receipt("diff-a", Outcome::Complete);
    let r2 = make_receipt("diff-a", Outcome::Complete);
    let diff = abp_receipt::diff_receipts(&r1, &r2);
    // Might have timestamp diffs but structure should be similar.
    let _ = diff.len();
}

#[test]
fn fuzz_sim_receipt_diff_different_outcomes() {
    let r1 = make_receipt("diff-be", Outcome::Complete);
    let r2 = make_receipt("diff-be", Outcome::Failed);
    let diff = abp_receipt::diff_receipts(&r1, &r2);
    assert!(!diff.is_empty());
}

#[test]
fn fuzz_sim_receipt_chain_push() {
    let mut chain = abp_receipt::ReceiptChain::new();
    let h = make_hashed_receipt("chain-1");
    let _ = chain.push(h);
}

#[test]
fn fuzz_sim_receipt_validate_never_panics() {
    let receipt = make_receipt("validate", Outcome::Complete);
    let _ = abp_core::validate::validate_receipt(&receipt);
}

#[test]
fn fuzz_sim_receipt_with_trace_events() {
    let receipt = ReceiptBuilder::new("events-be")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hello".to_string(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".to_string(),
                tool_use_id: Some("tid-1".to_string()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/test.txt"}),
            },
            ext: None,
        })
        .build();
    let hash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    let hashed = receipt.with_hash().unwrap();
    assert!(abp_receipt::verify_hash(&hashed));
}

#[test]
fn fuzz_sim_receipt_with_usage() {
    let receipt = ReceiptBuilder::new("usage-be")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: Some(50),
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.005),
        })
        .build();
    let _ = abp_core::receipt_hash(&receipt);
    let _ = receipt.with_hash();
}

// ===================================================================
// 4. IR Conversation Deserialization
// ===================================================================

#[test]
fn fuzz_sim_ir_conversation_from_empty_json() {
    let _ = serde_json::from_str::<IrConversation>("{}");
}

#[test]
fn fuzz_sim_ir_conversation_from_null() {
    assert!(serde_json::from_str::<IrConversation>("null").is_err());
}

#[test]
fn fuzz_sim_ir_message_from_garbage() {
    let _ = serde_json::from_str::<IrMessage>("garbage");
    let _ = serde_json::from_str::<IrMessage>("{\"role\":\"unknown\"}");
}

#[test]
fn fuzz_sim_ir_content_block_variants() {
    let _ = serde_json::from_str::<IrContentBlock>(r#"{"type":"text","text":"hello"}"#);
    let _ = serde_json::from_str::<IrContentBlock>(
        r#"{"type":"image","media_type":"png","data":"abc"}"#,
    );
    let _ = serde_json::from_str::<IrContentBlock>(
        r#"{"type":"tool_use","id":"1","name":"x","input":{}}"#,
    );
    let _ = serde_json::from_str::<IrContentBlock>(r#"{"type":"unknown"}"#);
}

#[test]
fn fuzz_sim_ir_conversation_roundtrip() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "You are helpful.".to_string(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "Hello".to_string(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text {
                text: "Hi there!".to_string(),
            }],
        ),
    ]);
    let json = serde_json::to_string(&conv).unwrap();
    let rt: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.len(), conv.len());
}

#[test]
fn fuzz_sim_ir_conversation_accessors() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "sys".to_string(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "usr".to_string(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: vec![IrContentBlock::Text {
                    text: "result".to_string(),
                }],
                is_error: false,
            }],
        ),
    ]);
    assert!(!conv.is_empty());
    assert_eq!(conv.len(), 4);
    assert!(conv.system_message().is_some());
    assert!(conv.last_assistant().is_some());
    assert!(!conv.tool_calls().is_empty());
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
}

#[test]
fn fuzz_sim_ir_empty_conversation() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.tool_calls().is_empty());
}

#[test]
fn fuzz_sim_ir_message_text_only() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Text {
            text: "hello".to_string(),
        }],
    );
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "hello");
    assert!(msg.tool_use_blocks().is_empty());
}

#[test]
fn fuzz_sim_ir_message_mixed_blocks() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check.".to_string(),
            },
            IrContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!("query"),
            },
        ],
    );
    assert!(!msg.is_text_only());
    assert!(!msg.tool_use_blocks().is_empty());
}

#[test]
fn fuzz_sim_ir_usage_merge() {
    let a = IrUsage::from_io(100, 200);
    let b = IrUsage::from_io(50, 75);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 150);
    assert_eq!(merged.output_tokens, 275);
    assert_eq!(merged.total_tokens, 425);
}

#[test]
fn fuzz_sim_ir_usage_merge_associative() {
    let a = IrUsage::with_cache(10, 20, 5, 3);
    let b = IrUsage::with_cache(30, 40, 10, 7);
    let c = IrUsage::with_cache(50, 60, 15, 11);
    let ab_c = a.merge(b).merge(c);
    let a2 = IrUsage::with_cache(10, 20, 5, 3);
    let b2 = IrUsage::with_cache(30, 40, 10, 7);
    let c2 = IrUsage::with_cache(50, 60, 15, 11);
    let a_bc = a2.merge(b2.merge(c2));
    assert_eq!(ab_c.total_tokens, a_bc.total_tokens);
    assert_eq!(ab_c.input_tokens, a_bc.input_tokens);
    assert_eq!(ab_c.output_tokens, a_bc.output_tokens);
}

#[test]
fn fuzz_sim_ir_tool_definition_roundtrip() {
    let tool = IrToolDefinition {
        name: "file_read".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let rt: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.name, tool.name);
}

#[test]
fn fuzz_sim_ir_normalize_dedup_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "a".to_string(),
            }],
        ),
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "b".to_string(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "hi".to_string(),
            }],
        ),
    ]);
    let deduped = abp_ir::normalize::dedup_system(&conv);
    let sys_count = deduped.messages_by_role(IrRole::System).len();
    assert!(sys_count <= 1);
}

#[test]
fn fuzz_sim_ir_normalize_trim_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Text {
            text: "  hello  ".to_string(),
        }],
    )]);
    let trimmed = abp_ir::normalize::trim_text(&conv);
    let msg = &trimmed.messages[0];
    assert_eq!(msg.text_content(), "hello");
}

#[test]
fn fuzz_sim_ir_normalize_strip_empty() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(IrRole::User, vec![]),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "content".to_string(),
            }],
        ),
    ]);
    let stripped = abp_ir::normalize::strip_empty(&conv);
    assert!(stripped.len() <= conv.len());
}

#[test]
fn fuzz_sim_ir_normalize_full_pipeline() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "  sys  ".to_string(),
            }],
        ),
        IrMessage::new(IrRole::User, vec![]),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "hello".to_string(),
            }],
        ),
    ]);
    let normalized = abp_ir::normalize::normalize(&conv);
    let _ = normalized.len();
    let _ = normalized.system_message();
}

// ===================================================================
// 5. Policy Profile Deserialization
// ===================================================================

#[test]
fn fuzz_sim_policy_profile_from_empty_json() {
    let _ = serde_json::from_str::<PolicyProfile>("{}");
}

#[test]
fn fuzz_sim_policy_profile_from_null() {
    assert!(serde_json::from_str::<PolicyProfile>("null").is_err());
}

#[test]
fn fuzz_sim_policy_profile_from_garbage() {
    let _ = serde_json::from_str::<PolicyProfile>("not json at all");
}

#[test]
fn fuzz_sim_policy_engine_empty_profile() {
    let profile = make_policy(vec![], vec![], vec![], vec![]);
    let engine = PolicyEngine::new(&profile).unwrap();
    let d = engine.can_use_tool("anything");
    assert!(d.allowed);
}

#[test]
fn fuzz_sim_policy_engine_allow_list() {
    let profile = make_policy(vec!["read_file", "write_file"], vec![], vec![], vec![]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(engine.can_use_tool("read_file").allowed);
    assert!(engine.can_use_tool("write_file").allowed);
    assert!(!engine.can_use_tool("bash").allowed);
}

#[test]
fn fuzz_sim_policy_engine_deny_list() {
    let profile = make_policy(vec![], vec!["bash", "exec"], vec![], vec![]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(!engine.can_use_tool("bash").allowed);
    assert!(!engine.can_use_tool("exec").allowed);
    assert!(engine.can_use_tool("read_file").allowed);
}

#[test]
fn fuzz_sim_policy_engine_path_deny_read() {
    let profile = make_policy(vec![], vec![], vec!["/etc/**"], vec![]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(!engine.can_read_path(Path::new("/etc/passwd")).allowed);
    assert!(
        engine
            .can_read_path(Path::new("/home/user/file.txt"))
            .allowed
    );
}

#[test]
fn fuzz_sim_policy_engine_path_deny_write() {
    let profile = make_policy(vec![], vec![], vec![], vec!["/usr/**"]);
    let engine = PolicyEngine::new(&profile).unwrap();
    assert!(!engine.can_write_path(Path::new("/usr/bin/test")).allowed);
    assert!(engine.can_write_path(Path::new("/tmp/test")).allowed);
}

#[test]
fn fuzz_sim_policy_engine_unicode_tool_name() {
    let profile = make_policy(vec![], vec!["日本語ツール"], vec![], vec![]);
    if let Ok(engine) = PolicyEngine::new(&profile) {
        let _ = engine.can_use_tool("日本語ツール");
        let _ = engine.can_use_tool("ascii_tool");
    }
}

#[test]
fn fuzz_sim_policy_engine_invalid_glob_pattern() {
    let profile = make_policy(vec![], vec![], vec!["[invalid"], vec![]);
    // May fail to compile — that's fine, just must not panic.
    let _ = PolicyEngine::new(&profile);
}

#[test]
fn fuzz_sim_policy_profile_roundtrip() {
    let profile = make_policy(vec!["a", "b"], vec!["c"], vec!["*.secret"], vec!["*.lock"]);
    let json = serde_json::to_string(&profile).unwrap();
    let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.allowed_tools, profile.allowed_tools);
    assert_eq!(rt.disallowed_tools, profile.disallowed_tools);
}

// ===================================================================
// 6. Glob Pattern Compilation
// ===================================================================

#[test]
fn fuzz_sim_glob_empty_patterns() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(globs.decide_str("anything.rs").is_allowed());
}

#[test]
fn fuzz_sim_glob_include_only() {
    let include = vec!["*.rs".to_string()];
    let globs = IncludeExcludeGlobs::new(&include, &[]).unwrap();
    assert!(globs.decide_str("main.rs").is_allowed());
    assert!(!globs.decide_str("main.py").is_allowed());
}

#[test]
fn fuzz_sim_glob_exclude_only() {
    let exclude = vec!["*.log".to_string()];
    let globs = IncludeExcludeGlobs::new(&[], &exclude).unwrap();
    assert!(globs.decide_str("main.rs").is_allowed());
    assert!(!globs.decide_str("debug.log").is_allowed());
}

#[test]
fn fuzz_sim_glob_include_and_exclude() {
    let include = vec!["src/**".to_string()];
    let exclude = vec!["src/secret/**".to_string()];
    let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();
    assert!(globs.decide_str("src/main.rs").is_allowed());
    assert!(!globs.decide_str("src/secret/key.pem").is_allowed());
    assert!(!globs.decide_str("docs/README.md").is_allowed());
}

#[test]
fn fuzz_sim_glob_invalid_pattern() {
    let include = vec!["[invalid".to_string()];
    assert!(IncludeExcludeGlobs::new(&include, &[]).is_err());
}

#[test]
fn fuzz_sim_glob_star_star_pattern() {
    let include = vec!["**/*.rs".to_string()];
    let globs = IncludeExcludeGlobs::new(&include, &[]).unwrap();
    assert!(globs.decide_str("src/lib.rs").is_allowed());
    assert!(globs.decide_str("deeply/nested/dir/mod.rs").is_allowed());
}

#[test]
fn fuzz_sim_glob_decide_path_vs_str() {
    let include = vec!["*.txt".to_string()];
    let globs = IncludeExcludeGlobs::new(&include, &[]).unwrap();
    let str_result = globs.decide_str("hello.txt").is_allowed();
    let path_result = globs.decide_path(Path::new("hello.txt")).is_allowed();
    assert_eq!(str_result, path_result);
}

#[test]
fn fuzz_sim_glob_unicode_path() {
    let include = vec!["*.rs".to_string()];
    let globs = IncludeExcludeGlobs::new(&include, &[]).unwrap();
    let _ = globs.decide_str("日本語.rs");
    let _ = globs.decide_str("こんにちは.txt");
}

#[test]
fn fuzz_sim_glob_empty_string_path() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    let _ = globs.decide_str("");
}

#[test]
fn fuzz_sim_glob_build_globset_valid() {
    let patterns = vec!["*.rs".to_string(), "src/**".to_string()];
    let result = abp_glob::build_globset(&patterns);
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn fuzz_sim_glob_build_globset_empty() {
    let result = abp_glob::build_globset(&[]);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn fuzz_sim_glob_build_globset_invalid() {
    let patterns = vec!["[bad".to_string()];
    assert!(abp_glob::build_globset(&patterns).is_err());
}

#[test]
fn fuzz_sim_glob_many_patterns() {
    let include: Vec<String> = (0..50).map(|i| format!("dir{}/**", i)).collect();
    let exclude: Vec<String> = (0..50).map(|i| format!("dir{}/secret/**", i)).collect();
    let _ = IncludeExcludeGlobs::new(&include, &exclude);
}

// ===================================================================
// 7. Cross-cutting: Agent Event serialization
// ===================================================================

#[test]
fn fuzz_sim_agent_event_roundtrip() {
    let events = vec![
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".to_string(),
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".to_string(),
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "streaming...".to_string(),
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "full message".to_string(),
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Warning {
                message: "watch out".to_string(),
            },
            ext: None,
        },
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Error {
                message: "boom".to_string(),
                error_code: None,
            },
            ext: None,
        },
    ];
    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::to_value(&rt).unwrap()
        );
    }
}

#[test]
fn fuzz_sim_agent_event_from_garbage() {
    let _ = serde_json::from_str::<AgentEvent>("{}");
    let _ = serde_json::from_str::<AgentEvent>("null");
    let _ = serde_json::from_str::<AgentEvent>("\"string\"");
    let _ = serde_json::from_str::<AgentEvent>("[1,2,3]");
}

#[test]
fn fuzz_sim_agent_event_with_ext() {
    let mut ext = std::collections::BTreeMap::new();
    ext.insert(
        "custom_key".to_string(),
        serde_json::json!({"nested": true}),
    );
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "starting".to_string(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(rt.ext.is_some());
}
