/**
 * Mapped Mode Conformance Tests
 *
 * Tests for Claude→Gemini mapped mode implementation.
 * Verifies:
 * 1. Supported features map correctly
 * 2. Unsupported features fail early with correct error code
 * 3. Emulated features are marked as such in receipt
 * 4. Two-stage validation works
 */

const assert = require("node:assert");
const { describe, it, beforeEach, afterEach } = require("node:test");

const {
  SupportLevel,
  GeminiCapabilities,
  TOOL_MAPPING,
  UNSUPPORTED_FEATURES,
  getToolMapping,
  getFeatureSupport,
  getCapabilityManifest,
} = require("../capabilities");

const {
  ErrorCodes,
  ErrorNames,
  createError,
  validateFacade,
  validateCapabilities,
  extractRequiredCapabilities,
  mapClaudeToGemini,
  mapModel,
  mapToolList,
  mapPermissionMode,
  createMappedReceiptAdditions,
} = require("../mapper");

const { createMockAdapter } = require("./mock-adapter");

// ============================================================================
// Test Fixtures
// ============================================================================

const validClaudeRequest = {
  prompt: "Read the main.rs file and explain it",
  cwd: "/workspace/project",
  allowed_tools: ["Read", "Glob", "Grep"],
  permission_mode: "auto",
};

const requestWithExtendedThinking = {
  prompt: "Think carefully about this problem",
  extended_thinking: true,
  thinking_budget: 10000,
};

const requestWithAgentTeams = {
  prompt: "Coordinate multiple agents",
  agent_teams: [
    { name: "researcher", role: "research" },
    { name: "writer", role: "write" },
  ],
};

const requestWithHooks = {
  prompt: "Do something",
  hooks: {
    pre_tool_use: ["validate_input"],
    post_tool_use: ["log_result"],
  },
};

const requestWithMemory = {
  prompt: "Remember this",
  memory: true,
  enable_memory: true,
};

const requestWithCheckpointing = {
  prompt: "Long running task",
  checkpointing: true,
  checkpoint_interval: 5,
};

const requestWithMcpServers = {
  prompt: "Use MCP tools",
  mcp_servers: {
    filesystem: {
      command: "mcp-filesystem",
      args: ["/workspace"],
    },
    github: {
      url: "https://mcp.github.com/sse",
    },
  },
};

// ============================================================================
// Capability Tests
// ============================================================================

describe("Capabilities", () => {
  describe("getToolMapping", () => {
    it("should map Read to read_file", () => {
      const result = getToolMapping("Read");
      assert.strictEqual(result.geminiTool, "read_file");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should map Write to write_file", () => {
      const result = getToolMapping("Write");
      assert.strictEqual(result.geminiTool, "write_file");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should map Bash to shell", () => {
      const result = getToolMapping("Bash");
      assert.strictEqual(result.geminiTool, "shell");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should return unsupported for NotebookEdit", () => {
      const result = getToolMapping("NotebookEdit");
      assert.strictEqual(result.geminiTool, null);
      assert.strictEqual(result.supportLevel, SupportLevel.Unsupported);
    });

    it("should pass through unknown tools as emulated", () => {
      const result = getToolMapping("custom_mcp_tool");
      assert.strictEqual(result.geminiTool, "custom_mcp_tool");
      assert.strictEqual(result.supportLevel, SupportLevel.Emulated);
    });
  });

  describe("getFeatureSupport", () => {
    it("should return native for streaming", () => {
      assert.strictEqual(getFeatureSupport("streaming"), SupportLevel.Native);
    });

    it("should return native for code_execution", () => {
      assert.strictEqual(getFeatureSupport("code_execution"), SupportLevel.Native);
    });

    it("should return emulated for hooks_pre_tool_use", () => {
      assert.strictEqual(getFeatureSupport("hooks_pre_tool_use"), SupportLevel.Emulated);
    });

    it("should return unsupported for extended_thinking", () => {
      assert.strictEqual(getFeatureSupport("extended_thinking"), SupportLevel.Unsupported);
    });
  });

  describe("getCapabilityManifest", () => {
    it("should return complete manifest", () => {
      const manifest = getCapabilityManifest();
      assert.strictEqual(manifest.backend, "gemini");
      assert.ok(manifest.capabilities);
      assert.ok(manifest.tool_mapping);
    });
  });
});

// ============================================================================
// Error Taxonomy Tests
// ============================================================================

describe("Error Taxonomy", () => {
  describe("ErrorCodes", () => {
    it("should have correct code for UNSUPPORTED_FEATURE", () => {
      assert.strictEqual(ErrorCodes.UNSUPPORTED_FEATURE, "E001");
    });

    it("should have correct code for UNSUPPORTED_TOOL", () => {
      assert.strictEqual(ErrorCodes.UNSUPPORTED_TOOL, "E002");
    });

    it("should have correct code for BACKEND_CAPABILITY_MISSING", () => {
      assert.strictEqual(ErrorCodes.BACKEND_CAPABILITY_MISSING, "E006");
    });
  });

  describe("createError", () => {
    it("should create structured error with all fields", () => {
      const error = createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Extended thinking not supported",
        feature: "extended_thinking",
        suggestion: "Use native Gemini reasoning",
      });

      assert.strictEqual(error.code, "E001");
      assert.strictEqual(error.name, "UnsupportedFeature");
      assert.strictEqual(error.feature, "extended_thinking");
      assert.ok(error.message);
      assert.ok(error.timestamp);
      assert.ok(error.documentation_url);
    });

    it("should include available alternatives for tool errors", () => {
      const error = createError(ErrorCodes.UNSUPPORTED_TOOL, {
        message: "NotebookEdit not available",
        feature: "NotebookEdit",
        available_alternatives: ["shell with jq"],
      });

      assert.strictEqual(error.code, "E002");
      assert.deepStrictEqual(error.available_alternatives, ["shell with jq"]);
    });
  });
});

// ============================================================================
// Stage 1: Facade Validation Tests
// ============================================================================

describe("Facade Validation", () => {
  describe("validateFacade", () => {
    it("should pass valid requests", () => {
      const result = validateFacade(validClaudeRequest);
      assert.strictEqual(result.valid, true);
      assert.deepStrictEqual(result.errors, []);
    });

    it("should fail on extended_thinking", () => {
      const result = validateFacade(requestWithExtendedThinking);
      assert.strictEqual(result.valid, false);
      assert.strictEqual(result.errors.length, 1);
      assert.strictEqual(result.errors[0].code, ErrorCodes.UNSUPPORTED_FEATURE);
      assert.strictEqual(result.errors[0].feature, "extended_thinking");
    });

    it("should fail on agent_teams", () => {
      const result = validateFacade(requestWithAgentTeams);
      assert.strictEqual(result.valid, false);
      assert.strictEqual(result.errors.length, 1);
      assert.strictEqual(result.errors[0].code, ErrorCodes.UNSUPPORTED_FEATURE);
      assert.strictEqual(result.errors[0].feature, "agent_teams");
    });

    it("should warn on context_compaction", () => {
      const result = validateFacade({
        prompt: "test",
        context_compaction: true,
      });
      assert.strictEqual(result.valid, true);
      assert.strictEqual(result.warnings.length, 1);
      assert.ok(result.warnings[0].message.includes("compaction"));
    });

    it("should fail on strict session resume", () => {
      const result = validateFacade({
        prompt: "test",
        session_id: "sess_123",
        session_resume: "strict",
      });
      assert.strictEqual(result.valid, false);
      assert.strictEqual(result.errors[0].feature, "claude_session_semantics");
    });

    it("should fail on unsupported tools in allowed_tools", () => {
      const result = validateFacade({
        prompt: "test",
        allowed_tools: ["Read", "NotebookEdit"],
      });
      assert.strictEqual(result.valid, false);
      assert.strictEqual(result.errors[0].code, ErrorCodes.UNSUPPORTED_TOOL);
    });

    it("should fail on invalid request format", () => {
      const result = validateFacade(null);
      assert.strictEqual(result.valid, false);
      assert.strictEqual(result.errors[0].code, ErrorCodes.UNSUPPORTED_FEATURE);
    });
  });
});

// ============================================================================
// Stage 2: Capability Validation Tests
// ============================================================================

describe("Capability Validation", () => {
  describe("extractRequiredCapabilities", () => {
    it("should extract streaming and tools as base requirements", () => {
      const caps = extractRequiredCapabilities({ tools: [] });
      assert.ok(caps.includes("streaming"));
      assert.ok(caps.includes("tools"));
    });

    it("should extract web_search capability", () => {
      const caps = extractRequiredCapabilities({ web_search: true });
      assert.ok(caps.includes("web_search"));
    });

    it("should extract code_execution capability", () => {
      const caps = extractRequiredCapabilities({ code_execution: true });
      assert.ok(caps.includes("code_execution"));
    });

    it("should extract vision capability", () => {
      const caps = extractRequiredCapabilities({ images: true });
      assert.ok(caps.includes("vision"));
    });

    it("should extract mcp_client capability", () => {
      const caps = extractRequiredCapabilities({
        mcp_servers: { test: { command: "test" } },
      });
      assert.ok(caps.includes("mcp_client"));
    });

    it("should extract hook capabilities", () => {
      const caps = extractRequiredCapabilities({ hooks: true });
      assert.ok(caps.includes("hooks_pre_tool_use"));
      assert.ok(caps.includes("hooks_post_tool_use"));
    });

    it("should extract checkpointing capability", () => {
      const caps = extractRequiredCapabilities({ checkpointing: true });
      assert.ok(caps.includes("checkpointing"));
    });
  });

  describe("validateCapabilities", () => {
    it("should pass when all capabilities are native", () => {
      const result = validateCapabilities(
        { tools: ["read_file"] },
        GeminiCapabilities
      );
      assert.strictEqual(result.valid, true);
    });

    it("should pass when capabilities are emulated", () => {
      const result = validateCapabilities(
        { hooks: true },
        GeminiCapabilities
      );
      assert.strictEqual(result.valid, true);
      assert.ok(result.capabilities_used.emulated.includes("hooks_pre_tool_use"));
    });

    it("should fail when capability is unsupported", () => {
      const result = validateCapabilities(
        { extended_thinking: true },
        GeminiCapabilities
      );
      assert.strictEqual(result.valid, false);
      assert.ok(result.capabilities_used.unsupported.includes("extended_thinking"));
    });

    it("should categorize capabilities correctly", () => {
      const result = validateCapabilities(
        {
          tools: ["read_file"],
          hooks: true,
          web_search: true,
        },
        GeminiCapabilities
      );

      assert.ok(result.capabilities_used.native.includes("streaming"));
      assert.ok(result.capabilities_used.native.includes("web_search"));
      assert.ok(result.capabilities_used.emulated.includes("hooks_pre_tool_use"));
    });
  });
});

// ============================================================================
// Mapping Tests
// ============================================================================

describe("Mapping", () => {
  describe("mapClaudeToGemini", () => {
    it("should map prompt directly", () => {
      const { geminiRequest } = mapClaudeToGemini(validClaudeRequest);
      assert.strictEqual(geminiRequest.prompt, validClaudeRequest.prompt);
    });

    it("should map cwd directly", () => {
      const { geminiRequest } = mapClaudeToGemini(validClaudeRequest);
      assert.strictEqual(geminiRequest.cwd, validClaudeRequest.cwd);
    });

    it("should map allowed_tools to tools array", () => {
      const { geminiRequest } = mapClaudeToGemini(validClaudeRequest);
      assert.ok(geminiRequest.tools.includes("read_file"));
      assert.ok(geminiRequest.tools.includes("glob"));
      assert.ok(geminiRequest.tools.includes("grep"));
    });

    it("should include mapping warnings for hooks", () => {
      const { mappingWarnings } = mapClaudeToGemini(requestWithHooks);
      const hookWarning = mappingWarnings.find((w) => w.feature === "hooks");
      assert.ok(hookWarning);
      assert.ok(hookWarning.message.includes("emulated"));
    });

    it("should mark emulated capabilities for memory", () => {
      const { capabilitiesUsed } = mapClaudeToGemini(requestWithMemory);
      assert.ok(capabilitiesUsed.emulated.includes("memory"));
    });

    it("should mark emulated capabilities for checkpointing", () => {
      const { capabilitiesUsed } = mapClaudeToGemini(requestWithCheckpointing);
      assert.ok(capabilitiesUsed.emulated.includes("checkpointing"));
    });

    it("should map MCP servers with validation", () => {
      const { geminiRequest, mappingWarnings } = mapClaudeToGemini(requestWithMcpServers);
      assert.ok(geminiRequest.mcpServers);
      assert.ok(geminiRequest.mcpServers.filesystem);
      assert.ok(geminiRequest.mcpServers.github);
    });

    it("should set ABP internal flags for emulated features", () => {
      const { geminiRequest } = mapClaudeToGemini(requestWithHooks);
      assert.ok(geminiRequest._abp_hooks);
    });
  });

  describe("mapModel", () => {
    it("should map claude-3-opus to gemini", () => {
      const result = mapModel("claude-3-opus");
      assert.ok(result.startsWith("gemini"));
    });

    it("should return default for unknown models", () => {
      const result = mapModel("unknown-model");
      assert.strictEqual(result, "unknown-model");
    });

    it("should return default when no model specified", () => {
      const result = mapModel(null);
      assert.ok(result.startsWith("gemini"));
    });
  });

  describe("mapToolList", () => {
    it("should map native tools", () => {
      const { coreTools, warnings } = mapToolList(["Read", "Write", "Bash"]);
      assert.ok(coreTools.includes("read_file"));
      assert.ok(coreTools.includes("write_file"));
      assert.ok(coreTools.includes("shell"));
    });

    it("should exclude unsupported tools", () => {
      const { excludeTools, warnings } = mapToolList(["Read", "NotebookEdit"]);
      assert.ok(excludeTools.includes("notebook_edit"));
      assert.ok(warnings.some((w) => w.feature === "NotebookEdit"));
    });
  });

  describe("mapPermissionMode", () => {
    it("should map auto mode", () => {
      const { settings } = mapPermissionMode("auto");
      assert.strictEqual(settings.sandbox, true);
      assert.ok(settings.autoApprove.includes("read_file"));
    });

    it("should map plan mode", () => {
      const { settings, warning } = mapPermissionMode("plan");
      assert.strictEqual(settings.planningMode, true);
      assert.ok(warning);
    });

    it("should use default for unknown mode", () => {
      const { settings } = mapPermissionMode("unknown");
      assert.strictEqual(settings.sandbox, true);
    });
  });
});

// ============================================================================
// Receipt Tests
// ============================================================================

describe("Receipt", () => {
  describe("createMappedReceiptAdditions", () => {
    it("should include mode information", () => {
      const receipt = createMappedReceiptAdditions({});
      assert.strictEqual(receipt.mode, "mapped");
      assert.strictEqual(receipt.source_dialect, "claude");
      assert.strictEqual(receipt.target_engine, "gemini");
    });

    it("should include mapping warnings", () => {
      const warnings = [{ level: "info", message: "test warning" }];
      const receipt = createMappedReceiptAdditions({ mappingWarnings: warnings });
      assert.deepStrictEqual(receipt.mapping_warnings, warnings);
    });

    it("should include capabilities used", () => {
      const capabilitiesUsed = {
        native: ["streaming"],
        emulated: ["hooks"],
        unsupported: [],
      };
      const receipt = createMappedReceiptAdditions({ capabilitiesUsed });
      assert.deepStrictEqual(receipt.capabilities_used, capabilitiesUsed);
    });

    it("should include mapping metadata", () => {
      const receipt = createMappedReceiptAdditions({});
      assert.ok(receipt.mapping_metadata);
      assert.ok(receipt.mapping_metadata.mapper_version);
      assert.ok(receipt.mapping_metadata.mapped_at);
    });
  });
});

// ============================================================================
// Integration Tests with Mock Adapter
// ============================================================================

describe("Integration", () => {
  describe("Mock Adapter", () => {
    it("should create adapter with custom responses", async () => {
      const adapter = createMockAdapter({
        responses: [{ text: "Hello from Gemini" }],
      });

      const events = [];
      const ctx = {
        workOrder: { task: "test" },
        sdkOptions: {},
        emitAssistantDelta: (text) => events.push({ type: "delta", text }),
        emitAssistantMessage: (text) => events.push({ type: "message", text }),
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        log: () => {},
      };

      await adapter.run(ctx);

      assert.ok(events.some((e) => e.type === "message"));
      assert.ok(events.some((e) => e.text.includes("Hello from Gemini")));
    });

    it("should track run history", async () => {
      const adapter = createMockAdapter();

      const ctx = {
        workOrder: { task: "test task" },
        sdkOptions: { model: "gemini-2.0" },
        emitAssistantDelta: () => {},
        emitAssistantMessage: () => {},
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        log: () => {},
      };

      await adapter.run(ctx);

      const history = adapter.getRunHistory();
      assert.strictEqual(history.length, 1);
      assert.strictEqual(history[0].workOrder.task, "test task");
    });

    it("should handle configured errors", async () => {
      const adapter = createMockAdapter({
        errors: [{ message: "Simulated failure" }],
      });

      const ctx = {
        workOrder: { task: "test" },
        sdkOptions: {},
        emitAssistantDelta: () => {},
        emitAssistantMessage: () => {},
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: (msg) => {},
        log: () => {},
      };

      await assert.rejects(async () => {
        await adapter.run(ctx);
      }, /Simulated failure/);
    });
  });
});

// ============================================================================
// Edge Cases
// ============================================================================

describe("Edge Cases", () => {
  it("should handle empty allowed_tools", () => {
    const result = validateFacade({ prompt: "test", allowed_tools: [] });
    assert.strictEqual(result.valid, true);
  });

  it("should handle missing prompt", () => {
    const { geminiRequest } = mapClaudeToGemini({});
    assert.strictEqual(geminiRequest.prompt, undefined);
  });

  it("should handle empty MCP servers", () => {
    const { geminiRequest } = mapClaudeToGemini({ mcp_servers: {} });
    assert.deepStrictEqual(geminiRequest.mcpServers, {});
  });

  it("should handle MCP server without command or url", () => {
    const { geminiRequest, mappingWarnings } = mapClaudeToGemini({
      mcp_servers: {
        invalid: { foo: "bar" },
      },
    });
    assert.ok(!geminiRequest.mcpServers.invalid);
    assert.ok(mappingWarnings.some((w) => w.message.includes("missing command or url")));
  });

  it("should handle multiple unsupported features", () => {
    const result = validateFacade({
      prompt: "test",
      extended_thinking: true,
      agent_teams: [{ name: "test" }],
    });
    assert.strictEqual(result.valid, false);
    assert.strictEqual(result.errors.length, 2);
  });
});
