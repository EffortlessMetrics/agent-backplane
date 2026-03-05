#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;

use abp_capability::{NegotiationResult, negotiate_capabilities};
use abp_config::BackplaneConfig;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder, receipt_hash,
};
use abp_dialect::Dialect;
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_ir::normalize;
use abp_mapper::{
    ClaudeGeminiIrMapper, IrMapper, OpenAiClaudeIrMapper, OpenAiGeminiIrMapper, default_ir_mapper,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_receipt(trace_len: usize, artifact_count: usize) -> Receipt {
    let mut builder = ReceiptBuilder::new("bench-backend").outcome(Outcome::Complete);
    for i in 0..trace_len {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "x".repeat(i % 64 + 1),
            },
            ext: None,
        });
    }
    for i in 0..artifact_count {
        builder = builder.add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: format!("artifacts/file_{i}.patch"),
        });
    }
    builder.build()
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Refactor the authentication module for better security").build()
}

fn make_agent_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tu-001".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/auth.rs"}),
        },
        ext: None,
    }
}

fn make_conversation(msg_count: usize) -> IrConversation {
    let mut msgs = vec![IrMessage::text(
        IrRole::System,
        "You are a helpful coding assistant.",
    )];
    for i in 0..msg_count {
        if i % 2 == 0 {
            msgs.push(IrMessage::text(
                IrRole::User,
                format!("Question {i}: How do I fix this bug?"),
            ));
        } else {
            msgs.push(IrMessage::text(
                IrRole::Assistant,
                format!("Answer {i}: Here's how you fix it..."),
            ));
        }
    }
    // Add a system message in the middle to exercise dedup
    msgs.push(IrMessage::text(
        IrRole::System,
        "Additional instruction: be concise.",
    ));
    IrConversation::from_messages(msgs)
}

fn make_rich_conversation() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "  You are a helpful assistant.  ",
        ))
        .push(IrMessage::text(IrRole::User, "  Hello world  "))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think about this...".into(),
                },
                IrContentBlock::Text {
                    text: "Hi! ".into(),
                },
                IrContentBlock::Text {
                    text: "How can I help?".into(),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "Read".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu-1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::System, "  Extra system prompt  "))
}

fn make_manifest() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(Capability::ToolGlob, SupportLevel::Native);
    m.insert(Capability::ToolGrep, SupportLevel::Native);
    m.insert(Capability::ToolUse, SupportLevel::Native);
    m.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    m.insert(Capability::ImageInput, SupportLevel::Unsupported);
    m
}

fn make_hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        make_manifest(),
    )
}

fn make_run_envelope() -> Envelope {
    let wo = make_work_order();
    Envelope::Run {
        id: "run-bench-001".into(),
        work_order: wo,
    }
}

fn make_event_envelope() -> Envelope {
    Envelope::Event {
        ref_id: "run-bench-001".into(),
        event: make_agent_event(),
    }
}

fn make_final_envelope() -> Envelope {
    Envelope::Final {
        ref_id: "run-bench-001".into(),
        receipt: make_receipt(5, 1),
    }
}

fn make_fatal_envelope() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-bench-001".into()),
        error: "out of memory".into(),
        error_code: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Receipt hashing — varying payload sizes
// ═══════════════════════════════════════════════════════════════════════════

fn bench_receipt_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash");

    for &(label, trace, artifacts) in &[
        ("empty", 0, 0),
        ("small", 5, 2),
        ("medium", 50, 10),
        ("large", 500, 50),
    ] {
        let receipt = make_receipt(trace, artifacts);
        let json_len = serde_json::to_string(&receipt).unwrap().len();
        group.throughput(Throughput::Bytes(json_len as u64));
        group.bench_with_input(
            BenchmarkId::new("with_hash", label),
            &receipt,
            |b, receipt| {
                b.iter(|| {
                    let mut r = receipt.clone();
                    let _ = black_box(r.with_hash().unwrap());
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("receipt_hash_fn", label),
            &receipt,
            |b, receipt| {
                b.iter(|| {
                    let _ = black_box(receipt_hash(receipt).unwrap());
                });
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Envelope codec — JSONL encode/decode for all variants
// ═══════════════════════════════════════════════════════════════════════════

fn bench_envelope_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_codec");

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", make_hello_envelope()),
        ("run", make_run_envelope()),
        ("event", make_event_envelope()),
        ("final", make_final_envelope()),
        ("fatal", make_fatal_envelope()),
    ];

    for (name, envelope) in &variants {
        group.bench_with_input(BenchmarkId::new("encode", name), envelope, |b, env| {
            b.iter(|| black_box(JsonlCodec::encode(env).unwrap()));
        });

        let encoded = JsonlCodec::encode(envelope).unwrap();
        let line = encoded.trim();
        group.bench_with_input(
            BenchmarkId::new("decode", name),
            &line.to_string(),
            |b, line| {
                b.iter(|| black_box(JsonlCodec::decode(line).unwrap()));
            },
        );

        group.bench_with_input(BenchmarkId::new("roundtrip", name), envelope, |b, env| {
            b.iter(|| {
                let encoded = JsonlCodec::encode(env).unwrap();
                black_box(JsonlCodec::decode(encoded.trim()).unwrap())
            });
        });
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Serde roundtrip — serialize + deserialize for core types
// ═══════════════════════════════════════════════════════════════════════════

fn bench_serde_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_roundtrip");

    // WorkOrder
    let wo = make_work_order();
    group.bench_function("work_order/serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&wo).unwrap()));
    });
    let wo_json = serde_json::to_string(&wo).unwrap();
    group.bench_function("work_order/deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<WorkOrder>(&wo_json).unwrap()));
    });

    // Receipt (small)
    let receipt_s = make_receipt(5, 2);
    group.bench_function("receipt_small/serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&receipt_s).unwrap()));
    });
    let receipt_s_json = serde_json::to_string(&receipt_s).unwrap();
    group.bench_function("receipt_small/deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<Receipt>(&receipt_s_json).unwrap()));
    });

    // Receipt (large)
    let receipt_l = make_receipt(200, 20);
    group.bench_function("receipt_large/serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&receipt_l).unwrap()));
    });
    let receipt_l_json = serde_json::to_string(&receipt_l).unwrap();
    group.bench_function("receipt_large/deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<Receipt>(&receipt_l_json).unwrap()));
    });

    // AgentEvent
    let event = make_agent_event();
    group.bench_function("agent_event/serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&event).unwrap()));
    });
    let event_json = serde_json::to_string(&event).unwrap();
    group.bench_function("agent_event/deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<AgentEvent>(&event_json).unwrap()));
    });

    // BackplaneConfig
    let config = BackplaneConfig::default();
    group.bench_function("config/serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&config).unwrap()));
    });
    let config_json = serde_json::to_string(&config).unwrap();
    group.bench_function("config/deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<BackplaneConfig>(&config_json).unwrap()));
    });

    // RuntimeConfig
    let rt_config = RuntimeConfig {
        model: Some("gpt-4".into()),
        max_budget_usd: Some(1.0),
        max_turns: Some(10),
        ..RuntimeConfig::default()
    };
    group.bench_function("runtime_config/serialize", |b| {
        b.iter(|| black_box(serde_json::to_string(&rt_config).unwrap()));
    });
    let rt_json = serde_json::to_string(&rt_config).unwrap();
    group.bench_function("runtime_config/deserialize", |b| {
        b.iter(|| black_box(serde_json::from_str::<RuntimeConfig>(&rt_json).unwrap()));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. IR normalization — conversation normalization passes
// ═══════════════════════════════════════════════════════════════════════════

fn bench_ir_normalization(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_normalization");

    // Small conversation
    let small_conv = make_conversation(4);
    group.bench_function("normalize/small_4msg", |b| {
        b.iter(|| black_box(normalize::normalize(&small_conv)));
    });

    // Medium conversation
    let medium_conv = make_conversation(20);
    group.bench_function("normalize/medium_20msg", |b| {
        b.iter(|| black_box(normalize::normalize(&medium_conv)));
    });

    // Large conversation
    let large_conv = make_conversation(100);
    group.bench_function("normalize/large_100msg", |b| {
        b.iter(|| black_box(normalize::normalize(&large_conv)));
    });

    // Rich conversation with mixed content blocks
    let rich_conv = make_rich_conversation();
    group.bench_function("normalize/rich_mixed", |b| {
        b.iter(|| black_box(normalize::normalize(&rich_conv)));
    });

    // Individual passes
    group.bench_function("dedup_system/medium", |b| {
        b.iter(|| black_box(normalize::dedup_system(&medium_conv)));
    });
    group.bench_function("trim_text/medium", |b| {
        b.iter(|| black_box(normalize::trim_text(&medium_conv)));
    });
    group.bench_function("merge_adjacent_text/rich", |b| {
        b.iter(|| black_box(normalize::merge_adjacent_text(&rich_conv)));
    });
    group.bench_function("strip_empty/medium", |b| {
        b.iter(|| black_box(normalize::strip_empty(&medium_conv)));
    });
    group.bench_function("strip_metadata/medium", |b| {
        b.iter(|| black_box(normalize::strip_metadata(&medium_conv, &[])));
    });
    group.bench_function("extract_system/medium", |b| {
        b.iter(|| black_box(normalize::extract_system(&medium_conv)));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Mapper throughput — cross-dialect IR mapping
// ═══════════════════════════════════════════════════════════════════════════

fn bench_mapper_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapper_throughput");

    let conv = make_rich_conversation();

    // OpenAI → Claude
    let oai_claude = OpenAiClaudeIrMapper;
    group.bench_function("openai_to_claude", |b| {
        b.iter(|| {
            black_box(
                oai_claude
                    .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
                    .unwrap(),
            )
        });
    });

    // Claude → OpenAI
    group.bench_function("claude_to_openai", |b| {
        b.iter(|| {
            black_box(
                oai_claude
                    .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
                    .unwrap(),
            )
        });
    });

    // OpenAI → Gemini
    let oai_gemini = OpenAiGeminiIrMapper;
    group.bench_function("openai_to_gemini", |b| {
        b.iter(|| {
            black_box(
                oai_gemini
                    .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
                    .unwrap(),
            )
        });
    });

    // Claude → Gemini
    let claude_gemini = ClaudeGeminiIrMapper;
    group.bench_function("claude_to_gemini", |b| {
        b.iter(|| {
            black_box(
                claude_gemini
                    .map_request(Dialect::Claude, Dialect::Gemini, &conv)
                    .unwrap(),
            )
        });
    });

    // Factory-based lookup + mapping
    group.bench_function("factory_openai_to_claude", |b| {
        b.iter(|| {
            let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
            black_box(
                mapper
                    .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
                    .unwrap(),
            )
        });
    });

    // Map a larger conversation
    let large_conv = make_conversation(50);
    group.bench_function("openai_to_claude/large_50msg", |b| {
        b.iter(|| {
            black_box(
                oai_claude
                    .map_request(Dialect::OpenAi, Dialect::Claude, &large_conv)
                    .unwrap(),
            )
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Policy evaluation — PolicyEngine allow/deny checks
// ═══════════════════════════════════════════════════════════════════════════

fn bench_policy_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval");

    // Compile a policy
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into(), "Glob".into()],
        disallowed_tools: vec!["Bash*".into(), "Delete*".into()],
        deny_read: vec!["**/.env".into(), "**/.env.*".into(), "**/id_rsa".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        ..PolicyProfile::default()
    };

    group.bench_function("compile", |b| {
        b.iter(|| black_box(PolicyEngine::new(&policy).unwrap()));
    });

    let engine = PolicyEngine::new(&policy).unwrap();

    // Tool checks — allowed
    group.bench_function("can_use_tool/allowed", |b| {
        b.iter(|| black_box(engine.can_use_tool("Read")));
    });

    // Tool checks — denied by denylist
    group.bench_function("can_use_tool/denied", |b| {
        b.iter(|| black_box(engine.can_use_tool("BashExec")));
    });

    // Tool checks — denied by missing allowlist
    group.bench_function("can_use_tool/not_in_allowlist", |b| {
        b.iter(|| black_box(engine.can_use_tool("WebFetch")));
    });

    // Path checks — read allowed
    group.bench_function("can_read/allowed", |b| {
        b.iter(|| black_box(engine.can_read_path(Path::new("src/lib.rs"))));
    });

    // Path checks — read denied
    group.bench_function("can_read/denied", |b| {
        b.iter(|| black_box(engine.can_read_path(Path::new(".env"))));
    });

    // Path checks — write allowed
    group.bench_function("can_write/allowed", |b| {
        b.iter(|| black_box(engine.can_write_path(Path::new("src/main.rs"))));
    });

    // Path checks — write denied
    group.bench_function("can_write/denied", |b| {
        b.iter(|| black_box(engine.can_write_path(Path::new(".git/config"))));
    });

    // Deep nested path
    group.bench_function("can_write/deep_nested", |b| {
        b.iter(|| {
            black_box(engine.can_write_path(Path::new("a/b/c/d/e/node_modules/pkg/index.js")))
        });
    });

    // Empty policy (everything allowed)
    let open_engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    group.bench_function("empty_policy/tool", |b| {
        b.iter(|| black_box(open_engine.can_use_tool("Anything")));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Glob matching — include/exclude compilation and matching
// ═══════════════════════════════════════════════════════════════════════════

fn bench_glob_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_matching");

    let include: Vec<String> = vec!["src/**".into(), "tests/**".into(), "benches/**".into()];
    let exclude: Vec<String> = vec![
        "src/generated/**".into(),
        "tests/fixtures/**".into(),
        "**/*.bak".into(),
    ];

    // Compilation
    group.bench_function("compile/include_only", |b| {
        b.iter(|| black_box(IncludeExcludeGlobs::new(&include, &[]).unwrap()));
    });
    group.bench_function("compile/exclude_only", |b| {
        b.iter(|| black_box(IncludeExcludeGlobs::new(&[], &exclude).unwrap()));
    });
    group.bench_function("compile/both", |b| {
        b.iter(|| black_box(IncludeExcludeGlobs::new(&include, &exclude).unwrap()));
    });

    let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();

    // Matching — allowed
    group.bench_function("decide/allowed", |b| {
        b.iter(|| black_box(globs.decide_str("src/lib.rs")));
    });

    // Matching — denied by exclude
    group.bench_function("decide/denied_exclude", |b| {
        b.iter(|| black_box(globs.decide_str("src/generated/output.rs")));
    });

    // Matching — denied by missing include
    group.bench_function("decide/denied_missing_include", |b| {
        b.iter(|| black_box(globs.decide_str("docs/readme.md")));
    });

    // Matching — path-based API
    group.bench_function("decide_path/allowed", |b| {
        b.iter(|| black_box(globs.decide_path(Path::new("tests/unit.rs"))));
    });

    // Many patterns
    let many_include: Vec<String> = (0..50).map(|i| format!("dir_{i}/**")).collect();
    let many_exclude: Vec<String> = (0..20).map(|i| format!("dir_{i}/secret/**")).collect();
    group.bench_function("compile/many_patterns", |b| {
        b.iter(|| black_box(IncludeExcludeGlobs::new(&many_include, &many_exclude).unwrap()));
    });

    let many_globs = IncludeExcludeGlobs::new(&many_include, &many_exclude).unwrap();
    group.bench_function("decide/many_patterns_hit", |b| {
        b.iter(|| black_box(many_globs.decide_str("dir_25/file.rs")));
    });
    group.bench_function("decide/many_patterns_miss", |b| {
        b.iter(|| black_box(many_globs.decide_str("other/file.rs")));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Capability negotiation — manifest comparison
// ═══════════════════════════════════════════════════════════════════════════

fn bench_capability_negotiation(c: &mut Criterion) {
    let mut group = c.benchmark_group("capability_negotiation");

    let manifest = make_manifest();

    // Small requirement set — all native
    let small_reqs = vec![Capability::Streaming, Capability::ToolRead];
    group.bench_function("negotiate/small_all_native", |b| {
        b.iter(|| black_box(negotiate_capabilities(&small_reqs, &manifest)));
    });

    // Medium requirement set — mixed
    let medium_reqs = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolBash,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::Vision,
    ];
    group.bench_function("negotiate/medium_mixed", |b| {
        b.iter(|| black_box(negotiate_capabilities(&medium_reqs, &manifest)));
    });

    // Large requirement set
    let large_reqs = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
    ];
    group.bench_function("negotiate/large_15caps", |b| {
        b.iter(|| black_box(negotiate_capabilities(&large_reqs, &manifest)));
    });

    // Empty requirements (fast path)
    group.bench_function("negotiate/empty", |b| {
        b.iter(|| black_box(negotiate_capabilities(&[], &manifest)));
    });

    // Empty manifest (all unsupported)
    let empty_manifest: CapabilityManifest = BTreeMap::new();
    group.bench_function("negotiate/empty_manifest", |b| {
        b.iter(|| black_box(negotiate_capabilities(&medium_reqs, &empty_manifest)));
    });

    // Viability check on result
    let result = negotiate_capabilities(&medium_reqs, &manifest);
    group.bench_function("is_viable", |b| {
        b.iter(|| black_box(result.is_viable()));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// Group & main
// ═══════════════════════════════════════════════════════════════════════════

criterion_group!(
    benches,
    bench_receipt_hashing,
    bench_envelope_codec,
    bench_serde_roundtrip,
    bench_ir_normalization,
    bench_mapper_throughput,
    bench_policy_evaluation,
    bench_glob_matching,
    bench_capability_negotiation,
);

criterion_main!(benches);
