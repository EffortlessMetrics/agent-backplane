// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hot-path benchmarks: WorkOrder construction, receipt hashing, serde
//! roundtrips, glob matching, IR lowering (OpenAI→Gemini), and capability
//! negotiation.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::BTreeMap;

use abp_capability::{check_capability, negotiate};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, MinSupport, Outcome, Receipt, ReceiptBuilder, SupportLevel,
    WorkOrder, WorkOrderBuilder, canonical_json, receipt_hash,
};
use abp_gemini_sdk::lowering as gemini_lowering;
use abp_glob::IncludeExcludeGlobs;
use abp_openai_sdk::api::{ChatCompletionRequest, FunctionCall, Message, Tool, ToolCall, FunctionDefinition};
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_lowering;
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Build a realistic `ChatCompletionRequest` with `n` messages.
fn make_openai_request(n: usize) -> ChatCompletionRequest {
    let mut messages = Vec::with_capacity(n);
    messages.push(Message::System {
        content: "You are a helpful coding assistant. Follow instructions precisely.".into(),
    });
    for i in 1..n {
        if i % 3 == 1 {
            messages.push(Message::User {
                content: format!("User message {i} — please refactor the auth module."),
            });
        } else if i % 3 == 2 {
            messages.push(Message::Assistant {
                content: Some(format!("Sure, let me look at the code for step {i}.")),
                tool_calls: Some(vec![ToolCall {
                    id: format!("call_{i}"),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: format!("read_file_{i}"),
                        arguments: format!(r#"{{"path":"src/auth_{i}.rs"}}"#),
                    },
                }]),
            });
        } else {
            messages.push(Message::Tool {
                tool_call_id: format!("call_{}", i - 1),
                content: format!("fn handler_{i}() {{ /* code */ }}"),
            });
        }
    }
    ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages,
        temperature: Some(0.7),
        max_tokens: Some(4096),
        tools: Some(vec![Tool {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "read_file".into(),
                description: Some("Read a file from the workspace".into()),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "path": {"type": "string"} },
                    "required": ["path"]
                })),
                strict: None,
            },
        }]),
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    }
}

fn make_openai_messages(n: usize) -> Vec<OpenAIMessage> {
    (0..n)
        .map(|i| match i % 3 {
            0 => OpenAIMessage {
                role: "user".into(),
                content: Some(format!("User message {i} with realistic padding text.")),
                tool_calls: None,
                tool_call_id: None,
            },
            1 => OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: format!("call_{i}"),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: format!("tool_{i}"),
                        arguments: format!(r#"{{"path":"src/file_{i}.rs"}}"#),
                    },
                }]),
                tool_call_id: None,
            },
            _ => OpenAIMessage {
                role: "tool".into(),
                content: Some(format!("Result for tool call {}", i - 1)),
                tool_calls: None,
                tool_call_id: Some(format!("call_{}", i - 1)),
            },
        })
        .collect()
}

fn make_receipt(trace_len: usize) -> Receipt {
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
    builder.build()
}

fn sample_paths(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            if i % 4 == 0 {
                format!("src/module_{}/handler.rs", i / 4)
            } else if i % 4 == 1 {
                format!("tests/test_{}.rs", i)
            } else if i % 4 == 2 {
                format!("node_modules/pkg_{}/index.js", i)
            } else {
                format!("target/debug/build/dep_{}/out/gen.rs", i)
            }
        })
        .collect()
}

const ALL_CAPS: &[Capability] = &[
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
];

fn manifest_of(n: usize) -> CapabilityManifest {
    ALL_CAPS
        .iter()
        .take(n)
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect()
}

fn requirements_of(n: usize) -> CapabilityRequirements {
    CapabilityRequirements {
        required: ALL_CAPS
            .iter()
            .take(n)
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder construction from OpenAI request
// ═══════════════════════════════════════════════════════════════════════════

fn bench_work_order_from_openai(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping/work_order_from_openai");

    for msg_count in [10, 100, 1000] {
        let req = make_openai_request(msg_count);
        let req_json = serde_json::to_string(&req).unwrap();
        group.throughput(Throughput::Bytes(req_json.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("messages", msg_count),
            &req,
            |b, r| {
                b.iter(|| WorkOrder::from(black_box(r.clone())));
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Receipt hashing (canonical JSON → SHA-256)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_receipt_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping/receipt_hash");

    for &(label, size) in &[("small_5", 5), ("medium_50", 50), ("large_500", 500)] {
        let receipt = make_receipt(size);
        let json_len = serde_json::to_string(&receipt).unwrap().len();
        group.throughput(Throughput::Bytes(json_len as u64));

        group.bench_with_input(BenchmarkId::new("hash", label), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });

        group.bench_with_input(BenchmarkId::new("with_hash", label), &receipt, |b, r| {
            b.iter(|| black_box(r.clone()).with_hash().unwrap());
        });
    }

    // Canonical JSON on a nested structure
    let nested: BTreeMap<String, Vec<String>> = (0..100)
        .map(|i| (format!("key_{i:04}"), (0..20).map(|j| format!("v{j}")).collect()))
        .collect();
    let nested_len = serde_json::to_string(&nested).unwrap().len();
    group.throughput(Throughput::Bytes(nested_len as u64));
    group.bench_function("canonical_json_nested", |b| {
        b.iter(|| canonical_json(black_box(&nested)).unwrap());
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Receipt serialization roundtrip
// ═══════════════════════════════════════════════════════════════════════════

fn bench_receipt_serde_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping/receipt_serde_roundtrip");

    for &(label, size) in &[("tiny_2", 2), ("medium_50", 50), ("large_200", 200)] {
        let receipt = make_receipt(size);
        let json = serde_json::to_string(&receipt).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("roundtrip", label), &receipt, |b, r| {
            b.iter(|| {
                let s = serde_json::to_string(black_box(r)).unwrap();
                serde_json::from_str::<Receipt>(&s).unwrap()
            });
        });
    }

    // WorkOrder roundtrip for comparison
    let wo = WorkOrderBuilder::new("Refactor the authentication module")
        .root("/workspace/project")
        .model("gpt-4o")
        .max_turns(25)
        .build();
    let wo_json = serde_json::to_string(&wo).unwrap();
    group.throughput(Throughput::Bytes(wo_json.len() as u64));
    group.bench_function("work_order_roundtrip", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&wo)).unwrap();
            serde_json::from_str::<WorkOrder>(&s).unwrap()
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Glob pattern matching (IncludeExcludeGlobs.decide on 1000 paths)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_glob_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping/glob_decide");

    let include = vec!["src/**".into(), "tests/**".into(), "benches/**".into()];
    let exclude = vec![
        "node_modules/**".into(),
        "target/**".into(),
        ".git/**".into(),
    ];

    let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();

    // Baseline: 100 paths
    let paths_100 = sample_paths(100);
    group.throughput(Throughput::Elements(100));
    group.bench_with_input(
        BenchmarkId::new("paths", 100),
        &(&globs, &paths_100),
        |b, (g, ps)| {
            b.iter(|| {
                for p in *ps {
                    black_box(g.decide_str(black_box(p)));
                }
            });
        },
    );

    // Scaled: 1000 paths
    let paths_1000 = sample_paths(1000);
    group.throughput(Throughput::Elements(1000));
    group.bench_with_input(
        BenchmarkId::new("paths", 1000),
        &(&globs, &paths_1000),
        |b, (g, ps)| {
            b.iter(|| {
                for p in *ps {
                    black_box(g.decide_str(black_box(p)));
                }
            });
        },
    );

    // Many patterns (20 include + 20 exclude)
    let many_inc: Vec<String> = (0..20).map(|i| format!("src/mod_{i}/**/*.rs")).collect();
    let many_exc: Vec<String> = (0..20).map(|i| format!("vendor_{i}/**")).collect();
    let heavy_globs = IncludeExcludeGlobs::new(&many_inc, &many_exc).unwrap();
    group.throughput(Throughput::Elements(1000));
    group.bench_with_input(
        BenchmarkId::new("heavy_patterns_paths", 1000),
        &(&heavy_globs, &paths_1000),
        |b, (g, ps)| {
            b.iter(|| {
                for p in *ps {
                    black_box(g.decide_str(black_box(p)));
                }
            });
        },
    );

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. IR lowering (OpenAI IR → Gemini IR)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_ir_lowering(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping/ir_openai_to_gemini");

    for msg_count in [6, 30, 120] {
        let openai_msgs = make_openai_messages(msg_count);

        group.throughput(Throughput::Elements(msg_count as u64));

        // OpenAI → IR
        group.bench_with_input(
            BenchmarkId::new("openai_to_ir", msg_count),
            &openai_msgs,
            |b, m| {
                b.iter(|| openai_lowering::to_ir(black_box(m)));
            },
        );

        // IR → Gemini
        let ir = openai_lowering::to_ir(&openai_msgs);
        group.bench_with_input(BenchmarkId::new("ir_to_gemini", msg_count), &ir, |b, c| {
            b.iter(|| gemini_lowering::from_ir(black_box(c)));
        });

        // Full pipeline: OpenAI → IR → Gemini
        group.bench_with_input(
            BenchmarkId::new("openai_to_gemini_full", msg_count),
            &openai_msgs,
            |b, m| {
                b.iter(|| {
                    let ir = openai_lowering::to_ir(black_box(m));
                    gemini_lowering::from_ir(&ir)
                });
            },
        );

        // Gemini → IR (reverse direction baseline)
        let gemini_contents = gemini_lowering::from_ir(&ir);
        group.bench_with_input(
            BenchmarkId::new("gemini_to_ir", msg_count),
            &gemini_contents,
            |b, gc| {
                b.iter(|| gemini_lowering::to_ir(black_box(gc), None));
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Capability negotiation (query 100 capabilities)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_capability_negotiation(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping/capability_negotiation");

    // Baseline: small manifest (5 caps)
    let small_manifest = manifest_of(5);
    let small_reqs = requirements_of(5);
    group.throughput(Throughput::Elements(5));
    group.bench_function("negotiate_5", |b| {
        b.iter(|| negotiate(black_box(&small_manifest), black_box(&small_reqs)));
    });

    // Scaled: full manifest (all caps)
    let full_manifest = manifest_of(ALL_CAPS.len());
    let full_reqs = requirements_of(ALL_CAPS.len());
    group.throughput(Throughput::Elements(ALL_CAPS.len() as u64));
    group.bench_function("negotiate_all", |b| {
        b.iter(|| negotiate(black_box(&full_manifest), black_box(&full_reqs)));
    });

    // 100 individual capability checks
    let manifest_full = manifest_of(ALL_CAPS.len());
    let empty_manifest: CapabilityManifest = BTreeMap::new();
    group.throughput(Throughput::Elements(100));
    group.bench_function("check_100_hit", |b| {
        b.iter(|| {
            for _ in 0..100 {
                black_box(check_capability(
                    black_box(&manifest_full),
                    black_box(&Capability::Streaming),
                ));
            }
        });
    });
    group.bench_function("check_100_miss", |b| {
        b.iter(|| {
            for _ in 0..100 {
                black_box(check_capability(
                    black_box(&empty_manifest),
                    black_box(&Capability::Streaming),
                ));
            }
        });
    });

    // Mixed: negotiate with partial overlap
    let partial_manifest = manifest_of(10);
    let over_reqs = requirements_of(20);
    group.throughput(Throughput::Elements(20));
    group.bench_function("negotiate_partial_overlap", |b| {
        b.iter(|| negotiate(black_box(&partial_manifest), black_box(&over_reqs)));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// Criterion groups & main
// ═══════════════════════════════════════════════════════════════════════════

criterion_group!(
    benches,
    bench_work_order_from_openai,
    bench_receipt_hashing,
    bench_receipt_serde_roundtrip,
    bench_glob_matching,
    bench_ir_lowering,
    bench_capability_negotiation,
);
criterion_main!(benches);
