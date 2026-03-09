const assert = require("node:assert");
const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const path = require("node:path");
const { test } = require("node:test");

const HOST_PATH = path.resolve(__dirname, "../host.js");
const MOCK_SDK_V2_PATH = path.resolve(__dirname, "mock-sdk-v2.js");

function runSidecar(workOrder, extraEnv = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn("node", [HOST_PATH], {
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        ...process.env,
        ABP_CLAUDE_SDK_MODULE: MOCK_SDK_V2_PATH,
        ...extraEnv,
      },
    });

    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });

    proc.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });

    proc.on("error", reject);

    proc.on("close", (code) => {
      if (code !== 0 && code !== null) {
        reject(new Error(`sidecar exited with code ${code}: ${stderr}`));
        return;
      }

      const messages = stdout
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((line) => {
          try {
            return JSON.parse(line);
          } catch (_) {
            return null;
          }
        })
        .filter(Boolean);

      resolve({ messages, stderr });
    });

    proc.stdin.write(
      JSON.stringify({
        t: "run",
        id: crypto.randomUUID(),
        work_order: workOrder,
      }) + "\n"
    );
    proc.stdin.end();
  });
}

function makeV2WorkOrder(task, extraVendor = {}) {
  return {
    id: crypto.randomUUID(),
    task,
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        claude: {
          v2_preview: true,
          ...extraVendor,
        },
      },
    },
  };
}

// -----------------------------------------------------------------------
// Test: V2 prompt transport produces correct events and preview labels
// -----------------------------------------------------------------------
test("V2 prompt transport streams events and labels receipt as preview", async () => {
  const workOrder = makeV2WorkOrder("summarize README.md");

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((m) => m.t === "event");
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");

  // Preview labels
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v2_preview");
  assert.strictEqual(final.receipt.usage_raw.api_version, "v2_preview");
  assert.strictEqual(final.receipt.usage_raw.preview, true);

  // Should have assistant deltas from V2 prompt
  const deltas = events.filter((e) => e.event?.type === "assistant_delta");
  assert(
    deltas.some((e) => e.event?.text === "V2 "),
    "expected V2 assistant delta"
  );

  // Should have tool events
  const toolCalls = events.filter((e) => e.event?.type === "tool_call");
  assert(
    toolCalls.some((e) => e.event?.tool_name === "Read"),
    "expected V2 Read tool call"
  );

  const toolResults = events.filter((e) => e.event?.type === "tool_result");
  assert(
    toolResults.some((e) => e.event?.tool_name === "Read"),
    "expected V2 Read tool result"
  );

  // Usage
  assert.strictEqual(final.receipt.usage.input_tokens, 50);
  assert.strictEqual(final.receipt.usage.output_tokens, 12);
});

// -----------------------------------------------------------------------
// Test: V2 prompt transport with sdk_surface selector
// -----------------------------------------------------------------------
test("V2 adapter selected via claude.sdk_surface = ts_v2_preview", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test sdk_surface selection",
    lane: "patch_first",
    workspace: { root: process.cwd(), mode: "pass_through" },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        claude: {
          sdk_surface: "ts_v2_preview",
        },
      },
    },
  };

  const { messages } = await runSidecar(workOrder);
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v2_preview");
  assert.strictEqual(final.receipt.usage_raw.preview, true);
  assert.strictEqual(final.receipt.outcome, "complete");
});

// -----------------------------------------------------------------------
// Test: V2 session transport (create + send + close)
// -----------------------------------------------------------------------
test("V2 session transport creates session, streams, and emits lifecycle events", async () => {
  const workOrder = makeV2WorkOrder("session test", {
    options: {
      sessionId: "v2-sess-001",
    },
  });

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((m) => m.t === "event");
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v2_preview");
  assert.strictEqual(final.receipt.usage_raw.transport, "v2_session");

  // Session lifecycle: session_started
  const sessionStarted = events.filter((e) => e.event?.type === "session_started");
  assert(
    sessionStarted.length > 0,
    "expected session_started event"
  );

  // Session lifecycle: session_completed
  const sessionCompleted = events.filter((e) => e.event?.type === "session_completed");
  assert(
    sessionCompleted.length > 0,
    "expected session_completed event"
  );

  // Session deltas
  const deltas = events.filter((e) => e.event?.type === "assistant_delta");
  assert(
    deltas.some((e) => e.event?.text === "session "),
    "expected V2 session assistant delta"
  );

  // Usage
  assert.strictEqual(final.receipt.usage.input_tokens, 60);
  assert.strictEqual(final.receipt.usage.output_tokens, 15);
});

// -----------------------------------------------------------------------
// Test: V2 session resume transport
// -----------------------------------------------------------------------
test("V2 session resume emits session_resumed and uses resume transport", async () => {
  const workOrder = makeV2WorkOrder("resume test", {
    options: {
      sessionId: "v2-sess-002",
      resume: "checkpoint-xyz",
    },
  });

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((m) => m.t === "event");
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage_raw.transport, "v2_session_resume");
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v2_preview");

  // Session lifecycle: session_resumed
  const sessionResumed = events.filter((e) => e.event?.type === "session_resumed");
  assert(
    sessionResumed.some(
      (e) =>
        e.event?.session_id === "v2-sess-002" &&
        e.event?.resumed_from === "checkpoint-xyz"
    ),
    "expected session_resumed event with session_id and resumed_from"
  );

  // Session lifecycle: session_completed
  const sessionCompleted = events.filter((e) => e.event?.type === "session_completed");
  assert(sessionCompleted.length > 0, "expected session_completed event");

  // Resume-specific deltas
  const deltas = events.filter((e) => e.event?.type === "assistant_delta");
  assert(
    deltas.some((e) => e.event?.text === "resumed "),
    "expected V2 resumed assistant delta"
  );

  // Usage
  assert.strictEqual(final.receipt.usage.input_tokens, 30);
  assert.strictEqual(final.receipt.usage.output_tokens, 8);
});

// -----------------------------------------------------------------------
// Test: V1-only option rejection
// -----------------------------------------------------------------------
test("V1-only options produce warnings and are stripped in V2 mode", async () => {
  const workOrder = makeV2WorkOrder("v1 rejection test", {
    options: {
      forkSession: true,
      cliPath: "/usr/bin/claude",
      extraArgs: ["--verbose"],
      stderr: "pipe",
      maxBufferSize: 1048576,
      // V2-compatible option that should survive
      model: "claude-sonnet-4-20250514",
    },
  });

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((m) => m.t === "event");
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");

  // Should have warnings for each V1-only option
  const warnings = events.filter((e) => e.event?.type === "warning");
  const warningTexts = warnings.map((e) => e.event?.message || "");

  const expectedV1Keys = ["forkSession", "cliPath", "extraArgs", "stderr", "maxBufferSize"];
  for (const key of expectedV1Keys) {
    assert(
      warningTexts.some((text) => text.includes(key)),
      `expected warning for V1-only option '${key}'`
    );
  }

  // Preview labels should still be present
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v2_preview");
  assert.strictEqual(final.receipt.usage_raw.preview, true);
});

// -----------------------------------------------------------------------
// Test: Preview labels are always present on V2 receipts
// -----------------------------------------------------------------------
test("V2 receipt always contains preview labels in usage_raw", async () => {
  const workOrder = makeV2WorkOrder("preview label check");

  const { messages } = await runSidecar(workOrder);
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  const usageRaw = final.receipt.usage_raw;

  assert.strictEqual(usageRaw.sdk_surface, "ts_v2_preview",
    "usage_raw.sdk_surface must be 'ts_v2_preview'");
  assert.strictEqual(usageRaw.api_version, "v2_preview",
    "usage_raw.api_version must be 'v2_preview'");
  assert.strictEqual(usageRaw.preview, true,
    "usage_raw.preview must be true");
});

// -----------------------------------------------------------------------
// Test: Unit tests for buildV2Request and rejectV1OnlyOptions
// -----------------------------------------------------------------------
test("buildV2Request constructs request from work order", () => {
  const adapter = require("../adapter-v2.js");
  const ctx = {
    workOrder: {
      id: crypto.randomUUID(),
      task: "unit test task",
      context: {
        files: ["src/main.rs"],
      },
      config: {
        vendor: {
          claude: {
            options: {
              model: "claude-sonnet-4-20250514",
              maxTurns: 5,
              effort: "high",
            },
          },
        },
      },
    },
    sdkOptions: {},
  };

  const request = adapter._test.buildV2Request(ctx);

  assert(request.prompt.includes("unit test task"), "prompt should contain task");
  assert(request.prompt.includes("src/main.rs"), "prompt should contain context files");
  assert.strictEqual(request.options.model, "claude-sonnet-4-20250514");
  assert.strictEqual(request.options.maxTurns, 5);
  assert.strictEqual(request.options.effort, "high");
});

test("rejectV1OnlyOptions strips V1-only keys and emits warnings", () => {
  const adapter = require("../adapter-v2.js");
  const warnings = [];
  const ctx = {
    emitWarning(msg) { warnings.push(msg); },
  };

  const options = {
    model: "claude-sonnet-4-20250514",
    forkSession: true,
    cliPath: "/usr/bin/claude",
    extraArgs: ["--debug"],
    stderr: "pipe",
    maxBufferSize: 9999,
    maxTurns: 10,
  };

  const cleaned = adapter._test.rejectV1OnlyOptions(ctx, options);

  // V1-only keys should be removed
  assert.strictEqual(cleaned.forkSession, undefined);
  assert.strictEqual(cleaned.cliPath, undefined);
  assert.strictEqual(cleaned.extraArgs, undefined);
  assert.strictEqual(cleaned.stderr, undefined);
  assert.strictEqual(cleaned.maxBufferSize, undefined);

  // V2-compatible keys should survive
  assert.strictEqual(cleaned.model, "claude-sonnet-4-20250514");
  assert.strictEqual(cleaned.maxTurns, 10);

  // Should have 5 warnings
  assert.strictEqual(warnings.length, 5);
  for (const key of adapter._test.V1_ONLY) {
    assert(
      warnings.some((w) => w.includes(key)),
      `expected warning for '${key}'`
    );
  }
});

test("v2UsageRaw always includes preview fields", () => {
  const adapter = require("../adapter-v2.js");
  const raw = adapter._test.v2UsageRaw({ custom: "value" });

  assert.strictEqual(raw.sdk_surface, "ts_v2_preview");
  assert.strictEqual(raw.api_version, "v2_preview");
  assert.strictEqual(raw.preview, true);
  assert.strictEqual(raw.custom, "value");
});

test("selectTransport picks session when sessionId present", () => {
  const adapter = require("../adapter-v2.js");

  const sdk = {
    target: {
      unstable_v2_prompt: () => {},
      unstable_v2_createSession: () => {},
    },
  };

  assert.strictEqual(
    adapter._test.selectTransport(sdk, { sessionId: "abc" }),
    "session"
  );
  assert.strictEqual(
    adapter._test.selectTransport(sdk, {}),
    "prompt"
  );
  assert.strictEqual(
    adapter._test.selectTransport(sdk, { resume: "xyz", sessionId: "abc" }),
    "session"
  );
});

test("selectTransport picks prompt when session methods unavailable", () => {
  const adapter = require("../adapter-v2.js");

  const promptOnlySdk = {
    target: {
      unstable_v2_prompt: () => {},
    },
  };

  assert.strictEqual(
    adapter._test.selectTransport(promptOnlySdk, { sessionId: "abc" }),
    "prompt"
  );
  assert.strictEqual(
    adapter._test.selectTransport(promptOnlySdk, {}),
    "prompt"
  );
});

test("selectTransport returns null when no V2 methods available", () => {
  const adapter = require("../adapter-v2.js");

  const emptySdk = { target: {} };
  assert.strictEqual(adapter._test.selectTransport(emptySdk, {}), null);
});

// -----------------------------------------------------------------------
// Test: V2 prompt with session-only SDK falls back to session transport
// -----------------------------------------------------------------------
test("V2 falls back to session transport when prompt is unavailable", async () => {
  const workOrder = makeV2WorkOrder("session-only sdk test");

  const { messages } = await runSidecar(workOrder, {
    MOCK_V2_NO_PROMPT: "1",
  });
  const final = messages.find((m) => m.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v2_preview");
  // Should use session transport since prompt is unavailable
  assert.strictEqual(final.receipt.usage_raw.transport, "v2_session");
});

// -----------------------------------------------------------------------
// Test: V2 retry on retriable error
// -----------------------------------------------------------------------
test("V2 retries on retriable error and succeeds on second attempt", async () => {
  const workOrder = makeV2WorkOrder("retry test");
  const { messages } = await runSidecar(workOrder, {
    MOCK_V2_PROMPT_RETRIABLE: "1",
    ABP_CLAUDE_RETRY_DELAY_MS: "10",
  });
  const events = messages.filter((m) => m.t === "event");
  const final = messages.find((m) => m.t === "final");
  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");
  // Should have a warning about the retry
  const warnings = events.filter((e) => e.event?.type === "warning");
  assert(warnings.some((w) => w.event?.message?.includes("retrying")), "expected retry warning");
});

// -----------------------------------------------------------------------
// Test: V2 does NOT retry non-retriable error
// -----------------------------------------------------------------------
test("V2 does not retry non-retriable errors", async () => {
  const workOrder = makeV2WorkOrder("non-retriable test");
  const { messages } = await runSidecar(workOrder, {
    MOCK_V2_PROMPT_NON_RETRIABLE: "1",
  });
  const final = messages.find((m) => m.t === "final");
  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "failed");
  // Should NOT have retry warnings
  const events = messages.filter((m) => m.t === "event");
  const warnings = events.filter((e) => e.event?.type === "warning" && e.event?.message?.includes("retrying"));
  assert.strictEqual(warnings.length, 0, "should not have retry warnings for non-retriable error");
});

// -----------------------------------------------------------------------
// Test: V2 hook event mapping (unit test)
// -----------------------------------------------------------------------
test("emitV2Event handles hook events (pre_tool_use, post_tool_use)", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) { emitted.push({ type: "warning", message: msg }); },
    emitAssistantDelta(text) { emitted.push({ type: "assistant_delta", text }); },
    emitAssistantMessage(text) { emitted.push({ type: "assistant_message", text }); },
    emitError(msg) { emitted.push({ type: "error", message: msg }); },
    emitRaw(ev) { emitted.push({ type: "raw", ...ev }); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  // Pre tool use
  adapter._test.emitV2Event(ctx, {
    type: "pre_tool_use",
    tool_name: "bash",
    tool_use_id: "tu-1",
    input: { command: "ls" },
    decision: "allow",
  }, state);
  assert.strictEqual(emitted.length, 1);
  assert.strictEqual(emitted[0].type, "tool_call");
  assert.strictEqual(emitted[0].toolName, "bash");
  assert.deepStrictEqual(emitted[0].ext, { hook: "pre_tool_use", decision: "allow" });

  // Post tool use
  emitted.length = 0;
  adapter._test.emitV2Event(ctx, {
    type: "post_tool_use",
    tool_name: "bash",
    tool_use_id: "tu-1",
    output: "file1.txt",
  }, state);
  assert.strictEqual(emitted[0].type, "tool_result");
  assert.strictEqual(emitted[0].toolName, "bash");
  assert.deepStrictEqual(emitted[0].ext, { hook: "post_tool_use" });
  assert.strictEqual(emitted[0].isError, false);

  // Post tool use failure
  emitted.length = 0;
  adapter._test.emitV2Event(ctx, {
    type: "post_tool_use_failure",
    tool_name: "bash",
    tool_use_id: "tu-2",
    output: "command failed",
    is_error: true,
  }, state);
  assert.strictEqual(emitted[0].type, "tool_result");
  assert.deepStrictEqual(emitted[0].ext, { hook: "post_tool_use_failure" });
  assert.strictEqual(emitted[0].isError, true);
});

// -----------------------------------------------------------------------
// Test: V2 permission event mapping (unit test)
// -----------------------------------------------------------------------
test("emitV2Event handles permission_request events", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) { emitted.push({ type: "warning", message: msg }); },
    emitAssistantDelta(text) { emitted.push({ type: "assistant_delta", text }); },
    emitAssistantMessage(text) { emitted.push({ type: "assistant_message", text }); },
    emitError(msg) { emitted.push({ type: "error", message: msg }); },
    emitRaw(ev) { emitted.push(ev); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  adapter._test.emitV2Event(ctx, {
    type: "permission_request",
    tool_name: "write",
    input: { path: "/etc/passwd" },
  }, state);

  assert.strictEqual(emitted.length, 2);
  assert.strictEqual(emitted[0].type, "permission_requested");
  assert.strictEqual(emitted[0].tool_name, "write");
  assert.strictEqual(emitted[1].type, "permission_resolved");
  assert.strictEqual(emitted[1].granted, true);
});

// -----------------------------------------------------------------------
// Test: V2 checkpoint event mapping (unit test)
// -----------------------------------------------------------------------
test("emitV2Event handles checkpoint events", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) { emitted.push({ type: "warning", message: msg }); },
    emitAssistantDelta(text) { emitted.push({ type: "assistant_delta", text }); },
    emitAssistantMessage(text) { emitted.push({ type: "assistant_message", text }); },
    emitError(msg) { emitted.push({ type: "error", message: msg }); },
    emitRaw(ev) { emitted.push(ev); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  adapter._test.emitV2Event(ctx, {
    type: "checkpoint",
    checkpoint_id: "ckpt-42",
  }, state);

  // Should emit assistant_message + raw event with checkpoint_id
  const assistantMsgs = emitted.filter(e => e.type === "assistant_message");
  assert(assistantMsgs.some(e => e.text?.includes("ckpt-42")), "should mention checkpoint id");
  const rawEvents = emitted.filter(e => e.ext?.checkpoint_id === "ckpt-42");
  assert(rawEvents.length > 0, "should emit raw event with checkpoint_id in ext");
});
