/**
 * Protocol conformance tests for the Node.js example sidecar.
 *
 * Spawns host.js as a child process, sends JSONL envelopes on stdin,
 * and asserts the expected responses appear on stdout.
 */

const assert = require("node:assert");
const { spawn } = require("node:child_process");
const path = require("node:path");
const crypto = require("node:crypto");
const { describe, it } = require("node:test");

const HOST = path.resolve(__dirname, "../host.js");

function collectOutput(workOrderOrEnvelopes, { timeout = 5000 } = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn(process.execPath, [HOST], {
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      proc.kill();
      reject(new Error("timeout"));
    }, timeout);

    proc.stdout.on("data", (d) => (stdout += d));
    proc.stderr.on("data", (d) => (stderr += d));

    proc.on("close", () => {
      clearTimeout(timer);
      const lines = stdout.trim().split("\n").filter(Boolean);
      const msgs = lines.map((l) => JSON.parse(l));
      resolve({ msgs, stderr });
    });

    proc.on("error", (err) => {
      clearTimeout(timer);
      reject(err);
    });

    const envelopes = Array.isArray(workOrderOrEnvelopes)
      ? workOrderOrEnvelopes
      : [
          {
            t: "run",
            id: crypto.randomUUID(),
            work_order: workOrderOrEnvelopes,
          },
        ];

    for (const env of envelopes) {
      proc.stdin.write(JSON.stringify(env) + "\n");
    }
    proc.stdin.end();
  });
}

function makeWorkOrder(overrides = {}) {
  return {
    id: crypto.randomUUID(),
    task: "test task",
    workspace: { root: process.cwd() },
    context: {},
    policy: {},
    config: { vendor: {} },
    ...overrides,
  };
}

describe("node sidecar protocol", () => {
  it("emits hello as the first envelope", async () => {
    const { msgs } = await collectOutput(makeWorkOrder());
    assert.strictEqual(msgs[0].t, "hello");
    assert.strictEqual(msgs[0].contract_version, "abp/v0.1");
    assert.ok(msgs[0].backend, "hello must include backend");
    assert.ok(msgs[0].capabilities, "hello must include capabilities");
  });

  it("emits events and final for a run envelope", async () => {
    const { msgs } = await collectOutput(makeWorkOrder({ task: "say hi" }));
    const events = msgs.filter((m) => m.t === "event");
    const finals = msgs.filter((m) => m.t === "final");
    assert.ok(events.length > 0, "should emit at least one event");
    assert.strictEqual(finals.length, 1, "should emit exactly one final");
  });

  it("final contains a valid receipt", async () => {
    const wo = makeWorkOrder();
    const { msgs } = await collectOutput(wo);
    const final = msgs.find((m) => m.t === "final");
    assert.ok(final.receipt, "final must contain receipt");
    const r = final.receipt;
    assert.strictEqual(r.meta.contract_version, "abp/v0.1");
    assert.strictEqual(r.meta.work_order_id, wo.id);
    assert.strictEqual(r.outcome, "complete");
    assert.ok(r.meta.duration_ms >= 0, "duration_ms should be non-negative");
  });

  it("responds to ping with pong", async () => {
    const { msgs } = await collectOutput([{ t: "ping", seq: 42 }]);
    const pong = msgs.find((m) => m.t === "pong");
    assert.ok(pong, "should respond with pong");
    assert.strictEqual(pong.seq, 42);
  });

  it("ignores cancel envelopes gracefully", async () => {
    const { msgs } = await collectOutput([{ t: "cancel", ref_id: "x" }]);
    // hello is always emitted; no fatal should appear
    assert.ok(msgs.some((m) => m.t === "hello"));
    assert.ok(!msgs.some((m) => m.t === "fatal"));
  });

  it("emits fatal for invalid JSON", async () => {
    const { msgs } = await collectOutput(
      // raw string will be written as-is; we need to bypass the helper
      [],
    );
    // Send raw invalid JSON by using a custom spawn
    const result = await new Promise((resolve, reject) => {
      const proc = spawn(process.execPath, [HOST], {
        stdio: ["pipe", "pipe", "pipe"],
      });
      let stdout = "";
      proc.stdout.on("data", (d) => (stdout += d));
      proc.on("close", () => {
        const lines = stdout.trim().split("\n").filter(Boolean);
        resolve(lines.map((l) => JSON.parse(l)));
      });
      proc.on("error", reject);
      proc.stdin.write("not valid json\n");
      proc.stdin.end();
    });
    assert.ok(result.some((m) => m.t === "fatal" && m.error.includes("invalid json")));
  });

  it("ref_id on events matches run id", async () => {
    const runId = crypto.randomUUID();
    const { msgs } = await collectOutput([
      { t: "run", id: runId, work_order: makeWorkOrder() },
    ]);
    const events = msgs.filter((m) => m.t === "event");
    for (const ev of events) {
      assert.strictEqual(ev.ref_id, runId);
    }
    const final = msgs.find((m) => m.t === "final");
    assert.strictEqual(final.ref_id, runId);
  });
});
