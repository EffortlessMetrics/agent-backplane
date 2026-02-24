const assert = require("node:assert");
const path = require("node:path");
const { test } = require("node:test");
const { pathToFileURL } = require("node:url");

function buildCtx(events) {
  return {
    workOrder: {
      id: "run_mock_kimi_1",
      task: "Summarize README",
      workspace: {
        root: process.cwd(),
      },
      context: {
        files: ["README.md"],
        snippets: [],
      },
      config: {
        model: "kimi-for-coding",
        vendor: {
          kimi: {
            model: "kimi-for-coding",
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

test("kimi adapter uses sdk transport and emits normalized events", async () => {
  process.env.ABP_KIMI_TRANSPORT = "sdk";
  process.env.ABP_KIMI_SDK_MODULE = pathToFileURL(
    path.resolve(__dirname, "./mock-sdk.cjs")
  ).href;

  const adapterPath = path.resolve(__dirname, "../adapter.js");
  delete require.cache[adapterPath];
  const adapter = require(adapterPath);

  const events = [];
  const result = await adapter.run(buildCtx(events));

  assert.strictEqual(result.outcome, "complete");
  assert.strictEqual(result.usage.input_tokens, 11);
  assert.strictEqual(result.usage.output_tokens, 7);
  assert.ok(events.some((event) => event.kind === "delta" && event.text.includes("hello")));
  assert.ok(events.some((event) => event.kind === "tool_call"));
  assert.ok(events.some((event) => event.kind === "tool_result"));

  delete process.env.ABP_KIMI_TRANSPORT;
  delete process.env.ABP_KIMI_SDK_MODULE;
});
