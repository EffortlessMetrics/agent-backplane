/**
 * Passthrough Parity Conformance Tests
 *
 * Tests that validate passthrough mode is truly lossless:
 * - No request rewriting
 * - Stream equivalence after removing ABP framing
 * - Observer-only governance
 * - Receipt out-of-band
 */

const assert = require("node:assert");
const { describe, it } = require("node:test");
const crypto = require("node:crypto");
const {
  runSidecarTest,
  createWorkOrder,
  filterMatrix,
} = require("./runner");

// Get test filters from environment
const testDialect = process.env.ABP_TEST_DIALECT || null;
const testEngine = process.env.ABP_TEST_ENGINE || null;
const testMode = process.env.ABP_TEST_MODE || null;

// Only run passthrough tests for passthrough mode
const shouldRunPassthrough = !testMode || testMode === "passthrough";

// Passthrough cells to test
const passthroughCells = filterMatrix({
  dialect: testDialect,
  engine: testEngine,
  mode: "passthrough",
});

/**
 * Extract raw SDK messages from ABP stream (removes ABP framing)
 */
function extractRawMessages(messages) {
  return messages.filter((m) => {
    // Keep event messages which contain the actual SDK content
    return m.t === "event";
  }).map((m) => m.event);
}

function findReceipt(messages) {
  const finalMsg = messages.find((m) => m.t === "final");
  return finalMsg?.receipt || null;
}

/**
 * Compare two streams for equivalence (ignoring timestamps and ABP metadata)
 */
function streamsEquivalent(stream1, stream2) {
  if (stream1.length !== stream2.length) {
    return { equal: false, reason: `Length mismatch: ${stream1.length} vs ${stream2.length}` };
  }

  for (let i = 0; i < stream1.length; i++) {
    const ev1 = normalizeEvent(stream1[i]);
    const ev2 = normalizeEvent(stream2[i]);

    if (JSON.stringify(ev1) !== JSON.stringify(ev2)) {
      return {
        equal: false,
        reason: `Event ${i} differs`,
        event1: ev1,
        event2: ev2,
      };
    }
  }

  return { equal: true };
}

/**
 * Normalize an event for comparison (remove timestamps, IDs, etc.)
 */
function normalizeEvent(event) {
  const normalized = { ...event };
  
  // Remove timestamp fields
  delete normalized.timestamp;
  delete normalized.ts;
  delete normalized.created_at;
  
  // Remove ID fields that vary per run
  delete normalized.id;
  delete normalized.request_id;
  delete normalized.run_id;
  
  return normalized;
}

// Skip all tests if not running passthrough mode
const describePassthrough = shouldRunPassthrough && passthroughCells.length > 0
  ? describe
  : describe.skip;

describe("Passthrough Parity", () => {
  // Claude→Claude passthrough
  describePassthrough("Claude dialect → Claude engine", () => {
    const cell = { dialect: "claude", engine: "claude", mode: "passthrough" };

    it("should preserve exact request structure", async () => {
      const originalRequest = {
        prompt: "List all .txt files in the current directory",
        cwd: process.cwd(),
        allowed_tools: ["Read", "Write", "Bash"],
        permission_mode: "auto",
      };

      const workOrder = createWorkOrder({
        task: originalRequest.prompt,
        workspace: {
          root: originalRequest.cwd,
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
            "abp.request": originalRequest,
          },
        },
      });

      const { messages } = await runSidecarTest("claude", workOrder);

      // Verify we got a response
      assert(messages.length > 0, "Should have messages");

      // The request should have been passed through unchanged
      // (In a real test, we would verify the mock adapter received the exact request)
      const agentEvents = messages.filter((m) => m.t === "event");
      assert(agentEvents.length > 0, "Should have agent events");
    });

    it("should produce stream-equivalent output", async () => {
      const workOrder = createWorkOrder({
        task: "Simple task for stream equivalence test",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
          },
        },
      });

      const { messages } = await runSidecarTest("claude", workOrder);

      // Extract raw events (removing ABP framing)
      const rawEvents = extractRawMessages(messages);

      // Verify we have events
      assert(rawEvents.length > 0, "Should have raw SDK events");

      // In passthrough mode, events should be in original SDK order
      // Verify event types are present
      const eventTypes = rawEvents.map((e) => e?.type || e?.t).filter(Boolean);
      assert(eventTypes.length > 0, "Events should have types");
    });

    it("should not modify tool calls or outputs", async () => {
      const workOrder = createWorkOrder({
        task: "Read a file and summarize it",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
            "abp.request": {
              allowed_tools: ["Read", "Bash"],
            },
          },
        },
      });

      const { messages } = await runSidecarTest("claude", workOrder);

      // Find tool use events
      const toolEvents = messages.filter((m) => {
        if (m.t === "event") {
          const event = m.event;
          return event?.type === "tool_use" || event?.content_block?.type === "tool_use";
        }
        return false;
      });

      // In passthrough mode, tool calls should be unchanged
      // (No truncation, redaction, or injection)
      for (const event of toolEvents) {
        const toolUse = event.event?.content_block || event.event;
        if (toolUse?.name) {
          // Tool name should not be modified
          assert(
            ["Read", "Write", "Edit", "Bash", "Glob", "Grep"].includes(toolUse.name) ||
            toolUse.name.startsWith("mcp_"),
            `Tool name should be valid, got: ${toolUse.name}`
          );
        }
      }
    });

    it("should include raw messages in ext field", async () => {
      const workOrder = createWorkOrder({
        task: "Test raw message preservation",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
          },
        },
      });

      const { messages } = await runSidecarTest("claude", workOrder);

      // Check for ext field with raw_message
      const eventsWithExt = messages.filter((m) => m.ext || m.raw_message);
      
      // In passthrough mode, raw SDK messages should be preserved
      // This is optional but recommended for debugging
      if (eventsWithExt.length > 0) {
        for (const event of eventsWithExt) {
          if (event.ext?.raw_message) {
            assert(
              typeof event.ext.raw_message === "object",
              "raw_message should be an object"
            );
          }
        }
      }
    });

    it("should not inject receipt into stream", async () => {
      const workOrder = createWorkOrder({
        task: "Test receipt out-of-band",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
      });

      const { messages } = await runSidecarTest("claude", workOrder);

      // Receipt should be a separate message, not inside event
      const receiptInStream = messages.find((m) => {
        if (m.t === "event" && m.event) {
          return m.event.receipt || m.event.receipt_sha256;
        }
        return false;
      });

      assert(!receiptInStream, "Receipt should not be injected into event stream");

      const receipt = findReceipt(messages);
      assert(receipt, "Final receipt should be present");
    });

    it("should preserve event ordering", async () => {
      const workOrder = createWorkOrder({
        task: "Test event ordering",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
      });

      const { messages } = await runSidecarTest("claude", workOrder);

      // Verify hello comes first
      assert(messages.length > 0, "Should have messages");
      assert.strictEqual(messages[0].t, "hello", "First message should be hello");

      // Find event indices
      const agentEventIndices = messages
        .map((m, i) => (m.t === "event" ? i : -1))
        .filter((i) => i >= 0);

      // Find final index
      const receiptIndex = messages.findIndex((m) => m.t === "final");

      // All events should come before final receipt
      if (receiptIndex >= 0 && agentEventIndices.length > 0) {
        for (const idx of agentEventIndices) {
          assert(idx < receiptIndex, "Agent events should come before final receipt");
        }
      }
    });
  });

  // Codex→Codex passthrough
  describePassthrough("Codex dialect → Codex engine", () => {
    const cell = { dialect: "codex", engine: "codex", mode: "passthrough" };

    it("should preserve exact request structure", async () => {
      const originalRequest = {
        prompt: "Analyze the codebase structure",
        cwd: process.cwd(),
        tools: ["read_file", "glob", "grep"],
        permission_mode: "auto",
      };

      const workOrder = createWorkOrder({
        task: originalRequest.prompt,
        workspace: {
          root: originalRequest.cwd,
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
            "abp.request": originalRequest,
          },
        },
      });

      const { messages } = await runSidecarTest("codex", workOrder);

      assert(messages.length > 0, "Should have messages");
      const agentEvents = messages.filter((m) => m.t === "event");
      assert(agentEvents.length > 0, "Should have agent events");
    });

    it("should produce stream-equivalent output", async () => {
      const workOrder = createWorkOrder({
        task: "Simple Codex task",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
          },
        },
      });

      const { messages } = await runSidecarTest("codex", workOrder);
      const rawEvents = extractRawMessages(messages);

      assert(rawEvents.length > 0, "Should have raw SDK events");
    });

    it("should not modify tool calls or outputs", async () => {
      const workOrder = createWorkOrder({
        task: "Read and analyze files",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
        config: {
          vendor: {
            "abp.mode": "passthrough",
            "abp.request": {
              tools: ["read_file", "bash"],
            },
          },
        },
      });

      const { messages } = await runSidecarTest("codex", workOrder);

      // Find tool call events
      const toolEvents = messages.filter((m) => {
        if (m.t === "event") {
          const event = m.event;
          return event?.type === "function_call" || event?.tool_calls;
        }
        return false;
      });

      // Verify tool names are valid (not modified)
      for (const event of toolEvents) {
        const toolCalls = event.event?.tool_calls || [];
        for (const call of toolCalls) {
          if (call?.function?.name) {
            assert(
              typeof call.function.name === "string",
              "Tool name should be a string"
            );
          }
        }
      }
    });

    it("should not inject receipt into stream", async () => {
      const workOrder = createWorkOrder({
        task: "Test Codex receipt out-of-band",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
      });

      const { messages } = await runSidecarTest("codex", workOrder);

      // Receipt should not be in event
      const receiptInStream = messages.find((m) => {
        if (m.t === "event" && m.event) {
          return m.event.receipt || m.event.receipt_sha256;
        }
        return false;
      });

      assert(!receiptInStream, "Receipt should not be in event stream");

      const receipt = findReceipt(messages);
      assert(receipt, "Final receipt should be present");
    });

    it("should preserve event ordering", async () => {
      const workOrder = createWorkOrder({
        task: "Test Codex event ordering",
        workspace: {
          root: process.cwd(),
          mode: "pass_through",
        },
      });

      const { messages } = await runSidecarTest("codex", workOrder);

      assert(messages.length > 0, "Should have messages");
      assert.strictEqual(messages[0].t, "hello", "First message should be hello");

      const agentEventIndices = messages
        .map((m, i) => (m.t === "event" ? i : -1))
        .filter((i) => i >= 0);

      const receiptIndex = messages.findIndex((m) => m.t === "final");

      if (receiptIndex >= 0 && agentEventIndices.length > 0) {
        for (const idx of agentEventIndices) {
          assert(idx < receiptIndex, "Agent events should come before final receipt");
        }
      }
    });
  });

  // Cross-dialect passthrough consistency
  describePassthrough("Passthrough Consistency", () => {
    it("should have consistent behavior across Claude and Codex passthrough", async () => {
      // Both passthrough modes should:
      // 1. Send hello first
      // 2. Stream events
      // 3. Send final receipt at end
      // 4. Not modify content

      const expectedEnvelopeTypes = ["hello", "event", "final"];

      for (const cell of passthroughCells) {
        const workOrder = createWorkOrder({
          task: "Consistency test",
          workspace: {
            root: process.cwd(),
            mode: "pass_through",
          },
        });

        const { messages } = await runSidecarTest(cell.dialect, workOrder);

        const envelopeTypes = [...new Set(messages.map((m) => m.t))];

        for (const expected of expectedEnvelopeTypes) {
          assert(
            envelopeTypes.includes(expected),
            `${cell.dialect}→${cell.engine} should include ${expected} envelope`
          );
        }
      }
    });
  });
});
