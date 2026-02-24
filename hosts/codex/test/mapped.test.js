/**
 * Mapped Mode Conformance Tests for Codex Sidecar
 *
 * Tests for Codex→Claude mapped mode implementation.
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
} = require("../capabilities");

const {
  ErrorCodes,
  ErrorNames,
  createError,
  validateFacade,
  validateCapabilities,
  extractRequiredCapabilities,
  mapCodexToClaude,
  mapThreadIdToSessionId,
  mapToolList,
  mapPermissionMode,
  mapModel,
  createMappedReceiptAdditions,
  mapClaudeToCodexResponse,
} = require("../mapper");

const { createMockAdapter, createMockClaudeAdapter } = require("./mock-adapter");

// ============================================================================
// Test Fixtures
// ============================================================================

const validCodexRequest = {
  prompt: "Read the main.rs file and explain it",
  cwd: "/workspace/project",
  tools: ["read_file", "glob", "grep"],
  permission_mode: "auto",
};

const requestWithDeprecatedFunctionCall = {
  prompt: "Call a function",
  function_call: { name: "get_weather" },
  functions: [{ name: "get_weather", parameters: {} }],
};

const requestWithAssistantsApi = {
  prompt: "Use assistant",
  assistant_id: "asst_123",
  run_id: "run_456",
};

const requestWithCodeInterpreter = {
  prompt: "Execute some code",
  code_interpreter: true,
  enable_code_interpreter: true,
};

const requestWithRetrieval = {
  prompt: "Search files",
  retrieval: true,
  file_search: { query: "important" },
};

const requestWithThreadInstructions = {
  prompt: "Do something",
  thread_instructions: "Be helpful",
  additional_instructions: "Also be concise",
};

const requestWithModelMapping = {
  prompt: "Test model mapping",
  model: "gpt-4-turbo",
};

const requestWithThreadId = {
  prompt: "Resume conversation",
  thread_id: "thread_abc123",
};

// ============================================================================
// Capability Tests
// ============================================================================

describe("Capabilities", () => {
  describe("getToolMapping", () => {
    it("should map read_file to Read", () => {
      const result = getToolMapping("read_file");
      assert.strictEqual(result.claudeTool, "Read");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should map write_file to Write", () => {
      const result = getToolMapping("write_file");
      assert.strictEqual(result.claudeTool, "Write");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should map code_execution to Bash", () => {
      const result = getToolMapping("code_execution");
      assert.strictEqual(result.claudeTool, "Bash");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should map shell to Bash", () => {
      const result = getToolMapping("shell");
      assert.strictEqual(result.claudeTool, "Bash");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should pass through Claude tools as native", () => {
      const result = getToolMapping("Read");
      assert.strictEqual(result.claudeTool, "Read");
      assert.strictEqual(result.supportLevel, SupportLevel.Native);
    });

    it("should pass through unknown tools as emulated", () => {
      const result = getToolMapping("custom_mcp_tool");
      assert.strictEqual(result.claudeTool, "custom_mcp_tool");
      assert.strictEqual(result.supportLevel, SupportLevel.Emulated);
    });
  });

  describe("getFeatureSupport", () => {
    it("should return native for streaming", () => {
      assert.strictEqual(getFeatureSupport("streaming"), SupportLevel.Native);
    });

    it("should return native for extended_thinking", () => {
      assert.strictEqual(getFeatureSupport("extended_thinking"), SupportLevel.Native);
    });

    it("should return native for hooks", () => {
      assert.strictEqual(getFeatureSupport("hooks"), SupportLevel.Native);
    });

    it("should return emulated for thread_resume", () => {
      assert.strictEqual(getFeatureSupport("thread_resume"), SupportLevel.Emulated);
    });

    it("should return unsupported for codex_thread_model", () => {
      assert.strictEqual(getFeatureSupport("codex_thread_model"), SupportLevel.Unsupported);
    });
  });

  describe("getModelMapping", () => {
    it("should map gpt-4 to Claude 3.5 Sonnet", () => {
      assert.strictEqual(
        getModelMapping("gpt-4"),
        "claude-3-5-sonnet-20241022"
      );
    });

    it("should map gpt-4o to Claude 3.5 Sonnet", () => {
      assert.strictEqual(
        getModelMapping("gpt-4o"),
        "claude-3-5-sonnet-20241022"
      );
    });

    it("should map gpt-3.5-turbo to Claude 3.5 Haiku", () => {
      assert.strictEqual(
        getModelMapping("gpt-3.5-turbo"),
        "claude-3-5-haiku-20241022"
      );
    });

    it("should map gpt-4o-mini to Claude 3.5 Haiku", () => {
      assert.strictEqual(
        getModelMapping("gpt-4o-mini"),
        "claude-3-5-haiku-20241022"
      );
    });

    it("should return default for unknown models", () => {
      assert.strictEqual(
        getModelMapping("unknown-model"),
        "claude-3-5-sonnet-20241022"
      );
    });

    it("should return default for null/undefined", () => {
      assert.strictEqual(
        getModelMapping(null),
        "claude-3-5-sonnet-20241022"
      );
      assert.strictEqual(
        getModelMapping(undefined),
        "claude-3-5-sonnet-20241022"
      );
    });
  });

  describe("getCapabilityManifest", () => {
    it("should return complete manifest", () => {
      const manifest = getCapabilityManifest();
      assert.strictEqual(manifest.backend, "claude");
      assert.strictEqual(manifest.dialect, "codex");
      assert.ok(manifest.capabilities);
      assert.ok(manifest.tool_mapping);
      assert.ok(manifest.model_mapping);
    });
  });

  describe("getToolAlternatives", () => {
    it("should return alternatives for code_interpreter", () => {
      const alternatives = getToolAlternatives("code_interpreter");
      assert.ok(Array.isArray(alternatives));
      assert.ok(alternatives.length > 0);
    });

    it("should return empty array for unknown tools", () => {
      const alternatives = getToolAlternatives("unknown_tool");
      assert.ok(Array.isArray(alternatives));
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

    it("should have correct code for AMBIGUOUS_MAPPING", () => {
      assert.strictEqual(ErrorCodes.AMBIGUOUS_MAPPING, "E003");
    });

    it("should have correct code for BACKEND_CAPABILITY_MISSING", () => {
      assert.strictEqual(ErrorCodes.BACKEND_CAPABILITY_MISSING, "E006");
    });

    it("should have correct code for BACKEND_UNAVAILABLE", () => {
      assert.strictEqual(ErrorCodes.BACKEND_UNAVAILABLE, "E007");
    });
  });

  describe("ErrorNames", () => {
    it("should have correct name for E001", () => {
      assert.strictEqual(ErrorNames.E001, "UnsupportedFeature");
    });

    it("should have correct name for E002", () => {
      assert.strictEqual(ErrorNames.E002, "UnsupportedTool");
    });
  });

  describe("createError", () => {
    it("should create structured error with all fields", () => {
      const error = createError(ErrorCodes.UNSUPPORTED_FEATURE, {
        message: "Assistants API not supported",
        feature: "codex_assistants_api",
        suggestion: "Use Claude native tools",
      });

      assert.strictEqual(error.code, "E001");
      assert.strictEqual(error.name, "UnsupportedFeature");
      assert.strictEqual(error.feature, "codex_assistants_api");
      assert.ok(error.message);
      assert.ok(error.timestamp);
      assert.ok(error.documentation_url);
    });

    it("should include available alternatives for tool errors", () => {
      const error = createError(ErrorCodes.UNSUPPORTED_TOOL, {
        message: "Tool not available",
        feature: "retrieval",
        available_alternatives: ["MCP servers", "Grep"],
      });

      assert.strictEqual(error.code, "E002");
      assert.deepStrictEqual(error.available_alternatives, ["MCP servers", "Grep"]);
    });

    it("should default dialect to codex and engine to claude", () => {
      const error = createError(ErrorCodes.UNSUPPORTED_FEATURE, {});
      assert.strictEqual(error.dialect, "codex");
      assert.strictEqual(error.engine, "claude");
    });
  });
});

// ============================================================================
// Facade Validation Tests (Stage 1)
// ============================================================================

describe("Facade Validation (Stage 1)", () => {
  describe("validateFacade", () => {
    it("should pass valid requests", () => {
      const result = validateFacade(validCodexRequest);
      assert.strictEqual(result.valid, true);
      assert.strictEqual(result.errors.length, 0);
    });

    it("should fail invalid request format", () => {
      const result = validateFacade(null);
      assert.strictEqual(result.valid, false);
      assert.ok(result.errors.some((e) => e.feature === "request_format"));
    });

    it("should fail deprecated function_call format", () => {
      const result = validateFacade(requestWithDeprecatedFunctionCall);
      assert.strictEqual(result.valid, false);
      assert.ok(
        result.errors.some((e) => e.feature === "function_call_deprecated")
      );
    });

    it("should fail Assistants API features", () => {
      const result = validateFacade(requestWithAssistantsApi);
      assert.strictEqual(result.valid, false);
      assert.ok(
        result.errors.some((e) => e.feature === "codex_assistants_api")
      );
    });

    it("should fail retrieval/file_search features", () => {
      const result = validateFacade(requestWithRetrieval);
      assert.strictEqual(result.valid, false);
      assert.ok(result.errors.some((e) => e.feature === "codex_retrieval"));
    });

    it("should warn about code interpreter emulation", () => {
      const result = validateFacade(requestWithCodeInterpreter);
      // Should pass (just warning)
      assert.strictEqual(result.valid, true);
      assert.ok(
        result.warnings.some((w) => w.feature === "code_execution")
      );
    });

    it("should warn about thread instructions merging", () => {
      const result = validateFacade(requestWithThreadInstructions);
      assert.strictEqual(result.valid, true);
      assert.ok(
        result.warnings.some((w) => w.feature === "thread_instructions")
      );
    });

    it("should warn about model mapping", () => {
      const result = validateFacade(requestWithModelMapping);
      assert.strictEqual(result.valid, true);
      assert.ok(
        result.warnings.some((w) => w.feature === "model_mapping")
      );
    });
  });
});

// ============================================================================
// Runtime Capability Validation Tests (Stage 2)
// ============================================================================

describe("Runtime Capability Validation (Stage 2)", () => {
  describe("validateCapabilities", () => {
    it("should validate native capabilities", () => {
      const mappedRequest = {
        allowed_tools: ["Read", "Write"],
      };
      const result = validateCapabilities(mappedRequest, {});
      assert.strictEqual(result.valid, true);
      assert.ok(result.capabilities_used.native.length > 0);
    });

    it("should detect emulated capabilities", () => {
      const mappedRequest = {
        session_id: "session_123",
        resume: true,
      };
      const result = validateCapabilities(mappedRequest, {});
      assert.ok(result.capabilities_used.emulated.includes("session_resume"));
    });

    it("should fail when native required for emulated capability", () => {
      const mappedRequest = {
        session_id: "session_123",
        resume: true,
        requires_native: ["session_resume"],
      };
      const result = validateCapabilities(mappedRequest, {});
      assert.strictEqual(result.valid, false);
      assert.ok(
        result.errors.some((e) => e.code === ErrorCodes.BACKEND_CAPABILITY_MISSING)
      );
    });
  });

  describe("extractRequiredCapabilities", () => {
    it("should extract streaming capability", () => {
      const caps = extractRequiredCapabilities({ prompt: "test" });
      assert.ok(caps.includes("streaming"));
    });

    it("should extract tool capabilities", () => {
      const caps = extractRequiredCapabilities({
        allowed_tools: ["Read", "Bash"],
      });
      assert.ok(caps.includes("tool_read"));
      assert.ok(caps.includes("tool_bash"));
    });

    it("should extract session capability", () => {
      const caps = extractRequiredCapabilities({
        session_id: "session_123",
        resume: true,
      });
      assert.ok(caps.includes("session_resume"));
    });

    it("should extract extended thinking capability", () => {
      const caps = extractRequiredCapabilities({
        extended_thinking: true,
      });
      assert.ok(caps.includes("extended_thinking"));
    });

    it("should return empty array for null/undefined", () => {
      assert.deepStrictEqual(extractRequiredCapabilities(null), []);
      assert.deepStrictEqual(extractRequiredCapabilities(undefined), []);
    });
  });
});

// ============================================================================
// Mapping Function Tests
// ============================================================================

describe("Mapping Functions", () => {
  describe("mapCodexToClaude", () => {
    it("should map prompt correctly", () => {
      const result = mapCodexToClaude({ prompt: "Hello" });
      assert.strictEqual(result.prompt, "Hello");
    });

    it("should map input to prompt", () => {
      const result = mapCodexToClaude({ input: "Hello" });
      assert.strictEqual(result.prompt, "Hello");
    });

    it("should map model", () => {
      const result = mapCodexToClaude({ model: "gpt-4" });
      assert.strictEqual(result.model, "claude-3-5-sonnet-20241022");
    });

    it("should map tools", () => {
      const result = mapCodexToClaude({ tools: ["read_file", "shell"] });
      assert.ok(result.allowed_tools.includes("Read"));
      assert.ok(result.allowed_tools.includes("Bash"));
    });

    it("should map thread_id to session_id", () => {
      const result = mapCodexToClaude({ thread_id: "thread_123" });
      assert.strictEqual(result.session_id, "codex_thread:thread_123");
      assert.strictEqual(result.resume, true);
    });

    it("should map instructions to system_prompt_additions", () => {
      const result = mapCodexToClaude({ instructions: "Be helpful" });
      assert.strictEqual(result.system_prompt_additions, "Be helpful");
    });

    it("should add Bash tool for code_interpreter", () => {
      const result = mapCodexToClaude({
        code_interpreter: true,
        tools: ["read_file"],
      });
      assert.ok(result.allowed_tools.includes("Bash"));
    });

    it("should map temperature", () => {
      const result = mapCodexToClaude({ temperature: 0.5 });
      assert.strictEqual(result.temperature, 0.5);
    });

    it("should map max_tokens", () => {
      const result = mapCodexToClaude({ max_tokens: 1000 });
      assert.strictEqual(result.max_tokens, 1000);
    });

    it("should map JSON mode", () => {
      const result = mapCodexToClaude({
        response_format: { type: "json_object" },
      });
      assert.strictEqual(result.structured_output, true);
    });
  });

  describe("mapThreadIdToSessionId", () => {
    it("should prefix thread ID with codex_thread:", () => {
      const result = mapThreadIdToSessionId("thread_abc123");
      assert.strictEqual(result, "codex_thread:thread_abc123");
    });
  });

  describe("mapToolList", () => {
    it("should map array of tool names", () => {
      const result = mapToolList(["read_file", "write_file", "shell"]);
      assert.ok(result.includes("Read"));
      assert.ok(result.includes("Write"));
      assert.ok(result.includes("Bash"));
    });

    it("should map array of tool objects", () => {
      const result = mapToolList([
        { name: "read_file" },
        { type: "shell" },
      ]);
      assert.ok(result.includes("Read"));
      assert.ok(result.includes("Bash"));
    });

    it("should deduplicate tools", () => {
      const result = mapToolList(["read_file", "Read", "file_read"]);
      assert.strictEqual(result.length, 1);
      assert.strictEqual(result[0], "Read");
    });
  });

  describe("mapPermissionMode", () => {
    it("should map auto to auto", () => {
      assert.strictEqual(mapPermissionMode("auto"), "auto");
    });

    it("should map automatic to auto", () => {
      assert.strictEqual(mapPermissionMode("automatic"), "auto");
    });

    it("should map interactive to plan", () => {
      assert.strictEqual(mapPermissionMode("interactive"), "plan");
    });

    it("should map semi-auto to acceptEdits", () => {
      assert.strictEqual(mapPermissionMode("semi-auto"), "acceptEdits");
    });

    it("should handle autoApprove array with *", () => {
      assert.strictEqual(mapPermissionMode(null, ["*"]), "auto");
    });

    it("should handle autoApprove array with specific tools", () => {
      assert.strictEqual(mapPermissionMode(null, ["read"]), "acceptEdits");
    });
  });

  describe("mapModel", () => {
    it("should delegate to getModelMapping", () => {
      assert.strictEqual(mapModel("gpt-4"), "claude-3-5-sonnet-20241022");
      assert.strictEqual(mapModel("gpt-3.5-turbo"), "claude-3-5-haiku-20241022");
    });
  });
});

// ============================================================================
// Receipt Tests
// ============================================================================

describe("Receipt Generation", () => {
  describe("createMappedReceiptAdditions", () => {
    it("should include mode and dialect info", () => {
      const codexRequest = { prompt: "test" };
      const claudeRequest = { prompt: "test" };
      const validation = { warnings: [] };

      const result = createMappedReceiptAdditions(
        codexRequest,
        claudeRequest,
        validation,
        {}
      );

      assert.strictEqual(result.mode, "mapped");
      assert.strictEqual(result.source_dialect, "codex");
      assert.strictEqual(result.target_engine, "claude");
    });

    it("should include mapping warnings", () => {
      const codexRequest = { prompt: "test" };
      const claudeRequest = { prompt: "test" };
      const validation = {
        warnings: [{ message: "Test warning" }],
      };

      const result = createMappedReceiptAdditions(
        codexRequest,
        claudeRequest,
        validation,
        {}
      );

      assert.strictEqual(result.mapping_warnings.length, 1);
    });

    it("should include capabilities used", () => {
      const codexRequest = { prompt: "test" };
      const claudeRequest = { prompt: "test" };
      const validation = {
        capabilities_used: {
          native: ["streaming"],
          emulated: [],
          unsupported: [],
        },
      };

      const result = createMappedReceiptAdditions(
        codexRequest,
        claudeRequest,
        validation,
        {}
      );

      assert.deepStrictEqual(result.capabilities_used.native, ["streaming"]);
    });

    it("should include session mapping", () => {
      const codexRequest = { thread_id: "thread_123" };
      const claudeRequest = { session_id: "codex_thread:thread_123" };
      const validation = { warnings: [] };
      const sessionMapping = {
        codexThreadId: "thread_123",
        claudeSessionId: "codex_thread:thread_123",
      };

      const result = createMappedReceiptAdditions(
        codexRequest,
        claudeRequest,
        validation,
        sessionMapping
      );

      assert.strictEqual(result.session_mapping.codex_thread_id, "thread_123");
      assert.strictEqual(
        result.session_mapping.claude_session_id,
        "codex_thread:thread_123"
      );
    });

    it("should include model mapping", () => {
      const codexRequest = { model: "gpt-4" };
      const claudeRequest = { model: "claude-3-5-sonnet-20241022" };
      const validation = { warnings: [] };

      const result = createMappedReceiptAdditions(
        codexRequest,
        claudeRequest,
        validation,
        {}
      );

      assert.strictEqual(result.model_mapping.original, "gpt-4");
      assert.strictEqual(result.model_mapping.mapped, "claude-3-5-sonnet-20241022");
    });

    it("should include tool mappings", () => {
      const codexRequest = { tools: ["read_file", "shell"] };
      const claudeRequest = { allowed_tools: ["Read", "Bash"] };
      const validation = { warnings: [] };

      const result = createMappedReceiptAdditions(
        codexRequest,
        claudeRequest,
        validation,
        {}
      );

      assert.strictEqual(result.tool_mappings.length, 2);
      assert.ok(result.tool_mappings.some((m) => m.codex_tool === "read_file"));
      assert.ok(result.tool_mappings.some((m) => m.codex_tool === "shell"));
    });
  });
});

// ============================================================================
// Reverse Mapping Tests
// ============================================================================

describe("Reverse Mapping (Claude → Codex)", () => {
  describe("mapClaudeToCodexResponse", () => {
    it("should map basic response", () => {
      const claudeResponse = {
        id: "msg_123",
        content: "Hello from Claude",
      };

      const result = mapClaudeToCodexResponse(claudeResponse);

      assert.strictEqual(result.object, "thread.message");
      assert.strictEqual(result.status, "completed");
      assert.strictEqual(result.content.length, 1);
      assert.strictEqual(result.content[0].type, "text");
    });

    it("should map content array", () => {
      const claudeResponse = {
        id: "msg_123",
        content: [{ type: "text", text: "Hello" }],
      };

      const result = mapClaudeToCodexResponse(claudeResponse);

      assert.strictEqual(result.content[0].text, "Hello");
    });

    it("should map tool calls", () => {
      const claudeResponse = {
        id: "msg_123",
        content: "Done",
        tool_calls: [
          {
            id: "toolu_123",
            name: "Read",
            input: { file_path: "/test.txt" },
          },
        ],
      };

      const result = mapClaudeToCodexResponse(claudeResponse);

      assert.ok(result.tool_calls);
      assert.strictEqual(result.tool_calls[0].type, "function");
      assert.strictEqual(result.tool_calls[0].function.name, "Read");
    });

    it("should map usage", () => {
      const claudeResponse = {
        id: "msg_123",
        content: "Done",
        usage: {
          input_tokens: 100,
          output_tokens: 200,
          total_tokens: 300,
        },
      };

      const result = mapClaudeToCodexResponse(claudeResponse);

      assert.strictEqual(result.usage.prompt_tokens, 100);
      assert.strictEqual(result.usage.completion_tokens, 200);
      assert.strictEqual(result.usage.total_tokens, 300);
    });

    it("should include thread_id from session mapping", () => {
      const claudeResponse = {
        id: "msg_123",
        content: "Done",
      };
      const sessionMapping = {
        codexThreadId: "thread_abc",
      };

      const result = mapClaudeToCodexResponse(claudeResponse, sessionMapping);

      assert.strictEqual(result.thread_id, "thread_abc");
    });
  });
});

// ============================================================================
// Mock Adapter Tests
// ============================================================================

describe("Mock Adapter", () => {
  describe("createMockAdapter", () => {
    it("should create adapter with default behavior", () => {
      const adapter = createMockAdapter();
      assert.strictEqual(adapter.name, "mock_codex_adapter");
      assert.strictEqual(typeof adapter.run, "function");
    });

    it("should emit configured responses", async () => {
      const adapter = createMockAdapter({
        responses: [{ text: "Hello from mock" }],
      });

      const events = [];
      await adapter.run({
        workOrder: { task: "test" },
        sdkOptions: {},
        emitAssistantDelta: (text) => events.push({ type: "delta", text }),
        emitAssistantMessage: (text) => events.push({ type: "message", text }),
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        writeArtifact: async () => {},
      });

      assert.ok(events.some((e) => e.type === "message" && e.text === "Hello from mock"));
    });

    it("should track run history", async () => {
      const adapter = createMockAdapter();

      await adapter.run({
        workOrder: { task: "test1" },
        sdkOptions: {},
        emitAssistantDelta: () => {},
        emitAssistantMessage: () => {},
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        writeArtifact: async () => {},
      });

      await adapter.run({
        workOrder: { task: "test2" },
        sdkOptions: {},
        emitAssistantDelta: () => {},
        emitAssistantMessage: () => {},
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        writeArtifact: async () => {},
      });

      const history = adapter.getRunHistory();
      assert.strictEqual(history.length, 2);
    });

    it("should handle thread management", async () => {
      const adapter = createMockAdapter({ threadSupport: true });

      await adapter.run({
        workOrder: { task: "test" },
        sdkOptions: { thread_id: "thread_123" },
        emitAssistantDelta: () => {},
        emitAssistantMessage: () => {},
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        writeArtifact: async () => {},
      });

      const thread = adapter.getThread("thread_123");
      assert.ok(thread);
      assert.strictEqual(thread.id, "thread_123");
    });
  });

  describe("createMockClaudeAdapter", () => {
    it("should create Claude adapter for mapped mode testing", () => {
      const adapter = createMockClaudeAdapter();
      assert.strictEqual(adapter.name, "mock_claude_adapter");
      assert.strictEqual(typeof adapter.run, "function");
    });

    it("should track mapped requests", async () => {
      const adapter = createMockClaudeAdapter();

      await adapter.run({
        workOrder: { task: "test" },
        sdkOptions: { model: "claude-3-5-sonnet-20241022" },
        emitAssistantDelta: () => {},
        emitAssistantMessage: () => {},
        emitToolCall: () => {},
        emitToolResult: () => {},
        emitWarning: () => {},
        emitError: () => {},
        writeArtifact: async () => {},
      });

      const history = adapter.getRunHistory();
      assert.strictEqual(history.length, 1);
      assert.strictEqual(history[0].mappedFrom, "codex");
    });
  });
});

// Run tests if executed directly
if (require.main === module) {
  console.log("Run with: node --test hosts/codex/test/mapped.test.js");
}
