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

test("hook events are normalized to tool_call/tool_result", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test hooks",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {},
    },
  };

  const { messages } = await runSidecar(workOrder, {
    MOCK_CLAUDE_HOOK_MODE: "1",
  });
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");

  const toolCalls = events.filter((event) => event.event?.type === "tool_call");
  assert(
    toolCalls.some((event) => event.event?.tool_name === "Write"),
    "expected hook pre_tool_use to emit tool_call for Write"
  );

  const toolResults = events.filter((event) => event.event?.type === "tool_result");
  assert(
    toolResults.some((event) => event.event?.tool_name === "Write"),
    "expected hook post_tool_use to emit tool_result for Write"
  );
});

test("typed SDK message dispatch handles content_block_delta and message_delta", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test typed dispatch",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {},
    },
  };

  const { messages } = await runSidecar(workOrder, {
    MOCK_CLAUDE_TYPED_MODE: "1",
  });
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");

  const assistantDeltas = events.filter((event) => event.event?.type === "assistant_delta");
  assert(
    assistantDeltas.some((event) => event.event?.text === "hello"),
    "expected content_block_delta to produce assistant_delta 'hello'"
  );
});

test("permission_requested events emitted when tool is denied by policy", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test permission denied",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {},
    policy: {
      disallowed_tools: ["Read"],
    },
    requirements: { required: [] },
    config: {
      vendor: {},
    },
  };

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((message) => message.t === "event");

  const permRequested = events.filter(
    (event) => event.event?.type === "permission_requested"
  );
  assert(
    permRequested.some((event) => event.event?.tool_name === "Read"),
    "expected permission_requested event for denied Read tool"
  );

  const permResolved = events.filter(
    (event) => event.event?.type === "permission_resolved"
  );
  assert(
    permResolved.some(
      (event) => event.event?.tool_name === "Read" && event.event?.granted === false
    ),
    "expected permission_resolved with granted=false for Read"
  );
});

test("AskUserQuestion is mapped to ask_user tool_call", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test ask user",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {},
    },
  };

  const { messages } = await runSidecar(workOrder, {
    MOCK_CLAUDE_ASK_USER_MODE: "1",
  });
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");

  const toolCalls = events.filter((event) => event.event?.type === "tool_call");
  assert(
    toolCalls.some((event) => event.event?.tool_name === "ask_user"),
    "expected AskUserQuestion to be mapped to ask_user tool_call"
  );
});

test("session lifecycle events emitted when sessionId is present", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test session lifecycle",
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
          options: {
            sessionId: "test-session-123",
          },
        },
      },
    },
  };

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((message) => message.t === "event");
  const final = messages.find((message) => message.t === "final");

  assert(final, "final envelope should be present");
  assert.strictEqual(final.receipt.outcome, "complete");
  assert.strictEqual(final.receipt.usage_raw.sdk_surface, "ts_v1");
  assert.strictEqual(final.receipt.usage_raw.session_id, "test-session-123");

  const sessionStarted = events.filter(
    (event) => event.event?.type === "session_started"
  );
  assert(
    sessionStarted.some((event) => event.event?.session_id === "test-session-123"),
    "expected session_started event with session_id"
  );
});

test("session_resumed emitted when resume option is present", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test session resume",
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
          options: {
            sessionId: "test-session-456",
            resume: "checkpoint-abc",
          },
        },
      },
    },
  };

  const { messages } = await runSidecar(workOrder);
  const events = messages.filter((message) => message.t === "event");

  const sessionResumed = events.filter(
    (event) => event.event?.type === "session_resumed"
  );
  assert(
    sessionResumed.some(
      (event) =>
        event.event?.session_id === "test-session-456" &&
        event.event?.resumed_from === "checkpoint-abc"
    ),
    "expected session_resumed event with session_id and resumed_from"
  );
});

test("V1 options pass through to SDK request", async () => {
  // This test validates that new V1 options are included in buildMappedRequest
  const adapter = require("../adapter.js");
  const ctx = {
    workOrder: {
      id: crypto.randomUUID(),
      task: "test v1 options",
      config: {
        vendor: {
          claude: {
            options: {
              tools: [{ name: "custom_tool", type: "function" }],
              continue: true,
              forkSession: true,
              maxBudgetUsd: 5.0,
              includePartialMessages: true,
              outputFormat: "json",
              effort: "high",
              persistSession: true,
              enableFileCheckpointing: true,
              sandbox: { enabled: true },
              thinking: { enabled: true, budget_tokens: 1000 },
              hooks: { onToolCall: "log" },
              agents: [{ name: "sub_agent" }],
              plugins: ["plugin_a"],
            },
          },
        },
      },
    },
    sdkOptions: {},
  };

  const request = adapter._test.buildMappedRequest(ctx);

  assert.deepStrictEqual(request.options.tools, [{ name: "custom_tool", type: "function" }]);
  assert.strictEqual(request.options.continue, true);
  assert.strictEqual(request.options.forkSession, true);
  assert.strictEqual(request.options.maxBudgetUsd, 5.0);
  assert.strictEqual(request.options.includePartialMessages, true);
  assert.strictEqual(request.options.outputFormat, "json");
  assert.strictEqual(request.options.effort, "high");
  assert.strictEqual(request.options.persistSession, true);
  assert.strictEqual(request.options.enableFileCheckpointing, true);
  assert.deepStrictEqual(request.options.sandbox, { enabled: true });
  assert.deepStrictEqual(request.options.thinking, { enabled: true, budget_tokens: 1000 });
  assert.deepStrictEqual(request.options.hooks, { onToolCall: "log" });
  assert.deepStrictEqual(request.options.agents, [{ name: "sub_agent" }]);
  assert.deepStrictEqual(request.options.plugins, ["plugin_a"]);
});

test("hello envelope reports updated capabilities", async () => {
  const workOrder = {
    id: crypto.randomUUID(),
    task: "test capabilities",
    lane: "patch_first",
    workspace: {
      root: process.cwd(),
      mode: "pass_through",
    },
    context: {},
    policy: {},
    requirements: { required: [] },
    config: {
      vendor: {},
    },
  };

  const { messages } = await runSidecar(workOrder);
  const hello = messages.find((message) => message.t === "hello");

  assert(hello, "hello envelope should be present");
  assert.strictEqual(hello.capabilities.session_resume, "native");
  assert.strictEqual(hello.capabilities.session_fork, "emulated");
  assert.strictEqual(hello.capabilities.tool_ask_user, "native");
  assert.strictEqual(hello.capabilities.permission_callback, "native");
  assert.strictEqual(hello.capabilities.checkpointing, "native");
});
