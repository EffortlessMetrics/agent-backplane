//! abp-core
//!
//! The stable contract for Agent Backplane.
//!
//! If you only take one dependency, take this one.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const CONTRACT_VERSION: &str = "abp/v0.1";

/// A single unit of work.
///
/// This is intentionally *not* a chat session. Sessions can exist underneath,
/// but the contract is step-oriented.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkOrder {
    pub id: Uuid,

    /// Human intent.
    pub task: String,

    pub lane: ExecutionLane,

    pub workspace: WorkspaceSpec,

    pub context: ContextPacket,

    pub policy: PolicyProfile,

    pub requirements: CapabilityRequirements,

    pub config: RuntimeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLane {
    /// Agent proposes a patch/diff. No direct mutation of the user's repo.
    PatchFirst,

    /// Agent can mutate a workspace (often a staged worktree).
    WorkspaceFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceSpec {
    /// Root folder for the step.
    pub root: String,

    /// How the runtime should treat the workspace.
    pub mode: WorkspaceMode,

    /// Optional include globs (evaluated relative to root).
    pub include: Vec<String>,

    /// Optional exclude globs (evaluated relative to root).
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    /// Use the workspace as-is.
    PassThrough,

    /// Create a sanitized copy (or worktree) before running tools.
    Staged,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ContextPacket {
    /// Explicit file paths to include (relative to workspace root).
    pub files: Vec<String>,

    /// Optional snippets (for UIs or preloaded context).
    pub snippets: Vec<ContextSnippet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnippet {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct RuntimeConfig {
    /// Preferred backend/model identifier.
    pub model: Option<String>,

    /// Optional vendor-specific flags (passed through adapters).
    pub vendor: BTreeMap<String, serde_json::Value>,

    /// Environment variables for the runtime.
    pub env: BTreeMap<String, String>,

    /// Hard cap on cost (best-effort).
    pub max_budget_usd: Option<f64>,

    /// Hard cap on turns/iterations (best-effort).
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct PolicyProfile {
    /// Tool allowlist. Empty means "backend default".
    pub allowed_tools: Vec<String>,

    /// Tool denylist.
    pub disallowed_tools: Vec<String>,

    /// Deny reading paths matching any of these globs.
    pub deny_read: Vec<String>,

    /// Deny writing/editing paths matching any of these globs.
    pub deny_write: Vec<String>,

    /// Network allowlist (domains or patterns).
    pub allow_network: Vec<String>,

    /// Network denylist (domains or patterns).
    pub deny_network: Vec<String>,

    /// Require explicit approval for these tools.
    pub require_approval_for: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CapabilityRequirements {
    pub required: Vec<CapabilityRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityRequirement {
    pub capability: Capability,
    pub min_support: MinSupport,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinSupport {
    /// Only accept native support.
    Native,

    /// Native or emulated is acceptable.
    Emulated,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Streaming,

    // Built-in-ish tool expectations.
    ToolRead,
    ToolWrite,
    ToolEdit,
    ToolBash,
    ToolGlob,
    ToolGrep,
    ToolWebSearch,
    ToolWebFetch,
    ToolAskUser,

    // Governance.
    HooksPreToolUse,
    HooksPostToolUse,

    // Session behavior.
    SessionResume,
    SessionFork,

    // Reversibility.
    Checkpointing,

    // Structure.
    StructuredOutputJsonSchema,

    // MCP integration.
    McpClient,
    McpServer,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Native,
    Emulated,
    Unsupported,

    /// Supported in principle, but disabled by policy or environment.
    Restricted { reason: String },
}

impl SupportLevel {
    pub fn satisfies(&self, min: &MinSupport) -> bool {
        match (min, self) {
            (MinSupport::Native, SupportLevel::Native) => true,
            (MinSupport::Native, _) => false,

            (MinSupport::Emulated, SupportLevel::Native) => true,
            (MinSupport::Emulated, SupportLevel::Emulated) => true,
            (MinSupport::Emulated, SupportLevel::Restricted { .. }) => true,
            (MinSupport::Emulated, SupportLevel::Unsupported) => false,
        }
    }
}

pub type CapabilityManifest = BTreeMap<Capability, SupportLevel>;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackendIdentity {
    /// Stable backend identifier.
    pub id: String,

    /// Backend runtime version (SDK version, CLI version, etc.).
    pub backend_version: Option<String>,

    /// Adapter version (your sidecar wrapper version).
    pub adapter_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Receipt {
    pub meta: RunMetadata,
    pub backend: BackendIdentity,
    pub capabilities: CapabilityManifest,

    /// Vendor-specific usage payload as reported.
    pub usage_raw: serde_json::Value,

    /// Normalized usage fields (best-effort).
    pub usage: UsageNormalized,

    pub trace: Vec<AgentEvent>,

    pub artifacts: Vec<ArtifactRef>,

    pub verification: VerificationReport,

    pub outcome: Outcome,

    /// Hash of the canonical receipt (filled in by the control plane).
    pub receipt_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunMetadata {
    pub run_id: Uuid,
    pub work_order_id: Uuid,
    pub contract_version: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct UsageNormalized {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,

    /// Copilot-style billing.
    pub request_units: Option<u64>,

    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactRef {
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct VerificationReport {
    pub git_diff: Option<String>,
    pub git_status: Option<String>,
    pub harness_ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentEvent {
    pub ts: DateTime<Utc>,

    #[serde(flatten)]
    pub kind: AgentEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEventKind {
    RunStarted { message: String },
    RunCompleted { message: String },

    AssistantDelta { text: String },
    AssistantMessage { text: String },

    ToolCall {
        tool_name: String,
        tool_use_id: Option<String>,
        parent_tool_use_id: Option<String>,
        input: serde_json::Value,
    },

    ToolResult {
        tool_name: String,
        tool_use_id: Option<String>,
        output: serde_json::Value,
        is_error: bool,
    },

    FileChanged { path: String, summary: String },

    CommandExecuted {
        command: String,
        exit_code: Option<i32>,
        output_preview: Option<String>,
    },

    Warning { message: String },

    Error { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("failed to serialize JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Produce a deterministic JSON string for hashing.
///
/// This is not a full JCS implementation, but it is stable for our types:
/// - keys are sorted (serde_json Map is a BTreeMap by default)
/// - numbers are serialized consistently by serde_json
pub fn canonical_json<T: Serialize>(value: &T) -> Result<String, ContractError> {
    let v = serde_json::to_value(value)?;
    Ok(serde_json::to_string(&v)?)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn receipt_hash(receipt: &Receipt) -> Result<String, ContractError> {
    let json = canonical_json(receipt)?;
    Ok(sha256_hex(json.as_bytes()))
}

impl Receipt {
    pub fn with_hash(mut self) -> Result<Self, ContractError> {
        let h = receipt_hash(&self)?;
        self.receipt_sha256 = Some(h);
        Ok(self)
    }
}
