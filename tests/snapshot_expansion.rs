// SPDX-License-Identifier: MIT OR Apache-2.0
//! Expanded snapshot test suite covering additional contract types and edge cases.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_capability::{generate_report, negotiate};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, SupportLevel, UsageNormalized, VerificationReport,
};
use abp_emulation::{EmulationConfig, EmulationEngine, EmulationStrategy};
use abp_error::{AbpError, AbpErrorDto, ErrorCode};
use abp_mapping::{MappingMatrix, known_rules, validate_mapping};
use abp_policy::PolicyEngine;
use abp_receipt::{ReceiptBuilder, ReceiptChain};

use abp_dialect::Dialect;

// ── Helpers ──────────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 5, 0).unwrap()
}

fn fixed_ts3() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 10, 0).unwrap()
}

fn fixed_uuid(n: u8) -> Uuid {
    Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, n])
}

fn sample_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.3.0".into()),
    }
}

fn sample_capabilities() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (Capability::ExtendedThinking, SupportLevel::Emulated),
    ])
}

fn event_at(ts: chrono::DateTime<Utc>, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. SDK shim request→IR→back roundtrips (all 6 dialects)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn shim_openai_request_to_ir_roundtrip() {
    use abp_shim_openai::{ChatCompletionRequest, Message};

    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You are helpful."),
            Message::user("Explain Rust traits."),
        ])
        .build();

    let ir = abp_shim_openai::request_to_ir(&req);
    let back = abp_shim_openai::ir_to_messages(&ir);

    insta::assert_json_snapshot!(json!({
        "ir": ir,
        "roundtrip_messages": back,
    }));
}

#[test]
fn shim_claude_request_to_ir_roundtrip() {
    use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role};

    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "What is Rust?".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Rust is a systems programming language.".into(),
                }],
            },
        ],
        system: Some("Be concise.".into()),
        temperature: Some(0.7),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let wo = abp_shim_claude::request_to_work_order(&req);
    insta::assert_json_snapshot!(json!({
        "task": wo.task,
        "model": wo.config.model,
    }));
}

#[test]
fn shim_gemini_request_to_dialect_roundtrip() {
    use abp_shim_gemini::{Content, GenerateContentRequest, Part};

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello Gemini")]))
        .add_content(Content::model(vec![Part::text("Hello!")]));

    let dialect_req = abp_shim_gemini::to_dialect_request(&req);
    insta::assert_json_snapshot!(dialect_req);
}

#[test]
fn shim_codex_request_to_ir_roundtrip() {
    use abp_shim_codex::{CodexRequestBuilder, codex_message};

    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![
            codex_message("user", "Fix the bug in main.rs"),
            codex_message("assistant", "I'll look at the code."),
        ])
        .build();

    let ir = abp_shim_codex::request_to_ir(&req);
    let wo = abp_shim_codex::request_to_work_order(&req);

    insta::assert_json_snapshot!(json!({
        "ir_message_count": ir.len(),
        "ir": ir,
        "work_order_task": wo.task,
    }));
}

#[test]
fn shim_kimi_request_to_ir_roundtrip() {
    use abp_shim_kimi::{KimiRequestBuilder, Message};

    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![
            Message::system("You are Kimi, a helpful assistant."),
            Message::user("Summarize this document."),
        ])
        .temperature(0.3)
        .build();

    let ir = abp_shim_kimi::request_to_ir(&req);
    let wo = abp_shim_kimi::request_to_work_order(&req);

    insta::assert_json_snapshot!(json!({
        "ir": ir,
        "work_order_task": wo.task,
        "work_order_model": wo.config.model,
    }));
}

#[test]
fn shim_copilot_request_to_ir_roundtrip() {
    use abp_shim_copilot::{CopilotRequestBuilder, Message};

    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You are a coding assistant."),
            Message::user("Write a hello world in Python"),
        ])
        .build();

    let ir = abp_shim_copilot::request_to_ir(&req);
    let back = abp_shim_copilot::ir_to_messages(&ir);

    insta::assert_json_snapshot!(json!({
        "ir": ir,
        "roundtrip_messages": back,
    }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Complex tool definitions with nested parameters
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complex_tool_definition_nested_params() {
    let tool = IrToolDefinition {
        name: "create_file".into(),
        description: "Create a file with nested configuration".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "content": {"type": "string", "description": "File content"},
                "options": {
                    "type": "object",
                    "properties": {
                        "encoding": {"type": "string", "enum": ["utf-8", "ascii", "latin-1"]},
                        "permissions": {
                            "type": "object",
                            "properties": {
                                "owner": {"type": "string", "enum": ["read", "write", "execute"]},
                                "group": {"type": "string", "enum": ["read", "write"]},
                                "other": {"type": "string", "enum": ["read", "none"]}
                            },
                            "required": ["owner"]
                        },
                        "overwrite": {"type": "boolean", "default": false}
                    },
                    "required": ["encoding"]
                },
                "tags": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "key": {"type": "string"},
                            "value": {"type": "string"}
                        },
                        "required": ["key", "value"]
                    }
                }
            },
            "required": ["path", "content", "options"]
        }),
    };

    insta::assert_json_snapshot!(tool);
}

#[test]
fn tool_definition_array_with_multiple_tools() {
    let tools = vec![
        IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        },
        IrToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]}),
        },
        IrToolDefinition {
            name: "execute_command".into(),
            description: "Run a shell command with arguments".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "env": {"type": "object", "additionalProperties": {"type": "string"}},
                    "cwd": {"type": "string"}
                },
                "required": ["command"]
            }),
        },
    ];

    insta::assert_json_snapshot!(tools);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Multi-turn conversation IR representation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_conversation_ir_five_turns() {
    let conv = IrConversation::from_messages(vec![
        IrMessage {
            role: IrRole::System,
            content: vec![IrContentBlock::Text {
                text: "You are a coding assistant. Always explain your reasoning.".into(),
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "How do I implement a binary search?".into(),
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentBlock::Text {
                text: "Binary search works by repeatedly dividing the search interval in half."
                    .into(),
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Can you show me the Rust code?".into(),
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::Assistant,
            content: vec![
                IrContentBlock::Text {
                    text: "Here's a binary search implementation:".into(),
                },
                IrContentBlock::Text {
                    text: "fn binary_search(arr: &[i32], target: i32) -> Option<usize> { todo!() }"
                        .into(),
                },
            ],
            metadata: BTreeMap::new(),
        },
    ]);

    insta::assert_json_snapshot!(conv);
}

#[test]
fn multi_turn_with_tool_use_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Read the main.rs file".into(),
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentBlock::ToolUse {
                id: "call_001".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::ToolResult {
                tool_use_id: "call_001".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() { println!(\"Hello\"); }".into(),
                }],
                is_error: false,
            }],
            metadata: BTreeMap::new(),
        },
        IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentBlock::Text {
                text: "The main.rs file contains a simple Hello World program.".into(),
            }],
            metadata: BTreeMap::new(),
        },
    ]);

    insta::assert_json_snapshot!(conv);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Large receipt with many events in trace
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_receipt_many_trace_events() {
    let mut trace = Vec::new();

    trace.push(event_at(
        fixed_ts(),
        AgentEventKind::RunStarted {
            message: "Starting large task".into(),
        },
    ));

    // 5 tool call + result pairs
    for i in 0..5 {
        trace.push(event_at(
            fixed_ts(),
            AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("call_{i:03}")),
                parent_tool_use_id: None,
                input: json!({"index": i}),
            },
        ));
        trace.push(event_at(
            fixed_ts(),
            AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("call_{i:03}")),
                output: json!(format!("result_{i}")),
                is_error: false,
            },
        ));
    }

    // Streaming deltas
    for chunk in [
        "The ",
        "analysis ",
        "is ",
        "complete. ",
        "Here ",
        "are ",
        "the ",
        "results.",
    ] {
        trace.push(event_at(
            fixed_ts(),
            AgentEventKind::AssistantDelta { text: chunk.into() },
        ));
    }

    trace.push(event_at(
        fixed_ts(),
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "modified file".into(),
        },
    ));

    trace.push(event_at(
        fixed_ts(),
        AgentEventKind::Warning {
            message: "Token budget at 85%".into(),
        },
    ));

    trace.push(event_at(
        fixed_ts(),
        AgentEventKind::RunCompleted {
            message: "Task finished successfully".into(),
        },
    ));

    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(1),
            work_order_id: fixed_uuid(2),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 45_000,
        },
        backend: sample_backend(),
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({
            "input_tokens": 8500,
            "output_tokens": 3200,
            "cache_read_input_tokens": 1000,
        }),
        usage: UsageNormalized {
            input_tokens: Some(8500),
            output_tokens: Some(3200),
            cache_read_tokens: Some(1000),
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.095),
        },
        trace,
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "output.patch".into(),
            },
            ArtifactRef {
                kind: "file".into(),
                path: "src/main.rs".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("+added line\n-removed line".into()),
            git_status: Some("M src/main.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let val = serde_json::to_value(&receipt).unwrap();
    insta::assert_json_snapshot!(val);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Receipt chain serialization format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_chain_three_entries_serialized() {
    let r1 = ReceiptBuilder::new("backend-a")
        .run_id(fixed_uuid(1))
        .work_order_id(fixed_uuid(10))
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("backend-b")
        .run_id(fixed_uuid(2))
        .work_order_id(fixed_uuid(11))
        .started_at(fixed_ts2())
        .finished_at(fixed_ts3())
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();

    let r3 = ReceiptBuilder::new("backend-c")
        .run_id(fixed_uuid(3))
        .work_order_id(fixed_uuid(12))
        .started_at(fixed_ts3())
        .finished_at(Utc.with_ymd_and_hms(2025, 1, 15, 12, 15, 0).unwrap())
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    chain.push(r3).unwrap();

    let serialized: Vec<serde_json::Value> = chain
        .iter()
        .map(|r| {
            json!({
                "run_id": r.meta.run_id,
                "outcome": r.outcome,
                "receipt_sha256": r.receipt_sha256,
                "backend_id": r.backend.id,
            })
        })
        .collect();

    insta::assert_json_snapshot!(json!({
        "chain_length": chain.len(),
        "entries": serialized,
    }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. PolicyProfile compilation results
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_full_compilation() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into(), "Delete".into()],
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.internal".into(), "localhost".into()],
        require_approval_for: vec!["Delete".into(), "Bash".into()],
    };

    let engine = PolicyEngine::new(&policy).unwrap();

    let results = json!({
        "tool_read_allowed": engine.can_use_tool("Read"),
        "tool_write_allowed": engine.can_use_tool("Write"),
        "tool_grep_allowed": engine.can_use_tool("Grep"),
        "tool_bash_denied": engine.can_use_tool("Bash"),
        "tool_delete_denied": engine.can_use_tool("Delete"),
        "tool_unknown_denied": engine.can_use_tool("SomeUnknown"),
        "read_src_allowed": engine.can_read_path(std::path::Path::new("src/main.rs")),
        "read_env_denied": engine.can_read_path(std::path::Path::new(".env")),
        "read_secrets_denied": engine.can_read_path(std::path::Path::new("config/secrets/key.pem")),
        "write_src_allowed": engine.can_write_path(std::path::Path::new("src/main.rs")),
        "write_git_denied": engine.can_write_path(std::path::Path::new(".git/config")),
        "write_node_modules_denied": engine.can_write_path(std::path::Path::new("node_modules/pkg/index.js")),
    });

    insta::assert_json_snapshot!(results);
}

#[test]
fn policy_empty_allowlist_denies_all_tools() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec![],
        ..Default::default()
    };

    let engine = PolicyEngine::new(&policy).unwrap();

    let results = json!({
        "read_denied": engine.can_use_tool("Read"),
        "write_denied": engine.can_use_tool("Write"),
        "bash_denied": engine.can_use_tool("Bash"),
    });

    insta::assert_json_snapshot!(results);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. CapabilityReport for each backend type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_report_full_native() {
    let manifest: CapabilityManifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
    ]);

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Native,
            },
        ],
    };

    let result = negotiate(&manifest, &reqs);
    let report = generate_report(&result);

    insta::assert_json_snapshot!(report);
}

#[test]
fn capability_report_mixed_emulation() {
    let manifest: CapabilityManifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Emulated),
        (Capability::ExtendedThinking, SupportLevel::Unsupported),
    ]);

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Native,
            },
        ],
    };

    let result = negotiate(&manifest, &reqs);
    let report = generate_report(&result);

    insta::assert_json_snapshot!(report);
}

#[test]
fn capability_report_completely_unsupported() {
    let manifest: CapabilityManifest = BTreeMap::new();

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Native,
            },
        ],
    };

    let result = negotiate(&manifest, &reqs);
    let report = generate_report(&result);

    insta::assert_json_snapshot!(report);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. MappingMatrix rule coverage report
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_matrix_from_known_rules() {
    let registry = known_rules();
    let matrix = MappingMatrix::from_registry(&registry);

    let mut coverage = BTreeMap::new();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let key = format!("{src:?}->{tgt:?}");
            coverage.insert(key, matrix.is_supported(src, tgt));
        }
    }

    insta::assert_json_snapshot!(coverage);
}

#[test]
fn mapping_validation_openai_to_claude_features() {
    let registry = known_rules();
    let results = validate_mapping(
        &registry,
        Dialect::OpenAi,
        Dialect::Claude,
        &[
            "tool_use".into(),
            "streaming".into(),
            "thinking".into(),
            "image_input".into(),
            "code_exec".into(),
        ],
    );

    let summary: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            json!({
                "feature": r.feature,
                "fidelity": r.fidelity,
                "error_count": r.errors.len(),
            })
        })
        .collect();

    insta::assert_json_snapshot!(summary);
}

#[test]
fn mapping_registry_rank_targets() {
    let registry = known_rules();
    let ranked = registry.rank_targets(
        Dialect::OpenAi,
        &["tool_use", "streaming", "thinking", "image_input"],
    );

    let snapshot: Vec<serde_json::Value> = ranked
        .iter()
        .map(|(d, count)| {
            json!({
                "dialect": format!("{d:?}"),
                "lossless_count": count,
            })
        })
        .collect();

    insta::assert_json_snapshot!(snapshot);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. EmulationPlan for each strategy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn emulation_engine_system_prompt_strategy() {
    let engine = EmulationEngine::with_defaults();

    let report = engine.check_missing(&[Capability::ExtendedThinking]);

    insta::assert_json_snapshot!(report);
}

#[test]
fn emulation_engine_post_processing_strategy() {
    let engine = EmulationEngine::with_defaults();

    let report = engine.check_missing(&[Capability::StructuredOutputJsonSchema]);

    insta::assert_json_snapshot!(report);
}

#[test]
fn emulation_engine_disabled_strategy() {
    let engine = EmulationEngine::with_defaults();

    let report = engine.check_missing(&[Capability::CodeExecution]);

    insta::assert_json_snapshot!(report);
}

#[test]
fn emulation_engine_mixed_capabilities() {
    let engine = EmulationEngine::with_defaults();

    let report = engine.check_missing(&[
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::ImageInput,
        Capability::StopSequences,
        Capability::Logprobs,
    ]);

    insta::assert_json_snapshot!(report);
}

#[test]
fn emulation_config_with_custom_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code execution by reasoning step-by-step.".into(),
        },
    );
    config.set(
        Capability::Logprobs,
        EmulationStrategy::Disabled {
            reason: "Logprobs cannot be emulated.".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::ExtendedThinking,
    ]);

    insta::assert_json_snapshot!(report);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Error code with full context
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_dto_backend_timeout_full_context() {
    let err = AbpError::new(
        ErrorCode::BackendTimeout,
        "Backend did not respond within 30s",
    )
    .with_context("backend_id", "sidecar:claude")
    .with_context("timeout_ms", 30_000)
    .with_context("attempt", 3)
    .with_context("work_order_id", "wo-abc-123");

    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn error_dto_policy_denied_with_path() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "Write access denied by policy")
        .with_context("path", ".git/config")
        .with_context("policy_rule", "deny_write: **/.git/**")
        .with_context("tool", "write_file");

    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn error_dto_dialect_mapping_failed() {
    let err = AbpError::new(
        ErrorCode::DialectMappingFailed,
        "Cannot map extended_thinking from Claude to OpenAI",
    )
    .with_context("source_dialect", "claude")
    .with_context("target_dialect", "openai")
    .with_context("feature", "extended_thinking")
    .with_context(
        "fidelity",
        json!({"type": "unsupported", "reason": "OpenAI has no thinking API"}),
    );

    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn error_dto_receipt_chain_broken() {
    let err = AbpError::new(
        ErrorCode::ReceiptChainBroken,
        "Receipt chain has a gap at position 2",
    )
    .with_context("chain_length", 5)
    .with_context("gap_position", 2)
    .with_context("expected_parent_hash", "sha256:abc123...")
    .with_context("actual_parent_hash", serde_json::Value::Null);

    let dto: AbpErrorDto = (&err).into();
    insta::assert_json_snapshot!(dto);
}

#[test]
fn error_all_categories_snapshot() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::PolicyDenied,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::DialectUnknown,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];

    let snapshot: Vec<serde_json::Value> = codes
        .iter()
        .map(|c| {
            json!({
                "code": c.as_str(),
                "category": c.category().to_string(),
            })
        })
        .collect();

    insta::assert_json_snapshot!(snapshot);
}
