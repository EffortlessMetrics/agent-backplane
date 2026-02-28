// SPDX-License-Identifier: MIT OR Apache-2.0
//! abp-core
#![deny(unsafe_code)]
//!
//! The stable contract for Agent Backplane.
//!
//! If you only take one dependency, take this one.

pub mod filter;
pub mod validate;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Current contract version string embedded in all wire messages and receipts.
///
/// # Examples
///
/// ```
/// assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
/// ```
pub const CONTRACT_VERSION: &str = "abp/v0.1";

/// A single unit of work.
///
/// This is intentionally *not* a chat session. Sessions can exist underneath,
/// but the contract is step-oriented.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkOrder {
    /// Unique identifier for this work order.
    pub id: Uuid,

    /// Human intent.
    pub task: String,

    /// Strategy for how the agent produces output.
    pub lane: ExecutionLane,

    /// Workspace root, staging mode, and include/exclude globs.
    pub workspace: WorkspaceSpec,

    /// Pre-loaded context files and snippets.
    pub context: ContextPacket,

    /// Security policy (tool/path restrictions).
    pub policy: PolicyProfile,

    /// Capability requirements the backend must satisfy.
    pub requirements: CapabilityRequirements,

    /// Runtime-level knobs (model, budget, vendor flags).
    pub config: RuntimeConfig,
}

/// Strategy for how the agent produces its output.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLane {
    /// Agent proposes a patch/diff. No direct mutation of the user's repo.
    PatchFirst,

    /// Agent can mutate a workspace (often a staged worktree).
    WorkspaceFirst,
}

/// Describes the workspace root, staging mode, and include/exclude globs.
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

/// How the runtime treats the workspace before handing it to a backend.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    /// Use the workspace as-is.
    PassThrough,

    /// Create a sanitized copy (or worktree) before running tools.
    Staged,
}

/// Pre-loaded context files and snippets attached to a [`WorkOrder`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ContextPacket {
    /// Explicit file paths to include (relative to workspace root).
    pub files: Vec<String>,

    /// Optional snippets (for UIs or preloaded context).
    pub snippets: Vec<ContextSnippet>,
}

/// A named text fragment included in [`ContextPacket`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextSnippet {
    /// Human-readable label for the snippet.
    pub name: String,
    /// The snippet text.
    pub content: String,
}

/// Runtime-level knobs: model selection, vendor flags, budget caps, etc.
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

/// Security policy: tool allow/deny lists, path restrictions, network rules.
///
/// An empty profile permits everything (no restrictions).
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

/// Set of capabilities the work order requires from its backend.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CapabilityRequirements {
    /// List of capability/support-level pairs the backend must satisfy.
    pub required: Vec<CapabilityRequirement>,
}

/// A single capability + minimum support level pair.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityRequirement {
    /// The capability being required.
    pub capability: Capability,
    /// Minimum acceptable support level.
    pub min_support: MinSupport,
}

/// Minimum acceptable [`SupportLevel`] threshold.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinSupport {
    /// Only accept native support.
    Native,

    /// Native or emulated is acceptable.
    Emulated,
}

/// A discrete feature that a backend may support (tools, hooks, MCP, etc.).
///
/// # Examples
///
/// ```
/// use abp_core::{Capability, SupportLevel, CapabilityManifest};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::ToolRead, SupportLevel::Native);
/// manifest.insert(Capability::Streaming, SupportLevel::Emulated);
///
/// assert!(manifest.contains_key(&Capability::ToolRead));
/// ```
#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
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

/// How well a backend supports a given [`Capability`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Native,
    Emulated,
    Unsupported,

    /// Supported in principle, but disabled by policy or environment.
    Restricted {
        reason: String,
    },
}

impl SupportLevel {
    /// Returns `true` if this support level meets or exceeds `min`.
    #[must_use]
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

/// Maps each [`Capability`] to its [`SupportLevel`] for a given backend.
pub type CapabilityManifest = BTreeMap<Capability, SupportLevel>;

/// Execution mode for how ABP processes requests.
///
/// - Passthrough: Lossless wrapping - ABP acts as observer/recorder only
/// - Mapped: Full dialect translation - ABP translates between dialects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Lossless wrapping mode. ABP passes requests directly to the SDK
    /// without modification. Stream is bitwise-equivalent to direct SDK call
    /// after removing ABP framing.
    Passthrough,

    /// Full dialect translation mode. ABP translates between different
    /// agent dialects, potentially modifying requests and responses.
    #[default]
    Mapped,
}

/// Identifies a backend and its version information.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackendIdentity {
    /// Stable backend identifier (e.g. `"mock"`, `"sidecar:node"`).
    pub id: String,

    /// Backend runtime version (SDK version, CLI version, etc.).
    pub backend_version: Option<String>,

    /// Adapter version (your sidecar wrapper version).
    pub adapter_version: Option<String>,
}

/// The outcome of a completed run: metadata, usage, trace, and verification.
///
/// Use [`Receipt::with_hash`] to compute and attach the canonical SHA-256 hash.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Receipt {
    /// Timing and identity metadata for this run.
    pub meta: RunMetadata,
    /// Backend that executed the work order.
    pub backend: BackendIdentity,
    /// Capability manifest reported by the backend.
    pub capabilities: CapabilityManifest,

    /// Execution mode used for this run.
    #[serde(default)]
    pub mode: ExecutionMode,

    /// Vendor-specific usage payload as reported.
    pub usage_raw: serde_json::Value,

    /// Normalized usage fields (best-effort).
    pub usage: UsageNormalized,

    /// Ordered log of events emitted during the run.
    pub trace: Vec<AgentEvent>,

    /// References to artifacts produced during the run.
    pub artifacts: Vec<ArtifactRef>,

    /// Git-based verification data captured after completion.
    pub verification: VerificationReport,

    /// High-level result status.
    pub outcome: Outcome,

    /// Hash of the canonical receipt (filled in by the control plane).
    pub receipt_sha256: Option<String>,
}

/// Timing and identity metadata for a single run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunMetadata {
    /// Unique run identifier.
    pub run_id: Uuid,
    /// The work order this run fulfilled.
    pub work_order_id: Uuid,
    /// Contract version used for this run.
    pub contract_version: String,
    /// Timestamp when the run started.
    pub started_at: DateTime<Utc>,
    /// Timestamp when the run finished.
    pub finished_at: DateTime<Utc>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Best-effort normalized token/cost counters across different backends.
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

/// High-level result status of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Complete,
    Partial,
    Failed,
}

/// Reference to an artifact produced during a run (e.g. a patch file).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactRef {
    /// Artifact type (e.g. `"patch"`, `"log"`).
    pub kind: String,
    /// Path to the artifact relative to the workspace root.
    pub path: String,
}

/// Git-based verification data captured after a run completes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct VerificationReport {
    /// Output of `git diff` in the workspace, if available.
    pub git_diff: Option<String>,
    /// Output of `git status --porcelain` in the workspace, if available.
    pub git_status: Option<String>,
    /// Whether the harness (if any) reported success.
    pub harness_ok: bool,
}

/// A timestamped event emitted by an agent during a run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentEvent {
    pub ts: DateTime<Utc>,

    #[serde(flatten)]
    pub kind: AgentEventKind,

    /// Extension field for passthrough mode raw data.
    ///
    /// In passthrough mode, this contains the original SDK message
    /// for lossless reconstruction. The key `raw_message` contains
    /// the verbatim SDK message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<BTreeMap<String, serde_json::Value>>,
}

/// The payload discriminator for [`AgentEvent`].
///
/// Serialized with `#[serde(tag = "type")]` — note this is different from
/// the protocol envelope which uses `#[serde(tag = "t")]`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEventKind {
    RunStarted {
        message: String,
    },
    RunCompleted {
        message: String,
    },

    AssistantDelta {
        text: String,
    },
    AssistantMessage {
        text: String,
    },

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

    FileChanged {
        path: String,
        summary: String,
    },

    CommandExecuted {
        command: String,
        exit_code: Option<i32>,
        output_preview: Option<String>,
    },

    Warning {
        message: String,
    },

    Error {
        message: String,
    },
}

/// Errors from contract-level operations (serialization, hashing).
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
///
/// # Errors
///
/// Returns [`ContractError::Json`] if the value cannot be serialized.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<String, ContractError> {
    let v = serde_json::to_value(value)?;
    Ok(serde_json::to_string(&v)?)
}

/// Compute the hex-encoded SHA-256 digest of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Compute the canonical hash of a receipt.
///
/// **Gotcha:** Sets `receipt_sha256` to `null` before hashing to prevent
/// the stored hash from being self-referential. Always prefer
/// [`Receipt::with_hash`] over calling this directly.
///
/// # Examples
///
/// ```
/// # use abp_core::*;
/// # use chrono::Utc;
/// # use uuid::Uuid;
/// # use std::collections::BTreeMap;
/// let receipt = Receipt {
///     meta: RunMetadata {
///         run_id: Uuid::nil(),
///         work_order_id: Uuid::nil(),
///         contract_version: CONTRACT_VERSION.to_string(),
///         started_at: Utc::now(),
///         finished_at: Utc::now(),
///         duration_ms: 42,
///     },
///     backend: BackendIdentity {
///         id: "mock".into(),
///         backend_version: None,
///         adapter_version: None,
///     },
///     capabilities: CapabilityManifest::new(),
///     mode: ExecutionMode::default(),
///     usage_raw: serde_json::json!({}),
///     usage: UsageNormalized::default(),
///     trace: vec![],
///     artifacts: vec![],
///     verification: VerificationReport::default(),
///     outcome: Outcome::Complete,
///     receipt_sha256: None,
/// };
///
/// let hash = receipt_hash(&receipt).unwrap();
/// assert_eq!(hash.len(), 64); // SHA-256 hex digest
///
/// // Hashing is deterministic — same receipt produces same hash.
/// assert_eq!(hash, receipt_hash(&receipt).unwrap());
/// ```
///
/// # Errors
///
/// Returns [`ContractError::Json`] if the receipt cannot be serialized.
pub fn receipt_hash(receipt: &Receipt) -> Result<String, ContractError> {
    // Important: `receipt_sha256` must not influence the hash input, otherwise
    // the stored hash becomes self-inconsistent.
    //
    // We canonicalize via serde_json::Value so we can force the field to `null`
    // without cloning the full receipt (which may include a large trace).
    let mut v = serde_json::to_value(receipt)?;
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let json = serde_json::to_string(&v)?;
    Ok(sha256_hex(json.as_bytes()))
}

/// Builder for constructing [`WorkOrder`]s ergonomically.
///
/// # Examples
///
/// ```
/// use abp_core::{WorkOrderBuilder, ExecutionLane};
///
/// let wo = WorkOrderBuilder::new("Fix the login bug")
///     .lane(ExecutionLane::WorkspaceFirst)
///     .root("/tmp/workspace")
///     .model("gpt-4")
///     .max_turns(10)
///     .build();
///
/// assert_eq!(wo.task, "Fix the login bug");
/// assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
/// assert_eq!(wo.config.max_turns, Some(10));
/// ```
#[derive(Debug)]
pub struct WorkOrderBuilder {
    task: String,
    lane: ExecutionLane,
    root: String,
    workspace_mode: WorkspaceMode,
    include: Vec<String>,
    exclude: Vec<String>,
    context: ContextPacket,
    policy: PolicyProfile,
    requirements: CapabilityRequirements,
    config: RuntimeConfig,
}

impl WorkOrderBuilder {
    #[must_use]
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            lane: ExecutionLane::PatchFirst,
            root: ".".into(),
            workspace_mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        }
    }

    #[must_use]
    pub fn lane(mut self, lane: ExecutionLane) -> Self {
        self.lane = lane;
        self
    }
    #[must_use]
    pub fn root(mut self, root: impl Into<String>) -> Self {
        self.root = root.into();
        self
    }
    #[must_use]
    pub fn workspace_mode(mut self, mode: WorkspaceMode) -> Self {
        self.workspace_mode = mode;
        self
    }
    #[must_use]
    pub fn include(mut self, patterns: Vec<String>) -> Self {
        self.include = patterns;
        self
    }
    #[must_use]
    pub fn exclude(mut self, patterns: Vec<String>) -> Self {
        self.exclude = patterns;
        self
    }
    #[must_use]
    pub fn context(mut self, ctx: ContextPacket) -> Self {
        self.context = ctx;
        self
    }
    #[must_use]
    pub fn policy(mut self, policy: PolicyProfile) -> Self {
        self.policy = policy;
        self
    }
    #[must_use]
    pub fn requirements(mut self, reqs: CapabilityRequirements) -> Self {
        self.requirements = reqs;
        self
    }
    #[must_use]
    pub fn config(mut self, config: RuntimeConfig) -> Self {
        self.config = config;
        self
    }
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.config.model = Some(model.into());
        self
    }
    #[must_use]
    pub fn max_budget_usd(mut self, budget: f64) -> Self {
        self.config.max_budget_usd = Some(budget);
        self
    }
    #[must_use]
    pub fn max_turns(mut self, turns: u32) -> Self {
        self.config.max_turns = Some(turns);
        self
    }

    #[must_use]
    pub fn build(self) -> WorkOrder {
        WorkOrder {
            id: Uuid::new_v4(),
            task: self.task,
            lane: self.lane,
            workspace: WorkspaceSpec {
                root: self.root,
                mode: self.workspace_mode,
                include: self.include,
                exclude: self.exclude,
            },
            context: self.context,
            policy: self.policy,
            requirements: self.requirements,
            config: self.config,
        }
    }
}

impl Receipt {
    /// Compute and attach the canonical SHA-256 hash, returning the updated receipt.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::Json`] if the receipt cannot be serialized.
    pub fn with_hash(mut self) -> Result<Self, ContractError> {
        // Ensure we hash the canonical form (receipt_sha256 treated as null).
        let h = receipt_hash(&self)?;
        self.receipt_sha256 = Some(h);
        Ok(self)
    }
}
