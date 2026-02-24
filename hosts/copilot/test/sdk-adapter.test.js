const assert = require("node:assert");
const path = require("node:path");
const { test } = require("node:test");
const { pathToFileURL } = require("node:url");

function buildCtx(events) {
  return {
    workOrder: {
      id: "run_mock_1",
      task: "Summarize README",
      workspace: {
        root: process.cwd(),
      },
      context: {
        files: ["README.md"],
        snippets: [],
      },
      config: {
        model: "gpt-4.1",
        vendor: {
          copilot: {
            model: "gpt-4.1",
          },
        },
        env: {},
      },
      policy: {},
    },
    policy: {},
    policyEngine: null,
    emitAssistantDelta: (text) => events.push({ kind: "delta", text }),
    emitAssistantMessage: (text) => events.push({ kind: "message", text }),
    emitToolCall: (event) => events.push({ kind: "tool_call", event }),
    emitToolResult: (event) => events.push({ kind: "tool_result", event }),
    emitWarning: (message) => events.push({ kind: "warning", message }),
    emitError: (message) => events.push({ kind: "error", message }),
    writeArtifact: () => "artifact/path.txt",
    log: () => {},
  };
}

test("copilot adapter uses SDK transport and emits normalized events", async () => {
  process.env.ABP_COPILOT_TRANSPORT = "sdk";
  process.env.ABP_COPILOT_SDK_MODULE = pathToFileURL(
    path.resolve(__dirname, "./mock-sdk.cjs")
  ).href;

  const adapterPath = path.resolve(__dirname, "../adapter.js");
  delete require.cache[adapterPath];
  const adapter = require(adapterPath);

  const events = [];
  const result = await adapter.run(buildCtx(events));

  assert.strictEqual(result.outcome, "complete");
  assert.strictEqual(result.usage.input_tokens, 5);
  assert.strictEqual(result.usage.output_tokens, 7);
  assert.ok(events.some((e) => e.kind === "delta" && e.text.includes("hello")));
  assert.ok(events.some((e) => e.kind === "tool_call"));
  assert.ok(events.some((e) => e.kind === "tool_result"));

  delete process.env.ABP_COPILOT_TRANSPORT;
  delete process.env.ABP_COPILOT_SDK_MODULE;
});
