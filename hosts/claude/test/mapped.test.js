const assert = require("node:assert");
const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const path = require("node:path");
const { test } = require("node:test");

const HOST_PATH = path.resolve(__dirname, "../host.js");
const MOCK_SDK_PATH = path.resolve(__dirname, "mock-sdk.js");

function runSidecar(workOrder, extraEnv = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn("node", [HOST_PATH], {
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        ...process.env,
        ABP_CLAUDE_SDK_MODULE: MOCK_SDK_PATH,
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

test("mapped mode streams assistant/tool events with local Claude SDK adapter", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "summarize README.md",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {
      files: ["README.md"],
    },
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {},
    },
  };

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.mode, "mapped");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage.input_tokens, 42);
  assert.strictEqual(final.receipt.usage.output_tokens, 7);
  assert.strictEqual(final.receipt.usage_raw.sdk_module, MOCK_SDK_PATH);

  const assistantDeltas = events.filter((event) => event.event?.type === "assistant_delta");
  assert(
    assistantDeltas.some((event) => event.event?.text === "Mapped "),
    "expected mapped assistant delta"
  );

  const toolCalls = events.filter((event) => event.event?.type === "tool_call");
  assert(
    toolCalls.some((event) => event.event?.tool_name === "Read"),
    "expected mapped Read tool call"
  );

  const toolResults = events.filter((event) => event.event?.type === "tool_result");
  assert(
    toolResults.some((event) => event.event?.tool_name === "Read"),
    "expected mapped Read tool result"
  );
});

test("mapped mode supports abp.client_mode with SDK client lifecycle", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "summarize README.md",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {
      files: ["README.md"],
    },
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          client_mode: true,
        },
      },
    },
  };

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.mode, "mapped");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage.input_tokens, 84);
  assert.strictEqual(final.receipt.usage.output_tokens, 14);
  assert.strictEqual(final.receipt.usage_raw.transport, "client");
  assert.strictEqual(final.receipt.usage_raw.client_mode, true);
  assert.strictEqual(final.receipt.usage_raw.sdk_module, MOCK_SDK_PATH);

  const assistantDeltas = events.filter((event) => event.event?.type === "assistant_delta");
  assert(
    assistantDeltas.some((event) => event.event?.text === "Client "),
    "expected client assistant delta"
  );

  const toolCalls = events.filter((event) => event.event?.type === "tool_call");
  assert(
    toolCalls.some((event) => event.event?.tool_name === "Read"),
    "expected client Read tool call"
  );
});

test("abp.client_mode falls back to query() when SDK client is unavailable", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "summarize README.md",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {
      files: ["README.md"],
    },
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {
        abp: {
          client_mode: true,
        },
      },
    },
  };

  const { messages } = await runSidecar(workOrder, {
    MOCK_CLAUDE_DISABLE_CLIENT: "1",
  });
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.mode, "mapped");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage.input_tokens, 42);
  assert.strictEqual(final.receipt.usage.output_tokens, 7);
  assert.strictEqual(final.receipt.usage_raw.transport, "query");
  assert.strictEqual(final.receipt.usage_raw.client_mode, false);

  const warnings = events.filter((event) => event.event?.type === "warning");
  assert(
    warnings.some((event) =>
      String(event.event?.message || "").includes("abp.client_mode=true requested")
    ),
    "expected warning when client_mode falls back to query()"
  );
});
