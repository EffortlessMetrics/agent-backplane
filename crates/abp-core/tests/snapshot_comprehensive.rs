// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive snapshot tests for abp-core serialization formats.

use abp_core::validate::ValidationError;
use abp_core::*;
use chrono::{TimeZone, Utc};
use insta::{assert_json_snapshot, assert_snapshot};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 6, 15, 12, 35, 0).unwrap()
}

// ── 1. Full WorkOrder with all fields populated ─────────────────────────

#[test]
fn snapshot_full_work_order_all_fields() {
    let wo = WorkOrder {
        id: Uuid::nil(),
        task: "Implement OAuth2 flow with PKCE".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/dev/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**/*.rs".into(), "tests/**".into()],
            exclude: vec!["target/**".into(), "*.log".into(), ".git/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/auth.rs".into(), "Cargo.toml".into()],
            snippets: vec![
                ContextSnippet {
                    name: "auth-spec".into(),
                    content: "Must support refresh tokens and PKCE challenge".into(),
                },
                ContextSnippet {
                    name: "api-docs".into(),
                    content: "See https://example.com/oauth2/spec".into(),
                },
            ],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into(), "glob".into(), "grep".into()],
            disallowed_tools: vec!["bash".into(), "web_fetch".into()],
            deny_read: vec![".env".into(), "**/*.key".into(), "secrets/**".into()],
            deny_write: vec!["Cargo.lock".into(), "LICENSE".into()],
            allow_network: vec!["api.example.com".into(), "*.github.com".into()],
            deny_network: vec!["evil.com".into(), "*.malware.net".into()],
            require_approval_for: vec!["write".into(), "edit".into(), "bash".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
                CapabilityRequirement {
                    capability: Capability::ToolWrite,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("claude-sonnet-4-20250514".into()),
            vendor: BTreeMap::from([
                (
                    "abp".into(),
                    json!({"mode": "mapped", "trace_level": "verbose"}),
                ),
                ("anthropic".into(), json!({"max_tokens": 8192})),
            ]),
            env: BTreeMap::from([
                ("RUST_LOG".into(), "debug".into()),
                ("CI".into(), "true".into()),
            ]),
            max_budget_usd: Some(5.0),
            max_turns: Some(25),
        },
    };
    assert_json_snapshot!("comprehensive_full_work_order", wo);
}

// ── 2. Full Receipt with hash ───────────────────────────────────────────

#[test]
fn snapshot_full_receipt_with_hash() {
    let ts_start = fixed_ts();
    let ts_end = fixed_ts2();
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::from_u128(42),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts_start,
            finished_at: ts_end,
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: Some("2024.6.1".into()),
            adapter_version: Some("0.1.3".into()),
        },
        capabilities: BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Emulated),
            (Capability::ToolBash, SupportLevel::Unsupported),
            (
                Capability::McpClient,
                SupportLevel::Restricted {
                    reason: "sandbox policy".into(),
                },
            ),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({
            "input_tokens": 1500,
            "output_tokens": 3200,
            "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 800
        }),
        usage: UsageNormalized {
            input_tokens: Some(1500),
            output_tokens: Some(3200),
            cache_read_tokens: Some(800),
            cache_write_tokens: Some(200),
            request_units: None,
            estimated_cost_usd: Some(0.042),
        },
        trace: vec![
            AgentEvent {
                ts: ts_start,
                kind: AgentEventKind::RunStarted {
                    message: "starting OAuth2 implementation".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: ts_end,
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "output.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("+pub fn authorize() -> Result<Token> {\n+    // PKCE flow\n+}".into()),
            git_status: Some("M src/auth.rs\nA src/oauth2.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
    .with_hash()
    .unwrap();

    let value = serde_json::to_value(&receipt).unwrap();
    assert_json_snapshot!("comprehensive_full_receipt_hashed", value, {
        ".receipt_sha256" => "[sha256]"
    });
}

// ── 3. Each AgentEventKind variant ──────────────────────────────────────

#[test]
fn snapshot_event_kind_run_started() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "initializing agent".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_run_started", event);
}

#[test]
fn snapshot_event_kind_run_completed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunCompleted {
            message: "all tasks done".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_run_completed", event);
}

#[test]
fn snapshot_event_kind_assistant_delta() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Here's how to ".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_assistant_delta", event);
}

#[test]
fn snapshot_event_kind_assistant_message() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "I've completed the refactoring. The changes include...".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_assistant_message", event);
}

#[test]
fn snapshot_event_kind_tool_call() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "write".into(),
            tool_use_id: Some("toolu_abc123".into()),
            parent_tool_use_id: Some("toolu_parent".into()),
            input: json!({
                "path": "src/auth.rs",
                "content": "pub fn login() {}\n"
            }),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_tool_call", event);
}

#[test]
fn snapshot_event_kind_tool_result() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "write".into(),
            tool_use_id: Some("toolu_abc123".into()),
            output: json!({"bytes_written": 18, "path": "src/auth.rs"}),
            is_error: false,
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_tool_result", event);
}

#[test]
fn snapshot_event_kind_tool_result_error() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("toolu_err".into()),
            output: json!("permission denied: /etc/shadow"),
            is_error: true,
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_tool_result_error", event);
}

#[test]
fn snapshot_event_kind_file_changed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/oauth2.rs".into(),
            summary: "added PKCE challenge generation".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_file_changed", event);
}

#[test]
fn snapshot_event_kind_command_executed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test -- --nocapture".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok. 42 passed; 0 failed".into()),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_command_executed", event);
}

#[test]
fn snapshot_event_kind_warning() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "approaching budget limit: $4.80 of $5.00 used".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_warning", event);
}

#[test]
fn snapshot_event_kind_error() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "compilation failed with 3 errors".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("comprehensive_event_error", event);
}

// ── 4. Each Capability variant serialization ────────────────────────────

#[test]
fn snapshot_all_capability_variants() {
    let manifest: CapabilityManifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (Capability::ToolEdit, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Unsupported),
        (Capability::ToolGlob, SupportLevel::Native),
        (Capability::ToolGrep, SupportLevel::Native),
        (Capability::ToolWebSearch, SupportLevel::Emulated),
        (Capability::ToolWebFetch, SupportLevel::Emulated),
        (Capability::ToolAskUser, SupportLevel::Unsupported),
        (Capability::HooksPreToolUse, SupportLevel::Native),
        (Capability::HooksPostToolUse, SupportLevel::Native),
        (Capability::SessionResume, SupportLevel::Emulated),
        (Capability::SessionFork, SupportLevel::Unsupported),
        (Capability::Checkpointing, SupportLevel::Native),
        (Capability::StructuredOutputJsonSchema, SupportLevel::Emulated),
        (
            Capability::McpClient,
            SupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        ),
        (
            Capability::McpServer,
            SupportLevel::Restricted {
                reason: "experimental".into(),
            },
        ),
    ]);
    let value = serde_json::to_value(&manifest).unwrap();
    assert_json_snapshot!("comprehensive_all_capabilities", value);
}

// ── 5. PolicyProfile with complex patterns ──────────────────────────────

#[test]
fn snapshot_complex_policy_profile() {
    let policy = PolicyProfile {
        allowed_tools: vec![
            "read".into(),
            "write".into(),
            "edit".into(),
            "glob".into(),
            "grep".into(),
        ],
        disallowed_tools: vec![
            "bash".into(),
            "web_search".into(),
            "web_fetch".into(),
            "ask_user".into(),
        ],
        deny_read: vec![
            ".env".into(),
            ".env.*".into(),
            "**/*.key".into(),
            "**/*.pem".into(),
            "**/secrets/**".into(),
            "~/.ssh/**".into(),
        ],
        deny_write: vec![
            "Cargo.lock".into(),
            "package-lock.json".into(),
            "*.toml".into(),
            ".github/**".into(),
        ],
        allow_network: vec![
            "api.example.com".into(),
            "*.github.com".into(),
            "registry.npmjs.org".into(),
        ],
        deny_network: vec![
            "*.malware.net".into(),
            "evil.com".into(),
            "10.0.0.0/8".into(),
        ],
        require_approval_for: vec![
            "write".into(),
            "edit".into(),
            "bash".into(),
            "web_fetch".into(),
        ],
    };
    assert_json_snapshot!("comprehensive_complex_policy", policy);
}

// ── 6. WorkOrderBuilder defaults ────────────────────────────────────────

#[test]
fn snapshot_work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("simple task").build();
    let wo_value = serde_json::to_value(&wo).unwrap();
    assert_json_snapshot!("comprehensive_builder_defaults", wo_value, {
        ".id" => "[uuid]"
    });
}

// ── 7. ReceiptBuilder defaults ──────────────────────────────────────────

#[test]
fn snapshot_receipt_builder_defaults() {
    let receipt = ReceiptBuilder::new("test-backend")
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .work_order_id(Uuid::nil())
        .build();
    let value = serde_json::to_value(&receipt).unwrap();
    assert_json_snapshot!("comprehensive_receipt_builder_defaults", value, {
        ".meta.run_id" => "[uuid]"
    });
}

// ── 8. ValidationError display messages ─────────────────────────────────

#[test]
fn snapshot_validation_error_missing_field() {
    let err = ValidationError::MissingField {
        field: "backend.id",
    };
    assert_snapshot!("comprehensive_validation_missing_field", err.to_string());
}

#[test]
fn snapshot_validation_error_invalid_hash() {
    let err = ValidationError::InvalidHash {
        expected: "abc123def456".into(),
        actual: "000000000000".into(),
    };
    assert_snapshot!("comprehensive_validation_invalid_hash", err.to_string());
}

#[test]
fn snapshot_validation_error_empty_backend() {
    let err = ValidationError::EmptyBackendId;
    assert_snapshot!("comprehensive_validation_empty_backend", err.to_string());
}

#[test]
fn snapshot_validation_error_invalid_outcome() {
    let err = ValidationError::InvalidOutcome {
        reason: "started_at is after finished_at".into(),
    };
    assert_snapshot!(
        "comprehensive_validation_invalid_outcome",
        err.to_string()
    );
}

// ── 9. Minimal (empty) WorkOrder ────────────────────────────────────────

#[test]
fn snapshot_minimal_work_order() {
    let wo = WorkOrder {
        id: Uuid::nil(),
        task: String::new(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_json_snapshot!("comprehensive_minimal_work_order", wo);
}

// ── 10. AgentEvent with ext field (passthrough) ─────────────────────────

#[test]
fn snapshot_event_with_ext_passthrough() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "response from SDK".into(),
        },
        ext: Some(BTreeMap::from([
            (
                "raw_message".into(),
                json!({
                    "id": "msg_xyz",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "response from SDK"}]
                }),
            ),
            ("sdk_version".into(), json!("2024.6.1")),
        ])),
    };
    assert_json_snapshot!("comprehensive_event_with_ext", event);
}

// ── 11. Receipt with Partial outcome ────────────────────────────────────

#[test]
fn snapshot_receipt_partial_outcome() {
    let receipt = ReceiptBuilder::new("timeout-backend")
        .outcome(Outcome::Partial)
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .work_order_id(Uuid::nil())
        .build();
    let value = serde_json::to_value(&receipt).unwrap();
    assert_json_snapshot!("comprehensive_receipt_partial", value, {
        ".meta.run_id" => "[uuid]"
    });
}

// ── 12. Receipt with Failed outcome ─────────────────────────────────────

#[test]
fn snapshot_receipt_failed_outcome() {
    let receipt = ReceiptBuilder::new("crash-backend")
        .outcome(Outcome::Failed)
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .work_order_id(Uuid::nil())
        .verification(VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: false,
        })
        .build();
    let value = serde_json::to_value(&receipt).unwrap();
    assert_json_snapshot!("comprehensive_receipt_failed", value, {
        ".meta.run_id" => "[uuid]"
    });
}
