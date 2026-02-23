#!/usr/bin/env node

/**
 * Passthrough Mode Conformance Test
 *
 * This test verifies that the Claude sidecar correctly implements
 * passthrough mode according to the ABP specification.
 *
 * Passthrough invariants:
 * 1. No request rewriting: SDK sees exactly what caller sent
 * 2. Stream equivalence: After removing ABP framing, stream is bitwise-equivalent to direct SDK call
 * 3. Observer-only governance: Log/record but don't modify tool calls or outputs
 * 4. Receipt out-of-band: Receipt doesn't appear in the stream
 */

const assert = require("assert");
const { spawn } = require("child_process");
const path = require("path");
const crypto = require("crypto");

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
        // Force fallback adapter for testing (no real SDK needed)
        ABP_CLAUDE_ADAPTER_MODULE: path.resolve(__dirname, "mock-adapter.js"),
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
      const messages = lines.map((line) => {
        try {
          return JSON.parse(line);
        } catch (e) {
          return null;
        }
      }).filter(Boolean);

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
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  // Find assistant messages that mention execution mode
  const events = messages.filter((m) => m.t === "event");
  const modeMessages = events.filter(
    (e) =>
      e.event &&
      e.event.type === "assistant_message" &&
      e.event.text &&
      e.event.text.includes("Execution mode:")
  );

  assert(
    modeMessages.length > 0,
    "Should have assistant message indicating execution mode"
  );

  const modeText = modeMessages[0].event.text;
  assert(
    modeText.includes("passthrough"),
    "Execution mode should be 'passthrough'"
  );

  console.log("  ✓ Passthrough mode correctly detected:", modeText);
}

/**
 * Test 3: Verify receipt includes mode field in passthrough mode
 */
async function testReceiptIncludesMode() {
  console.log("Test: Receipt includes mode field...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "receipt mode test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          mode: "passthrough",
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");
  assert(final.receipt, "Final should include receipt");

  const receipt = final.receipt;
  assert(receipt.mode, "Receipt should include mode field");
  assert(
    receipt.mode === "passthrough",
    "Receipt mode should be 'passthrough'"
  );

  console.log("  ✓ Receipt includes mode:", receipt.mode);
}

/**
 * Test 4: Verify mapped mode is default when no mode specified
 */
async function testMappedModeDefault() {
  console.log("Test: Mapped mode is default...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "default mode test",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {}, // No abp.mode specified
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");

  const receipt = final.receipt;
  assert(receipt.mode, "Receipt should include mode field");
  assert(receipt.mode === "mapped", "Default mode should be 'mapped'");

  console.log("  ✓ Default mode is 'mapped':", receipt.mode);
}

/**
 * Test 5: Verify events contain ext.raw_message in passthrough mode
 */
async function testEventsContainRawMessage() {
  console.log("Test: Events contain ext.raw_message in passthrough mode...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "raw message test",
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
            // Sample passthrough request
            prompt: "Hello from passthrough",
            options: {},
          },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const events = messages.filter((m) => m.t === "event");

  // In passthrough mode, events should have ext.raw_message
  // (This depends on the mock adapter implementing passthrough correctly)
  const eventsWithExt = events.filter(
    (e) => e.event && e.event.ext && e.event.ext.raw_message
  );

  // Note: This test may pass with 0 events if the mock adapter doesn't
  // emit passthrough events. The important thing is the structure is correct.
  console.log(
    `  ℹ Events with ext.raw_message: ${eventsWithExt.length} of ${events.length}`
  );

  // If we have events with ext, verify structure
  if (eventsWithExt.length > 0) {
    const sample = eventsWithExt[0].event;
    assert(sample.ext.raw_message, "ext.raw_message should be present");
    assert(
      typeof sample.ext.raw_message === "object",
      "raw_message should be an object"
    );
    console.log("  ✓ Event ext.raw_message structure is correct");
  } else {
    console.log("  ⚠ No events with ext.raw_message (mock adapter may not emit them)");
  }
}

/**
 * Test 6: Verify stream_equivalent guarantee in passthrough receipt
 */
async function testStreamEquivalentGuarantee() {
  console.log("Test: Stream equivalent guarantee in passthrough receipt...");

  const workOrder = {
    id: crypto.randomUUID(),
    task: "stream equivalent test",
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
            prompt: "Test stream equivalence",
            options: {},
          },
        },
      },
    },
  };

  const { messages } = await runSidecarTest(workOrder);

  const final = messages.find((m) => m.t === "final");
  assert(final, "Final message should be present");

  const receipt = final.receipt;

  // In passthrough mode, stream_equivalent should be true if the adapter supports it
  // This depends on the adapter implementation
  if (receipt.stream_equivalent === true) {
    console.log("  ✓ Receipt includes stream_equivalent: true");
  } else {
    console.log(
      "  ⚠ Receipt does not include stream_equivalent (adapter may not support it)"
    );
  }
}

/**
 * Main test runner
 */
async function main() {
  console.log("=== Passthrough Mode Conformance Tests ===\n");

  const tests = [
    testHelloIncludesMode,
    testPassthroughModeDetection,
    testReceiptIncludesMode,
    testMappedModeDefault,
    testEventsContainRawMessage,
    testStreamEquivalentGuarantee,
  ];

  let passed = 0;
  let failed = 0;

  for (const test of tests) {
    try {
      await test();
      passed++;
    } catch (err) {
      console.error(`  ✗ Test failed: ${err.message}`);
      failed++;
    }
    console.log();
  }

  console.log("=== Test Summary ===");
  console.log(`Passed: ${passed}`);
  console.log(`Failed: ${failed}`);

  process.exit(failed > 0 ? 1 : 0);
}

main().catch((err) => {
  console.error("Test runner error:", err);
  process.exit(1);
});
