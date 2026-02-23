/**
 * Gemini Capability Manifest
 *
 * Defines the capabilities of the Gemini backend and their support levels
 * when mapping from Claude-style dialect.
 *
 * Support Levels:
 * - native: Feature is supported directly by Gemini
 * - emulated: Feature can be emulated with acceptable fidelity via ABP
 * - unsupported: Feature cannot be mapped and will cause early failure
 */

const SupportLevel = {
  Native: "native",
  Emulated: "emulated",
  Unsupported: "unsupported",
};

/**
 * Gemini backend capability manifest
 */
const GeminiCapabilities = {
  // Streaming and basic features
  streaming: SupportLevel.Native,
  tools: SupportLevel.Native,
  vision: SupportLevel.Native,
  structured_output: SupportLevel.Native,

  // Tool mappings (Claude → Gemini)
  tool_read: SupportLevel.Native, // read_file
  tool_write: SupportLevel.Native, // write_file
  tool_edit: SupportLevel.Native, // edit_file
  tool_bash: SupportLevel.Native, // shell
  tool_glob: SupportLevel.Native, // glob
  tool_grep: SupportLevel.Native, // grep
  tool_web_search: SupportLevel.Native, // web_search (with grounding)
  tool_web_fetch: SupportLevel.Native, // web_fetch

  // Native Gemini features
  web_search: SupportLevel.Native, // via grounding
  code_execution: SupportLevel.Native, // built-in code exec
  json_mode: SupportLevel.Native,

  // Emulated features (via ABP layer)
  hooks_pre_tool_use: SupportLevel.Emulated, // ABP policy layer
  hooks_post_tool_use: SupportLevel.Emulated, // ABP policy layer
  session_resume: SupportLevel.Emulated, // via checkpoint files
  checkpointing: SupportLevel.Emulated, // via workspace snapshots
  mcp_client: SupportLevel.Emulated, // ABP-managed MCP
  memory: SupportLevel.Emulated, // ABP-owned jailed memory server

  // Claude-specific features that DON'T map to Gemini
  extended_thinking: SupportLevel.Unsupported, // Gemini has different reasoning
  agent_teams: SupportLevel.Unsupported, // Different subagent model
  context_compaction: SupportLevel.Unsupported, // Different mechanism
  file_checkpointing: SupportLevel.Unsupported, // Must use workspace snapshots
  claude_session_semantics: SupportLevel.Unsupported, // Different session model
};

/**
 * Tool name mapping from Claude to Gemini
 */
const TOOL_MAPPING = {
  // File operations
  Read: "read_file",
  Write: "write_file",
  Edit: "edit_file",
  MultiEdit: "edit_file", // Maps to multiple edit_file calls

  // Shell and execution
  Bash: "shell",
  "Bash.Output": "shell", // Long-running bash variant

  // Search
  Glob: "glob",
  Grep: "grep",

  // Web
  WebSearch: "web_search",
  WebFetch: "web_fetch",

  // Memory (emulated via ABP)
  Memory: "memory", // ABP-owned jailed memory server

  // Notebook (not directly mappable)
  NotebookEdit: null, // Unsupported
  NotebookRead: null, // Unsupported

  // Task/Agent delegation
  Task: "subagent", // Maps to Gemini subagent (different semantics)

  // Other common tools
  TodoWrite: "todo_write", // Maps to internal state
  Stop: "stop", // Maps to completion signal
};

/**
 * Features that are unsupported and will cause early failure
 */
const UNSUPPORTED_FEATURES = [
  {
    feature: "extended_thinking",
    reason: "Extended thinking is a Claude-specific feature not available in Gemini",
    suggestion: "Use Gemini's native reasoning capabilities or remove extended_thinking requirement",
  },
  {
    feature: "agent_teams",
    reason: "Gemini uses a different subagent model than Claude's Agent Teams",
    suggestion: "Use Gemini's native subagent orchestration or restructure workflow",
  },
  {
    feature: "context_compaction",
    reason: "Gemini has different context management mechanisms",
    suggestion: "Rely on Gemini's 1M context window or implement custom summarization",
  },
  {
    feature: "claude_session_semantics",
    reason: "Session semantics differ between Claude and Gemini",
    suggestion: "Use ABP's session emulation layer for compatibility",
  },
  {
    feature: "notebook_edit",
    reason: "Jupyter notebook operations are not supported in Gemini CLI",
    suggestion: "Use shell commands to manipulate notebook files directly",
  },
];

/**
 * Features that are emulated by ABP
 */
const EMULATED_FEATURES = [
  {
    feature: "hooks",
    emulation: "ABP policy enforcement layer",
    fidelity: "high",
    notes: "Pre/post tool hooks implemented via ABP interception",
  },
  {
    feature: "checkpointing",
    emulation: "ABP workspace snapshots",
    fidelity: "medium",
    notes: "Workspace state saved at reasoning boundaries",
  },
  {
    feature: "memory",
    emulation: "ABP-owned jailed memory server",
    fidelity: "high",
    notes: "Memory operations routed through ABP-managed MCP server",
  },
  {
    feature: "session_resume",
    emulation: "Checkpoint file restoration",
    fidelity: "medium",
    notes: "Session state restored from saved checkpoints",
  },
  {
    feature: "mcp_client",
    emulation: "ABP MCP gateway",
    fidelity: "high",
    notes: "MCP servers managed by ABP, exposed to Gemini",
  },
];

/**
 * Get the Gemini tool name for a Claude tool
 * @param {string} claudeTool - Claude tool name
 * @returns {{geminiTool: string|null, supportLevel: string, note?: string}}
 */
function getToolMapping(claudeTool) {
  const mapped = TOOL_MAPPING[claudeTool];

  if (mapped === undefined) {
    // Unknown tool - check if it's a custom MCP tool
    return {
      geminiTool: claudeTool, // Pass through as-is
      supportLevel: SupportLevel.Emulated,
      note: "Unknown tool, passed through for MCP handling",
    };
  }

  if (mapped === null) {
    return {
      geminiTool: null,
      supportLevel: SupportLevel.Unsupported,
      note: `Tool '${claudeTool}' has no Gemini equivalent`,
    };
  }

  return {
    geminiTool: mapped,
    supportLevel: SupportLevel.Native,
  };
}

/**
 * Check if a feature is supported
 * @param {string} feature - Feature name
 * @returns {string} Support level
 */
function getFeatureSupport(feature) {
  return GeminiCapabilities[feature] || SupportLevel.Unsupported;
}

/**
 * Get the full capability manifest for the Gemini backend
 * @returns {object}
 */
function getCapabilityManifest() {
  return {
    backend: "gemini",
    version: "1.0.0",
    capabilities: { ...GeminiCapabilities },
    tool_mapping: { ...TOOL_MAPPING },
  };
}

module.exports = {
  SupportLevel,
  GeminiCapabilities,
  TOOL_MAPPING,
  UNSUPPORTED_FEATURES,
  EMULATED_FEATURES,
  getToolMapping,
  getFeatureSupport,
  getCapabilityManifest,
};
