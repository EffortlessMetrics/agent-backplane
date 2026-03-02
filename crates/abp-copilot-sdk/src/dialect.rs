// SPDX-License-Identifier: MIT OR Apache-2.0
//! GitHub Copilot agent protocol dialect: config, request/response types, and mapping logic.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrder,
};
pub use abp_tooling::CanonicalToolDef;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version string for this dialect adapter.
pub const DIALECT_VERSION: &str = "copilot/v0.1";

/// Default model used when none is specified.
pub const DEFAULT_MODEL: &str = "gpt-4o";

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

/// Known GitHub Copilot model identifiers.
const KNOWN_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "gpt-4",
    "o1",
    "o1-mini",
    "o3-mini",
    "claude-sonnet-4",
    "claude-3.5-sonnet",
];

/// Map a vendor model name to the ABP canonical form (`copilot/<model>`).
#[must_use]
pub fn to_canonical_model(vendor_model: &str) -> String {
    format!("copilot/{vendor_model}")
}

/// Map an ABP canonical model name back to the vendor model name.
///
/// Strips the `copilot/` prefix if present; otherwise returns the input unchanged.
#[must_use]
pub fn from_canonical_model(canonical: &str) -> String {
    canonical
        .strip_prefix("copilot/")
        .unwrap_or(canonical)
        .to_string()
}

/// Returns `true` if `model` is a known Copilot model identifier.
#[must_use]
pub fn is_known_model(model: &str) -> bool {
    KNOWN_MODELS.contains(&model)
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

/// Build a [`CapabilityManifest`] describing what the Copilot backend supports.
#[must_use]
pub fn capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(Capability::ToolGlob, SupportLevel::Unsupported);
    m.insert(Capability::ToolGrep, SupportLevel::Unsupported);
    m.insert(Capability::ToolWebSearch, SupportLevel::Native);
    m.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    m.insert(Capability::HooksPreToolUse, SupportLevel::Emulated);
    m.insert(Capability::HooksPostToolUse, SupportLevel::Emulated);
    m.insert(Capability::McpClient, SupportLevel::Unsupported);
    m.insert(Capability::McpServer, SupportLevel::Unsupported);
    m
}

// ---------------------------------------------------------------------------
// Reference types
// ---------------------------------------------------------------------------

/// The type of a Copilot reference attached to a message or response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CopilotReferenceType {
    /// A file reference (path + optional content).
    File,
    /// A code snippet with location metadata.
    Snippet,
    /// A repository reference (owner/name).
    Repository,
    /// A web search result.
    WebSearchResult,
}

/// A reference attached to a Copilot message or response.
///
/// References provide structured context (files, snippets, repos, web results)
/// that the Copilot agent can use during processing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotReference {
    /// The reference type discriminator.
    #[serde(rename = "type")]
    pub ref_type: CopilotReferenceType,
    /// Unique identifier for this reference.
    pub id: String,
    /// Structured data payload for this reference.
    pub data: serde_json::Value,
    /// Optional metadata (e.g. display label, URI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// Tool types
// ---------------------------------------------------------------------------

/// The type of a Copilot tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CopilotToolType {
    /// A standard function tool.
    Function,
    /// A confirmation prompt tool requiring user approval.
    Confirmation,
}

/// Copilot-style tool definition.
///
/// Supports both standard function tools and confirmation prompt tools.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotTool {
    /// The tool type.
    #[serde(rename = "type")]
    pub tool_type: CopilotToolType,
    /// The function definition (for function tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<CopilotFunctionDef>,
    /// The confirmation definition (for confirmation tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<CopilotConfirmation>,
}

/// Function definition inside a Copilot tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Confirmation prompt definition for user approval flows.
///
/// When the agent needs user confirmation before proceeding with a
/// sensitive action, it emits a confirmation tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotConfirmation {
    /// Unique identifier for this confirmation.
    pub id: String,
    /// Title displayed to the user.
    pub title: String,
    /// Detailed message explaining what the user is approving.
    pub message: String,
    /// Whether the confirmation has been accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
}

/// Convert an ABP canonical tool definition to the Copilot function tool format.
#[must_use]
pub fn tool_def_to_copilot(def: &CanonicalToolDef) -> CopilotTool {
    CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: def.name.clone(),
            description: def.description.clone(),
            parameters: def.parameters_schema.clone(),
        }),
        confirmation: None,
    }
}

/// Convert a Copilot function tool definition back to the ABP canonical form.
///
/// Returns `None` if the tool is not a function type or has no function definition.
#[must_use]
pub fn tool_def_from_copilot(tool: &CopilotTool) -> Option<CanonicalToolDef> {
    let func = tool.function.as_ref()?;
    Some(CanonicalToolDef {
        name: func.name.clone(),
        description: func.description.clone(),
        parameters_schema: func.parameters.clone(),
    })
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// A single message in the Copilot conversation format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotMessage {
    /// Message role (`system`, `user`, or `assistant`).
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Optional display name for the message author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// References attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Vendor-specific configuration for the GitHub Copilot agent API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotConfig {
    /// GitHub token for API authentication.
    pub token: String,

    /// Base URL for the Copilot API.
    pub base_url: String,

    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,

    /// System prompt override.
    pub system_prompt: Option<String>,
}

impl Default for CopilotConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            base_url: "https://api.githubcopilot.com".into(),
            model: DEFAULT_MODEL.into(),
            system_prompt: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A request to the GitHub Copilot agent API.
///
/// Combines conversation messages with tool definitions, references,
/// and turn history for multi-turn interactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CopilotMessage>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotTool>>,
    /// Previous turns in the conversation (for multi-turn agents).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_history: Vec<CopilotTurnEntry>,
    /// Top-level references for the request (files, repos, snippets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<CopilotReference>,
}

/// An entry in the turn history for multi-turn conversations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotTurnEntry {
    /// The user message for this turn.
    pub request: String,
    /// The assistant response for this turn.
    pub response: String,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A non-streaming response from the Copilot agent API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotResponse {
    /// The assistant's reply text.
    pub message: String,
    /// References emitted in the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
    /// Errors reported during processing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_errors: Vec<CopilotError>,
    /// Confirmation prompt (if the agent requests user approval).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_confirmation: Option<CopilotConfirmation>,
    /// Function call request (if the agent wants to invoke a tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_call: Option<CopilotFunctionCall>,
}

/// An error reported by the Copilot agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotError {
    /// Error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Unique error identifier for correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

/// A function call emitted by the Copilot agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
    /// Unique call identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

// ---------------------------------------------------------------------------
// Streaming SSE event types
// ---------------------------------------------------------------------------

/// Server-sent events from the Copilot streaming API.
///
/// These events are delivered as SSE data lines; the `event:` prefix
/// determines which variant applies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CopilotStreamEvent {
    /// References emitted at the start of a response.
    CopilotReferences {
        /// The references payload.
        references: Vec<CopilotReference>,
    },
    /// Errors encountered during processing.
    CopilotErrors {
        /// The errors payload.
        errors: Vec<CopilotError>,
    },
    /// A text delta in the assistant's streaming reply.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// A function call emitted during streaming.
    FunctionCall {
        /// The function call details.
        function_call: CopilotFunctionCall,
    },
    /// A confirmation prompt for user approval.
    CopilotConfirmation {
        /// The confirmation details.
        confirmation: CopilotConfirmation,
    },
    /// Stream completed.
    Done {},
}

// ---------------------------------------------------------------------------
// Mapping: WorkOrder → CopilotRequest
// ---------------------------------------------------------------------------

/// Map an ABP [`WorkOrder`] to a [`CopilotRequest`].
///
/// Populates references from the work order's context files and snippets.
/// Uses the work order task as the initial user message.
pub fn map_work_order(wo: &WorkOrder, config: &CopilotConfig) -> CopilotRequest {
    let model = wo
        .config
        .model
        .as_deref()
        .unwrap_or(&config.model)
        .to_string();

    let mut references = Vec::new();

    // Map context files to file references.
    for (i, file_path) in wo.context.files.iter().enumerate() {
        references.push(CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: format!("file-{i}"),
            data: serde_json::json!({ "path": file_path }),
            metadata: None,
        });
    }

    // Map context snippets to snippet references.
    for (i, snippet) in wo.context.snippets.iter().enumerate() {
        references.push(CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: format!("snippet-{i}"),
            data: serde_json::json!({
                "name": snippet.name,
                "content": snippet.content,
            }),
            metadata: None,
        });
    }

    let mut messages = Vec::new();

    if let Some(system_prompt) = &config.system_prompt {
        messages.push(CopilotMessage {
            role: "system".into(),
            content: system_prompt.clone(),
            name: None,
            copilot_references: Vec::new(),
        });
    }

    messages.push(CopilotMessage {
        role: "user".into(),
        content: wo.task.clone(),
        name: None,
        copilot_references: references.clone(),
    });

    CopilotRequest {
        model,
        messages,
        tools: None,
        turn_history: Vec::new(),
        references,
    }
}

// ---------------------------------------------------------------------------
// Mapping: CopilotResponse → Vec<AgentEvent>
// ---------------------------------------------------------------------------

/// Map a [`CopilotResponse`] to a sequence of ABP [`AgentEvent`]s.
pub fn map_response(resp: &CopilotResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    // Emit assistant message if present.
    if !resp.message.is_empty() {
        events.push(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: resp.message.clone(),
            },
            ext: None,
        });
    }

    // Emit errors.
    for err in &resp.copilot_errors {
        events.push(AgentEvent {
            ts: now,
            kind: AgentEventKind::Error {
                message: format!("{}: {}", err.error_type, err.message),
            },
            ext: None,
        });
    }

    // Emit function call as tool call.
    if let Some(fc) = &resp.function_call {
        let input = serde_json::from_str(&fc.arguments)
            .unwrap_or(serde_json::Value::String(fc.arguments.clone()));
        events.push(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: fc.name.clone(),
                tool_use_id: fc.id.clone(),
                parent_tool_use_id: None,
                input,
            },
            ext: None,
        });
    }

    // Emit confirmation as a custom event via ext.
    if let Some(conf) = &resp.copilot_confirmation {
        let mut ext = BTreeMap::new();
        ext.insert(
            "copilot_confirmation".into(),
            serde_json::to_value(conf).unwrap_or(serde_json::Value::Null),
        );
        events.push(AgentEvent {
            ts: now,
            kind: AgentEventKind::Warning {
                message: format!("Confirmation required: {}", conf.title),
            },
            ext: Some(ext),
        });
    }

    events
}

// ---------------------------------------------------------------------------
// Mapping: CopilotStreamEvent → Vec<AgentEvent>
// ---------------------------------------------------------------------------

/// Map a single [`CopilotStreamEvent`] to zero or more ABP [`AgentEvent`]s.
pub fn map_stream_event(event: &CopilotStreamEvent) -> Vec<AgentEvent> {
    let now = Utc::now();

    match event {
        CopilotStreamEvent::TextDelta { text } => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantDelta { text: text.clone() },
                ext: None,
            }]
        }
        CopilotStreamEvent::FunctionCall { function_call } => {
            let input = serde_json::from_str(&function_call.arguments)
                .unwrap_or(serde_json::Value::String(function_call.arguments.clone()));
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::ToolCall {
                    tool_name: function_call.name.clone(),
                    tool_use_id: function_call.id.clone(),
                    parent_tool_use_id: None,
                    input,
                },
                ext: None,
            }]
        }
        CopilotStreamEvent::CopilotConfirmation { confirmation } => {
            let mut ext = BTreeMap::new();
            ext.insert(
                "copilot_confirmation".into(),
                serde_json::to_value(confirmation).unwrap_or(serde_json::Value::Null),
            );
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::Warning {
                    message: format!("Confirmation required: {}", confirmation.title),
                },
                ext: Some(ext),
            }]
        }
        CopilotStreamEvent::CopilotErrors { errors } => errors
            .iter()
            .map(|err| AgentEvent {
                ts: now,
                kind: AgentEventKind::Error {
                    message: format!("{}: {}", err.error_type, err.message),
                },
                ext: None,
            })
            .collect(),
        CopilotStreamEvent::CopilotReferences { references } => {
            if references.is_empty() {
                return vec![];
            }
            let mut ext = BTreeMap::new();
            ext.insert(
                "copilot_references".into(),
                serde_json::to_value(references).unwrap_or(serde_json::Value::Null),
            );
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::RunStarted {
                    message: format!(
                        "Copilot stream started with {} reference(s)",
                        references.len()
                    ),
                },
                ext: Some(ext),
            }]
        }
        CopilotStreamEvent::Done {} => {
            vec![AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: "Copilot stream completed".into(),
                },
                ext: None,
            }]
        }
    }
}

// ---------------------------------------------------------------------------
// Passthrough fidelity helpers
// ---------------------------------------------------------------------------

/// Wrap a raw [`CopilotStreamEvent`] in an ABP [`AgentEvent`] for passthrough mode.
///
/// The mapped event carries the original event JSON in `ext.raw_message` and a
/// `"dialect": "copilot"` marker so the receiver can reconstruct it losslessly.
pub fn to_passthrough_event(event: &CopilotStreamEvent) -> AgentEvent {
    let mapped = map_stream_event(event);
    let base = mapped.into_iter().next().unwrap_or_else(|| AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: String::new(),
        },
        ext: None,
    });

    let raw = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let mut ext = base.ext.unwrap_or_default();
    ext.insert("raw_message".into(), raw);
    ext.insert(
        "dialect".into(),
        serde_json::Value::String("copilot".into()),
    );

    AgentEvent {
        ts: base.ts,
        kind: base.kind,
        ext: Some(ext),
    }
}

/// Extract the original [`CopilotStreamEvent`] from a passthrough [`AgentEvent`].
///
/// Returns `None` if the event does not contain a `raw_message` extension field
/// or if deserialization fails.
pub fn from_passthrough_event(event: &AgentEvent) -> Option<CopilotStreamEvent> {
    let ext = event.ext.as_ref()?;
    let raw = ext.get("raw_message")?;
    serde_json::from_value(raw.clone()).ok()
}

/// Verify that a sequence of Copilot stream events survives a passthrough roundtrip.
///
/// Each event is wrapped into a passthrough [`AgentEvent`] and then extracted back.
/// Returns `true` if all events roundtrip without loss.
#[must_use]
pub fn verify_passthrough_fidelity(events: &[CopilotStreamEvent]) -> bool {
    events.iter().all(|e| {
        let wrapped = to_passthrough_event(e);
        from_passthrough_event(&wrapped).as_ref() == Some(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::WorkOrderBuilder;

    #[test]
    fn default_config_has_sensible_values() {
        let cfg = CopilotConfig::default();
        assert!(cfg.base_url.contains("githubcopilot"));
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn map_work_order_uses_task_as_user_message() {
        let wo = WorkOrderBuilder::new("Refactor auth module").build();
        let cfg = CopilotConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(req.messages[0].content.contains("Refactor auth module"));
    }

    #[test]
    fn map_work_order_respects_model_override() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
        let cfg = CopilotConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "gpt-4-turbo");
    }

    #[test]
    fn map_response_produces_assistant_message() {
        let resp = CopilotResponse {
            message: "Hello from Copilot!".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "Hello from Copilot!");
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn map_response_handles_function_call() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: Some(CopilotFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path": "src/main.rs"}"#.into(),
                id: Some("call_123".into()),
            }),
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_123"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
