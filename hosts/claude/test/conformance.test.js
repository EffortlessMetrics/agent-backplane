const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");

const FIXTURE_PATH = path.resolve(__dirname, "../../../tests/fixtures/claude_agent_conformance.json");
const fixtures = JSON.parse(fs.readFileSync(FIXTURE_PATH, "utf-8"));

// Test hello shape
test("conformance: hello_shape", () => {
  const adapter = require("../adapter.js");
  assert(adapter.name, "adapter should have a name");
  assert(adapter.version, "adapter should have a version");
  assert(adapter.capabilities, "adapter should have capabilities");
  assert.strictEqual(typeof adapter.capabilities.streaming, "string");
});

// Test V2 hello shape
test("conformance: hello_shape (V2)", () => {
  const adapter = require("../adapter-v2.js");
  assert(adapter.name, "V2 adapter should have a name");
  assert(adapter.version, "V2 adapter should have a version");
  assert(adapter.capabilities, "V2 adapter should have capabilities");
});

// Test hook pre_tool_use mapping via V1 adapter
test("conformance: hook_pre_tool_use (V1)", () => {
  const adapter = require("../adapter.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) {},
    emitAssistantDelta(text) {},
    emitAssistantMessage(text) {},
    emitError(msg) {},
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false, stopReason: null, numTurns: 0 };

  adapter._test.emitMappedMessage(ctx, {
    type: "pre_tool_use",
    tool_name: "bash",
    tool_use_id: "tu-conf-1",
    input: { command: "ls" },
  }, state);

  const toolCalls = emitted.filter(e => e.type === "tool_call");
  assert(toolCalls.length >= 1, "Expected tool_call from pre_tool_use");
  assert.strictEqual(toolCalls[0].ext?.hook, "pre_tool_use");
});

// Test hook pre_tool_use mapping via V2 adapter
test("conformance: hook_pre_tool_use (V2)", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) {},
    emitAssistantDelta(text) {},
    emitAssistantMessage(text) {},
    emitError(msg) {},
    emitRaw(ev) { emitted.push(ev); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  adapter._test.emitV2Event(ctx, {
    type: "pre_tool_use",
    tool_name: "bash",
    tool_use_id: "tu-conf-1",
    input: { command: "ls" },
    decision: "allow",
  }, state);

  const toolCalls = emitted.filter(e => e.type === "tool_call");
  assert(toolCalls.length >= 1, "Expected tool_call from pre_tool_use");
  assert.strictEqual(toolCalls[0].ext?.hook, "pre_tool_use");
});

// Test hook post_tool_use mapping via V1
test("conformance: hook_post_tool_use (V1)", () => {
  const adapter = require("../adapter.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) {},
    emitAssistantDelta(text) {},
    emitAssistantMessage(text) {},
    emitError(msg) {},
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false, stopReason: null, numTurns: 0 };

  adapter._test.emitMappedMessage(ctx, {
    type: "post_tool_use",
    tool_name: "bash",
    tool_use_id: "tu-conf-2",
    output: "file1.txt",
  }, state);

  const toolResults = emitted.filter(e => e.type === "tool_result");
  assert(toolResults.length >= 1, "Expected tool_result from post_tool_use");
  assert.strictEqual(toolResults[0].ext?.hook, "post_tool_use");
});

// Test hook post_tool_use mapping via V2
test("conformance: hook_post_tool_use (V2)", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) {},
    emitAssistantDelta(text) {},
    emitAssistantMessage(text) {},
    emitError(msg) {},
    emitRaw(ev) { emitted.push(ev); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  adapter._test.emitV2Event(ctx, {
    type: "post_tool_use",
    tool_name: "bash",
    tool_use_id: "tu-conf-2",
    output: "file1.txt",
  }, state);

  const toolResults = emitted.filter(e => e.type === "tool_result");
  assert(toolResults.length >= 1, "Expected tool_result from post_tool_use");
  assert.strictEqual(toolResults[0].ext?.hook, "post_tool_use");
});

// Test permission flow via V2
test("conformance: permission_flow (V2)", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) {},
    emitAssistantDelta(text) {},
    emitAssistantMessage(text) {},
    emitError(msg) {},
    emitRaw(ev) { emitted.push(ev); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  adapter._test.emitV2Event(ctx, {
    type: "permission_request",
    tool_name: "write",
    input: { path: "/tmp/test" },
  }, state);

  const types = emitted.map(e => e.type);
  assert(types.includes("permission_requested"), "Expected permission_requested");
  assert(types.includes("permission_resolved"), "Expected permission_resolved");
  const resolved = emitted.find(e => e.type === "permission_resolved");
  assert.strictEqual(resolved.granted, true);
});

// Test checkpoint via V2
test("conformance: checkpoint_event (V2)", () => {
  const adapter = require("../adapter-v2.js");
  const emitted = [];
  const ctx = {
    emitToolCall(ev) { emitted.push({ type: "tool_call", ...ev }); },
    emitToolResult(ev) { emitted.push({ type: "tool_result", ...ev }); },
    emitWarning(msg) {},
    emitAssistantDelta(text) { emitted.push({ type: "assistant_delta", text }); },
    emitAssistantMessage(text) { emitted.push({ type: "assistant_message", text }); },
    emitError(msg) {},
    emitRaw(ev) { emitted.push(ev); },
  };
  const state = { usageRaw: {}, lastAssistantText: "", sawAssistantDelta: false, sawAssistantMessage: false };

  adapter._test.emitV2Event(ctx, {
    type: "checkpoint",
    checkpoint_id: "ckpt-conf-1",
  }, state);

  const msgs = emitted.filter(e => e.type === "assistant_message");
  assert(msgs.length >= 1, "Expected assistant_message for checkpoint");
  const hasCheckpointExt = emitted.some(e => e.ext?.checkpoint_id === "ckpt-conf-1");
  assert(hasCheckpointExt, "Expected checkpoint_id in ext");
});

// Test receipt shape (integration)
test("conformance: receipt_shape", () => {
  const fixture = fixtures.fixtures.find(f => f.id === "receipt_shape");
  assert(fixture, "receipt_shape fixture should exist");
  const requiredFields = fixture.assertions.receipt.has_fields;
  assert(Array.isArray(requiredFields));
  assert(requiredFields.includes("meta"));
  assert(requiredFields.includes("outcome"));
});

// Test fixture file loads correctly
test("conformance: fixture file loads and has expected structure", () => {
  assert.strictEqual(fixtures.version, "1.0");
  assert(Array.isArray(fixtures.fixtures));
  assert(fixtures.fixtures.length >= 10, `Expected at least 10 fixtures, got ${fixtures.fixtures.length}`);

  // All fixtures have an id and description
  for (const f of fixtures.fixtures) {
    assert(f.id, "Every fixture must have an id");
    assert(f.description, "Every fixture must have a description");
    assert(f.assertions, "Every fixture must have assertions");
  }
});
