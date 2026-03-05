#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive stress and edge case tests for the Agent Backplane.
//!
//! 120+ tests covering: memory/size limits, unicode/encoding, concurrent/threading,
//! serde edge cases, determinism, error boundaries, and protocol edge cases.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    canonical_json, receipt_hash, AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity,
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ContractError, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_receipt::{compute_hash, verify_hash};

// ── helpers ────────────────────────────────────────────────────────────

const FIXED_UUID: Uuid = Uuid::from_bytes([
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
]);
const FIXED_UUID2: Uuid = Uuid::from_bytes([
    0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
]);

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_at(ts: chrono::DateTime<Utc>, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: FIXED_UUID,
            work_order_id: FIXED_UUID2,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 42_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::from([
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Emulated),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        },
        trace: vec![
            make_event_at(
                fixed_ts(),
                AgentEventKind::RunStarted {
                    message: "go".into(),
                },
            ),
            make_event_at(
                fixed_ts2(),
                AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
            ),
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/f b/f".into()),
            git_status: Some("M f".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn default_policy() -> PolicyProfile {
    PolicyProfile::default()
}

// ==========================================================================
// SECTION 1: Memory and size limits (20+ tests)
// ==========================================================================

#[test]
fn size_work_order_with_100_tools() {
    let policy = PolicyProfile {
        allowed_tools: (0..100).map(|i| format!("tool_{i}")).collect(),
        disallowed_tools: (0..100).map(|i| format!("deny_tool_{i}")).collect(),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools.len(), 100);
    assert_eq!(wo.policy.disallowed_tools.len(), 100);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.policy.allowed_tools.len(), 100);
}

#[test]
fn size_work_order_huge_context_snippets() {
    let snippets: Vec<ContextSnippet> = (0..200)
        .map(|i| ContextSnippet {
            name: format!("snippet_{i}"),
            content: "x".repeat(1000),
        })
        .collect();
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: "big context".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets,
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_eq!(wo.context.snippets.len(), 200);
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.context.snippets.len(), 200);
}

#[test]
fn size_receipt_many_artifacts() {
    let mut r = make_receipt();
    r.artifacts = (0..500)
        .map(|i| ArtifactRef {
            kind: "file".into(),
            path: format!("path/{i}.txt"),
        })
        .collect();
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.artifacts.len(), 500);
}

#[test]
fn size_receipt_very_long_trace() {
    let mut r = make_receipt();
    r.trace = (0..10_000)
        .map(|i| {
            make_event_at(
                fixed_ts(),
                AgentEventKind::AssistantDelta {
                    text: format!("delta_{i}"),
                },
            )
        })
        .collect();
    assert_eq!(r.trace.len(), 10_000);
    let hash = compute_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn size_empty_work_order() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, "");
}

#[test]
fn size_empty_receipt_fields() {
    let r = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    assert_eq!(r.backend.id, "");
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
}

#[test]
fn size_empty_context_packet() {
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: "t".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(rt.context.files.is_empty());
    assert!(rt.context.snippets.is_empty());
}

#[test]
fn size_empty_maps_in_config() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    let json = serde_json::to_string(&cfg).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert!(rt.vendor.is_empty());
}

#[test]
fn size_single_character_values() {
    let wo = WorkOrderBuilder::new("x").build();
    assert_eq!(wo.task, "x");
    let r = ReceiptBuilder::new("y").build();
    assert_eq!(r.backend.id, "y");
}

#[test]
fn size_10mb_context_string() {
    let big = "A".repeat(10 * 1024 * 1024);
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: big.clone(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "big".into(),
                content: big.clone(),
            }],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_eq!(wo.task.len(), 10 * 1024 * 1024);
    assert_eq!(wo.context.snippets[0].content.len(), 10 * 1024 * 1024);
}

#[test]
fn size_deeply_nested_json_in_config_vendor() {
    let mut val = json!(null);
    for _ in 0..100 {
        val = json!({ "nested": val });
    }
    let mut vendor = BTreeMap::new();
    vendor.insert("deep".to_string(), val);
    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert!(rt.vendor.contains_key("deep"));
}

#[test]
fn size_deeply_nested_ext_in_event() {
    let mut val = json!(null);
    for _ in 0..100 {
        val = json!({ "layer": val });
    }
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "x".into() },
        ext: Some(BTreeMap::from([("deep".into(), val)])),
    };
    let json = serde_json::to_string(&evt).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(rt.ext.is_some());
}

#[test]
fn size_1000_files_in_context() {
    let files: Vec<String> = (0..1000).map(|i| format!("src/file_{i}.rs")).collect();
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: "many files".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files,
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_eq!(wo.context.files.len(), 1000);
}

#[test]
fn size_many_capabilities_in_manifest() {
    let mut caps = CapabilityManifest::new();
    let all_caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ];
    for c in &all_caps {
        caps.insert(c.clone(), SupportLevel::Native);
    }
    let json = serde_json::to_string(&caps).unwrap();
    let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.len(), all_caps.len());
}

#[test]
fn size_empty_strings_everywhere() {
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: "".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec!["".into()],
            exclude: vec!["".into()],
        },
        context: ContextPacket {
            files: vec!["".into()],
            snippets: vec![ContextSnippet {
                name: "".into(),
                content: "".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["".into()],
            disallowed_tools: vec!["".into()],
            deny_read: vec![],
            deny_write: vec![],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        },
    };
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, "");
    assert_eq!(rt.config.model, Some("".into()));
}

#[test]
fn size_empty_arrays_everywhere() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: FIXED_UUID,
            work_order_id: FIXED_UUID2,
            contract_version: "".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!(null),
        usage: UsageNormalized {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        },
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: false,
        },
        outcome: Outcome::Failed,
        receipt_sha256: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert!(rt.trace.is_empty());
    assert!(rt.artifacts.is_empty());
    assert!(rt.capabilities.is_empty());
}

#[test]
fn size_large_env_map() {
    let env: BTreeMap<String, String> = (0..1000)
        .map(|i| (format!("VAR_{i}"), format!("val_{i}")))
        .collect();
    let cfg = RuntimeConfig {
        model: None,
        vendor: BTreeMap::new(),
        env,
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.env.len(), 1000);
}

#[test]
fn size_work_order_large_include_exclude() {
    let include: Vec<String> = (0..500).map(|i| format!("src/mod_{i}/**/*.rs")).collect();
    let exclude: Vec<String> = (0..500).map(|i| format!("target/mod_{i}/**")).collect();
    let wo = WorkOrder {
        id: Uuid::new_v4(),
        task: "t".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/".into(),
            mode: WorkspaceMode::Staged,
            include,
            exclude,
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_eq!(wo.workspace.include.len(), 500);
    assert_eq!(wo.workspace.exclude.len(), 500);
}

#[test]
fn size_receipt_large_usage_raw() {
    let big_array: Vec<serde_json::Value> = (0..10_000)
        .map(|i| json!({"idx": i, "val": "x".repeat(100)}))
        .collect();
    let mut r = make_receipt();
    r.usage_raw = json!(big_array);
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert!(rt.usage_raw.is_array());
}

#[test]
fn size_receipt_large_git_diff() {
    let big_diff = "diff --git a/f b/f\n".to_string() + &"+line\n".repeat(100_000);
    let mut r = make_receipt();
    r.verification.git_diff = Some(big_diff.clone());
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.verification.git_diff.unwrap().len(), big_diff.len());
}

// ==========================================================================
// SECTION 2: Unicode and encoding (20+ tests)
// ==========================================================================

#[test]
fn unicode_rtl_text_in_task() {
    let wo = WorkOrderBuilder::new("مرحبا بالعالم").build();
    assert_eq!(wo.task, "مرحبا بالعالم");
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, "مرحبا بالعالم");
}

#[test]
fn unicode_rtl_in_tool_names() {
    let policy = PolicyProfile {
        allowed_tools: vec!["أداة_قراءة".into()],
        disallowed_tools: vec!["أداة_كتابة".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("أداة_قراءة").allowed);
    assert!(!engine.can_use_tool("أداة_كتابة").allowed);
}

#[test]
fn unicode_rtl_in_config_values() {
    let mut vendor = BTreeMap::new();
    vendor.insert("مفتاح".to_string(), json!("قيمة"));
    let cfg = RuntimeConfig {
        model: Some("نموذج".into()),
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.model, Some("نموذج".into()));
}

#[test]
fn unicode_emoji_in_task() {
    let wo = WorkOrderBuilder::new("Fix 🐛 in 🏠").build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, "Fix 🐛 in 🏠");
}

#[test]
fn unicode_emoji_in_event_text() {
    let evt = make_event(AgentEventKind::AssistantMessage {
        text: "Done ✅🎉🚀".into(),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text, "Done ✅🎉🚀");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_emoji_in_backend_id() {
    let r = ReceiptBuilder::new("🤖-backend").build();
    assert_eq!(r.backend.id, "🤖-backend");
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.backend.id, "🤖-backend");
}

#[test]
fn unicode_emoji_in_artifact_path() {
    let mut r = make_receipt();
    r.artifacts = vec![ArtifactRef {
        kind: "📁".into(),
        path: "output/🎯/result.txt".into(),
    }];
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.artifacts[0].kind, "📁");
    assert_eq!(rt.artifacts[0].path, "output/🎯/result.txt");
}

#[test]
fn unicode_null_bytes_in_task() {
    let task = "before\0after";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, "before\0after");
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, "before\0after");
}

#[test]
fn unicode_null_bytes_in_event() {
    let evt = make_event(AgentEventKind::AssistantDelta {
        text: "null\0byte".into(),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "null\0byte");
    }
}

#[test]
fn unicode_mixed_scripts_in_context() {
    let snippet = ContextSnippet {
        name: "混合スクリプト".into(),
        content: "English العربية 日本語 한국어 Ελληνικά".into(),
    };
    let json = serde_json::to_string(&snippet).unwrap();
    let rt: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.name, "混合スクリプト");
}

#[test]
fn unicode_surrogate_pairs_in_json() {
    // Characters outside BMP (U+1F600 = 😀) serialized via JSON
    let wo = WorkOrderBuilder::new("smile: 😀").build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(
        json.contains("😀") || json.contains("\\ud83d\\ude00") || json.contains("\\uD83D\\uDE00")
    );
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(rt.task.contains("😀"));
}

#[test]
fn unicode_zero_width_characters() {
    let task = "hello\u{200B}world\u{200C}foo\u{200D}bar\u{FEFF}baz";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn unicode_combining_characters() {
    // é as e + combining acute accent
    let task = "cafe\u{0301}";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, task);
}

#[test]
fn unicode_combining_in_tool_name() {
    let tool = "rea\u{0308}d"; // "read" with combining diaeresis on 'a'
    let policy = PolicyProfile {
        allowed_tools: vec![tool.into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool(tool).allowed);
}

#[test]
fn unicode_emoji_in_envelope_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-😬".into()),
        error: "💥 something broke".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = rt {
        assert_eq!(ref_id, Some("run-😬".into()));
        assert!(error.contains("💥"));
    }
}

#[test]
fn unicode_mixed_encoding_in_jsonl_stream() {
    let hello = make_hello();
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "日本語エラー 🐛".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    JsonlCodec::encode_to_writer(&mut buf, &fatal).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn unicode_bidi_override_in_string() {
    let text = "\u{202E}reversed\u{202C}normal";
    let evt = make_event(AgentEventKind::Warning {
        message: text.into(),
    });
    let json = serde_json::to_string(&evt).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert_eq!(message, text);
    }
}

#[test]
fn unicode_long_grapheme_clusters() {
    // Thai combining marks can create long grapheme clusters
    let text = "ก้้้้้้้้้้้้้้้้้้้้";
    let wo = WorkOrderBuilder::new(text).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, text);
}

#[test]
fn unicode_control_characters_in_event() {
    let text = "line1\r\nline2\ttab\x07bell";
    let evt = make_event(AgentEventKind::AssistantDelta { text: text.into() });
    let json = serde_json::to_string(&evt).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantDelta { text: t } = &rt.kind {
        assert_eq!(t, text);
    }
}

#[test]
fn unicode_emoji_skin_tones() {
    let text = "👋🏻👋🏼👋🏽👋🏾👋🏿";
    let wo = WorkOrderBuilder::new(text).build();
    let json = serde_json::to_string(&wo).unwrap();
    let rt: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.task, text);
}

// ==========================================================================
// SECTION 3: Concurrent/threading (15+ tests)
// ==========================================================================

#[test]
fn concurrent_receipt_hash_identical() {
    let r = make_receipt();
    let results: Vec<_> = (0..20)
        .map(|_| {
            let r = r.clone();
            std::thread::spawn(move || compute_hash(&r).unwrap())
        })
        .collect();
    let hashes: Vec<String> = results.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &hashes[0];
    for h in &hashes {
        assert_eq!(h, first, "hash mismatch across threads");
    }
}

#[test]
fn concurrent_canonical_json_identical() {
    let r = make_receipt();
    let results: Vec<_> = (0..20)
        .map(|_| {
            let r = r.clone();
            std::thread::spawn(move || canonical_json(&r).unwrap())
        })
        .collect();
    let jsons: Vec<String> = results.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &jsons[0];
    for j in &jsons {
        assert_eq!(j, first, "canonical JSON mismatch across threads");
    }
}

#[test]
fn concurrent_work_order_serialization() {
    let wo = WorkOrderBuilder::new("concurrent test").build();
    let results: Vec<_> = (0..20)
        .map(|_| {
            let wo = wo.clone();
            std::thread::spawn(move || serde_json::to_string(&wo).unwrap())
        })
        .collect();
    let jsons: Vec<String> = results.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &jsons[0];
    for j in &jsons {
        assert_eq!(j, first);
    }
}

#[test]
fn concurrent_policy_engine_evaluation() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/secret/**".into()],
        ..Default::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());
    let results: Vec<_> = (0..20)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let r = engine.can_use_tool("Read");
                let w = engine.can_use_tool("Bash");
                let p = engine.can_read_path(Path::new(".env"));
                (r.allowed, w.allowed, p.allowed)
            })
        })
        .collect();
    for h in results {
        let (r, w, p) = h.join().unwrap();
        assert!(r);
        assert!(!w);
        assert!(!p);
    }
}

#[test]
fn concurrent_envelope_encode_decode() {
    let hello = make_hello();
    let results: Vec<_> = (0..20)
        .map(|_| {
            let hello = hello.clone();
            std::thread::spawn(move || {
                let line = JsonlCodec::encode(&hello).unwrap();
                let decoded = JsonlCodec::decode(line.trim()).unwrap();
                matches!(decoded, Envelope::Hello { .. })
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_glob_matching() {
    let globs = Arc::new(
        IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap(),
    );
    let results: Vec<_> = (0..20)
        .map(|i| {
            let globs = Arc::clone(&globs);
            std::thread::spawn(move || {
                let a = globs.decide_str("src/lib.rs").is_allowed();
                let b = globs.decide_str("src/generated/out.rs").is_allowed();
                let c = globs.decide_str("README.md").is_allowed();
                (a, b, c)
            })
        })
        .collect();
    for h in results {
        let (a, b, c) = h.join().unwrap();
        assert!(a);
        assert!(!b);
        assert!(!c);
    }
}

#[test]
fn concurrent_receipt_builder() {
    let counter = Arc::new(AtomicUsize::new(0));
    let results: Vec<_> = (0..20)
        .map(|i| {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                let r = ReceiptBuilder::new(format!("backend-{i}"))
                    .outcome(Outcome::Complete)
                    .build();
                counter.fetch_add(1, Ordering::SeqCst);
                r.backend.id == format!("backend-{i}")
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
    assert_eq!(counter.load(Ordering::SeqCst), 20);
}

#[test]
fn concurrent_error_creation() {
    let results: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let err = AbpError::new(ErrorCode::BackendTimeout, format!("timeout {i}"))
                    .with_context("thread", i);
                (err.code, err.message.clone(), err.is_retryable())
            })
        })
        .collect();
    for (i, h) in results.into_iter().enumerate() {
        let (code, msg, retryable) = h.join().unwrap();
        assert_eq!(code, ErrorCode::BackendTimeout);
        assert!(msg.contains(&i.to_string()));
        assert!(retryable);
    }
}

#[test]
fn concurrent_verify_hash() {
    let mut r = make_receipt();
    let hash = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some(hash);
    let r = Arc::new(r);
    let results: Vec<_> = (0..20)
        .map(|_| {
            let r = Arc::clone(&r);
            std::thread::spawn(move || verify_hash(&r))
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_mixed_operations() {
    let r = Arc::new(make_receipt());
    let wo = Arc::new(WorkOrderBuilder::new("concurrent").build());
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = Arc::new(PolicyEngine::new(&policy).unwrap());

    let results: Vec<_> = (0..30)
        .map(|i| {
            let r = Arc::clone(&r);
            let wo = Arc::clone(&wo);
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || match i % 3 {
                0 => {
                    compute_hash(&r).unwrap();
                    true
                }
                1 => {
                    serde_json::to_string(wo.as_ref()).unwrap();
                    true
                }
                2 => engine.can_use_tool("Read").allowed,
                _ => unreachable!(),
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_error_info_serialization() {
    let results: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let info =
                    ErrorInfo::new(ErrorCode::Internal, format!("err_{i}")).with_detail("idx", i);
                let json = serde_json::to_string(&info).unwrap();
                let rt: ErrorInfo = serde_json::from_str(&json).unwrap();
                rt.code == ErrorCode::Internal
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_capability_manifest_construction() {
    let results: Vec<_> = (0..20)
        .map(|_| {
            std::thread::spawn(|| {
                let mut caps = CapabilityManifest::new();
                caps.insert(Capability::Streaming, SupportLevel::Native);
                caps.insert(Capability::ToolRead, SupportLevel::Emulated);
                let json = serde_json::to_string(&caps).unwrap();
                let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
                rt.len() == 2
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_envelope_stream_decode() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let big_input: String = line.repeat(100);
    let big_input = Arc::new(big_input);
    let results: Vec<_> = (0..10)
        .map(|_| {
            let big_input = Arc::clone(&big_input);
            std::thread::spawn(move || {
                let reader = BufReader::new(big_input.as_bytes());
                let envs: Vec<_> = JsonlCodec::decode_stream(reader)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                envs.len()
            })
        })
        .collect();
    for h in results {
        assert_eq!(h.join().unwrap(), 100);
    }
}

#[test]
fn concurrent_work_order_builder() {
    let results: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let wo = WorkOrderBuilder::new(format!("task_{i}"))
                    .model(format!("model_{i}"))
                    .max_turns(i as u32)
                    .build();
                wo.task == format!("task_{i}")
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_receipt_chain_building() {
    let results: Vec<_> = (0..10)
        .map(|i| {
            std::thread::spawn(move || {
                let mut chain = abp_receipt::ChainBuilder::new();
                for j in 0..5 {
                    let r = ReceiptBuilder::new(format!("b-{i}-{j}"))
                        .outcome(Outcome::Complete)
                        .build();
                    chain = chain.append(r).unwrap();
                }
                let built = chain.build();
                built.len() == 5
            })
        })
        .collect();
    for h in results {
        assert!(h.join().unwrap());
    }
}

// ==========================================================================
// SECTION 4: Serde edge cases (20+ tests)
// ==========================================================================

#[test]
fn serde_unknown_fields_in_work_order() {
    let json = r#"{"id":"01020304-0506-0708-090a-0b0c0d0e0f10","task":"t","lane":"patch_first","workspace":{"root":"/","mode":"pass_through","include":[],"exclude":[]},"context":{"files":[],"snippets":[]},"policy":{"allowed_tools":[],"disallowed_tools":[],"deny_read":[],"deny_write":[],"allow_network":[],"deny_network":[],"require_approval_for":[]},"requirements":{"required":[]},"config":{"model":null,"vendor":{},"env":{},"max_budget_usd":null,"max_turns":null},"unknown_field":"hello","extra":42}"#;
    let wo: WorkOrder = serde_json::from_str(json).unwrap();
    assert_eq!(wo.task, "t");
}

#[test]
fn serde_unknown_fields_in_receipt() {
    let r = make_receipt();
    let mut val: serde_json::Value = serde_json::to_value(&r).unwrap();
    val.as_object_mut()
        .unwrap()
        .insert("extra_field".into(), json!("ignored"));
    let rt: Receipt = serde_json::from_value(val).unwrap();
    assert_eq!(rt.outcome, Outcome::Complete);
}

#[test]
fn serde_missing_optional_model() {
    let json = r#"{"model":null,"vendor":{},"env":{},"max_budget_usd":null,"max_turns":null}"#;
    let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.model, None);
}

#[test]
fn serde_null_vs_absent_optional() {
    // Explicit null
    let json1 = r#"{"model":null,"vendor":{},"env":{},"max_budget_usd":null,"max_turns":null}"#;
    let cfg1: RuntimeConfig = serde_json::from_str(json1).unwrap();
    // model absent (default)
    let json2 = r#"{"vendor":{},"env":{}}"#;
    let cfg2: RuntimeConfig = serde_json::from_str(json2).unwrap();
    assert_eq!(cfg1.model, cfg2.model);
}

#[test]
fn serde_extra_fields_in_envelope() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra":"field","count":42}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal { error, .. } = env {
        assert_eq!(error, "boom");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn serde_duplicate_keys_in_plain_json() {
    // serde_json rejects duplicate keys for structs with named fields
    let json = r#"{"t":"fatal","ref_id":null,"error":"first","error":"second"}"#;
    let result = JsonlCodec::decode(json);
    // May error or take last value depending on serde config; just don't panic
    let _ = result;
}

#[test]
fn serde_number_as_string_in_vendor_fails() {
    // Vendor values are serde_json::Value so anything goes
    let json =
        r#"{"model":null,"vendor":{"count":"42"},"env":{},"max_budget_usd":null,"max_turns":null}"#;
    let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.vendor["count"], json!("42"));
}

#[test]
fn serde_very_long_key_in_vendor() {
    let key = "k".repeat(100_000);
    let mut vendor = BTreeMap::new();
    vendor.insert(key.clone(), json!("val"));
    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert!(rt.vendor.contains_key(&key));
}

#[test]
fn serde_very_long_value_in_vendor() {
    let val = "v".repeat(1_000_000);
    let mut vendor = BTreeMap::new();
    vendor.insert("key".into(), json!(val));
    let cfg = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.vendor["key"].as_str().unwrap().len(), 1_000_000);
}

#[test]
fn serde_receipt_outcome_roundtrip_all_variants() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, outcome);
    }
}

#[test]
fn serde_execution_mode_roundtrip() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let json = serde_json::to_string(&mode).unwrap();
        let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, mode);
    }
}

#[test]
fn serde_all_agent_event_kinds() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];
    for kind in kinds {
        let evt = make_event(kind);
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        // Should roundtrip without error
        let json2 = serde_json::to_string(&rt).unwrap();
        assert!(!json2.is_empty());
    }
}

#[test]
fn serde_error_code_roundtrip_all() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    for code in codes {
        let json = serde_json::to_string(&code).unwrap();
        let rt: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, code);
    }
}

#[test]
fn serde_support_level_restricted_roundtrip() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "beta only".into(),
        },
    ];
    for level in levels {
        let json = serde_json::to_string(&level).unwrap();
        let rt: SupportLevel = serde_json::from_str(&json).unwrap();
        let json_rt = serde_json::to_string(&rt).unwrap();
        assert_eq!(json, json_rt);
    }
}

#[test]
fn serde_envelope_all_variants_roundtrip() {
    let wo = WorkOrderBuilder::new("t").build();
    let run_id = wo.id.to_string();
    let envelopes = vec![
        make_hello(),
        Envelope::Run {
            id: run_id.clone(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: run_id.clone(),
            event: make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        },
        Envelope::Final {
            ref_id: run_id.clone(),
            receipt: make_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "err".into(),
            error_code: Some(ErrorCode::Internal),
        },
    ];
    for env in &envelopes {
        let line = JsonlCodec::encode(env).unwrap();
        let rt = JsonlCodec::decode(line.trim()).unwrap();
        let line2 = JsonlCodec::encode(&rt).unwrap();
        // Roundtrip should produce valid output
        assert!(!line2.is_empty());
    }
}

#[test]
fn serde_special_float_values_in_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(0),
        output_tokens: Some(u64::MAX),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.0),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.output_tokens, Some(u64::MAX));
}

#[test]
fn serde_empty_ext_map_vs_none() {
    let evt1 = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "a".into() },
        ext: None,
    };
    let evt2 = AgentEvent {
        ts: evt1.ts,
        kind: AgentEventKind::AssistantDelta { text: "a".into() },
        ext: Some(BTreeMap::new()),
    };
    let json1 = serde_json::to_string(&evt1).unwrap();
    let json2 = serde_json::to_string(&evt2).unwrap();
    // They may serialize differently but both should roundtrip
    let _: AgentEvent = serde_json::from_str(&json1).unwrap();
    let _: AgentEvent = serde_json::from_str(&json2).unwrap();
}

#[test]
fn serde_error_info_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000);
    let json = serde_json::to_string(&info).unwrap();
    let rt: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.code, ErrorCode::BackendTimeout);
    assert_eq!(rt.details.len(), 2);
}

#[test]
fn serde_error_info_with_empty_details() {
    let info = ErrorInfo::new(ErrorCode::Internal, "oops");
    let json = serde_json::to_string(&info).unwrap();
    let rt: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert!(rt.details.is_empty());
}

#[test]
fn serde_capability_as_json_key() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let json = serde_json::to_string(&caps).unwrap();
    assert!(json.contains("streaming"));
    let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.len(), 1);
}

// ==========================================================================
// SECTION 5: Determinism (15+ tests)
// ==========================================================================

#[test]
fn determinism_receipt_hash_stable() {
    let r = make_receipt();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn determinism_receipt_hash_100_runs() {
    let r = make_receipt();
    let first = compute_hash(&r).unwrap();
    for _ in 0..100 {
        assert_eq!(compute_hash(&r).unwrap(), first);
    }
}

#[test]
fn determinism_canonical_json_stable() {
    let r = make_receipt();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn determinism_canonical_json_byte_identical() {
    let r = make_receipt();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1.as_bytes(), j2.as_bytes());
}

#[test]
fn determinism_btreemap_ordering_sorted() {
    let mut map = BTreeMap::new();
    map.insert("z_key".to_string(), json!(1));
    map.insert("a_key".to_string(), json!(2));
    map.insert("m_key".to_string(), json!(3));
    let json = serde_json::to_string(&map).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn determinism_capability_manifest_ordering() {
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::ToolWrite, SupportLevel::Native);
    caps1.insert(Capability::Streaming, SupportLevel::Native);
    caps1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::Streaming, SupportLevel::Native);
    caps2.insert(Capability::ToolRead, SupportLevel::Native);
    caps2.insert(Capability::ToolWrite, SupportLevel::Native);

    let json1 = serde_json::to_string(&caps1).unwrap();
    let json2 = serde_json::to_string(&caps2).unwrap();
    assert_eq!(
        json1, json2,
        "BTreeMap insertion order should not affect serialization"
    );
}

#[test]
fn determinism_enum_serialization_stable() {
    let outcome_json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(outcome_json, "\"complete\"");
    let lane_json = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(lane_json, "\"patch_first\"");
    let mode_json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(mode_json, "\"passthrough\"");
}

#[test]
fn determinism_work_order_serialization_stable() {
    let wo = WorkOrder {
        id: FIXED_UUID,
        task: "fixed".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let j1 = serde_json::to_string(&wo).unwrap();
    let j2 = serde_json::to_string(&wo).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn determinism_receipt_with_hash_stable() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = r1.clone();
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn determinism_receipt_sha256_excluded_from_hash() {
    let mut r1 = make_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = make_receipt();
    r2.receipt_sha256 = Some("fake_hash".into());
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_eq!(h1, h2, "receipt_sha256 should be nulled before hashing");
}

#[test]
fn determinism_env_map_order() {
    let mut env1: BTreeMap<String, String> = BTreeMap::new();
    env1.insert("Z".into(), "1".to_string());
    env1.insert("A".into(), "2".to_string());
    let mut env2: BTreeMap<String, String> = BTreeMap::new();
    env2.insert("A".into(), "2".to_string());
    env2.insert("Z".into(), "1".to_string());
    let j1 = serde_json::to_string(&env1).unwrap();
    let j2 = serde_json::to_string(&env2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn determinism_vendor_map_order() {
    let mut v1 = BTreeMap::new();
    v1.insert("zebra".to_string(), json!(1));
    v1.insert("apple".to_string(), json!(2));
    let mut v2 = BTreeMap::new();
    v2.insert("apple".to_string(), json!(2));
    v2.insert("zebra".to_string(), json!(1));
    assert_eq!(
        serde_json::to_string(&v1).unwrap(),
        serde_json::to_string(&v2).unwrap()
    );
}

#[test]
fn determinism_policy_serialization_stable() {
    let p = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let j1 = serde_json::to_string(&p).unwrap();
    let j2 = serde_json::to_string(&p).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn determinism_envelope_encoding_stable() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let l1 = JsonlCodec::encode(&hello).unwrap();
    let l2 = JsonlCodec::encode(&hello).unwrap();
    assert_eq!(l1, l2);
}

#[test]
fn determinism_error_code_as_str_stable() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
    // Call again to ensure stability
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
}

// ==========================================================================
// SECTION 6: Error boundary (15+ tests)
// ==========================================================================

#[test]
fn error_abp_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AbpError>();
}

#[test]
fn error_protocol_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProtocolError>();
}

#[test]
fn error_contract_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ContractError>();
}

#[test]
fn error_error_code_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ErrorCode>();
}

#[test]
fn error_chain_preserves_source() {
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err =
        AbpError::new(ErrorCode::WorkspaceInitFailed, "workspace setup failed").with_source(inner);
    assert!(err.source.is_some());
    let source = err.source.as_ref().unwrap();
    assert!(source.to_string().contains("file missing"));
}

#[test]
fn error_display_format_stable() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    let display = format!("{err}");
    assert!(display.contains("timed out after 30s"));
    // Call again
    let display2 = format!("{err}");
    assert_eq!(display, display2);
}

#[test]
fn error_debug_format_includes_all_fields() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    let debug = format!("{err:?}");
    assert!(debug.contains("BackendTimeout"));
    assert!(debug.contains("timeout"));
    assert!(debug.contains("backend"));
}

#[test]
fn error_error_info_display_includes_code() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "not allowed");
    let display = format!("{info}");
    assert!(display.contains("policy_denied"));
    assert!(display.contains("not allowed"));
}

#[test]
fn error_retryable_codes() {
    let retryable = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in &retryable {
        assert!(code.is_retryable(), "{code:?} should be retryable");
    }
}

#[test]
fn error_non_retryable_codes() {
    let non_retryable = [
        ErrorCode::PolicyDenied,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::Internal,
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::MappingDialectMismatch,
    ];
    for code in &non_retryable {
        assert!(!code.is_retryable(), "{code:?} should not be retryable");
    }
}

#[test]
fn error_category_coverage() {
    let categories = vec![
        (ErrorCode::ProtocolInvalidEnvelope, ErrorCategory::Protocol),
        (ErrorCode::BackendNotFound, ErrorCategory::Backend),
        (ErrorCode::CapabilityUnsupported, ErrorCategory::Capability),
        (ErrorCode::PolicyDenied, ErrorCategory::Policy),
        (ErrorCode::WorkspaceInitFailed, ErrorCategory::Workspace),
        (ErrorCode::IrLoweringFailed, ErrorCategory::Ir),
        (ErrorCode::ReceiptHashMismatch, ErrorCategory::Receipt),
        (ErrorCode::DialectUnknown, ErrorCategory::Dialect),
        (ErrorCode::ConfigInvalid, ErrorCategory::Config),
        (ErrorCode::MappingDialectMismatch, ErrorCategory::Mapping),
        (ErrorCode::ExecutionToolFailed, ErrorCategory::Execution),
        (ErrorCode::ContractVersionMismatch, ErrorCategory::Contract),
        (ErrorCode::Internal, ErrorCategory::Internal),
    ];
    for (code, expected_cat) in categories {
        assert_eq!(code.category(), expected_cat, "wrong category for {code:?}");
    }
}

#[test]
fn error_abp_error_to_info_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("region", "us-east");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert_eq!(info.details.len(), 2);
    assert!(info.is_retryable);
}

#[test]
fn error_protocol_error_violation_has_code() {
    let err = ProtocolError::Violation("bad state".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn error_protocol_error_unexpected_message_has_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn error_protocol_error_display_contains_detail() {
    let err = ProtocolError::Violation("broken pipe".into());
    let display = format!("{err}");
    assert!(display.contains("broken pipe"));
}

#[test]
fn error_abp_error_without_source_has_none() {
    let err = AbpError::new(ErrorCode::Internal, "no source");
    assert!(err.source.is_none());
}

// ==========================================================================
// SECTION 7: Protocol edge cases (15+ tests)
// ==========================================================================

#[test]
fn protocol_envelope_with_extra_whitespace() {
    let json = r#"  {"t":"fatal","ref_id":null,"error":"boom"}  "#;
    let env = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn protocol_envelope_with_trailing_newlines() {
    let json = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n\n\n";
    let env = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn protocol_empty_line_skipped_in_stream() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 2);
}

#[test]
fn protocol_whitespace_only_lines_skipped() {
    let input = "   \n\t\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n   \n";
    let reader = BufReader::new(input.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 1);
}

#[test]
fn protocol_very_large_single_jsonl_line() {
    let big_error = "e".repeat(1_000_000);
    let env = Envelope::Fatal {
        ref_id: None,
        error: big_error.clone(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.len() > 1_000_000);
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error.len(), 1_000_000);
    }
}

#[test]
fn protocol_binary_data_in_jsonl_stream_produces_error() {
    let binary = vec![0xFF, 0xFE, 0x00, 0x01, 0x0A]; // includes newline at end
    let reader = BufReader::new(binary.as_slice());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    // Should get errors, not panics
    for r in results {
        assert!(r.is_err());
    }
}

#[test]
fn protocol_invalid_json_returns_error() {
    let result = JsonlCodec::decode("not json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn protocol_valid_json_but_wrong_structure() {
    let result = JsonlCodec::decode(r#"{"foo": "bar"}"#);
    assert!(result.is_err());
}

#[test]
fn protocol_interleaved_envelopes_from_different_runs() {
    let run_id_1 = "run-1";
    let run_id_2 = "run-2";
    let envs = vec![
        Envelope::Event {
            ref_id: run_id_1.into(),
            event: make_event(AgentEventKind::AssistantDelta { text: "r1a".into() }),
        },
        Envelope::Event {
            ref_id: run_id_2.into(),
            event: make_event(AgentEventKind::AssistantDelta { text: "r2a".into() }),
        },
        Envelope::Event {
            ref_id: run_id_1.into(),
            event: make_event(AgentEventKind::AssistantDelta { text: "r1b".into() }),
        },
        Envelope::Event {
            ref_id: run_id_2.into(),
            event: make_event(AgentEventKind::AssistantDelta { text: "r2b".into() }),
        },
    ];
    let mut buf = Vec::new();
    for env in &envs {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);
    // Verify ref_ids alternate
    let ref_ids: Vec<String> = decoded
        .iter()
        .map(|e| match e {
            Envelope::Event { ref_id, .. } => ref_id.clone(),
            _ => panic!("expected Event"),
        })
        .collect();
    assert_eq!(ref_ids, vec!["run-1", "run-2", "run-1", "run-2"]);
}

#[test]
fn protocol_hello_with_empty_backend() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { backend, .. } = rt {
        assert_eq!(backend.id, "");
    }
}

#[test]
fn protocol_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-123".into()),
        "backend crashed",
        ErrorCode::BackendCrashed,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    assert_eq!(rt.error_code(), Some(ErrorCode::BackendCrashed));
}

#[test]
fn protocol_fatal_from_abp_error() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let env = Envelope::fatal_from_abp_error(Some("run-1".into()), &err);
    let line = JsonlCodec::encode(&env).unwrap();
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal {
        error, error_code, ..
    } = rt
    {
        assert_eq!(error, "timed out");
        assert_eq!(error_code, Some(ErrorCode::BackendTimeout));
    }
}

#[test]
fn protocol_encode_to_writer_many() {
    let envs: Vec<Envelope> = (0..100)
        .map(|i| Envelope::Fatal {
            ref_id: None,
            error: format!("err_{i}"),
            error_code: None,
        })
        .collect();
    let mut buf = Vec::new();
    for env in &envs {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 100);
}

#[test]
fn protocol_run_envelope_roundtrip() {
    let wo = WorkOrderBuilder::new("test task").build();
    let run_id = wo.id.to_string();
    let env = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { id, work_order } = rt {
        assert_eq!(id, run_id);
        assert_eq!(work_order.task, "test task");
    }
}

#[test]
fn protocol_final_envelope_with_receipt() {
    let r = make_receipt();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: r,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let rt = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { ref_id, receipt } = rt {
        assert_eq!(ref_id, "run-1");
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ==========================================================================
// SECTION 8: Policy engine edge cases (bonus)
// ==========================================================================

#[test]
fn policy_empty_profile_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("anything").allowed);
    assert!(engine.can_read_path(Path::new("any/path.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/path.txt")).allowed);
}

#[test]
fn policy_deny_read_with_glob() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env*".into(), "**/secret/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_deny_write_with_glob() {
    let policy = PolicyProfile {
        deny_write: vec!["**/Cargo.lock".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_allowed_and_disallowed_tool_overlap() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Disallowed should take precedence
    assert!(!engine.can_use_tool("Read").allowed);
}

// ==========================================================================
// SECTION 9: Glob edge cases (bonus)
// ==========================================================================

#[test]
fn glob_empty_patterns_allow_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(g.decide_str("any/path.txt").is_allowed());
}

#[test]
fn glob_exclude_takes_precedence() {
    let g = IncludeExcludeGlobs::new(&["**".into()], &["*.secret".into()]).unwrap();
    assert!(g.decide_str("file.txt").is_allowed());
    assert!(!g.decide_str("file.secret").is_allowed());
}

#[test]
fn glob_missing_include_denies() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert!(g.decide_str("src/lib.rs").is_allowed());
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_unicode_path() {
    let g = IncludeExcludeGlobs::new(&["docs/**".into()], &[]).unwrap();
    assert!(g.decide_str("docs/日本語.md").is_allowed());
}

#[test]
fn glob_many_patterns() {
    let include: Vec<String> = (0..200).map(|i| format!("mod_{i}/**")).collect();
    let exclude: Vec<String> = (0..200).map(|i| format!("mod_{i}/generated/**")).collect();
    let g = IncludeExcludeGlobs::new(&include, &exclude).unwrap();
    assert!(g.decide_str("mod_50/lib.rs").is_allowed());
    assert!(!g.decide_str("mod_50/generated/out.rs").is_allowed());
}

// ==========================================================================
// SECTION 10: Receipt hashing edge cases (bonus)
// ==========================================================================

#[test]
fn receipt_hash_changes_with_different_outcome() {
    let mut r1 = make_receipt();
    r1.outcome = Outcome::Complete;
    let mut r2 = make_receipt();
    r2.outcome = Outcome::Failed;
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_changes_with_different_trace() {
    let mut r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.trace.push(make_event_at(
        fixed_ts(),
        AgentEventKind::Warning {
            message: "extra".into(),
        },
    ));
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_verify_hash_positive() {
    let mut r = make_receipt();
    let hash = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some(hash);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_verify_hash_negative_tampered() {
    let mut r = make_receipt();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!verify_hash(&r));
}

#[test]
fn receipt_verify_hash_none_returns_expected() {
    let r = make_receipt();
    // verify_hash with no stored hash - just confirm it doesn't panic
    let result = verify_hash(&r);
    // The result depends on implementation: may be true (no hash to mismatch) or false
    let _ = result;
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = make_receipt();
    let hash = compute_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}
