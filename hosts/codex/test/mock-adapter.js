/**
 * Mock Codex Adapter for Testing
 *
 * Provides a deterministic, controllable adapter for testing both
 * passthrough and mapped modes without requiring actual Codex SDK installation.
 */

const EventEmitter = require("node:events");

const ADAPTER_NAME = "mock_codex_adapter";
const ADAPTER_VERSION = "0.1.0-test";

/**
 * Create a mock adapter with configurable behavior
 * @param {object} options - Mock configuration
 * @returns {object} Mock adapter module
 */
function createMockAdapter(options = {}) {
  const {
    responses = [],
    toolCalls = [],
    errors = [],
    delay = 0,
    usage = { input_tokens: 100, output_tokens: 200, total_tokens: 300 },
    threadSupport = true,
  } = options;

  let callCount = 0;
  let runHistory = [];
  let threads = new Map();

  const events = new EventEmitter();

  async function run(ctx) {
    const { 
      workOrder, 
      sdkOptions, 
      emitAssistantDelta, 
      emitAssistantMessage, 
      emitToolCall, 
      emitToolResult, 
      emitWarning, 
      emitError 
    } = ctx;

    // Record this run
    const runRecord = {
      workOrder,
      sdkOptions,
      timestamp: new Date().toISOString(),
    };
    runHistory.push(runRecord);
    events.emit("run", runRecord);

    // Check for configured errors
    if (errors.length > callCount && errors[callCount]) {
      const err = errors[callCount];
      callCount++;
      emitError(err.message || err);
      throw new Error(err.message || err);
    }

    // Simulate delay
    if (delay > 0) {
      await new Promise((resolve) => setTimeout(resolve, delay));
    }

    // Handle thread management
    let threadId = sdkOptions?.threadId || sdkOptions?.thread_id;
    if (threadSupport && threadId) {
      // Resume existing thread
      if (!threads.has(threadId)) {
        threads.set(threadId, {
          id: threadId,
          messages: [],
          created_at: new Date().toISOString(),
        });
      }
      const thread = threads.get(threadId);
      thread.messages.push({
        role: "user",
        content: workOrder.task || sdkOptions?.prompt,
        timestamp: new Date().toISOString(),
      });
    }

    // Emit configured responses
    if (responses.length > callCount) {
      const response = responses[callCount];
      callCount++;

      if (response.text) {
        // Emit text in chunks to simulate streaming
        const chunks = response.text.match(/.{1,20}/g) || [];
        for (const chunk of chunks) {
          emitAssistantDelta(chunk);
          await new Promise((r) => setTimeout(r, 10));
        }
        emitAssistantMessage(response.text);
      }

      if (response.toolCalls) {
        for (const tc of response.toolCalls) {
          emitToolCall({
            toolName: tc.toolName,
            toolUseId: tc.toolUseId || `mock_toolu_${Date.now()}`,
            input: tc.input,
          });

          // Emit result if provided
          if (tc.result) {
            emitToolResult({
              toolName: tc.toolName,
              toolUseId: tc.toolUseId || `mock_toolu_${Date.now()}`,
              output: tc.result,
              isError: tc.isError || false,
            });
          }
        }
      }

      if (response.threadId) {
        // Store thread for resume testing
        threads.set(response.threadId, {
          id: response.threadId,
          messages: [{ role: "assistant", content: response.text }],
          created_at: new Date().toISOString(),
        });
      }
    } else {
      // Default response
      emitAssistantDelta("Mock response from Codex adapter");
      emitAssistantMessage("Mock response from Codex adapter");
    }

    // Emit configured tool calls
    for (const tc of toolCalls) {
      emitToolCall({
        toolName: tc.toolName,
        toolUseId: tc.toolUseId || `mock_toolu_${Date.now()}`,
        input: tc.input,
      });
    }

    callCount++;
  }

  function reset() {
    callCount = 0;
    runHistory = [];
    threads.clear();
  }

  function getRunHistory() {
    return [...runHistory];
  }

  function getCallCount() {
    return callCount;
  }

  function getThread(threadId) {
    return threads.get(threadId);
  }

  function createThread(id) {
    const threadId = id || `thread_${Date.now()}`;
    threads.set(threadId, {
      id: threadId,
      messages: [],
      created_at: new Date().toISOString(),
    });
    return threads.get(threadId);
  }

  return {
    name: ADAPTER_NAME,
    version: ADAPTER_VERSION,
    run,
    reset,
    getRunHistory,
    getCallCount,
    getThread,
    createThread,
    events,
    // Expose test helpers
    _isCodexAvailable: async () => true,
    _options: options,
    _threads: threads,
  };
}

/**
 * Create a mock adapter that simulates Claude backend for mapped mode testing
 * @param {object} options - Mock configuration
 * @returns {object} Mock Claude adapter module
 */
function createMockClaudeAdapter(options = {}) {
  const {
    responses = [],
    delay = 0,
  } = options;

  let callCount = 0;
  let runHistory = [];

  async function run(ctx) {
    const { 
      workOrder, 
      sdkOptions, 
      emitAssistantDelta, 
      emitAssistantMessage, 
      emitToolCall, 
      emitToolResult,
      emitWarning,
      emitError 
    } = ctx;

    // Record this run
    const runRecord = {
      workOrder,
      sdkOptions,
      timestamp: new Date().toISOString(),
      mappedFrom: "codex",
    };
    runHistory.push(runRecord);

    // Simulate delay
    if (delay > 0) {
      await new Promise((resolve) => setTimeout(resolve, delay));
    }

    // Log that we received mapped request
    emitWarning(`Claude adapter received mapped request (model: ${sdkOptions?.model})`);

    // Emit configured responses
    if (responses.length > callCount) {
      const response = responses[callCount];
      callCount++;

      if (response.text) {
        const chunks = response.text.match(/.{1,20}/g) || [];
        for (const chunk of chunks) {
          emitAssistantDelta(chunk);
          await new Promise((r) => setTimeout(r, 10));
        }
        emitAssistantMessage(response.text);
      }

      if (response.toolCalls) {
        for (const tc of response.toolCalls) {
          emitToolCall({
            toolName: tc.toolName,
            toolUseId: tc.toolUseId || `clu_${Date.now()}`,
            input: tc.input,
          });

          if (tc.result) {
            emitToolResult({
              toolName: tc.toolName,
              toolUseId: tc.toolUseId || `clu_${Date.now()}`,
              output: tc.result,
              isError: tc.isError || false,
            });
          }
        }
      }
    } else {
      // Default response
      emitAssistantDelta("Mock response from Claude adapter (mapped from Codex)");
      emitAssistantMessage("Mock response from Claude adapter (mapped from Codex)");
    }

    callCount++;
  }

  function reset() {
    callCount = 0;
    runHistory = [];
  }

  function getRunHistory() {
    return [...runHistory];
  }

  return {
    name: "mock_claude_adapter",
    version: "0.1.0-test",
    run,
    reset,
    getRunHistory,
  };
}

// Default mock adapters
const defaultMockAdapter = createMockAdapter();
const defaultMockClaudeAdapter = createMockClaudeAdapter();

// Export as adapter module (what host.js expects when loading via ABP_CODEX_ADAPTER_MODULE)
module.exports = {
  // Adapter interface (required by host.js)
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run: defaultMockAdapter.run,
  
  // Test utilities
  ADAPTER_NAME,
  ADAPTER_VERSION,
  createMockAdapter,
  createMockClaudeAdapter,
  defaultMockAdapter,
  defaultMockClaudeAdapter,
};
