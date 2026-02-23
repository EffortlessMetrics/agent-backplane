/**
 * Mock Adapter for Passthrough Testing
 *
 * This adapter simulates the Claude SDK behavior for testing purposes.
 * It supports both mapped and passthrough modes.
 */

const ADAPTER_VERSION = "0.1-test";

/**
 * Get execution mode from work order
 */
function getExecutionMode(workOrder) {
  const vendor = workOrder.config && workOrder.config.vendor;
  if (!vendor || typeof vendor !== "object") {
    return "mapped";
  }
  const abp = vendor.abp;
  if (!abp || typeof abp !== "object") {
    return "mapped";
  }
  return abp.mode === "passthrough" ? "passthrough" : "mapped";
}

/**
 * Get passthrough request from work order
 */
function getPassthroughRequest(workOrder) {
  const vendor = workOrder.config && workOrder.config.vendor;
  if (!vendor || typeof vendor !== "object") {
    return null;
  }
  const abp = vendor.abp;
  if (!abp || typeof abp !== "object") {
    return null;
  }
  return abp.request || null;
}

module.exports = {
  name: "mock_passthrough_adapter",
  version: ADAPTER_VERSION,
  capabilities: {
    streaming: "native",
    passthrough: "native",
    stream_equivalent: "native",
    tool_read: "emulated",
    tool_write: "emulated",
    tool_edit: "emulated",
    tool_bash: "emulated",
  },

  async run(ctx) {
    const mode = getExecutionMode(ctx.workOrder);

    if (mode === "passthrough") {
      return runPassthrough(ctx);
    }
    return runMapped(ctx);
  },
};

/**
 * Run in mapped mode (traditional ABP behavior)
 */
async function runMapped(ctx) {
  ctx.emitAssistantMessage("Mock adapter running in mapped mode.");
  ctx.emitAssistantMessage(`Task: ${ctx.workOrder.task}`);

  // Simulate some work
  await new Promise((resolve) => setTimeout(resolve, 100));

  ctx.emitAssistantMessage("Mock task completed.");

  return {
    usageRaw: {
      mode: "mapped",
      input_tokens: 100,
      output_tokens: 50,
    },
    usage: {
      input_tokens: 100,
      output_tokens: 50,
    },
    outcome: "complete",
  };
}

/**
 * Run in passthrough mode (lossless wrapping)
 *
 * Passthrough invariants:
 * 1. No request rewriting: SDK sees exactly what caller sent
 * 2. Stream equivalence: After removing ABP framing, stream is bitwise-equivalent
 * 3. Observer-only governance: Log/record but don't modify
 */
async function runPassthrough(ctx) {
  const rawRequest = getPassthroughRequest(ctx.workOrder);

  ctx.emitAssistantMessage("Mock adapter running in passthrough mode.");

  if (!rawRequest) {
    ctx.emitWarning("No passthrough request provided, using fallback behavior.");
    return {
      usageRaw: { mode: "passthrough_fallback" },
      outcome: "partial",
    };
  }

  // PASSTHROUGH INVARIANT: Use the request exactly as provided
  // Simulate SDK streaming response
  const mockMessages = [
    { type: "message_start", usage: { input_tokens: 50 } },
    { type: "content_block_delta", delta: "Hello " },
    { type: "content_block_delta", delta: "from " },
    { type: "content_block_delta", delta: "passthrough!" },
    { type: "message_delta", usage: { output_tokens: 10 } },
    { type: "message_stop" },
  ];

  // Emit each message as a passthrough event with raw_message in ext
  for (const rawMessage of mockMessages) {
    ctx.emitPassthroughEvent(rawMessage);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }

  return {
    usageRaw: {
      mode: "passthrough",
      input_tokens: 50,
      output_tokens: 10,
    },
    usage: {
      input_tokens: 50,
      output_tokens: 10,
    },
    outcome: "complete",
    stream_equivalent: true, // Guarantee: stream is bitwise-equivalent
  };
}
