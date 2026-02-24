/**
 * Codex → Claude Mapper
 *
 * Implements the opinionated mapping from Codex-style dialect to Claude engine.
 * This is the "mapped" mode with early failures for unmappable features.
 *
 * Mapping Strategy:
 * - Direct Mapping (native): File ops, shell, search, web
 * - Emulated Mapping: Thread→Session, code execution via Bash
 * - Unsupported (fail early): Codex thread model, Assistants API, deprecated function_call
 */

const {
  SupportLevel,
  ClaudeCapabilities,
  TOOL_MAPPING,
  MODEL_MAPPING,
  UNSUPPORTED_FEATURES,
  EMULATED_FEATURES,
  getToolMapping,
  getFeatureSupport,
  getModelMapping,
  getToolAlternatives,
} = require("./capabilities");

// ============================================================================
// Error Taxonomy (from docs/dialect_engine_matrix.md)
// ============================================================================

const ErrorCodes = {
  UNSUPPORTED_FEATURE: "E001",
  UNSUPPORTED_TOOL: "E002",
  AMBIGUOUS_MAPPING: "E003",
  REQUIRES_INTERACTIVE_APPROVAL: "E004",
  UNSAFE_BY_POLICY: "E005",
  BACKEND_CAPABILITY_MISSING: "E006",
  BACKEND_UNAVAILABLE: "E007",
};

const ErrorNames = {
  E001: "UnsupportedFeature",
  E002: "UnsupportedTool",
  E003: "AmbiguousMapping",
  E004: "RequiresInteractiveApproval",
  E005: "UnsafeByPolicy",
  E006: "BackendCapabilityMissing",
  E007: "BackendUnavailable",
};

/**
 * Create a mapping error
 * @param {string} code - Error code from ErrorCodes
 * @param {object} details - Error details
 * @returns {object} Structured error
 */
function createError(code, details = {}) {
  return {
    code,
    name: ErrorNames[code] || "UnknownError",
    message: details.message || `Mapping error: ${code}`,
    feature: details.feature || null,
    dialect: details.dialect || "codex",
    engine: details.engine || "claude",
    suggestion: details.suggestion || null,
    available_alternatives: details.available_alternatives || null,
    documentation_url: `https://docs.abp.dev/errors/${code}`,
    timestamp: new Date().toISOString(),
  };
}

// ============================================================================
// Stage 1: Facade Validation
// ============================================================================

/**
 * Validate request at facade level before any backend interaction
 * @param {object} codexRequest - The Codex-style request
 * @returns {{valid: boolean, errors: array, warnings: array}}
 */
function validateFacade(codexRequest) {
  const errors = [];
  const warnings = [];

  if (!codexRequest || typeof codexRequest !== "object") {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Invalid request: expected object",
        feature: "request_format",
      })
    );
    return { valid: false, errors, warnings };
  }

  // Check for deprecated function_call format
  if (codexRequest.function_call || codexRequest.functions) {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Deprecated function_call format is not supported. Use tools instead.",
        feature: "function_call_deprecated",
        suggestion: "Migrate to the tools/tool_choice format which maps directly to Claude",
      })
    );
  }

  // Check for OpenAI Assistants API features
  if (codexRequest.assistant_id || codexRequest.run_id) {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "OpenAI Assistants API features are not directly supported by Claude",
        feature: "codex_assistants_api",
        suggestion: "Use Claude's native tool use and session management instead",
      })
    );
  }

  // Check for Codex-specific thread options that don't map
  if (codexRequest.thread_instructions || codexRequest.additional_instructions) {
    warnings.push({
      level: "warning",
      message: "Thread-level instructions will be merged into system prompt",
      feature: "thread_instructions",
      note: "Claude uses a single system prompt rather than per-thread instructions",
    });
  }

  // Check for Codex code interpreter
  if (codexRequest.code_interpreter || codexRequest.enable_code_interpreter) {
    warnings.push({
      level: "warning",
      message: "Code interpreter will be emulated via Bash tool with sandbox",
      feature: "code_execution",
      note: "Claude doesn't have a built-in code interpreter, but Bash provides equivalent functionality",
    });
  }

  // Check for retrieval / file search
  if (codexRequest.retrieval || codexRequest.file_search) {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Codex retrieval/file_search is not directly supported",
        feature: "codex_retrieval",
        suggestion: "Use MCP servers for retrieval or implement custom search with Grep/Glob",
      })
    );
  }

  // Validate tools
  if (Array.isArray(codexRequest.tools)) {
    for (const tool of codexRequest.tools) {
      const toolName = typeof tool === "string" ? tool : tool.name || tool.type;
      const mapping = getToolMapping(toolName);
      if (mapping.supportLevel === SupportLevel.Unsupported && mapping.claudeTool === null) {
        errors.push(
          createError(ErrorCodes.UNSUPPORTED_TOOL, {
            message: `Tool '${toolName}' has no Claude equivalent`,
            feature: toolName,
            available_alternatives: getToolAlternatives(toolName),
          })
        );
      }
    }
  }

  // Check for ambiguous model mapping
  if (codexRequest.model) {
    const claudeModel = getModelMapping(codexRequest.model);
    if (claudeModel === MODEL_MAPPING.default && !codexRequest.model.startsWith("claude")) {
      warnings.push({
        level: "info",
        message: `Model '${codexRequest.model}' will be mapped to '${claudeModel}'`,
        feature: "model_mapping",
        note: "Capability differences may exist between models",
      });
    }
  }

  return {
    valid: errors.length === 0,
    errors,
    warnings,
  };
}

// ============================================================================
// Stage 2: Runtime Capability Check
// ============================================================================

/**
 * Validate request against actual backend capabilities
 * @param {object} mappedRequest - The mapped Claude request
 * @param {object} backendCapabilities - Actual backend capability manifest
 * @returns {{valid: boolean, errors: array, capabilities_used: object}}
 */
function validateCapabilities(mappedRequest, backendCapabilities) {
  const errors = [];
  const capabilitiesUsed = {
    native: [],
    emulated: [],
    unsupported: [],
  };

  // Check each required capability
  const requiredCapabilities = extractRequiredCapabilities(mappedRequest);

  for (const cap of requiredCapabilities) {
    const supportLevel =
      backendCapabilities[cap] || getFeatureSupport(cap);

    switch (supportLevel) {
      case SupportLevel.Native:
        capabilitiesUsed.native.push(cap);
        break;
      case SupportLevel.Emulated:
        capabilitiesUsed.emulated.push(cap);
        // Check if native is required
        if (mappedRequest.requires_native && mappedRequest.requires_native.includes(cap)) {
          errors.push(
            createError(ErrorCodes.BACKEND_CAPABILITY_MISSING, {
              message: `Capability '${cap}' requires native support but is only emulated`,
              feature: cap,
              suggestion: `Remove '${cap}' from requires_native or accept emulation`,
            })
          );
        }
        break;
      case SupportLevel.Unsupported:
        capabilitiesUsed.unsupported.push(cap);
        errors.push(
          createError(ErrorCodes.BACKEND_CAPABILITY_MISSING, {
            message: `Capability '${cap}' is not supported by Claude backend`,
            feature: cap,
          })
        );
        break;
      default:
        capabilitiesUsed.emulated.push(cap);
    }
  }

  return {
    valid: errors.length === 0,
    errors,
    capabilities_used: capabilitiesUsed,
  };
}

/**
 * Extract required capabilities from a mapped request
 * @param {object} mappedRequest - The mapped request
 * @returns {string[]} List of required capabilities
 */
function extractRequiredCapabilities(mappedRequest) {
  const capabilities = new Set();

  if (!mappedRequest || typeof mappedRequest !== "object") {
    return [];
  }

  // Basic capabilities
  if (mappedRequest.prompt || mappedRequest.messages) {
    capabilities.add("streaming");
  }

  // Tool capabilities
  if (Array.isArray(mappedRequest.allowed_tools)) {
    for (const tool of mappedRequest.allowed_tools) {
      const toolLower = tool.toLowerCase();
      if (toolLower.includes("read") || toolLower === "read") {
        capabilities.add("tool_read");
      }
      if (toolLower.includes("write") || toolLower === "write") {
        capabilities.add("tool_write");
      }
      if (toolLower.includes("edit") || toolLower === "edit") {
        capabilities.add("tool_edit");
      }
      if (toolLower.includes("bash") || toolLower.includes("shell") || toolLower.includes("execute")) {
        capabilities.add("tool_bash");
      }
      if (toolLower.includes("glob")) {
        capabilities.add("tool_glob");
      }
      if (toolLower.includes("grep") || toolLower.includes("search")) {
        capabilities.add("tool_grep");
      }
      if (toolLower.includes("web")) {
        capabilities.add("tool_web_search");
      }
    }
  }

  // Session capabilities
  if (mappedRequest.session_id || mappedRequest.resume) {
    capabilities.add("session_resume");
  }

  // Extended thinking
  if (mappedRequest.extended_thinking || mappedRequest.thinking_budget) {
    capabilities.add("extended_thinking");
  }

  // Hooks
  if (mappedRequest.hooks) {
    capabilities.add("hooks");
  }

  return Array.from(capabilities);
}

// ============================================================================
// Mapping Functions
// ============================================================================

/**
 * Map Codex request to Claude request
 * @param {object} codexRequest - Codex-style request
 * @returns {object} Claude-style request
 */
function mapCodexToClaude(codexRequest) {
  const claudeRequest = {
    // Direct mappings
    prompt: codexRequest.prompt || codexRequest.input || "",
    cwd: codexRequest.cwd || codexRequest.working_directory,
  };

  // Map model
  if (codexRequest.model) {
    claudeRequest.model = getModelMapping(codexRequest.model);
  }

  // Map tools
  if (Array.isArray(codexRequest.tools)) {
    claudeRequest.allowed_tools = mapToolList(codexRequest.tools);
  }

  // Map permission mode
  if (codexRequest.permission_mode || codexRequest.auto_approve) {
    claudeRequest.permission_mode = mapPermissionMode(codexRequest.permission_mode, codexRequest.auto_approve);
  }

  // Handle thread → session mapping
  if (codexRequest.thread_id) {
    claudeRequest.session_id = mapThreadIdToSessionId(codexRequest.thread_id);
    claudeRequest.resume = true;
  }

  // Map instructions to system prompt addition
  if (codexRequest.instructions || codexRequest.additional_instructions) {
    claudeRequest.system_prompt_additions = codexRequest.instructions || codexRequest.additional_instructions;
  }

  // Map code execution
  if (codexRequest.code_interpreter || codexRequest.enable_code_interpreter) {
    // Ensure Bash is in allowed tools
    if (!claudeRequest.allowed_tools) {
      claudeRequest.allowed_tools = [];
    }
    if (!claudeRequest.allowed_tools.includes("Bash")) {
      claudeRequest.allowed_tools.push("Bash");
    }
  }

  // Map temperature and other parameters
  if (codexRequest.temperature !== undefined) {
    claudeRequest.temperature = codexRequest.temperature;
  }
  if (codexRequest.max_tokens !== undefined) {
    claudeRequest.max_tokens = codexRequest.max_tokens;
  }
  if (codexRequest.top_p !== undefined) {
    claudeRequest.top_p = codexRequest.top_p;
  }

  // Map stop sequences
  if (codexRequest.stop) {
    claudeRequest.stop_sequences = Array.isArray(codexRequest.stop) 
      ? codexRequest.stop 
      : [codexRequest.stop];
  }

  // Map JSON mode
  if (codexRequest.response_format?.type === "json_object") {
    claudeRequest.structured_output = true;
    claudeRequest.output_schema = { type: "object" };
  }

  // Map context/files
  if (codexRequest.context || codexRequest.file_ids) {
    claudeRequest.context = {
      files: codexRequest.file_ids || [],
      ...codexRequest.context,
    };
  }

  return claudeRequest;
}

/**
 * Map Codex thread ID to Claude session ID
 * @param {string} threadId - Codex thread ID
 * @returns {string} Claude session ID
 */
function mapThreadIdToSessionId(threadId) {
  // For now, use the thread ID as-is but with a prefix to track origin
  // In a full implementation, this would maintain a mapping table
  return `codex_thread:${threadId}`;
}

/**
 * Map a list of Codex tools to Claude tools
 * @param {Array} codexTools - List of Codex tool names or objects
 * @returns {string[]} List of Claude tool names
 */
function mapToolList(codexTools) {
  const claudeTools = [];

  for (const tool of codexTools) {
    const toolName = typeof tool === "string" ? tool : tool.name || tool.type;
    const mapping = getToolMapping(toolName);

    if (mapping.claudeTool) {
      claudeTools.push(mapping.claudeTool);
    }
    // Skip tools with no mapping (will have been caught by validation)
  }

  // Deduplicate
  return [...new Set(claudeTools)];
}

/**
 * Map Codex permission mode to Claude permission mode
 * @param {string} mode - Codex permission mode
 * @param {Array} autoApprove - Codex auto-approve list
 * @returns {string} Claude permission mode
 */
function mapPermissionMode(mode, autoApprove) {
  // Handle autoApprove array
  if (Array.isArray(autoApprove) && autoApprove.length > 0) {
    if (autoApprove.includes("*") || autoApprove.includes("all")) {
      return "auto";
    }
    // Partial auto-approve maps to "acceptEdits"
    return "acceptEdits";
  }

  // Map named modes
  switch (mode) {
    case "auto":
    case "automatic":
      return "auto";
    case "interactive":
    case "manual":
      return "plan";
    case "semi-auto":
      return "acceptEdits";
    default:
      return "auto"; // Default to auto for Codex compatibility
  }
}

/**
 * Map Codex model name to Claude model
 * @param {string} codexModel - Codex model name
 * @returns {string} Claude model name
 */
function mapModel(codexModel) {
  return getModelMapping(codexModel);
}

// ============================================================================
// Receipt Additions for Mapped Mode
// ============================================================================

/**
 * Create receipt additions for mapped mode
 * @param {object} codexRequest - Original Codex request
 * @param {object} claudeRequest - Mapped Claude request
 * @param {object} validation - Validation result
 * @param {object} sessionMapping - Thread to session mapping
 * @returns {object} Receipt additions
 */
function createMappedReceiptAdditions(codexRequest, claudeRequest, validation, sessionMapping = {}) {
  return {
    mode: "mapped",
    source_dialect: "codex",
    target_engine: "claude",
    mapping_warnings: validation.warnings || [],
    capabilities_used: validation.capabilities_used || {
      native: [],
      emulated: [],
      unsupported: [],
    },
    session_mapping: {
      codex_thread_id: codexRequest.thread_id || null,
      claude_session_id: sessionMapping.claudeSessionId || null,
    },
    model_mapping: {
      original: codexRequest.model || "default",
      mapped: claudeRequest.model || MODEL_MAPPING.default,
    },
    tool_mappings: extractToolMappings(codexRequest.tools, claudeRequest.allowed_tools),
  };
}

/**
 * Extract tool mapping details for receipt
 * @param {Array} codexTools - Original Codex tools
 * @param {Array} claudeTools - Mapped Claude tools
 * @returns {object} Tool mapping details
 */
function extractToolMappings(codexTools, claudeTools) {
  const mappings = [];

  if (!Array.isArray(codexTools)) {
    return mappings;
  }

  for (const tool of codexTools) {
    const toolName = typeof tool === "string" ? tool : tool.name || tool.type;
    const mapping = getToolMapping(toolName);

    mappings.push({
      codex_tool: toolName,
      claude_tool: mapping.claudeTool,
      support_level: mapping.supportLevel,
      note: mapping.note,
    });
  }

  return mappings;
}

// ============================================================================
// Reverse Mapping (Claude → Codex for responses)
// ============================================================================

/**
 * Map Claude response back to Codex format
 * @param {object} claudeResponse - Claude response
 * @param {object} sessionMapping - Session mapping info
 * @returns {object} Codex-style response
 */
function mapClaudeToCodexResponse(claudeResponse, sessionMapping = {}) {
  const codexResponse = {
    id: claudeResponse.id || `codex_${Date.now()}`,
    object: "thread.message",
    created_at: Math.floor(Date.now() / 1000),
    status: "completed",
    content: [],
  };

  // Map content
  if (claudeResponse.content) {
    if (typeof claudeResponse.content === "string") {
      codexResponse.content.push({
        type: "text",
        text: claudeResponse.content,
      });
    } else if (Array.isArray(claudeResponse.content)) {
      codexResponse.content = claudeResponse.content.map(block => mapContentBlock(block));
    }
  }

  // Map tool calls
  if (claudeResponse.tool_calls || claudeResponse.tool_use) {
    codexResponse.tool_calls = (claudeResponse.tool_calls || [claudeResponse.tool_use]).map(tc => ({
      id: tc.id || tc.tool_use_id,
      type: "function",
      function: {
        name: tc.name || tc.tool_name,
        arguments: typeof tc.input === "string" ? tc.input : JSON.stringify(tc.input),
      },
    }));
  }

  // Include thread mapping
  if (sessionMapping.codexThreadId) {
    codexResponse.thread_id = sessionMapping.codexThreadId;
  }

  // Map usage
  if (claudeResponse.usage) {
    codexResponse.usage = {
      prompt_tokens: claudeResponse.usage.input_tokens || 0,
      completion_tokens: claudeResponse.usage.output_tokens || 0,
      total_tokens: claudeResponse.usage.total_tokens || 0,
    };
  }

  return codexResponse;
}

/**
 * Map Claude content block to Codex format
 * @param {object} block - Claude content block
 * @returns {object} Codex content block
 */
function mapContentBlock(block) {
  if (!block || typeof block !== "object") {
    return { type: "text", text: String(block) };
  }

  switch (block.type) {
    case "text":
      return {
        type: "text",
        text: block.text || "",
      };
    case "tool_use":
      return {
        type: "tool_use",
        id: block.id,
        name: block.name,
        input: block.input,
      };
    case "tool_result":
      return {
        type: "tool_result",
        tool_use_id: block.tool_use_id,
        content: block.content,
        is_error: block.is_error || false,
      };
    default:
      return block;
  }
}

module.exports = {
  // Error handling
  ErrorCodes,
  ErrorNames,
  createError,

  // Validation
  validateFacade,
  validateCapabilities,
  extractRequiredCapabilities,

  // Mapping
  mapCodexToClaude,
  mapThreadIdToSessionId,
  mapToolList,
  mapPermissionMode,
  mapModel,

  // Receipt
  createMappedReceiptAdditions,

  // Reverse mapping
  mapClaudeToCodexResponse,
  mapContentBlock,
};
