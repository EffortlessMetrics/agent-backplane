#![allow(dead_code, unused_imports, unused_variables)]

//! Comprehensive conformance test harness validating SDK shim fidelity
//! across all supported dialects.
//!
//! Covers:
//! 1. Type parity tests (15): serde-roundtrip for each shim's request/response types
//! 2. Streaming parity tests (10): streaming types exist and roundtrip for each SDK
//! 3. Error code stability tests (10): all ABP error codes are stable strings
//! 4. Capability registry completeness (10): all 6 dialects registered with capabilities
//! 5. Receipt determinism tests (10): identical hashes for identical inputs
//! 6. Policy enforcement tests (5): tool/read/write policies work correctly

use std::collections::BTreeMap;
use std::path::Path;

use abp_capability::CapabilityRegistry;
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, BackendIdentity,
    Capability, CapabilityManifest, ExecutionMode, Outcome, Receipt, ReceiptBuilder, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::{Envelope, JsonlCodec};
use abp_sdk_types::{
    CanonicalToolDef, Dialect, DialectRequest, DialectResponse, DialectStreamChunk, ModelConfig,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_receipt(backend_id: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend_id).outcome(outcome).build()
}

fn make_receipt_with_trace(backend_id: &str, events: Vec<AgentEvent>) -> Receipt {
    let mut b = ReceiptBuilder::new(backend_id).outcome(Outcome::Complete);
    for e in events {
        b = b.add_trace_event(e);
    }
    b.build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. TYPE PARITY TESTS (15) — SDK shim request/response serde roundtrips
// ═══════════════════════════════════════════════════════════════════════════

mod type_parity {
    use super::*;

    // ── OpenAI ──

    #[test]
    fn openai_request_serde_roundtrip() {
        let req = abp_sdk_types::openai::OpenAiRequest {
            model: "gpt-4o".into(),
            messages: vec![abp_sdk_types::openai::OpenAiMessage {
                role: "user".into(),
                content: Some("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            tool_choice: None,
            temperature: Some(0.7),
            max_tokens: Some(1024),
            response_format: None,
            stream: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: abp_sdk_types::openai::OpenAiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gpt-4o");
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.temperature, Some(0.7));
        assert_eq!(back.max_tokens, Some(1024));
    }

    #[test]
    fn openai_response_serde_roundtrip() {
        let resp = abp_sdk_types::openai::OpenAiResponse {
            id: "chatcmpl-123".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: abp_sdk_types::openai::OpenAiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "chatcmpl-123");
        assert_eq!(back.object, "chat.completion");
    }

    #[test]
    fn openai_dialect_request_tagged_roundtrip() {
        let req = DialectRequest::OpenAi(abp_sdk_types::openai::OpenAiRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            response_format: None,
            stream: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"dialect\":\"open_ai\""));
        let back: DialectRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.dialect(), Dialect::OpenAi);
        assert_eq!(back.model(), "gpt-4o");
    }

    // ── Claude ──

    #[test]
    fn claude_request_serde_roundtrip() {
        let req = abp_sdk_types::claude::ClaudeRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            system: Some("You are helpful.".into()),
            messages: vec![abp_sdk_types::claude::ClaudeMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: None,
            thinking: None,
            stream: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: abp_sdk_types::claude::ClaudeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "claude-sonnet-4-20250514");
        assert_eq!(back.max_tokens, 4096);
        assert_eq!(back.system.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn claude_response_serde_roundtrip() {
        let resp = abp_sdk_types::claude::ClaudeResponse {
            id: "msg_abc".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![abp_sdk_types::claude::ClaudeContentBlock::Text {
                text: "Hello!".into(),
            }],
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: abp_sdk_types::claude::ClaudeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "msg_abc");
        assert_eq!(back.role, "assistant");
    }

    // ── Gemini ──

    #[test]
    fn gemini_request_serde_roundtrip() {
        let req = abp_sdk_types::gemini::GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![abp_sdk_types::gemini::GeminiContent {
                role: "user".into(),
                parts: vec![abp_sdk_types::gemini::GeminiPart::Text("hello".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: abp_sdk_types::gemini::GeminiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gemini-2.5-flash");
        assert_eq!(back.contents.len(), 1);
    }

    #[test]
    fn gemini_response_serde_roundtrip() {
        let resp = abp_sdk_types::gemini::GeminiResponse {
            candidates: vec![abp_sdk_types::gemini::GeminiCandidate {
                content: abp_sdk_types::gemini::GeminiContent {
                    role: "model".into(),
                    parts: vec![abp_sdk_types::gemini::GeminiPart::Text("hi".into())],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: abp_sdk_types::gemini::GeminiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.candidates.len(), 1);
    }

    // ── Codex ──

    #[test]
    fn codex_request_serde_roundtrip() {
        use abp_sdk_types::codex::{CodexInputItem, CodexRequest};
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "write hello world".into(),
            }],
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            tools: vec![],
            text: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CodexRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "codex-mini-latest");
        assert_eq!(back.max_output_tokens, Some(2048));
    }

    #[test]
    fn codex_response_serde_roundtrip() {
        use abp_sdk_types::codex::{CodexResponse, CodexResponseItem};
        let resp = CodexResponse {
            id: "resp_001".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![abp_sdk_types::codex::CodexContentPart::OutputText {
                    text: "done".into(),
                }],
            }],
            usage: None,
            status: Some("completed".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CodexResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "resp_001");
        assert_eq!(back.status.as_deref(), Some("completed"));
    }

    // ── Copilot ──

    #[test]
    fn copilot_request_serde_roundtrip() {
        use abp_sdk_types::copilot::{CopilotMessage, CopilotRequest};
        let req = CopilotRequest {
            model: "copilot-chat".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "explain this code".into(),
                name: None,
                copilot_references: vec![],
            }],
            tools: None,
            turn_history: vec![],
            references: vec![],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CopilotRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "copilot-chat");
        assert_eq!(back.messages.len(), 1);
    }

    #[test]
    fn copilot_response_serde_roundtrip() {
        use abp_sdk_types::copilot::CopilotResponse;
        let resp = CopilotResponse {
            message: "Here is an explanation.".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CopilotResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message, "Here is an explanation.");
    }

    // ── Kimi ──

    #[test]
    fn kimi_request_serde_roundtrip() {
        use abp_sdk_types::kimi::{KimiMessage, KimiRequest};
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage {
                role: "user".into(),
                content: Some("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(512),
            temperature: Some(0.3),
            stream: None,
            tools: None,
            use_search: Some(true),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: KimiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "moonshot-v1-8k");
        assert_eq!(back.use_search, Some(true));
    }

    #[test]
    fn kimi_response_serde_roundtrip() {
        use abp_sdk_types::kimi::{KimiChoice, KimiResponse, KimiResponseMessage};
        let resp = KimiResponse {
            id: "kimi-001".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: KimiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "kimi-001");
        assert_eq!(back.choices.len(), 1);
    }

    // ── Cross-dialect model config ──

    #[test]
    fn model_config_serde_roundtrip() {
        let cfg = ModelConfig {
            model: "gpt-4o".into(),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            top_p: None,
            stop_sequences: Some(vec!["STOP".into()]),
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. STREAMING PARITY TESTS (10) — streaming types roundtrip for each SDK
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_parity {
    use super::*;

    #[test]
    fn openai_stream_chunk_roundtrip() {
        let chunk = abp_sdk_types::openai::OpenAiStreamChunk {
            id: "chatcmpl-chunk-1".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![abp_sdk_types::openai::OpenAiStreamChoice {
                index: 0,
                delta: abp_sdk_types::openai::OpenAiStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: abp_sdk_types::openai::OpenAiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "chatcmpl-chunk-1");
        assert_eq!(back.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn claude_stream_event_message_start_roundtrip() {
        let event = abp_sdk_types::claude::ClaudeStreamEvent::MessageStart {
            message: abp_sdk_types::claude::ClaudeResponse {
                id: "msg_123".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: abp_sdk_types::claude::ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            abp_sdk_types::claude::ClaudeStreamEvent::MessageStart { .. }
        ));
    }

    #[test]
    fn claude_stream_event_content_block_delta_roundtrip() {
        let event = abp_sdk_types::claude::ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: abp_sdk_types::claude::ClaudeStreamDelta::TextDelta {
                text: "partial".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: abp_sdk_types::claude::ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            abp_sdk_types::claude::ClaudeStreamEvent::ContentBlockDelta { .. }
        ));
    }

    #[test]
    fn gemini_stream_chunk_roundtrip() {
        let chunk = abp_sdk_types::gemini::GeminiStreamChunk {
            candidates: vec![abp_sdk_types::gemini::GeminiCandidate {
                content: abp_sdk_types::gemini::GeminiContent {
                    role: "model".into(),
                    parts: vec![abp_sdk_types::gemini::GeminiPart::Text("delta".into())],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: abp_sdk_types::gemini::GeminiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.candidates.len(), 1);
    }

    #[test]
    fn codex_stream_event_roundtrip() {
        use abp_sdk_types::codex::{CodexResponse, CodexStreamEvent};
        let event = CodexStreamEvent::ResponseCreated {
            response: CodexResponse {
                id: "resp_s1".into(),
                model: "codex-mini-latest".into(),
                output: vec![],
                usage: None,
                status: Some("in_progress".into()),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, CodexStreamEvent::ResponseCreated { .. }));
    }

    #[test]
    fn copilot_stream_event_text_delta_roundtrip() {
        use abp_sdk_types::copilot::CopilotStreamEvent;
        let event = CopilotStreamEvent::TextDelta {
            text: "partial output".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, CopilotStreamEvent::TextDelta { .. }));
    }

    #[test]
    fn copilot_stream_event_done_roundtrip() {
        use abp_sdk_types::copilot::CopilotStreamEvent;
        let event = CopilotStreamEvent::Done {};
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, CopilotStreamEvent::Done {}));
    }

    #[test]
    fn kimi_stream_chunk_roundtrip() {
        use abp_sdk_types::kimi::{KimiChunkChoice, KimiChunkDelta, KimiStreamChunk};
        let chunk = KimiStreamChunk {
            id: "kimi-stream-1".into(),
            object: "chat.completion.chunk".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: Some("assistant".into()),
                    content: Some("hello".into()),
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: KimiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "kimi-stream-1");
    }

    #[test]
    fn dialect_stream_chunk_tagged_roundtrip_openai() {
        let chunk = DialectStreamChunk::OpenAi(abp_sdk_types::openai::OpenAiStreamChunk {
            id: "c1".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
        });
        let json = serde_json::to_string(&chunk).unwrap();
        let back: DialectStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.dialect(), Dialect::OpenAi);
    }

    #[test]
    fn dialect_stream_chunk_tagged_roundtrip_gemini() {
        let chunk = DialectStreamChunk::Gemini(abp_sdk_types::gemini::GeminiStreamChunk {
            candidates: vec![],
            usage_metadata: None,
        });
        let json = serde_json::to_string(&chunk).unwrap();
        let back: DialectStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.dialect(), Dialect::Gemini);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ERROR CODE STABILITY TESTS (10) — all ABP error codes are stable strings
// ═══════════════════════════════════════════════════════════════════════════

mod error_code_stability {
    use super::*;

    /// All ErrorCode variants and their expected stable string representations.
    const ALL_ERROR_CODES: &[(ErrorCode, &str)] = &[
        (
            ErrorCode::ProtocolInvalidEnvelope,
            "protocol_invalid_envelope",
        ),
        (
            ErrorCode::ProtocolHandshakeFailed,
            "protocol_handshake_failed",
        ),
        (ErrorCode::ProtocolMissingRefId, "protocol_missing_ref_id"),
        (
            ErrorCode::ProtocolUnexpectedMessage,
            "protocol_unexpected_message",
        ),
        (
            ErrorCode::ProtocolVersionMismatch,
            "protocol_version_mismatch",
        ),
        (
            ErrorCode::MappingUnsupportedCapability,
            "mapping_unsupported_capability",
        ),
        (
            ErrorCode::MappingDialectMismatch,
            "mapping_dialect_mismatch",
        ),
        (
            ErrorCode::MappingLossyConversion,
            "mapping_lossy_conversion",
        ),
        (ErrorCode::MappingUnmappableTool, "mapping_unmappable_tool"),
        (ErrorCode::BackendNotFound, "backend_not_found"),
        (ErrorCode::BackendUnavailable, "backend_unavailable"),
        (ErrorCode::BackendTimeout, "backend_timeout"),
        (ErrorCode::BackendRateLimited, "backend_rate_limited"),
        (ErrorCode::BackendAuthFailed, "backend_auth_failed"),
        (ErrorCode::BackendModelNotFound, "backend_model_not_found"),
        (ErrorCode::BackendCrashed, "backend_crashed"),
        (ErrorCode::ExecutionToolFailed, "execution_tool_failed"),
        (
            ErrorCode::ExecutionWorkspaceError,
            "execution_workspace_error",
        ),
        (
            ErrorCode::ExecutionPermissionDenied,
            "execution_permission_denied",
        ),
        (
            ErrorCode::ContractVersionMismatch,
            "contract_version_mismatch",
        ),
        (
            ErrorCode::ContractSchemaViolation,
            "contract_schema_violation",
        ),
        (
            ErrorCode::ContractInvalidReceipt,
            "contract_invalid_receipt",
        ),
        (ErrorCode::CapabilityUnsupported, "capability_unsupported"),
        (
            ErrorCode::CapabilityEmulationFailed,
            "capability_emulation_failed",
        ),
        (ErrorCode::PolicyDenied, "policy_denied"),
        (ErrorCode::PolicyInvalid, "policy_invalid"),
        (ErrorCode::WorkspaceInitFailed, "workspace_init_failed"),
        (
            ErrorCode::WorkspaceStagingFailed,
            "workspace_staging_failed",
        ),
        (ErrorCode::IrLoweringFailed, "ir_lowering_failed"),
        (ErrorCode::IrInvalid, "ir_invalid"),
        (ErrorCode::ReceiptHashMismatch, "receipt_hash_mismatch"),
        (ErrorCode::ReceiptChainBroken, "receipt_chain_broken"),
        (ErrorCode::DialectUnknown, "dialect_unknown"),
        (ErrorCode::DialectMappingFailed, "dialect_mapping_failed"),
        (ErrorCode::ConfigInvalid, "config_invalid"),
        (ErrorCode::Internal, "internal"),
    ];

    #[test]
    fn all_error_codes_have_stable_as_str() {
        for (code, expected) in ALL_ERROR_CODES {
            assert_eq!(
                code.as_str(),
                *expected,
                "ErrorCode::{code:?} as_str() mismatch"
            );
        }
    }

    #[test]
    fn error_codes_follow_category_prefix_pattern() {
        for (code, stable_str) in ALL_ERROR_CODES {
            let category_str = format!("{}", code.category());
            if *stable_str != "internal" {
                assert!(
                    stable_str.starts_with(&category_str),
                    "ErrorCode::{code:?} stable str '{stable_str}' should start with category '{category_str}'"
                );
            }
        }
    }

    #[test]
    fn error_codes_serde_roundtrip() {
        for (code, _) in ALL_ERROR_CODES {
            let json = serde_json::to_string(code).unwrap();
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(*code, back);
        }
    }

    #[test]
    fn error_code_categories_are_exhaustive() {
        let categories: std::collections::BTreeSet<_> = ALL_ERROR_CODES
            .iter()
            .map(|(code, _)| code.category())
            .collect();
        assert!(categories.contains(&ErrorCategory::Protocol));
        assert!(categories.contains(&ErrorCategory::Backend));
        assert!(categories.contains(&ErrorCategory::Mapping));
        assert!(categories.contains(&ErrorCategory::Execution));
        assert!(categories.contains(&ErrorCategory::Contract));
        assert!(categories.contains(&ErrorCategory::Capability));
        assert!(categories.contains(&ErrorCategory::Policy));
        assert!(categories.contains(&ErrorCategory::Workspace));
        assert!(categories.contains(&ErrorCategory::Ir));
        assert!(categories.contains(&ErrorCategory::Receipt));
        assert!(categories.contains(&ErrorCategory::Dialect));
        assert!(categories.contains(&ErrorCategory::Config));
        assert!(categories.contains(&ErrorCategory::Internal));
    }

    #[test]
    fn all_error_codes_have_nonempty_message() {
        for (code, _) in ALL_ERROR_CODES {
            let msg = code.message();
            assert!(!msg.is_empty(), "ErrorCode::{code:?} has empty message");
        }
    }

    #[test]
    fn retryable_codes_are_backend_errors() {
        let retryable: Vec<_> = ALL_ERROR_CODES
            .iter()
            .filter(|(code, _)| code.is_retryable())
            .collect();
        assert!(!retryable.is_empty());
        for (code, _) in &retryable {
            assert_eq!(
                code.category(),
                ErrorCategory::Backend,
                "Retryable code {code:?} should be a backend error"
            );
        }
    }

    #[test]
    fn non_retryable_codes_include_policy_and_contract() {
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::ContractSchemaViolation.is_retryable());
        assert!(!ErrorCode::IrInvalid.is_retryable());
    }

    #[test]
    fn error_code_count_is_36() {
        assert_eq!(ALL_ERROR_CODES.len(), 36, "Expected exactly 36 error codes");
    }

    #[test]
    fn abp_error_preserves_code_through_construction() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        assert!(err.message.contains("timed out"));
    }

    #[test]
    fn error_code_display_returns_human_readable_message() {
        let code = ErrorCode::BackendTimeout;
        let display = format!("{code}");
        assert_eq!(display, "backend timed out");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. CAPABILITY REGISTRY COMPLETENESS (10) — all 6 dialects registered
// ═══════════════════════════════════════════════════════════════════════════

mod capability_registry_completeness {
    use super::*;

    #[test]
    fn default_registry_has_six_backends() {
        let reg = CapabilityRegistry::with_defaults();
        assert_eq!(reg.len(), 6, "Default registry should contain 6 backends");
    }

    #[test]
    fn registry_contains_openai_manifest() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("openai/gpt-4o"));
    }

    #[test]
    fn registry_contains_claude_manifest() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    }

    #[test]
    fn registry_contains_gemini_manifest() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("google/gemini-1.5-pro"));
    }

    #[test]
    fn registry_contains_kimi_manifest() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("moonshot/kimi"));
    }

    #[test]
    fn registry_contains_codex_manifest() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("openai/codex"));
    }

    #[test]
    fn registry_contains_copilot_manifest() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("github/copilot"));
    }

    #[test]
    fn all_default_manifests_have_streaming_capability() {
        let reg = CapabilityRegistry::with_defaults();
        for name in reg.names() {
            let manifest = reg.get(name).unwrap();
            assert!(
                manifest.contains_key(&Capability::Streaming),
                "Backend '{name}' should declare Streaming capability"
            );
        }
    }

    #[test]
    fn all_default_manifests_have_tool_use() {
        let reg = CapabilityRegistry::with_defaults();
        for name in reg.names() {
            let manifest = reg.get(name).unwrap();
            assert!(
                manifest.contains_key(&Capability::ToolUse),
                "Backend '{name}' should declare ToolUse capability"
            );
        }
    }

    #[test]
    fn registry_query_streaming_returns_all_six() {
        let reg = CapabilityRegistry::with_defaults();
        let results = reg.query_capability(&Capability::Streaming);
        assert_eq!(results.len(), 6);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. RECEIPT DETERMINISM TESTS (10) — identical hashes for identical inputs
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_determinism {
    use super::*;

    #[test]
    fn receipt_hash_is_deterministic() {
        let r = make_receipt("mock", Outcome::Complete);
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_hash_is_64_hex_chars() {
        let r = make_receipt("mock", Outcome::Complete);
        let h = receipt_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn with_hash_attaches_hash_field() {
        let r = make_receipt("mock", Outcome::Complete).with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
        assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn hash_excludes_receipt_sha256_field() {
        let r1 = make_receipt("mock", Outcome::Complete);
        let mut r2 = r1.clone();
        r2.receipt_sha256 = Some("bogus".into());
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_eq!(h1, h2, "receipt_sha256 field must not affect hash");
    }

    #[test]
    fn different_backends_produce_different_hashes() {
        let r1 = make_receipt("mock-a", Outcome::Complete);
        let r2 = make_receipt("mock-b", Outcome::Complete);
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn different_outcomes_produce_different_hashes() {
        let r1 = make_receipt("mock", Outcome::Complete);
        let r2 = make_receipt("mock", Outcome::Failed);
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn receipt_with_trace_events_hashes_consistently() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Hello".into(),
        })];
        let r = make_receipt_with_trace("mock", events);
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_hashed_and_roundtripped_through_json() {
        let r = make_receipt("mock", Outcome::Complete).with_hash().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let back: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r.receipt_sha256, back.receipt_sha256);
    }

    #[test]
    fn receipt_contract_version_is_current() {
        let r = make_receipt("mock", Outcome::Complete);
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn canonical_json_is_deterministic() {
        let v = json!({"z": 1, "a": 2, "m": 3});
        let c1 = canonical_json(&v).unwrap();
        let c2 = canonical_json(&v).unwrap();
        assert_eq!(c1, c2);
        // Keys should be sorted
        assert!(c1.starts_with("{\"a\":"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. POLICY ENFORCEMENT TESTS (5) — tool/read/write policies
// ═══════════════════════════════════════════════════════════════════════════

mod policy_enforcement {
    use super::*;
    use abp_core::PolicyProfile;

    #[test]
    fn disallowed_tool_is_denied() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool("Bash");
        assert!(!decision.allowed);
        assert!(decision.reason.as_deref().unwrap().contains("Bash"));
    }

    #[test]
    fn allowed_tool_is_permitted() {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".into(), "Grep".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("Grep").allowed);
    }

    #[test]
    fn deny_read_blocks_matching_path() {
        let policy = PolicyProfile {
            deny_read: vec!["**/.env*".into(), "**/secret*".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
        assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_write_blocks_matching_path() {
        let policy = PolicyProfile {
            deny_write: vec!["**/.git/**".into(), "**/Cargo.lock".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
        assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn empty_policy_permits_everything() {
        let policy = PolicyProfile::default();
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("AnyTool").allowed);
        assert!(engine.can_read_path(Path::new("anything.txt")).allowed);
        assert!(engine.can_write_path(Path::new("anywhere/file.rs")).allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BONUS: Cross-cutting conformance assertions
// ═══════════════════════════════════════════════════════════════════════════

mod cross_cutting {
    use super::*;

    #[test]
    fn dialect_enum_has_six_variants() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_labels_are_nonempty() {
        for d in Dialect::all() {
            assert!(!d.label().is_empty());
        }
    }

    #[test]
    fn dialect_serde_roundtrip_all() {
        for d in Dialect::all() {
            let json = serde_json::to_string(d).unwrap();
            let back: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(*d, back);
        }
    }

    #[test]
    fn canonical_tool_def_roundtrip() {
        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn contract_version_format() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
        assert!(CONTRACT_VERSION.starts_with("abp/v"));
    }

    #[test]
    fn envelope_hello_roundtrip() {
        let backend = BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        };
        let env = Envelope::hello(backend, CapabilityManifest::new());
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.contains("\"t\":\"hello\""));
        let back = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(back, Envelope::Hello { .. }));
    }

    #[test]
    fn receipt_builder_defaults_are_sane() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.backend.id, "test");
        assert_eq!(r.outcome, Outcome::Complete);
        assert!(r.receipt_sha256.is_none());
        assert!(r.trace.is_empty());
        assert!(r.artifacts.is_empty());
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn work_order_builder_defaults_are_sane() {
        let wo = WorkOrderBuilder::new("test task").build();
        assert_eq!(wo.task, "test task");
        assert_eq!(wo.config.model, None);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BONUS: Shim client construction smoke tests
// ═══════════════════════════════════════════════════════════════════════════

mod shim_smoke {
    #[test]
    fn openai_shim_request_builder() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("hello")])
            .temperature(0.5)
            .max_tokens(1024)
            .build();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn gemini_shim_pipeline_client_model() {
        let client = abp_shim_gemini::PipelineClient::new("gemini-2.5-flash");
        assert_eq!(client.model(), "gemini-2.5-flash");
    }

    #[test]
    fn codex_shim_client_model() {
        let client = abp_shim_codex::CodexClient::new("codex-mini-latest");
        assert_eq!(client.model(), "codex-mini-latest");
    }

    #[test]
    fn copilot_shim_client_model() {
        let client = abp_shim_copilot::CopilotClient::new("copilot-chat");
        assert_eq!(client.model(), "copilot-chat");
    }

    #[test]
    fn kimi_shim_client_model() {
        let client = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
        assert_eq!(client.model(), "moonshot-v1-8k");
    }
}
