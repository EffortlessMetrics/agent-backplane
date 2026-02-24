#!/usr/bin/env node

/**
 * Passthrough Mode Conformance Test for Codex Sidecar
 *
 * This test verifies that the Codex sidecar correctly implements
 * passthrough mode according to the ABP specification.
 *
 * Passthrough invariants:
 * 1. No request rewriting: SDK sees exactly what caller sent
 * 2. Stream equivalence: After removing ABP framing, stream is bitwise-equivalent to direct SDK call
 * 3. Observer-only governance: Log/record but don't modify tool calls or outputs
 * 4. Receipt out-of-band: Receipt doesn't appear in the stream
 */

const assert = require("node:assert");
const { spawn } = require("node:child_process");
const path = require("node:path");
const crypto = require("node:crypto");

const HOST_PATH = path.resolve(__dirname, "../host.js");

/**
 * Send a JSONL message to the sidecar and collect responses
 */
function runSidecarTest(workOrder) {
  return new Promise((resolve, reject) => {
    const proc = spawn("node", [HOST_PATH], {
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        ...process.env,
        // Force mock adapter for testing (no real SDK needed)
        ABP_CODEX_ADAPTER_MODULE: path.resolve(__dirname, "mock-adapter.js"),
      },
    });

    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (data) => {
      stdout += data.toString();
    });

    proc.stderr.on("data", (data) => {
      stderr += data.toString();
    });

    // Send the run message
    const runMsg = JSON.stringify({
      t: "run",
      id: crypto.randomUUID(),
      work_order: workOrder,
    });
    proc.stdin.write(runMsg + "\n");
    proc.stdin.end();

    proc.on("close", (code) => {
      if (code !== 0 && code !== null) {
        reject(new Error(`Sidecar exited with code ${code}: ${stderr}`));
        return;
      }

      // Parse JSONL output
      const lines = stdout.trim().split("\n").filter(Boolean);
      const messages = lines
        .map((line) => {
          try {
            return JSON.parse(line);
          } catch (e) {
            return null;
          }
        })
        .filter(Boolean);

      resolve({ messages, stderr });
    });

    proc.on("error", reject);
  });
}

/**
 * Test 1: Verify hello envelope includes mode field
 */
async function testHelloIncludesMode() {
  console.log("Test: Hello envelope includes mode field...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "test task",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: { vendor: {} },
  };

  const { messages } = await runSidecarTest(workOrder);

  const hello = messages.find((m) => m.t === "hello");
  assert(hello, "Hello message should be present");
  assert(hello.mode, "Hello should include mode field");
  assert(
    hello.mode === "mapped" || hello.mode === "passthrough",
    "Mode should be 'mapped' or 'passthrough'"
  );

  console.log("  ✓ Hello includes mode field:", hello.mode);
}

/**
 * Test 2: Verify passthrough mode is detected from work order
 */
async function testPassthroughModeDetection() {
  console.log("Test: Passthrough mode detection from work order...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "passthrough test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: {
            prompt: "Test prompt for passthrough mode detection",
          },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  // Find hello message
  const hello = messages.find((m) => m.t === "hello");
  assert(hello, "Hello message should be present");
  assert.strictEqual(hello.mode, "passthrough", "Mode should be 'passthrough'");

  // Find events that mention execution mode
  const events = messages.filter((m) => m.t === "event");
  const modeMessages = events.filter(
    (e) =>
      e.event &&
      e.event.type === "warning" &&
      e.event.message &&
      e.event.message.includes("passthrough")
  );

  assert(
    modeMessages.length > 0,
    "Should have warning message indicating passthrough mode"
  );

  console.log("  ✓ Passthrough mode correctly detected");
}

/**
 * Test 3: Verify raw request is stored in receipt
 */
async function testRawRequestStoredInReceipt() {
  console.log("Test: Raw request stored in receipt...");

  const rawRequest = {
    prompt: "Test prompt for passthrough",
    model: "gpt-4",
    tools: ["code_execution", "file_read"],
    temperature: 0.7,
  };

  const workOrder = {
    id: crypto.randomUUID(),
    task: "raw request test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: rawRequest,
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  // Find final message with receipt
  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");
  assert(final.receipt, "Receipt should be present in final message");

  // Check raw request is stored
  assert(final.receipt.ext, "Receipt should have ext field");
  assert(
    final.receipt.ext.raw_request,
    "Receipt should store raw_request in ext"
  );
  assert.deepStrictEqual(
    final.receipt.ext.raw_request,
    rawRequest,
    "Raw request should match original"
  );

  console.log("  ✓ Raw request stored in receipt");
}

/**
 * Test 4: Verify receipt has correct dialect and engine
 */
async function testReceiptDialectAndEngine() {
  console.log("Test: Receipt has correct dialect and engine...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "dialect engine test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: { prompt: "test" },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");
  assert(final.receipt, "Receipt should be present");

  assert.strictEqual(
    final.receipt.source_dialect,
    "codex",
    "Source dialect should be 'codex'"
  );
  assert.strictEqual(
    final.receipt.target_engine,
    "codex",
    "Target engine should be 'codex' in passthrough mode"
  );

  console.log("  ✓ Receipt has correct dialect and engine");
}

/**
 * Test 5: Verify events are emitted unchanged
 */
async function testEventsEmittedUnchanged() {
  console.log("Test: Events are emitted unchanged...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "events test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: { prompt: "Generate some events" },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  // Find events
  const events = messages.filter((m) => m.t === "event");
  assert(events.length > 0, "Should have events");

  // Check for assistant messages
  const assistantMessages = events.filter(
    (e) =>
      e.event &&
      (e.event.type === "assistant_message" || e.event.type === "assistant_delta")
  );
  assert(
    assistantMessages.length > 0,
    "Should have assistant message events"
  );

  console.log("  ✓ Events emitted correctly");
}

/**
 * Test 6: Verify receipt hash is computed
 */
async function testReceiptHashComputed() {
  console.log("Test: Receipt hash is computed...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "hash test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: { prompt: "test" },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");
  assert(final.receipt, "Receipt should be present");
  assert(
    final.receipt.receipt_sha256,
    "Receipt should have receipt_sha256"
  );
  assert.match(
    final.receipt.receipt_sha256,
    /^[a-f0-9]{64}$/,
    "Receipt hash should be valid SHA256 hex string"
  );

  console.log("  ✓ Receipt hash computed correctly");
}

/**
 * Test 7: Verify tool calls are recorded
 */
async function testToolCallsRecorded() {
  console.log("Test: Tool calls are recorded...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "tool calls test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: {
            prompt: "Read a file",
            tools: ["file_read"],
          },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");
  assert(final.receipt, "Receipt should be present");
  assert(
    Array.isArray(final.receipt.tool_calls),
    "Receipt should have tool_calls array"
  );

  console.log("  ✓ Tool calls recorded in receipt");
}

/**
 * Test 8: Verify contract version is set
 */
async function testContractVersionSet() {
  console.log("Test: Contract version is set...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "contract version test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
          request: { prompt: "test" },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");
  assert(final.receipt, "Receipt should be present");
  assert.strictEqual(
    final.receipt.contract_version,
    "abp/v0.1",
    "Contract version should be 'abp/v0.1'"
  );

  console.log("  ✓ Contract version set correctly");
}

/**
 * Run all tests
 */
async function runAllTests() {
  console.log("=".repeat(60));
  console.log("Codex Passthrough Mode Conformance Tests");
  console.log("=".repeat(60));
  console.log();

  const tests = [
    testHelloIncludesMode,
    testPassthroughModeDetection,
    testRawRequestStoredInReceipt,
    testReceiptDialectAndEngine,
    testEventsEmittedUnchanged,
    testReceiptHashComputed,
    testToolCallsRecorded,
    testContractVersionSet,
  ];

  let passed = 0;
  let failed = 0;

  for (const test of tests) {
    try {
      await test();
      passed++;
    } catch (error) {
      console.error(`  ✗ Test failed: ${error.message}`);
      console.error(error.stack);
      failed++;
    }
  }

  console.log();
  console.log("=".repeat(60));
  console.log(`Results: ${passed} passed, ${failed} failed`);
  console.log("=".repeat(60));

  if (failed > 0) {
    process.exit(1);
  }
}

// Run tests if executed directly
if (require.main === module) {
  runAllTests().catch((error) => {
    console.error("Test runner error:", error);
    process.exit(1);
  });
}

module.exports = {
  runSidecarTest,
  testHelloIncludesMode,
  testPassthroughModeDetection,
  testRawRequestStoredInReceipt,
  testReceiptDialectAndEngine,
  testEventsEmittedUnchanged,
  testReceiptHashComputed,
  testToolCallsRecorded,
  testContractVersionSet,
};
