/**
 * Claude → Gemini Mapper
 *
 * Implements the opinionated mapping from Claude-style dialect to Gemini engine.
 * This is the "mapped" mode with early failures for unmappable features.
 *
 * Mapping Strategy:
 * - Direct Mapping (native): File ops, shell, search, web
 * - Emulated Mapping: Memory, checkpointing, hooks via ABP layer
 * - Unsupported (fail early): Agent Teams, extended thinking, Claude-specific features
 */

const {
  SupportLevel,
  GeminiCapabilities,
  TOOL_MAPPING,
  UNSUPPORTED_FEATURES,
  EMULATED_FEATURES,
  getToolMapping,
  getFeatureSupport,
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
    dialect: details.dialect || "claude",
    engine: details.engine || "gemini",
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
 * @param {object} claudeRequest - The Claude-style request
 * @returns {{valid: boolean, errors: array, warnings: array}}
 */
function validateFacade(claudeRequest) {
  const errors = [];
  const warnings = [];

  if (!claudeRequest || typeof claudeRequest !== "object") {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Invalid request: expected object",
        feature: "request_format",
      })
    );
    return { valid: false, errors, warnings };
  }

  // Check for extended thinking
  if (claudeRequest.thinking || claudeRequest.extended_thinking) {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Extended thinking is not supported by Gemini backend",
        feature: "extended_thinking",
        suggestion:
          "Use Gemini's native reasoning capabilities or remove extended_thinking requirement",
      })
    );
  }

  // Check for agent teams
  if (claudeRequest.agent_teams || claudeRequest.teams) {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Agent Teams are not supported by Gemini backend",
        feature: "agent_teams",
        suggestion:
          "Use Gemini's native subagent orchestration or restructure workflow",
      })
    );
  }

  // Check for context compaction settings
  if (claudeRequest.context_compaction || claudeRequest.compaction_threshold) {
    warnings.push({
      level: "warning",
      message:
        "Context compaction settings will be ignored; Gemini has different context management",
      feature: "context_compaction",
      note: "Gemini's 1M context window may reduce need for compaction",
    });
  }

  // Check for Claude-specific session semantics
  if (claudeRequest.session_id && claudeRequest.session_resume === "strict") {
    errors.push(
      createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message:
          "Strict session resume semantics are not directly supported by Gemini",
        feature: "claude_session_semantics",
        suggestion:
          "Use ABP's session emulation layer for compatibility (mode: 'lenient')",
      })
    );
  }

  // Validate tools
  if (Array.isArray(claudeRequest.allowed_tools)) {
    for (const tool of claudeRequest.allowed_tools) {
      const mapping = getToolMapping(tool);
      if (mapping.supportLevel === SupportLevel.Unsupported) {
        errors.push(
          createError(ErrorCodes.UNSUPPORTED_TOOL, {
            message: `Tool '${tool}' has no Gemini equivalent`,
            feature: tool,
            available_alternatives: getToolAlternatives(tool),
          })
        );
      }
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
 * @param {object} mappedRequest - The mapped Gemini request
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
            message: `Capability '${cap}' is not supported by Gemini backend`,
            feature: cap,
          })
        );
        break;
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

  // Always required
  capabilities.add("streaming");
  capabilities.add("tools");

  // Check for web search
  if (mappedRequest.web_search || mappedRequest.grounding) {
    capabilities.add("web_search");
  }

  // Check for code execution
  if (mappedRequest.code_execution) {
    capabilities.add("code_execution");
  }

  // Check for vision
  if (mappedRequest.images || mappedRequest.vision) {
    capabilities.add("vision");
  }

  // Check for structured output
  if (mappedRequest.response_schema || mappedRequest.json_mode) {
    capabilities.add("structured_output");
  }

  // Check for extended thinking (unsupported feature)
  if (mappedRequest.extended_thinking || mappedRequest.thinking) {
    capabilities.add("extended_thinking");
  }

  // Check tools
  if (Array.isArray(mappedRequest.tools)) {
    for (const tool of mappedRequest.tools) {
      if (tool.includes("read") || tool.includes("file")) {
        capabilities.add("tool_read");
      }
      if (tool.includes("write") || tool.includes("edit")) {
        capabilities.add("tool_write");
        capabilities.add("tool_edit");
      }
      if (tool.includes("shell") || tool.includes("bash")) {
        capabilities.add("tool_bash");
      }
      if (tool.includes("glob")) {
        capabilities.add("tool_glob");
      }
      if (tool.includes("grep")) {
        capabilities.add("tool_grep");
      }
      if (tool.includes("web_search")) {
        capabilities.add("tool_web_search");
      }
      if (tool.includes("web_fetch")) {
        capabilities.add("tool_web_fetch");
      }
    }
  }

  // Check for MCP servers
  if (mappedRequest.mcp_servers && Object.keys(mappedRequest.mcp_servers).length > 0) {
    capabilities.add("mcp_client");
  }

  // Check for hooks
  if (mappedRequest.hooks || mappedRequest.pre_tool_hooks || mappedRequest.post_tool_hooks) {
    capabilities.add("hooks_pre_tool_use");
    capabilities.add("hooks_post_tool_use");
  }

  // Check for checkpointing
  if (mappedRequest.checkpointing || mappedRequest.checkpoint_interval) {
    capabilities.add("checkpointing");
  }

  // Check for memory
  if (mappedRequest.memory || mappedRequest.enable_memory) {
    capabilities.add("memory");
  }

  return Array.from(capabilities);
}

// ============================================================================
// Core Mapping Functions
// ============================================================================

/**
 * Map Claude request to Gemini configuration
 * @param {object} claudeRequest - Claude-style request
 * @returns {{geminiRequest: object, mappingWarnings: array, capabilitiesUsed: object}}
 */
function mapClaudeToGemini(claudeRequest) {
  const mappingWarnings = [];
  const capabilitiesUsed = {
    native: [],
    emulated: [],
    unsupported: [],
  };

  // Start with base Gemini request structure
  const geminiRequest = {
    // Direct mappings
    prompt: claudeRequest.prompt || claudeRequest.task,
    cwd: claudeRequest.cwd || process.cwd(),

    // Mapped tool configuration
    tools: [],
    excludeTools: [],

    // Gemini-specific settings
    model: mapModel(claudeRequest.model),
    temperature: claudeRequest.temperature,
    maxOutputTokens: claudeRequest.max_tokens,

    // Sandbox and safety (default settings, will be updated by permission_mode mapping)
    sandbox: {},

    // Context and settings
    context: {},
  };

  // Map allowed_tools to Gemini's coreTools/excludeTools
  if (Array.isArray(claudeRequest.allowed_tools)) {
    const { coreTools, excludeTools, warnings } = mapToolList(claudeRequest.allowed_tools);
    geminiRequest.tools = coreTools;
    geminiRequest.excludeTools = excludeTools;
    mappingWarnings.push(...warnings);
    capabilitiesUsed.native.push("tools");
  }

  // Map permission_mode to sandbox settings
  if (claudeRequest.permission_mode) {
    const sandboxMapping = mapPermissionMode(claudeRequest.permission_mode);
    Object.assign(geminiRequest.sandbox, sandboxMapping.settings);
    if (sandboxMapping.warning) {
      mappingWarnings.push(sandboxMapping.warning);
    }
  }

  // Map setting_sources to GEMINI.md loading
  if (claudeRequest.setting_sources) {
    const contextMapping = mapSettingSources(claudeRequest.setting_sources);
    geminiRequest.context = { ...geminiRequest.context, ...contextMapping.context };
    if (contextMapping.warnings.length > 0) {
      mappingWarnings.push(...contextMapping.warnings);
    }
  }

  // Map MCP servers (direct with validation)
  if (claudeRequest.mcp_servers) {
    const mcpMapping = mapMcpServers(claudeRequest.mcp_servers);
    geminiRequest.mcpServers = mcpMapping.servers;
    capabilitiesUsed.emulated.push("mcp_client");
    if (mcpMapping.warnings.length > 0) {
      mappingWarnings.push(...mcpMapping.warnings);
    }
  }

  // Map hooks to ABP policy layer
  if (claudeRequest.hooks) {
    mappingWarnings.push({
      level: "info",
      message: "Hooks will be emulated via ABP policy enforcement layer",
      feature: "hooks",
    });
    capabilitiesUsed.emulated.push("hooks_pre_tool_use", "hooks_post_tool_use");
    geminiRequest._abp_hooks = claudeRequest.hooks; // ABP internal
  }

  // Map memory settings
  if (claudeRequest.memory || claudeRequest.enable_memory) {
    mappingWarnings.push({
      level: "info",
      message: "Memory will be provided via ABP-owned jailed memory server",
      feature: "memory",
    });
    capabilitiesUsed.emulated.push("memory");
    geminiRequest._abp_memory = true; // ABP internal flag
  }

  // Map checkpointing
  if (claudeRequest.checkpointing) {
    mappingWarnings.push({
      level: "info",
      message: "Checkpointing will be emulated via ABP workspace snapshots",
      feature: "checkpointing",
    });
    capabilitiesUsed.emulated.push("checkpointing");
    geminiRequest._abp_checkpoint = {
      enabled: true,
      interval: claudeRequest.checkpoint_interval || 10, // Default 10 turns
    };
  }

  // Deduplicate capabilities
  capabilitiesUsed.native = [...new Set(capabilitiesUsed.native)];
  capabilitiesUsed.emulated = [...new Set(capabilitiesUsed.emulated)];

  return {
    geminiRequest,
    mappingWarnings,
    capabilitiesUsed,
  };
}

/**
 * Map Claude model to Gemini model
 * @param {string} claudeModel - Claude model identifier
 * @returns {string} Gemini model identifier
 */
function mapModel(claudeModel) {
  const modelMapping = {
    "claude-3-opus": "gemini-2.0-flash-exp",
    "claude-3-sonnet": "gemini-2.0-flash-exp",
    "claude-3-haiku": "gemini-2.0-flash-exp",
    "claude-3-5-sonnet": "gemini-2.0-flash-exp",
    "claude-3-5-haiku": "gemini-2.0-flash-exp",
    "claude-sonnet-4": "gemini-2.0-flash-exp",
    "claude-opus-4": "gemini-2.0-flash-exp",
  };

  if (!claudeModel) {
    return "gemini-2.0-flash-exp"; // Default
  }

  return modelMapping[claudeModel] || claudeModel;
}

/**
 * Map Claude tool list to Gemini's coreTools/excludeTools
 * @param {string[]} allowedTools - Claude allowed tools
 * @returns {{coreTools: string[], excludeTools: string[], warnings: array}}
 */
function mapToolList(allowedTools) {
  const coreTools = [];
  const excludeTools = [];
  const warnings = [];

  // Default excluded tools in Gemini
  const defaultExcluded = ["notebook_read", "notebook_edit"];

  for (const claudeTool of allowedTools) {
    const mapping = getToolMapping(claudeTool);

    if (mapping.supportLevel === SupportLevel.Native) {
      coreTools.push(mapping.geminiTool);
    } else if (mapping.supportLevel === SupportLevel.Unsupported) {
      excludeTools.push(claudeTool.toLowerCase());
      warnings.push({
        level: "warning",
        message: mapping.note || `Tool '${claudeTool}' is not supported`,
        feature: claudeTool,
      });
    } else {
      // Emulated - pass through for ABP handling
      coreTools.push(mapping.geminiTool);
      warnings.push({
        level: "info",
        message: `Tool '${claudeTool}' will be emulated`,
        feature: claudeTool,
      });
    }
  }

  // Add default exclusions
  for (const excluded of defaultExcluded) {
    if (!excludeTools.includes(excluded)) {
      excludeTools.push(excluded);
    }
  }

  return { coreTools, excludeTools, warnings };
}

/**
 * Map Claude permission_mode to Gemini sandbox settings
 * @param {string} permissionMode - Claude permission mode
 * @returns {{settings: object, warning?: object}}
 */
function mapPermissionMode(permissionMode) {
  const mapping = {
    auto: {
      settings: {
        sandbox: true,
        autoApprove: ["read_file", "glob", "grep"],
        requireApproval: ["write_file", "edit_file", "shell"],
      },
    },
    acceptEdits: {
      settings: {
        sandbox: true,
        autoApprove: ["read_file", "glob", "grep", "edit_file"],
        requireApproval: ["write_file", "shell"],
      },
    },
    plan: {
      settings: {
        sandbox: true,
        autoApprove: ["read_file", "glob", "grep"],
        requireApproval: ["write_file", "edit_file", "shell"],
        planningMode: true,
      },
      warning: {
        level: "info",
        message: "Plan mode mapped to Gemini planning mode with similar semantics",
        feature: "permission_mode",
      },
    },
    default: {
      settings: {
        sandbox: true,
        requireApproval: ["write_file", "edit_file", "shell"],
      },
    },
  };

  return mapping[permissionMode] || mapping.default;
}

/**
 * Map Claude setting_sources to Gemini context loading
 * @param {object} settingSources - Claude setting sources
 * @returns {{context: object, warnings: array}}
 */
function mapSettingSources(settingSources) {
  const context = {};
  const warnings = [];

  if (settingSources.project === false) {
    context.ignoreProjectGeminimd = true;
    warnings.push({
      level: "info",
      message: "Project GEMINI.md loading disabled",
      feature: "setting_sources",
    });
  }

  if (settingSources.user === false) {
    context.ignoreUserGeminimd = true;
    warnings.push({
      level: "info",
      message: "User GEMINI.md loading disabled",
      feature: "setting_sources",
    });
  }

  // Map CLAUDE.md to GEMINI.md context
  if (settingSources.claudemd !== false) {
    context.loadClaudemd = true;
    warnings.push({
      level: "info",
      message: "CLAUDE.md will be loaded as additional context",
      feature: "setting_sources",
    });
  }

  return { context, warnings };
}

/**
 * Map MCP servers configuration
 * @param {object} mcpServers - MCP servers config
 * @returns {{servers: object, warnings: array}}
 */
function mapMcpServers(mcpServers) {
  const servers = {};
  const warnings = [];

  for (const [name, config] of Object.entries(mcpServers)) {
    // Validate MCP server config
    if (!config.command && !config.url) {
      warnings.push({
        level: "warning",
        message: `MCP server '${name}' missing command or url`,
        feature: "mcp_servers",
      });
      continue;
    }

    // Pass through with validation
    servers[name] = {
      command: config.command,
      args: config.args || [],
      env: config.env || {},
      url: config.url, // For HTTP-based MCP servers
    };

    if (config.url) {
      warnings.push({
        level: "info",
        message: `MCP server '${name}' using HTTP transport`,
        feature: "mcp_servers",
      });
    }
  }

  return { servers, warnings };
}

/**
 * Get alternative tools for unsupported tools
 * @param {string} tool - Unsupported tool name
 * @returns {string[]} List of alternatives
 */
function getToolAlternatives(tool) {
  const alternatives = {
    NotebookEdit: ["shell (with jq/python for notebook manipulation)"],
    NotebookRead: ["read_file (notebooks are JSON)"],
    Task: ["subagent"],
  };

  return alternatives[tool] || [];
}

// ============================================================================
// Receipt Enhancement
// ============================================================================

/**
 * Create mapped mode receipt additions
 * @param {object} options - Receipt options
 * @returns {object} Receipt additions for mapped mode
 */
function createMappedReceiptAdditions(options = {}) {
  return {
    mode: "mapped",
    source_dialect: "claude",
    target_engine: "gemini",
    mapping_warnings: options.mappingWarnings || [],
    capabilities_used: options.capabilitiesUsed || {
      native: [],
      emulated: [],
      unsupported: [],
    },
    mapping_metadata: {
      mapper_version: "1.0.0",
      mapped_at: new Date().toISOString(),
    },
  };
}

// ============================================================================
// Exports
// ============================================================================

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
  mapClaudeToGemini,
  mapModel,
  mapToolList,
  mapPermissionMode,
  mapSettingSources,
  mapMcpServers,
  getToolAlternatives,

  // Receipt
  createMappedReceiptAdditions,
};
