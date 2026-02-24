/**
 * Mock Gemini Adapter for Testing
 *
 * Provides a deterministic, controllable adapter for testing the mapped mode
 * without requiring actual Gemini CLI installation.
 */

const EventEmitter = require("node:events");

const ADAPTER_NAME = "mock_gemini_adapter";
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
  } = options;

  let callCount = 0;
  let runHistory = [];

  const events = new EventEmitter();

  async function run(ctx) {
    const { workOrder, sdkOptions, emitAssistantDelta, emitAssistantMessage, emitToolCall, emitToolResult, emitWarning, emitError } = ctx;

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
    } else {
      // Default response
      emitAssistantDelta("Mock response from Gemini adapter");
      emitAssistantMessage("Mock response from Gemini adapter");
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
  }

  function getRunHistory() {
    return [...runHistory];
  }

  function getCallCount() {
    return callCount;
  }

  return {
    name: ADAPTER_NAME,
    version: ADAPTER_VERSION,
    run,
    reset,
    getRunHistory,
    getCallCount,
    events,
    // Expose test helpers
    _isGeminiAvailable: async () => true,
    _options: options,
  };
}

// Default mock adapter
const defaultMockAdapter = createMockAdapter();

module.exports = {
  ADAPTER_NAME,
  ADAPTER_VERSION,
  createMockAdapter,
  defaultMockAdapter,
};
