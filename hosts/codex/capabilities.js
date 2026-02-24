/**
 * Codex Capability Manifest
 *
 * Defines the capabilities of the Codex backend and their support levels
 * when mapping from Codex-style dialect to Claude engine.
 *
 * Support Levels:
 * - native: Feature is supported directly by Claude
 * - emulated: Feature can be emulated with acceptable fidelity via ABP
 * - unsupported: Feature cannot be mapped and will cause early failure
 */

const SupportLevel = {
  Native: "native",
  Emulated: "emulated",
  Unsupported: "unsupported",
};

/**
 * Claude backend capability manifest (for Codex→Claude mapping)
 */
const ClaudeCapabilities = {
  // Streaming and basic features
  streaming: SupportLevel.Native,
  tools: SupportLevel.Native,
  vision: SupportLevel.Native,
  structured_output: SupportLevel.Native,

  // Tool mappings (Codex → Claude)
  tool_read: SupportLevel.Native, // Read
  tool_write: SupportLevel.Native, // Write
  tool_edit: SupportLevel.Native, // Edit
  tool_bash: SupportLevel.Native, // Bash
  tool_glob: SupportLevel.Native, // Glob
  tool_grep: SupportLevel.Native, // Grep
  tool_web_search: SupportLevel.Native, // WebSearch
  tool_web_fetch: SupportLevel.Native, // WebFetch

  // Native Claude features
  extended_thinking: SupportLevel.Native, // Claude-specific
  agent_teams: SupportLevel.Native, // Claude subagents
  hooks: SupportLevel.Native, // Pre/post tool hooks

  // Emulated features (via ABP layer)
  thread_resume: SupportLevel.Emulated, // Codex thread → Claude session mapping
  code_execution: SupportLevel.Emulated, // Via Bash tool
  json_mode: SupportLevel.Emulated, // Via structured output

  // Codex-specific features that DON'T map to Claude
  codex_thread_model: SupportLevel.Unsupported, // Different session model
  codex_assistants_api: SupportLevel.Unsupported, // OpenAI Assistants API specific
  function_calling_deprecated: SupportLevel.Unsupported, // Old function_call format
};

/**
 * Tool name mapping from Codex to Claude
 * This is the REVERSE of the Claude→Gemini mapping
 */
const TOOL_MAPPING = {
  // File operations (Codex naming → Claude naming)
  read_file: "Read",
  write_file: "Write",
  edit_file: "Edit",
  file_read: "Read",
  file_write: "Write",

  // Shell and execution
  code_execution: "Bash",
  shell: "Bash",
  execute: "Bash",
  run_command: "Bash",

  // Search
  glob: "Glob",
  grep: "Grep",
  search_files: "Grep",

  // Web
  web_search: "WebSearch",
  web_fetch: "WebFetch",
  browser: "WebFetch",

  // Memory (emulated via ABP)
  memory: "Memory",

  // Task/Agent delegation
  subagent: "Task",
  delegate: "Task",

  // Other common tools
  todo_write: "TodoWrite",
  stop: "Stop",
};

/**
 * Features that are unsupported and will cause early failure
 */
const UNSUPPORTED_FEATURES = [
  {
    feature: "codex_thread_model",
    reason: "Codex thread model differs from Claude session model",
    suggestion: "Use ABP's thread-to-session mapping or restructure to use Claude sessions",
  },
  {
    feature: "codex_assistants_api",
    reason: "OpenAI Assistants API features are not directly supported by Claude",
    suggestion: "Use Claude's native tool use and session management instead",
  },
  {
    feature: "function_call_deprecated",
    reason: "Legacy function_call format is deprecated and not supported",
    suggestion: "Use the newer tool_use format which maps directly to Claude",
  },
  {
    feature: "codex_code_interpreter",
    reason: "Codex code interpreter has different semantics than Claude tools",
    suggestion: "Use Claude's Bash tool for code execution via ABP emulation",
  },
  {
    feature: "codex_retrieval",
    reason: "Codex retrieval API is specific to OpenAI infrastructure",
    suggestion: "Use Claude's native context handling or MCP servers for retrieval",
  },
];

/**
 * Features that are emulated by ABP
 */
const EMULATED_FEATURES = [
  {
    feature: "thread_resume",
    emulation: "Thread ID to Session ID mapping",
    fidelity: "high",
    notes: "Codex thread IDs mapped to Claude session IDs via ABP tracking",
  },
  {
    feature: "code_execution",
    emulation: "Bash tool with sandbox",
    fidelity: "high",
    notes: "Code execution routed through Claude's Bash tool with ABP policy",
  },
  {
    feature: "json_mode",
    emulation: "Structured output via prompt engineering",
    fidelity: "medium",
    notes: "JSON mode emulated via prompt instructions and response parsing",
  },
  {
    feature: "streaming_deltas",
    emulation: "Content block streaming",
    fidelity: "high",
    notes: "Codex delta format translated to Claude content blocks",
  },
];

/**
 * Model mapping from Codex to Claude
 */
const MODEL_MAPPING = {
  // GPT-4 models → Claude 3.5 Sonnet (comparable capability)
  "gpt-4": "claude-3-5-sonnet-20241022",
  "gpt-4-turbo": "claude-3-5-sonnet-20241022",
  "gpt-4-turbo-preview": "claude-3-5-sonnet-20241022",
  "gpt-4o": "claude-3-5-sonnet-20241022",
  "gpt-4o-mini": "claude-3-5-haiku-20241022",
  
  // GPT-3.5 → Claude 3.5 Haiku (comparable speed/cost)
  "gpt-3.5-turbo": "claude-3-5-haiku-20241022",
  "gpt-3.5-turbo-16k": "claude-3-5-haiku-20241022",
  
  // o1 models → Claude with extended thinking
  "o1": "claude-3-5-sonnet-20241022",
  "o1-preview": "claude-3-5-sonnet-20241022",
  "o1-mini": "claude-3-5-haiku-20241022",
  
  // Codex-specific models
  "codex-davinci": "claude-3-5-sonnet-20241022",
  "codex-cushman": "claude-3-5-haiku-20241022",
  
  // Default fallback
  "default": "claude-3-5-sonnet-20241022",
};

/**
 * Get the Claude tool name for a Codex tool
 * @param {string} codexTool - Codex tool name
 * @returns {{claudeTool: string|null, supportLevel: string, note?: string}}
 */
function getToolMapping(codexTool) {
  // Normalize tool name
  const normalizedTool = codexTool.toLowerCase().replace(/[-_]/g, "_");
  
  // Check direct mapping
  const mapped = TOOL_MAPPING[normalizedTool] || TOOL_MAPPING[codexTool];
  
  if (mapped) {
    return {
      claudeTool: mapped,
      supportLevel: SupportLevel.Native,
    };
  }
  
  // Check if it's a known Claude tool (passthrough)
  const claudeTools = ["Read", "Write", "Edit", "MultiEdit", "Bash", "Glob", "Grep", 
                       "WebSearch", "WebFetch", "Memory", "Task", "TodoWrite", "Stop"];
  if (claudeTools.includes(codexTool)) {
    return {
      claudeTool: codexTool,
      supportLevel: SupportLevel.Native,
      note: "Tool is already in Claude format",
    };
  }
  
  // Unknown tool - check if it's a custom MCP tool
  return {
    claudeTool: codexTool, // Pass through as-is
    supportLevel: SupportLevel.Emulated,
    note: "Unknown tool, passed through for MCP handling",
  };
}

/**
 * Check if a feature is supported
 * @param {string} feature - Feature name
 * @returns {string} Support level
 */
function getFeatureSupport(feature) {
  return ClaudeCapabilities[feature] || SupportLevel.Emulated;
}

/**
 * Get the Claude model for a Codex model name
 * @param {string} codexModel - Codex model name
 * @returns {string} Claude model name
 */
function getModelMapping(codexModel) {
  if (!codexModel) {
    return MODEL_MAPPING.default;
  }
  
  // Check exact match
  if (MODEL_MAPPING[codexModel]) {
    return MODEL_MAPPING[codexModel];
  }
  
  // Check partial match (e.g., gpt-4-0125-preview)
  const normalizedModel = codexModel.toLowerCase();
  for (const [key, value] of Object.entries(MODEL_MAPPING)) {
    if (normalizedModel.startsWith(key.toLowerCase())) {
      return value;
    }
  }
  
  // Default to Claude 3.5 Sonnet
  return MODEL_MAPPING.default;
}

/**
 * Get the complete capability manifest
 * @returns {object} Capability manifest
 */
function getCapabilityManifest() {
  return {
    backend: "claude",
    dialect: "codex",
    version: "0.1.0",
    capabilities: { ...ClaudeCapabilities },
    tool_mapping: { ...TOOL_MAPPING },
    model_mapping: { ...MODEL_MAPPING },
    unsupported_features: [...UNSUPPORTED_FEATURES],
    emulated_features: [...EMULATED_FEATURES],
  };
}

/**
 * Get alternatives for an unsupported tool
 * @param {string} tool - Tool name
 * @returns {string[]} Alternative tools
 */
function getToolAlternatives(tool) {
  const alternatives = {
    notebook_edit: ["Edit (for raw notebook JSON)", "Bash (with jupyter CLI)"],
    notebook_read: ["Read (for raw notebook JSON)", "Bash (with jupyter CLI)"],
    code_interpreter: ["Bash (with sandbox)", "Task (for complex execution)"],
    retrieval: ["MCP servers", "Grep/Glob for local files", "WebFetch for remote"],
  };
  
  return alternatives[tool.toLowerCase().replace(/[-_]/g, "_")] || [];
}

module.exports = {
  SupportLevel,
  ClaudeCapabilities,
  TOOL_MAPPING,
  MODEL_MAPPING,
  UNSUPPORTED_FEATURES,
  EMULATED_FEATURES,
  getToolMapping,
  getFeatureSupport,
  getModelMapping,
  getCapabilityManifest,
  getToolAlternatives,
};
