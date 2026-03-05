// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive benchmark suite covering the critical hot paths of the
//! agent-backplane stack: receipt hashing, serde roundtrips, policy
//! evaluation, capability negotiation, IR conversion, stream processing,
//! and protocol parsing.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;

use abp_capability::{check_capability, negotiate};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, MinSupport, Outcome, PolicyProfile,
    ReceiptBuilder, SupportLevel, WorkOrderBuilder, receipt_hash,
};
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering;
use abp_policy::PolicyEngine;
use abp_protocol::stream::StreamParser;
use abp_protocol::{Envelope, JsonlCodec};
use abp_stream::{EventFilter, EventRecorder, EventStats, EventTransform, StreamPipelineBuilder};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Receipt hashing — varying payload sizes
// ═══════════════════════════════════════════════════════════════════════════

fn make_receipt(trace_len: usize) -> abp_core::Receipt {
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

fn bench_receipt_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/receipt_hash");

    for &(label, size) in &[("small", 5), ("medium", 50), ("large", 500)] {
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

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Serde roundtrip — WorkOrder, Receipt, AgentEvent (JSON)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_serde_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/serde_roundtrip");

    // WorkOrder
    let wo = WorkOrderBuilder::new("Refactor the authentication module for improved security")
        .root("/tmp/bench-workspace")
        .model("gpt-4")
        .max_turns(20)
        .build();
    let wo_json = serde_json::to_string(&wo).unwrap();
    group.throughput(Throughput::Bytes(wo_json.len() as u64));
    group.bench_function("work_order", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&wo)).unwrap();
            serde_json::from_str::<abp_core::WorkOrder>(&s).unwrap()
        });
    });

    // Receipt (20 events)
    let receipt = make_receipt(20);
    let receipt_json = serde_json::to_string(&receipt).unwrap();
    group.throughput(Throughput::Bytes(receipt_json.len() as u64));
    group.bench_function("receipt_20ev", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&receipt)).unwrap();
            serde_json::from_str::<abp_core::Receipt>(&s).unwrap()
        });
    });

    // AgentEvent — ToolCall variant
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-001".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    let ev_json = serde_json::to_string(&event).unwrap();
    group.throughput(Throughput::Bytes(ev_json.len() as u64));
    group.bench_function("agent_event", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&event)).unwrap();
            serde_json::from_str::<AgentEvent>(&s).unwrap()
        });
    });

    // AgentEvent — AssistantDelta variant
    let delta = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "Here is a reasonably sized token output.".into(),
        },
        ext: None,
    };
    group.bench_function("agent_event_delta", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&delta)).unwrap();
            serde_json::from_str::<AgentEvent>(&s).unwrap()
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Policy evaluation — check_tool / check_read / check_write
// ═══════════════════════════════════════════════════════════════════════════

fn make_policy(n_tools: usize, n_paths: usize) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: (0..n_tools).map(|i| format!("Denied{i}*")).collect(),
        deny_read: (0..n_paths).map(|i| format!("secret{i}/**")).collect(),
        deny_write: (0..n_paths).map(|i| format!("locked{i}/**")).collect(),
        ..PolicyProfile::default()
    }
}

fn bench_policy_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/policy_eval");

    for rule_count in [1, 10, 100] {
        let policy = make_policy(rule_count, rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Tool allowed
        group.bench_with_input(
            BenchmarkId::new("tool_allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("ReadFile")));
            },
        );

        // Tool denied
        group.bench_with_input(
            BenchmarkId::new("tool_denied", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("Denied0Match")));
            },
        );

        // Write allowed
        group.bench_with_input(
            BenchmarkId::new("write_allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("src/lib.rs"))));
            },
        );

        // Write denied
        group.bench_with_input(
            BenchmarkId::new("write_denied", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("locked0/data.txt"))));
            },
        );

        // Read denied
        group.bench_with_input(
            BenchmarkId::new("read_denied", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_read_path(black_box(Path::new("secret0/keys.pem"))));
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability negotiation — negotiate() with various manifest sizes
// ═══════════════════════════════════════════════════════════════════════════

const CAPS: &[Capability] = &[
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
    CAPS.iter()
        .take(n)
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect()
}

fn requirements_of(n: usize) -> CapabilityRequirements {
    CapabilityRequirements {
        required: CAPS
            .iter()
            .take(n)
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

fn bench_capability_negotiation(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/capability_negotiate");

    for count in [1, 10, CAPS.len()] {
        let manifest = manifest_of(count);
        let reqs = requirements_of(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("negotiate", count),
            &(&manifest, &reqs),
            |b, (m, r)| {
                b.iter(|| negotiate(black_box(m), black_box(r)));
            },
        );
    }

    // Single check_capability lookup
    let full = manifest_of(CAPS.len());
    let empty: CapabilityManifest = BTreeMap::new();
    group.bench_function("check_hit", |b| {
        b.iter(|| check_capability(black_box(&full), black_box(&Capability::Streaming)));
    });
    group.bench_function("check_miss", |b| {
        b.iter(|| check_capability(black_box(&empty), black_box(&Capability::Streaming)));
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. IR conversion — OpenAI lowering roundtrip
// ═══════════════════════════════════════════════════════════════════════════

fn make_openai_messages(n: usize) -> Vec<OpenAIMessage> {
    let mut msgs = Vec::with_capacity(n);
    for i in 0..n {
        if i % 3 == 0 {
            msgs.push(OpenAIMessage {
                role: "user".into(),
                content: Some(format!("User message {i} with some realistic padding.")),
                tool_calls: None,
                tool_call_id: None,
            });
        } else if i % 3 == 1 {
            msgs.push(OpenAIMessage {
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
            });
        } else {
            msgs.push(OpenAIMessage {
                role: "tool".into(),
                content: Some(format!("Result for tool call {}", i - 1)),
                tool_calls: None,
                tool_call_id: Some(format!("call_{}", i - 1)),
            });
        }
    }
    msgs
}

fn bench_ir_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/ir_conversion");

    for count in [3, 15, 60] {
        let msgs = make_openai_messages(count);

        group.throughput(Throughput::Elements(count as u64));

        // to_ir
        group.bench_with_input(BenchmarkId::new("to_ir", count), &msgs, |b, m| {
            b.iter(|| lowering::to_ir(black_box(m)));
        });

        // from_ir
        let conv = lowering::to_ir(&msgs);
        group.bench_with_input(BenchmarkId::new("from_ir", count), &conv, |b, c| {
            b.iter(|| lowering::from_ir(black_box(c)));
        });

        // roundtrip
        group.bench_with_input(BenchmarkId::new("roundtrip", count), &msgs, |b, m| {
            b.iter(|| {
                let ir = lowering::to_ir(black_box(m));
                lowering::from_ir(&ir)
            });
        });
    }

    // IR JSON serde roundtrip
    let conv = {
        let msgs: Vec<IrMessage> = (0..20)
            .map(|i| {
                IrMessage::new(
                    if i % 2 == 0 {
                        IrRole::User
                    } else {
                        IrRole::Assistant
                    },
                    vec![IrContentBlock::Text {
                        text: format!("Message {i} padding."),
                    }],
                )
            })
            .collect();
        IrConversation::from_messages(msgs)
    };
    group.bench_function("ir_json_roundtrip_20msg", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&conv)).unwrap();
            serde_json::from_str::<IrConversation>(&json).unwrap()
        });
    });

    // IrToolDefinition serde
    let tools: Vec<IrToolDefinition> = (0..20)
        .map(|i| IrToolDefinition {
            name: format!("tool_{i}"),
            description: format!("Benchmark tool {i}"),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": {"type": "string"} },
                "required": ["path"]
            }),
        })
        .collect();
    group.bench_function("ir_tool_defs_roundtrip_20", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&tools)).unwrap();
            serde_json::from_str::<Vec<IrToolDefinition>>(&json).unwrap()
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Stream processing — EventFilter, EventTransform, StreamPipeline
// ═══════════════════════════════════════════════════════════════════════════

fn make_events(n: usize) -> Vec<AgentEvent> {
    (0..n)
        .map(|i| AgentEvent {
            ts: Utc::now(),
            kind: if i % 5 == 0 {
                AgentEventKind::Error {
                    message: format!("err-{i}"),
                    error_code: None,
                }
            } else {
                AgentEventKind::AssistantDelta {
                    text: format!("tok-{i}"),
                }
            },
            ext: None,
        })
        .collect()
}

fn bench_stream_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/stream_processing");

    let events = make_events(200);

    // EventFilter — exclude errors
    let filter = EventFilter::exclude_errors();
    group.throughput(Throughput::Elements(events.len() as u64));
    group.bench_function("filter_exclude_errors_200", |b| {
        b.iter(|| {
            black_box(&events)
                .iter()
                .filter(|e| filter.matches(e))
                .count()
        });
    });

    // EventFilter — by_kind
    let kind_filter = EventFilter::by_kind("assistant_delta");
    group.bench_function("filter_by_kind_200", |b| {
        b.iter(|| {
            black_box(&events)
                .iter()
                .filter(|e| kind_filter.matches(e))
                .count()
        });
    });

    // EventTransform — identity
    let transform = EventTransform::identity();
    group.bench_function("transform_identity_200", |b| {
        b.iter(|| {
            for ev in black_box(&events) {
                black_box(transform.apply(ev.clone()));
            }
        });
    });

    // EventStats — observe
    group.bench_function("stats_observe_200", |b| {
        b.iter(|| {
            let stats = EventStats::new();
            for ev in black_box(&events) {
                stats.observe(ev);
            }
            stats.total_events()
        });
    });

    // EventRecorder — record
    group.bench_function("recorder_200", |b| {
        b.iter(|| {
            let recorder = EventRecorder::new();
            for ev in black_box(&events) {
                recorder.record(ev);
            }
            recorder.len()
        });
    });

    // StreamPipeline — full pipeline (filter + transform + stats + record)
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::identity())
        .with_stats(EventStats::new())
        .record()
        .build();

    group.bench_function("pipeline_full_200", |b| {
        b.iter(|| {
            for ev in black_box(&events) {
                black_box(pipeline.process(ev.clone()));
            }
        });
    });

    // Pipeline — filter only
    let pipeline_filter = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    group.bench_function("pipeline_filter_only_200", |b| {
        b.iter(|| {
            for ev in black_box(&events) {
                black_box(pipeline_filter.process(ev.clone()));
            }
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Protocol parsing — JSONL stream parser
// ═══════════════════════════════════════════════════════════════════════════

fn build_jsonl_stream(event_count: usize) -> Vec<u8> {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let wo = WorkOrderBuilder::new("bench task")
        .root("/tmp/bench")
        .model("gpt-4")
        .build();
    let run = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let fin = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: ReceiptBuilder::new("bench")
            .outcome(Outcome::Complete)
            .build(),
    };

    let mut lines = Vec::new();
    lines.push(JsonlCodec::encode(&hello).unwrap());
    lines.push(JsonlCodec::encode(&run).unwrap());
    for i in 0..event_count {
        let ev = Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("tok-{i}"),
                },
                ext: None,
            },
        };
        lines.push(JsonlCodec::encode(&ev).unwrap());
    }
    lines.push(JsonlCodec::encode(&fin).unwrap());
    lines.concat().into_bytes()
}

fn bench_protocol_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("core/protocol_parse");

    for event_count in [10, 100, 500] {
        let data = build_jsonl_stream(event_count);
        let total = event_count + 3; // hello + run + events + final
        group.throughput(Throughput::Bytes(data.len() as u64));

        // StreamParser — whole buffer
        group.bench_with_input(
            BenchmarkId::new("stream_parser_whole", event_count),
            &data,
            |b, d| {
                b.iter(|| {
                    let mut parser = StreamParser::new();
                    let results = parser.push(black_box(d));
                    assert_eq!(results.len(), total);
                });
            },
        );

        // StreamParser — chunked (simulating async I/O)
        group.bench_with_input(
            BenchmarkId::new("stream_parser_chunked_256", event_count),
            &data,
            |b, d| {
                b.iter(|| {
                    let mut parser = StreamParser::new();
                    let mut count = 0;
                    for chunk in d.chunks(256) {
                        count += parser.push(black_box(chunk)).len();
                    }
                    count += parser.finish().len();
                    assert_eq!(count, total);
                });
            },
        );

        // BufReader decode_stream
        group.bench_with_input(
            BenchmarkId::new("decode_stream", event_count),
            &data,
            |b, d| {
                b.iter(|| {
                    let reader = BufReader::new(black_box(d.as_slice()));
                    JsonlCodec::decode_stream(reader)
                        .filter(|r| r.is_ok())
                        .count()
                });
            },
        );
    }

    // Single envelope encode/decode
    let hello = Envelope::hello(
        BackendIdentity {
            id: "bench".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let trimmed = encoded.trim().to_string();

    group.bench_function("envelope_encode_hello", |b| {
        b.iter(|| JsonlCodec::encode(black_box(&hello)).unwrap());
    });
    group.bench_function("envelope_decode_hello", |b| {
        b.iter(|| JsonlCodec::decode(black_box(&trimmed)).unwrap());
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// Criterion groups & main
// ═══════════════════════════════════════════════════════════════════════════

criterion_group!(
    benches,
    bench_receipt_hashing,
    bench_serde_roundtrip,
    bench_policy_evaluation,
    bench_capability_negotiation,
    bench_ir_conversion,
    bench_stream_processing,
    bench_protocol_parsing,
);
criterion_main!(benches);
