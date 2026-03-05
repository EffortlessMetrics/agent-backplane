#!/usr/bin/env node

// Simple ABP sidecar example.
//
// Transport: JSONL over stdin/stdout.

const readline = require('readline');
const { randomUUID } = require('crypto');

function nowIso() {
  return new Date().toISOString();
}

function write(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

const backend = {
  id: "example_node_sidecar",
  backend_version: process.version,
  adapter_version: "0.1",
};

const capabilities = {
  streaming: "native",
  tool_read: "emulated",
  tool_write: "emulated",
  tool_edit: "emulated",
  structured_output_json_schema: "emulated",
};

// Hello first.
write({
  t: "hello",
  contract_version: "abp/v0.1",
  backend,
  capabilities,
});

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });

rl.on('line', (line) => {
  if (!line.trim()) return;
  let msg;
  try {
    msg = JSON.parse(line);
  } catch (e) {
    write({ t: "fatal", ref_id: null, error: `invalid json: ${e}` });
    return;
  }

  if (msg.t === "ping") {
    write({ t: "pong", seq: msg.seq });
    return;
  }

  if (msg.t === "cancel") {
    // v0.1: nothing in-flight to cancel for this simple sidecar
    return;
  }

  if (msg.t !== "run") {
    return;
  }

  const runId = msg.id || randomUUID();
  const workOrder = msg.work_order || {};
  const started = nowIso();

  const trace = [];

  function emit(kind) {
    const ev = { ts: nowIso(), ...kind };
    trace.push(ev);
    write({ t: "event", ref_id: runId, event: ev });
  }

  try {
    emit({ type: "run_started", message: `node sidecar starting: ${workOrder.task}` });
    emit({ type: "assistant_message", text: "Hello from the Node sidecar. Replace me with a real SDK adapter." });
    emit({ type: "assistant_message", text: `workspace root: ${(workOrder.workspace || {}).root}` });
    emit({ type: "run_completed", message: "node sidecar complete" });

    const finished = nowIso();

    const receipt = {
      meta: {
        run_id: runId,
        work_order_id: workOrder.id,
        contract_version: "abp/v0.1",
        started_at: started,
        finished_at: finished,
        duration_ms: Math.max(0, new Date(finished).getTime() - new Date(started).getTime()),
      },
      backend,
      capabilities,
      usage_raw: { note: "example_node_sidecar" },
      usage: {},
      trace,
      artifacts: [],
      verification: { git_diff: null, git_status: null, harness_ok: true },
      outcome: "complete",
      receipt_sha256: null,
    };

    write({ t: "final", ref_id: runId, receipt });
  } catch (err) {
    write({ t: "fatal", ref_id: runId, error: `run failed: ${err && err.message || err}` });
  }
});

// Graceful shutdown
function shutdown() {
  rl.close();
  process.exit(0);
}
process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

process.on('uncaughtException', (err) => {
  write({ t: "fatal", ref_id: null, error: `uncaught exception: ${err && err.message || err}` });
  process.exit(1);
});
