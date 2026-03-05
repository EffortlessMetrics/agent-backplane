#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive snapshot tests that verify the exact JSON serialization
//! format of all key ABP types. Any serialization format change will be
//! caught immediately.

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_error::ErrorCode;
use abp_protocol::Envelope;
use chrono::{TimeZone, Utc};
use serde_json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fixed UUID for deterministic tests.
fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
}

/// Fixed timestamp for deterministic tests.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 10, 31, 0).unwrap()
}

// ---------------------------------------------------------------------------
// 1. Outcome variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_outcome_complete() {
    let json = serde_json::to_string_pretty(&Outcome::Complete).unwrap();
    assert_eq!(json, r#""complete""#);
}

#[test]
fn snapshot_outcome_partial() {
    let json = serde_json::to_string_pretty(&Outcome::Partial).unwrap();
    assert_eq!(json, r#""partial""#);
}

#[test]
fn snapshot_outcome_failed() {
    let json = serde_json::to_string_pretty(&Outcome::Failed).unwrap();
    assert_eq!(json, r#""failed""#);
}

// ---------------------------------------------------------------------------
// 2. ExecutionLane variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_execution_lane_patch_first() {
    let json = serde_json::to_string_pretty(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(json, r#""patch_first""#);
}

#[test]
fn snapshot_execution_lane_workspace_first() {
    let json = serde_json::to_string_pretty(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(json, r#""workspace_first""#);
}

// ---------------------------------------------------------------------------
// 3. WorkspaceMode variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_workspace_mode_pass_through() {
    let json = serde_json::to_string_pretty(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(json, r#""pass_through""#);
}

#[test]
fn snapshot_workspace_mode_staged() {
    let json = serde_json::to_string_pretty(&WorkspaceMode::Staged).unwrap();
    assert_eq!(json, r#""staged""#);
}

// ---------------------------------------------------------------------------
// 4. ExecutionMode variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_execution_mode_passthrough() {
    let json = serde_json::to_string_pretty(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, r#""passthrough""#);
}

#[test]
fn snapshot_execution_mode_mapped() {
    let json = serde_json::to_string_pretty(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, r#""mapped""#);
}

// ---------------------------------------------------------------------------
// 5. MinSupport variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_min_support_native() {
    let json = serde_json::to_string_pretty(&MinSupport::Native).unwrap();
    assert_eq!(json, r#""native""#);
}

#[test]
fn snapshot_min_support_emulated() {
    let json = serde_json::to_string_pretty(&MinSupport::Emulated).unwrap();
    assert_eq!(json, r#""emulated""#);
}

// ---------------------------------------------------------------------------
// 6. SupportLevel variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_support_level_native() {
    let json = serde_json::to_string_pretty(&SupportLevel::Native).unwrap();
    assert_eq!(json, r#""native""#);
}

#[test]
fn snapshot_support_level_emulated() {
    let json = serde_json::to_string_pretty(&SupportLevel::Emulated).unwrap();
    assert_eq!(json, r#""emulated""#);
}

#[test]
fn snapshot_support_level_unsupported() {
    let json = serde_json::to_string_pretty(&SupportLevel::Unsupported).unwrap();
    assert_eq!(json, r#""unsupported""#);
}

#[test]
fn snapshot_support_level_restricted() {
    let val = SupportLevel::Restricted {
        reason: "disabled by admin".into(),
    };
    let json = serde_json::to_string_pretty(&val).unwrap();
    let expected = r#"{
  "restricted": {
    "reason": "disabled by admin"
  }
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 7. Capability (selected key variants)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_capability_streaming() {
    let json = serde_json::to_string_pretty(&Capability::Streaming).unwrap();
    assert_eq!(json, r#""streaming""#);
}

#[test]
fn snapshot_capability_tool_read() {
    let json = serde_json::to_string_pretty(&Capability::ToolRead).unwrap();
    assert_eq!(json, r#""tool_read""#);
}

#[test]
fn snapshot_capability_tool_write() {
    let json = serde_json::to_string_pretty(&Capability::ToolWrite).unwrap();
    assert_eq!(json, r#""tool_write""#);
}

#[test]
fn snapshot_capability_tool_bash() {
    let json = serde_json::to_string_pretty(&Capability::ToolBash).unwrap();
    assert_eq!(json, r#""tool_bash""#);
}

#[test]
fn snapshot_capability_mcp_client() {
    let json = serde_json::to_string_pretty(&Capability::McpClient).unwrap();
    assert_eq!(json, r#""mcp_client""#);
}

#[test]
fn snapshot_capability_extended_thinking() {
    let json = serde_json::to_string_pretty(&Capability::ExtendedThinking).unwrap();
    assert_eq!(json, r#""extended_thinking""#);
}

// ---------------------------------------------------------------------------
// 8. CapabilityManifest
// ---------------------------------------------------------------------------

#[test]
fn snapshot_capability_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::ToolBash, SupportLevel::Emulated);
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let expected = r#"{
  "streaming": "native",
  "tool_read": "native",
  "tool_bash": "emulated"
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 9. BackendIdentity
// ---------------------------------------------------------------------------

#[test]
fn snapshot_backend_identity_full() {
    let id = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.5.0".into()),
    };
    let json = serde_json::to_string_pretty(&id).unwrap();
    let expected = r#"{
  "id": "sidecar:node",
  "backend_version": "1.0.0",
  "adapter_version": "0.5.0"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_backend_identity_minimal() {
    let id = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    let json = serde_json::to_string_pretty(&id).unwrap();
    let expected = r#"{
  "id": "mock",
  "backend_version": null,
  "adapter_version": null
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 10. ContextSnippet
// ---------------------------------------------------------------------------

#[test]
fn snapshot_context_snippet() {
    let snippet = ContextSnippet {
        name: "README".into(),
        content: "# Hello World".into(),
    };
    let json = serde_json::to_string_pretty(&snippet).unwrap();
    let expected = r##"{
  "name": "README",
  "content": "# Hello World"
}"##;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 11. ContextPacket
// ---------------------------------------------------------------------------

#[test]
fn snapshot_context_packet_empty() {
    let ctx = ContextPacket::default();
    let json = serde_json::to_string_pretty(&ctx).unwrap();
    let expected = r#"{
  "files": [],
  "snippets": []
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_context_packet_populated() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "Focus on error handling".into(),
        }],
    };
    let json = serde_json::to_string_pretty(&ctx).unwrap();
    let expected = r#"{
  "files": [
    "src/main.rs",
    "Cargo.toml"
  ],
  "snippets": [
    {
      "name": "hint",
      "content": "Focus on error handling"
    }
  ]
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 12. WorkspaceSpec
// ---------------------------------------------------------------------------

#[test]
fn snapshot_workspace_spec() {
    let ws = WorkspaceSpec {
        root: "/tmp/workspace".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string_pretty(&ws).unwrap();
    let expected = r#"{
  "root": "/tmp/workspace",
  "mode": "staged",
  "include": [
    "src/**"
  ],
  "exclude": [
    "target/**"
  ]
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 13. PolicyProfile
// ---------------------------------------------------------------------------

#[test]
fn snapshot_policy_profile_empty() {
    let policy = PolicyProfile::default();
    let json = serde_json::to_string_pretty(&policy).unwrap();
    let expected = r#"{
  "allowed_tools": [],
  "disallowed_tools": [],
  "deny_read": [],
  "deny_write": [],
  "allow_network": [],
  "deny_network": [],
  "require_approval_for": []
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_policy_profile_populated() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["DeleteFile".into()],
    };
    let json = serde_json::to_string_pretty(&policy).unwrap();
    let expected = r#"{
  "allowed_tools": [
    "Read",
    "Write"
  ],
  "disallowed_tools": [
    "Bash"
  ],
  "deny_read": [
    "**/.env"
  ],
  "deny_write": [
    "**/.git/**"
  ],
  "allow_network": [
    "*.example.com"
  ],
  "deny_network": [
    "evil.com"
  ],
  "require_approval_for": [
    "DeleteFile"
  ]
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 14. CapabilityRequirements
// ---------------------------------------------------------------------------

#[test]
fn snapshot_capability_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let json = serde_json::to_string_pretty(&reqs).unwrap();
    let expected = r#"{
  "required": [
    {
      "capability": "streaming",
      "min_support": "native"
    },
    {
      "capability": "tool_bash",
      "min_support": "emulated"
    }
  ]
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 15. RuntimeConfig
// ---------------------------------------------------------------------------

#[test]
fn snapshot_runtime_config_empty() {
    let cfg = RuntimeConfig::default();
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let expected = r#"{
  "model": null,
  "vendor": {},
  "env": {},
  "max_budget_usd": null,
  "max_turns": null
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_runtime_config_populated() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "temperature".into(),
        serde_json::Value::Number(serde_json::Number::from_f64(0.7).unwrap()),
    );
    let mut env = BTreeMap::new();
    env.insert("OPENAI_API_KEY".into(), "sk-test".into());

    let cfg = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        env,
        max_budget_usd: Some(5.0),
        max_turns: Some(10),
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let expected = r#"{
  "model": "gpt-4",
  "vendor": {
    "temperature": 0.7
  },
  "env": {
    "OPENAI_API_KEY": "sk-test"
  },
  "max_budget_usd": 5.0,
  "max_turns": 10
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 16. UsageNormalized
// ---------------------------------------------------------------------------

#[test]
fn snapshot_usage_normalized_empty() {
    let usage = UsageNormalized::default();
    let json = serde_json::to_string_pretty(&usage).unwrap();
    let expected = r#"{
  "input_tokens": null,
  "output_tokens": null,
  "cache_read_tokens": null,
  "cache_write_tokens": null,
  "request_units": null,
  "estimated_cost_usd": null
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_usage_normalized_populated() {
    let usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.05),
    };
    let json = serde_json::to_string_pretty(&usage).unwrap();
    let expected = r#"{
  "input_tokens": 1000,
  "output_tokens": 500,
  "cache_read_tokens": 200,
  "cache_write_tokens": 100,
  "request_units": 3,
  "estimated_cost_usd": 0.05
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 17. VerificationReport
// ---------------------------------------------------------------------------

#[test]
fn snapshot_verification_report_empty() {
    let vr = VerificationReport::default();
    let json = serde_json::to_string_pretty(&vr).unwrap();
    let expected = r#"{
  "git_diff": null,
  "git_status": null,
  "harness_ok": false
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_verification_report_populated() {
    let vr = VerificationReport {
        git_diff: Some("diff --git a/f.rs b/f.rs".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string_pretty(&vr).unwrap();
    let expected = r#"{
  "git_diff": "diff --git a/f.rs b/f.rs",
  "git_status": "M src/main.rs",
  "harness_ok": true
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 18. ArtifactRef
// ---------------------------------------------------------------------------

#[test]
fn snapshot_artifact_ref() {
    let ar = ArtifactRef {
        kind: "patch".into(),
        path: "output/fix.patch".into(),
    };
    let json = serde_json::to_string_pretty(&ar).unwrap();
    let expected = r#"{
  "kind": "patch",
  "path": "output/fix.patch"
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 19-28. AgentEventKind variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_kind_run_started() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "Starting run".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "run_started",
  "message": "Starting run"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_run_completed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunCompleted {
            message: "Done".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "run_completed",
  "message": "Done"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_assistant_delta() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "assistant_delta",
  "text": "Hello"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_assistant_message() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "I will fix the bug.".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "assistant_message",
  "text": "I will fix the bug."
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_tool_call() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tu_001".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "tool_call",
  "tool_name": "Read",
  "tool_use_id": "tu_001",
  "parent_tool_use_id": null,
  "input": {
    "path": "src/main.rs"
  }
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_tool_result() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some("tu_001".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "tool_result",
  "tool_name": "Read",
  "tool_use_id": "tu_001",
  "output": {
    "content": "fn main() {}"
  },
  "is_error": false
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_file_changed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added error handling".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "file_changed",
  "path": "src/lib.rs",
  "summary": "Added error handling"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_command_executed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "command_executed",
  "command": "cargo test",
  "exit_code": 0,
  "output_preview": "test result: ok"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_warning() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "Approaching budget limit".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "warning",
  "message": "Approaching budget limit"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_error() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "Tool execution failed".into(),
            error_code: None,
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "error",
  "message": "Tool execution failed"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_agent_event_kind_error_with_code() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "Backend timed out".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "error",
  "message": "Backend timed out",
  "error_code": "backend_timeout"
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 29. AgentEvent with ext field
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"role": "assistant"}),
    );
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "Hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "assistant_message",
  "text": "Hi",
  "ext": {
    "raw_message": {
      "role": "assistant"
    }
  }
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 30. WorkOrder (full construction)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_work_order_full() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Fix login bug".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/auth.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Check token expiry".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec![],
            deny_write: vec!["**/.git/**".into()],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("claude-3".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(1.0),
            max_turns: Some(5),
        },
    };
    let json = serde_json::to_string_pretty(&wo).unwrap();
    let expected = r#"{
  "id": "00000000-0000-0000-0000-000000000001",
  "task": "Fix login bug",
  "lane": "workspace_first",
  "workspace": {
    "root": "/project",
    "mode": "staged",
    "include": [
      "src/**"
    ],
    "exclude": [
      "target/**"
    ]
  },
  "context": {
    "files": [
      "src/auth.rs"
    ],
    "snippets": [
      {
        "name": "hint",
        "content": "Check token expiry"
      }
    ]
  },
  "policy": {
    "allowed_tools": [
      "Read"
    ],
    "disallowed_tools": [
      "Bash"
    ],
    "deny_read": [],
    "deny_write": [
      "**/.git/**"
    ],
    "allow_network": [],
    "deny_network": [],
    "require_approval_for": []
  },
  "requirements": {
    "required": [
      {
        "capability": "tool_read",
        "min_support": "native"
      }
    ]
  },
  "config": {
    "model": "claude-3",
    "vendor": {},
    "env": {},
    "max_budget_usd": 1.0,
    "max_turns": 5
  }
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 31. Receipt (full construction with deterministic values)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_receipt_full() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        capabilities: {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m
        },
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"total_tokens": 1500}),
        usage: UsageNormalized {
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.03),
        },
        trace: vec![AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "Starting".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/f b/f".into()),
            git_status: Some("M f".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    let expected = r#"{
  "meta": {
    "run_id": "00000000-0000-0000-0000-000000000001",
    "work_order_id": "00000000-0000-0000-0000-000000000002",
    "contract_version": "abp/v0.1",
    "started_at": "2025-01-15T10:30:00Z",
    "finished_at": "2025-01-15T10:31:00Z",
    "duration_ms": 60000
  },
  "backend": {
    "id": "mock",
    "backend_version": "1.0.0",
    "adapter_version": null
  },
  "capabilities": {
    "streaming": "native"
  },
  "mode": "mapped",
  "usage_raw": {
    "total_tokens": 1500
  },
  "usage": {
    "input_tokens": 1000,
    "output_tokens": 500,
    "cache_read_tokens": null,
    "cache_write_tokens": null,
    "request_units": null,
    "estimated_cost_usd": 0.03
  },
  "trace": [
    {
      "ts": "2025-01-15T10:30:00Z",
      "type": "run_started",
      "message": "Starting"
    }
  ],
  "artifacts": [
    {
      "kind": "patch",
      "path": "out.patch"
    }
  ],
  "verification": {
    "git_diff": "diff --git a/f b/f",
    "git_status": "M f",
    "harness_ok": true
  },
  "outcome": "complete",
  "receipt_sha256": null
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 32. Receipt with hash
// ---------------------------------------------------------------------------

#[test]
fn snapshot_receipt_with_hash() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
    // The hash itself is deterministic for this exact receipt
    let json = serde_json::to_string_pretty(&hashed).unwrap();
    assert!(json.contains("\"receipt_sha256\":"));
}

// ---------------------------------------------------------------------------
// 33. RunMetadata
// ---------------------------------------------------------------------------

#[test]
fn snapshot_run_metadata() {
    let meta = RunMetadata {
        run_id: fixed_uuid(),
        work_order_id: fixed_uuid2(),
        contract_version: "abp/v0.1".into(),
        started_at: fixed_ts(),
        finished_at: fixed_ts2(),
        duration_ms: 60000,
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    let expected = r#"{
  "run_id": "00000000-0000-0000-0000-000000000001",
  "work_order_id": "00000000-0000-0000-0000-000000000002",
  "contract_version": "abp/v0.1",
  "started_at": "2025-01-15T10:30:00Z",
  "finished_at": "2025-01-15T10:31:00Z",
  "duration_ms": 60000
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 34-38. Envelope variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_envelope_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m
        },
    );
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "hello",
  "contract_version": "abp/v0.1",
  "backend": {
    "id": "sidecar:node",
    "backend_version": "1.0.0",
    "adapter_version": null
  },
  "capabilities": {
    "streaming": "native"
  },
  "mode": "mapped"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "hello",
  "contract_version": "abp/v0.1",
  "backend": {
    "id": "mock",
    "backend_version": null,
    "adapter_version": null
  },
  "capabilities": {},
  "mode": "passthrough"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_envelope_run() {
    let wo = WorkOrder {
        id: fixed_uuid(),
        task: "Test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "run",
  "id": "run-001",
  "work_order": {
    "id": "00000000-0000-0000-0000-000000000001",
    "task": "Test task",
    "lane": "patch_first",
    "workspace": {
      "root": ".",
      "mode": "staged",
      "include": [],
      "exclude": []
    },
    "context": {
      "files": [],
      "snippets": []
    },
    "policy": {
      "allowed_tools": [],
      "disallowed_tools": [],
      "deny_read": [],
      "deny_write": [],
      "allow_network": [],
      "deny_network": [],
      "require_approval_for": []
    },
    "requirements": {
      "required": []
    },
    "config": {
      "model": null,
      "vendor": {},
      "env": {},
      "max_budget_usd": null,
      "max_turns": null
    }
  }
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Working on it".into(),
            },
            ext: None,
        },
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "event",
  "ref_id": "run-001",
  "event": {
    "ts": "2025-01-15T10:30:00Z",
    "type": "assistant_message",
    "text": "Working on it"
  }
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_envelope_final() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt,
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    assert!(json.contains(r#""t": "final""#));
    assert!(json.contains(r#""ref_id": "run-001""#));
    assert!(json.contains(r#""outcome": "complete""#));
}

#[test]
fn snapshot_envelope_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "fatal",
  "ref_id": "run-001",
  "error": "out of memory"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-002".into()),
        "backend crashed",
        ErrorCode::BackendCrashed,
    );
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "fatal",
  "ref_id": "run-002",
  "error": "backend crashed",
  "error_code": "backend_crashed"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_envelope_fatal_no_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    let expected = r#"{
  "t": "fatal",
  "ref_id": null,
  "error": "startup failure"
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 42-43. BackplaneConfig
// ---------------------------------------------------------------------------

#[test]
fn snapshot_backplane_config_default() {
    let cfg = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        bind_address: None,
        port: None,
        policy_profiles: vec![],
        backends: BTreeMap::new(),
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let expected = r#"{
  "backends": {}
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_backplane_config_full() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["host.js".into()],
            timeout_secs: Some(120),
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["policy.toml".into()],
        backends,
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let expected = r#"{
  "default_backend": "mock",
  "workspace_dir": "/tmp/ws",
  "log_level": "debug",
  "receipts_dir": "/tmp/receipts",
  "bind_address": "127.0.0.1",
  "port": 8080,
  "policy_profiles": [
    "policy.toml"
  ],
  "backends": {
    "mock": {
      "type": "mock"
    },
    "node": {
      "type": "sidecar",
      "command": "node",
      "args": [
        "host.js"
      ],
      "timeout_secs": 120
    }
  }
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 44-45. BackendEntry variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_backend_entry_mock() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string_pretty(&entry).unwrap();
    let expected = r#"{
  "type": "mock"
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_backend_entry_sidecar() {
    let entry = BackendEntry::Sidecar {
        command: "python3".into(),
        args: vec!["host.py".into(), "--debug".into()],
        timeout_secs: Some(300),
    };
    let json = serde_json::to_string_pretty(&entry).unwrap();
    let expected = r#"{
  "type": "sidecar",
  "command": "python3",
  "args": [
    "host.py",
    "--debug"
  ],
  "timeout_secs": 300
}"#;
    assert_eq!(json, expected);
}

#[test]
fn snapshot_backend_entry_sidecar_no_timeout() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let json = serde_json::to_string_pretty(&entry).unwrap();
    let expected = r#"{
  "type": "sidecar",
  "command": "node",
  "args": []
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 47-80. ErrorCode variants
// ---------------------------------------------------------------------------

#[test]
fn snapshot_error_code_protocol_invalid_envelope() {
    let json = serde_json::to_string_pretty(&ErrorCode::ProtocolInvalidEnvelope).unwrap();
    assert_eq!(json, r#""protocol_invalid_envelope""#);
}

#[test]
fn snapshot_error_code_protocol_handshake_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::ProtocolHandshakeFailed).unwrap();
    assert_eq!(json, r#""protocol_handshake_failed""#);
}

#[test]
fn snapshot_error_code_protocol_missing_ref_id() {
    let json = serde_json::to_string_pretty(&ErrorCode::ProtocolMissingRefId).unwrap();
    assert_eq!(json, r#""protocol_missing_ref_id""#);
}

#[test]
fn snapshot_error_code_protocol_unexpected_message() {
    let json = serde_json::to_string_pretty(&ErrorCode::ProtocolUnexpectedMessage).unwrap();
    assert_eq!(json, r#""protocol_unexpected_message""#);
}

#[test]
fn snapshot_error_code_protocol_version_mismatch() {
    let json = serde_json::to_string_pretty(&ErrorCode::ProtocolVersionMismatch).unwrap();
    assert_eq!(json, r#""protocol_version_mismatch""#);
}

#[test]
fn snapshot_error_code_mapping_unsupported_capability() {
    let json = serde_json::to_string_pretty(&ErrorCode::MappingUnsupportedCapability).unwrap();
    assert_eq!(json, r#""mapping_unsupported_capability""#);
}

#[test]
fn snapshot_error_code_mapping_dialect_mismatch() {
    let json = serde_json::to_string_pretty(&ErrorCode::MappingDialectMismatch).unwrap();
    assert_eq!(json, r#""mapping_dialect_mismatch""#);
}

#[test]
fn snapshot_error_code_mapping_lossy_conversion() {
    let json = serde_json::to_string_pretty(&ErrorCode::MappingLossyConversion).unwrap();
    assert_eq!(json, r#""mapping_lossy_conversion""#);
}

#[test]
fn snapshot_error_code_mapping_unmappable_tool() {
    let json = serde_json::to_string_pretty(&ErrorCode::MappingUnmappableTool).unwrap();
    assert_eq!(json, r#""mapping_unmappable_tool""#);
}

#[test]
fn snapshot_error_code_backend_not_found() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendNotFound).unwrap();
    assert_eq!(json, r#""backend_not_found""#);
}

#[test]
fn snapshot_error_code_backend_unavailable() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendUnavailable).unwrap();
    assert_eq!(json, r#""backend_unavailable""#);
}

#[test]
fn snapshot_error_code_backend_timeout() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendTimeout).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
}

#[test]
fn snapshot_error_code_backend_rate_limited() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendRateLimited).unwrap();
    assert_eq!(json, r#""backend_rate_limited""#);
}

#[test]
fn snapshot_error_code_backend_auth_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendAuthFailed).unwrap();
    assert_eq!(json, r#""backend_auth_failed""#);
}

#[test]
fn snapshot_error_code_backend_model_not_found() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendModelNotFound).unwrap();
    assert_eq!(json, r#""backend_model_not_found""#);
}

#[test]
fn snapshot_error_code_backend_crashed() {
    let json = serde_json::to_string_pretty(&ErrorCode::BackendCrashed).unwrap();
    assert_eq!(json, r#""backend_crashed""#);
}

#[test]
fn snapshot_error_code_execution_tool_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::ExecutionToolFailed).unwrap();
    assert_eq!(json, r#""execution_tool_failed""#);
}

#[test]
fn snapshot_error_code_execution_workspace_error() {
    let json = serde_json::to_string_pretty(&ErrorCode::ExecutionWorkspaceError).unwrap();
    assert_eq!(json, r#""execution_workspace_error""#);
}

#[test]
fn snapshot_error_code_execution_permission_denied() {
    let json = serde_json::to_string_pretty(&ErrorCode::ExecutionPermissionDenied).unwrap();
    assert_eq!(json, r#""execution_permission_denied""#);
}

#[test]
fn snapshot_error_code_contract_version_mismatch() {
    let json = serde_json::to_string_pretty(&ErrorCode::ContractVersionMismatch).unwrap();
    assert_eq!(json, r#""contract_version_mismatch""#);
}

#[test]
fn snapshot_error_code_contract_schema_violation() {
    let json = serde_json::to_string_pretty(&ErrorCode::ContractSchemaViolation).unwrap();
    assert_eq!(json, r#""contract_schema_violation""#);
}

#[test]
fn snapshot_error_code_contract_invalid_receipt() {
    let json = serde_json::to_string_pretty(&ErrorCode::ContractInvalidReceipt).unwrap();
    assert_eq!(json, r#""contract_invalid_receipt""#);
}

#[test]
fn snapshot_error_code_capability_unsupported() {
    let json = serde_json::to_string_pretty(&ErrorCode::CapabilityUnsupported).unwrap();
    assert_eq!(json, r#""capability_unsupported""#);
}

#[test]
fn snapshot_error_code_capability_emulation_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::CapabilityEmulationFailed).unwrap();
    assert_eq!(json, r#""capability_emulation_failed""#);
}

#[test]
fn snapshot_error_code_policy_denied() {
    let json = serde_json::to_string_pretty(&ErrorCode::PolicyDenied).unwrap();
    assert_eq!(json, r#""policy_denied""#);
}

#[test]
fn snapshot_error_code_policy_invalid() {
    let json = serde_json::to_string_pretty(&ErrorCode::PolicyInvalid).unwrap();
    assert_eq!(json, r#""policy_invalid""#);
}

#[test]
fn snapshot_error_code_workspace_init_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::WorkspaceInitFailed).unwrap();
    assert_eq!(json, r#""workspace_init_failed""#);
}

#[test]
fn snapshot_error_code_workspace_staging_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::WorkspaceStagingFailed).unwrap();
    assert_eq!(json, r#""workspace_staging_failed""#);
}

#[test]
fn snapshot_error_code_ir_lowering_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::IrLoweringFailed).unwrap();
    assert_eq!(json, r#""ir_lowering_failed""#);
}

#[test]
fn snapshot_error_code_ir_invalid() {
    let json = serde_json::to_string_pretty(&ErrorCode::IrInvalid).unwrap();
    assert_eq!(json, r#""ir_invalid""#);
}

#[test]
fn snapshot_error_code_receipt_hash_mismatch() {
    let json = serde_json::to_string_pretty(&ErrorCode::ReceiptHashMismatch).unwrap();
    assert_eq!(json, r#""receipt_hash_mismatch""#);
}

#[test]
fn snapshot_error_code_receipt_chain_broken() {
    let json = serde_json::to_string_pretty(&ErrorCode::ReceiptChainBroken).unwrap();
    assert_eq!(json, r#""receipt_chain_broken""#);
}

#[test]
fn snapshot_error_code_dialect_unknown() {
    let json = serde_json::to_string_pretty(&ErrorCode::DialectUnknown).unwrap();
    assert_eq!(json, r#""dialect_unknown""#);
}

#[test]
fn snapshot_error_code_dialect_mapping_failed() {
    let json = serde_json::to_string_pretty(&ErrorCode::DialectMappingFailed).unwrap();
    assert_eq!(json, r#""dialect_mapping_failed""#);
}

#[test]
fn snapshot_error_code_config_invalid() {
    let json = serde_json::to_string_pretty(&ErrorCode::ConfigInvalid).unwrap();
    assert_eq!(json, r#""config_invalid""#);
}

#[test]
fn snapshot_error_code_internal() {
    let json = serde_json::to_string_pretty(&ErrorCode::Internal).unwrap();
    assert_eq!(json, r#""internal""#);
}

// ---------------------------------------------------------------------------
// 82. CONTRACT_VERSION constant
// ---------------------------------------------------------------------------

#[test]
fn snapshot_contract_version() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ---------------------------------------------------------------------------
// 83. CapabilityManifest empty
// ---------------------------------------------------------------------------

#[test]
fn snapshot_capability_manifest_empty() {
    let manifest = CapabilityManifest::new();
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    assert_eq!(json, "{}");
}

// ---------------------------------------------------------------------------
// 84. CapabilityRequirements empty
// ---------------------------------------------------------------------------

#[test]
fn snapshot_capability_requirements_empty() {
    let reqs = CapabilityRequirements::default();
    let json = serde_json::to_string_pretty(&reqs).unwrap();
    let expected = r#"{
  "required": []
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 85. WorkOrder via builder
// ---------------------------------------------------------------------------

#[test]
fn snapshot_work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("Do something").build();
    let json = serde_json::to_string_pretty(&wo).unwrap();
    // Verify structure contains expected fields (id is random)
    assert!(json.contains(r#""task": "Do something""#));
    assert!(json.contains(r#""lane": "patch_first""#));
    assert!(json.contains(r#""mode": "staged""#));
    assert!(json.contains(r#""root": ".""#));
}

// ---------------------------------------------------------------------------
// 86. ToolCall with no optional IDs
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_tool_call_minimal() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!("ls -la"),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "tool_call",
  "tool_name": "Bash",
  "tool_use_id": null,
  "parent_tool_use_id": null,
  "input": "ls -la"
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 87. ToolResult with error
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_tool_result_error() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "Bash".into(),
            tool_use_id: None,
            output: serde_json::json!("permission denied"),
            is_error: true,
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "tool_result",
  "tool_name": "Bash",
  "tool_use_id": null,
  "output": "permission denied",
  "is_error": true
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 88. CommandExecuted with null fields
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_command_executed_minimal() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "echo hi".into(),
            exit_code: None,
            output_preview: None,
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "command_executed",
  "command": "echo hi",
  "exit_code": null,
  "output_preview": null
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 89. ToolCall with parent_tool_use_id (nested tool)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_tool_call_nested() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Write".into(),
            tool_use_id: Some("tu_002".into()),
            parent_tool_use_id: Some("tu_001".into()),
            input: serde_json::json!({"path": "out.txt", "content": "data"}),
        },
        ext: None,
    };
    let json = serde_json::to_string_pretty(&event).unwrap();
    let expected = r#"{
  "ts": "2025-01-15T10:30:00Z",
  "type": "tool_call",
  "tool_name": "Write",
  "tool_use_id": "tu_002",
  "parent_tool_use_id": "tu_001",
  "input": {
    "content": "data",
    "path": "out.txt"
  }
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 90. Receipt outcome partial
// ---------------------------------------------------------------------------

#[test]
fn snapshot_receipt_outcome_partial() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Partial,
        receipt_sha256: None,
    };
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    assert!(json.contains(r#""outcome": "partial""#));
}

// ---------------------------------------------------------------------------
// 91. Receipt outcome failed
// ---------------------------------------------------------------------------

#[test]
fn snapshot_receipt_outcome_failed() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Failed,
        receipt_sha256: None,
    };
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    assert!(json.contains(r#""outcome": "failed""#));
}

// ---------------------------------------------------------------------------
// 92. Envelope event with tool_call
// ---------------------------------------------------------------------------

#[test]
fn snapshot_envelope_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "run-abc".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "Grep".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"pattern": "TODO"}),
            },
            ext: None,
        },
    };
    let json = serde_json::to_string_pretty(&env).unwrap();
    assert!(json.contains(r#""t": "event""#));
    assert!(json.contains(r#""ref_id": "run-abc""#));
    assert!(json.contains(r#""type": "tool_call""#));
    assert!(json.contains(r#""tool_name": "Grep""#));
}

// ---------------------------------------------------------------------------
// 93. Deserialization roundtrip: Outcome
// ---------------------------------------------------------------------------

#[test]
fn snapshot_outcome_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, parsed);
    }
}

// ---------------------------------------------------------------------------
// 94. Deserialization roundtrip: ErrorCode
// ---------------------------------------------------------------------------

#[test]
fn snapshot_error_code_roundtrip() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
    ];
    for code in codes {
        let json = serde_json::to_string(&code).unwrap();
        let parsed: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, parsed);
    }
}

// ---------------------------------------------------------------------------
// 95. Envelope discriminator uses "t" not "type"
// ---------------------------------------------------------------------------

#[test]
fn snapshot_envelope_uses_t_discriminator() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
    assert!(!json.contains(r#""type":"fatal""#));
}

// ---------------------------------------------------------------------------
// 96. AgentEventKind uses "type" discriminator
// ---------------------------------------------------------------------------

#[test]
fn snapshot_agent_event_uses_type_discriminator() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "test".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"warning""#));
}

// ---------------------------------------------------------------------------
// 97. BackendEntry uses "type" discriminator
// ---------------------------------------------------------------------------

#[test]
fn snapshot_backend_entry_uses_type_discriminator() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains(r#""type":"mock""#));
}

// ---------------------------------------------------------------------------
// 98. CapabilityManifest with restricted support level
// ---------------------------------------------------------------------------

#[test]
fn snapshot_capability_manifest_with_restricted() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed environment".into(),
        },
    );
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let expected = r#"{
  "tool_bash": {
    "restricted": {
      "reason": "sandboxed environment"
    }
  }
}"#;
    assert_eq!(json, expected);
}

// ---------------------------------------------------------------------------
// 99. Receipt passthrough mode
// ---------------------------------------------------------------------------

#[test]
fn snapshot_receipt_passthrough_mode() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    assert!(json.contains(r#""mode": "passthrough""#));
}

// ---------------------------------------------------------------------------
// 100. Multiple artifacts in receipt
// ---------------------------------------------------------------------------

#[test]
fn snapshot_receipt_multiple_artifacts() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 60000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "fix.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            },
        ],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    assert!(json.contains(r#""kind": "patch""#));
    assert!(json.contains(r#""kind": "log""#));
    assert!(json.contains(r#""path": "fix.patch""#));
    assert!(json.contains(r#""path": "run.log""#));
}
