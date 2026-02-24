#!/usr/bin/env node

// GitHub Copilot sidecar for Agent Backplane (ABP).
//
// This sidecar follows the ABP JSONL protocol:
//   - hello (first line)
//   - run
//   - event*
//   - final

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const { getCapabilityManifest } = require("./capabilities");

const CONTRACT_VERSION = "abp/v0.1";
const ADAPTER_VERSION = "0.1";
const MAX_INLINE_OUTPUT_BYTES = parseInt(
  process.env.ABP_COPILOT_MAX_INLINE_OUTPUT_BYTES || "8192",
  10
);

const ExecutionMode = {
  Mapped: "mapped",
  Passthrough: "passthrough",
};

function nowIso() {
  return new Date().toISOString();
}

function write(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function safeString(value) {
  if (value == null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  try {
    return JSON.stringify(value);
  } catch (_) {
    return String(value);
  }
}

function sanitizeFilePart(value) {
  return String(value || "artifact")
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+/, "")
    .replace(/-+$/, "")
    .slice(0, 64);
}

function canonicalizeGlobPattern(pattern) {
  return String(pattern || "").replace(/\\/g, "/");
}

function compileGlob(pattern) {
  const normalized = canonicalizeGlobPattern(pattern);
  let out = "^";
  for (let i = 0; i < normalized.length; i += 1) {
    const ch = normalized[i];
    if (ch === "*") {
      const next = normalized[i + 1];
      if (next === "*") {
        i += 1;
        if (normalized[i + 1] === "/") {
          i += 1;
          out += "(?:.*/)?";
        } else {
          out += ".*";
        }
      } else {
        out += "[^/]*";
      }
    } else if (ch === "?") {
      out += "[^/]";
    } else if ("+.^$|()[]{}".includes(ch)) {
      out += `\\${ch}`;
    } else {
      out += ch;
    }
  }
  out += "$";
  return new RegExp(out);
}

function compileGlobList(list) {
  if (!Array.isArray(list) || list.length === 0) {
    return [];
  }
  return list
    .map((p) => {
      try {
        return compileGlob(p);
      } catch (_) {
        return null;
      }
    })
    .filter(Boolean);
}

function matchesAny(matchers, value) {
  if (!matchers || matchers.length === 0) {
    return false;
  }
  return matchers.some((m) => m.test(value));
}

function toPosixPath(p) {
  return String(p || "").replace(/\\/g, "/");
}

function canonicalWithin(root, maybePath) {
  const rootReal = fs.realpathSync(root);
  const candidate = path.resolve(rootReal, maybePath || ".");
  const candidateReal = fs.existsSync(candidate)
    ? fs.realpathSync(candidate)
    : path.resolve(rootReal, maybePath || ".");
  const rel = path.relative(rootReal, candidateReal);
  const relPosix = toPosixPath(rel);
  if (
    relPosix === ".." ||
    relPosix.startsWith("../") ||
    path.isAbsolute(relPosix)
  ) {
    return null;
  }
  return relPosix || ".";
}

function collectPathValues(input) {
  if (!input || typeof input !== "object") {
    return [];
  }
  const values = [];
  for (const [k, v] of Object.entries(input)) {
    const key = k.toLowerCase();
    if (key.includes("path") || key.includes("file")) {
      if (typeof v === "string") {
        values.push(v);
      } else if (Array.isArray(v)) {
        for (const item of v) {
          if (typeof item === "string") {
            values.push(item);
          }
        }
      }
    }
  }
  return values;
}

function buildPolicyEngine(policy, workspaceRoot) {
  const allowedTools = compileGlobList(policy.allowed_tools || []);
  const disallowedTools = compileGlobList(policy.disallowed_tools || []);
  const denyRead = compileGlobList(policy.deny_read || []);
  const denyWrite = compileGlobList(policy.deny_write || []);
  const requireApprovalFor = compileGlobList(policy.require_approval_for || []);
  const allowNetwork = compileGlobList(policy.allow_network || []);
  const denyNetwork = compileGlobList(policy.deny_network || []);

  function canUseTool(toolName) {
    if (matchesAny(disallowedTools, toolName)) {
      return { allowed: false, reason: `tool '${toolName}' is disallowed` };
    }
    if (allowedTools.length > 0 && !matchesAny(allowedTools, toolName)) {
      return {
        allowed: false,
        reason: `tool '${toolName}' is not in allowed_tools`,
      };
    }
    return { allowed: true };
  }

  function requiresApproval(toolName) {
    return matchesAny(requireApprovalFor, toolName);
  }

  function canReadPath(relPath) {
    if (matchesAny(denyRead, toPosixPath(relPath))) {
      return { allowed: false, reason: `read denied for '${relPath}'` };
    }
    return { allowed: true };
  }

  function canWritePath(relPath) {
    if (matchesAny(denyWrite, toPosixPath(relPath))) {
      return { allowed: false, reason: `write denied for '${relPath}'` };
    }
    return { allowed: true };
  }

  function canAccessNetwork(hostname) {
    if (!hostname) {
      return { allowed: true };
    }
    if (matchesAny(denyNetwork, hostname)) {
      return {
        allowed: false,
        reason: `network denied for '${hostname}'`,
      };
    }
    if (allowNetwork.length > 0 && !matchesAny(allowNetwork, hostname)) {
      return {
        allowed: false,
        reason: `network '${hostname}' not in allow_network`,
      };
    }
    return { allowed: true };
  }

  function extractHostname(input) {
    if (!input || typeof input !== "object") {
      return null;
    }
    for (const [k, v] of Object.entries(input)) {
      const key = k.toLowerCase();
      if (
        key.includes("url") ||
        key.includes("uri") ||
        key.includes("host") ||
        key.includes("endpoint")
      ) {
        try {
          const url = new URL(v);
          return url.hostname;
        } catch (_) {
          return null;
        }
      }
    }
    return null;
  }

  function preTool(toolName, input) {
    const decision = canUseTool(toolName);
    if (!decision.allowed) {
      return decision;
    }
    if (requiresApproval(toolName)) {
      return {
        allowed: false,
        reason: `tool '${toolName}' requires approval (request_permission workflow is not configured in ABP v0.1)`,
      };
    }

    const lower = String(toolName).toLowerCase();
    const paths = collectPathValues(input);
    for (const rawPath of paths) {
      const rel = canonicalWithin(workspaceRoot, rawPath);
      if (!rel) {
        return {
          allowed: false,
          reason: `path escapes workspace root: '${rawPath}'`,
        };
      }
      if (["read", "glob", "grep", "list"].some((needle) => lower.includes(needle))) {
        const check = canReadPath(rel);
        if (!check.allowed) {
          return check;
        }
      }
      if (
        ["write", "edit", "patch", "delete", "rm", "mkdir", "copy", "move"].some((needle) =>
          lower.includes(needle)
        )
      ) {
        const check = canWritePath(rel);
        if (!check.allowed) {
          return check;
        }
      }
    }

    if (lower.includes("web") || lower.includes("fetch") || lower.includes("http")) {
      const hostname = extractHostname(input);
      const netDecision = canAccessNetwork(hostname);
      if (!netDecision.allowed) {
        return netDecision;
      }
    }

    return { allowed: true };
  }

  return {
    canUseTool,
    requiresApproval,
    canReadPath,
    canWritePath,
    canAccessNetwork,
    preTool,
  };
}

function computeReceiptHash(receipt) {
  const v = JSON.parse(JSON.stringify(receipt));
  v.receipt_sha256 = null;
  return crypto.createHash("sha256").update(JSON.stringify(v)).digest("hex");
}

function trimToolOutput(runCtx, toolName, output) {
  const text = safeString(output);
  const size = Buffer.byteLength(text, "utf8");
  if (size <= MAX_INLINE_OUTPUT_BYTES) {
    return output;
  }

  const baseName = `${sanitizeFilePart(toolName || "tool")}-${Date.now()}.txt`;
  return runCtx.writeArtifact("tool_output", baseName, text);
}

function loadAdapter() {
  const customPath = process.env.ABP_COPILOT_ADAPTER_MODULE;
  if (customPath) {
    const resolved = path.resolve(customPath);
    const mod = require(resolved);
    const adapter = mod && mod.default ? mod.default : mod;
    if (!adapter || typeof adapter.run !== "function") {
      throw new Error(
        `Custom adapter '${resolved}' must export object with async run(ctx)`
      );
    }
    return {
      name: adapter.name || "copilot_custom_adapter",
      version: adapter.version || null,
      run: adapter.run,
    };
  }

  const adapter = require("./adapter");
  if (!adapter || typeof adapter.run !== "function") {
    throw new Error("Invalid default Copilot adapter module");
  }
  return adapter;
}

function getExecutionMode(workOrder) {
  const mode =
    workOrder && workOrder.config && workOrder.config.vendor
      ? workOrder.config.vendor.abp?.mode
      : null;
  return mode === ExecutionMode.Passthrough ? ExecutionMode.Passthrough : ExecutionMode.Mapped;
}

function buildRequestOptions(workOrder) {
  const vendor = (workOrder.config && workOrder.config.vendor) || {};
  const copilotVendor = vendor.copilot || {};
  const policy = workOrder.policy || {};

  return {
    model: workOrder.config && workOrder.config.model ? workOrder.config.model : copilotVendor.model,
    reasoningEffort: copilotVendor.reasoningEffort,
    systemMessage: copilotVendor.systemMessage,
    allowedTools: policy.allowed_tools,
    disallowedTools: policy.disallowed_tools,
    vendor,
  };
}

function parseUsage(raw) {
  if (!raw || typeof raw !== "object") {
    return {};
  }
  const usage = raw.usage && typeof raw.usage === "object" ? raw.usage : raw;
  const pick = (keys) => {
    for (const key of keys) {
      if (typeof usage[key] === "number") {
        return usage[key];
      }
    }
    return undefined;
  };

  return {
    input_tokens: pick(["input_tokens", "inputTokens", "prompt_tokens", "promptTokens"]),
    output_tokens: pick(["output_tokens", "outputTokens", "completion_tokens", "completionTokens"]),
    cache_read_tokens: pick(["cache_read_tokens", "cacheReadTokens"]),
    cache_write_tokens: pick(["cache_write_tokens", "cacheWriteTokens"]),
    request_units: pick(["request_units", "requestUnits"]),
    estimated_cost_usd: pick(["estimated_cost_usd", "estimatedCostUsd"]),
  };
}

async function handleRun(runId, workOrder, adapter, backendCaps, mode) {
  const startedAt = nowIso();
  const workspaceRoot = (workOrder.workspace && workOrder.workspace.root) || process.cwd();
  const policyEngine = buildPolicyEngine(workOrder.policy || {}, workspaceRoot);
  const artifactRoot = path.join(workspaceRoot, ".agent-backplane", "artifacts", runId);
  fs.mkdirSync(artifactRoot, { recursive: true });
  const trace = [];
  const artifacts = [];
  const toolCalls = [];

  function emit(event) {
    const ev = { ts: nowIso(), ...event };
    trace.push(ev);
    write({ t: "event", ref_id: runId, event: ev });
  }

  function emitAssistantDelta(text) {
    emit({ type: "assistant_delta", text: String(text || "") });
  }

  function emitAssistantMessage(text) {
    emit({ type: "assistant_message", text: String(text || "") });
  }

function writeArtifact(kind, suggestedName, content) {
  const base = sanitizeFilePart(suggestedName || kind || "artifact") || "artifact";
  const fileName = base.includes(".") ? base : `${base}.txt`;
  const abs = path.join(artifactRoot, fileName);
  fs.writeFileSync(abs, safeString(content), "utf8");
  const rel = toPosixPath(path.relative(workspaceRoot, abs));
  const id = path.normalize(rel);
  artifacts.push({ kind, path: id });
  return id;
}

  function emitToolCall({ toolName, toolUseId, parentToolUseId, input }) {
    const decision = policyEngine.preTool(String(toolName || "tool"), input || {});
    if (!decision.allowed) {
      const msg = `tool denied: ${decision.reason || "policy"} (${toolName})`;
      emit({ type: "warning", message: msg });
      emit({
        type: "tool_result",
        tool_name: String(toolName || "tool"),
        tool_use_id: toolUseId || null,
        output: { denied: true, reason: decision.reason || "policy" },
        is_error: true,
      });
      return null;
    }

    const id = toolUseId || `toolu_${crypto.randomUUID().replace(/-/g, "")}`;
    const record = {
      tool_name: String(toolName || "tool"),
      tool_use_id: id,
      parent_tool_use_id: parentToolUseId || null,
      input: input || {},
    };
    toolCalls.push(record);
    emit({ type: "tool_call", ...record });
    return id;
  }

  function emitToolResult({ toolName, toolUseId, output, isError }) {
    const safeOutput = trimToolOutput(
      { writeArtifact },
      String(toolName || "tool"),
      output
    );
    emit({
      type: "tool_result",
      tool_name: String(toolName || "tool"),
      tool_use_id: toolUseId || null,
      output: safeOutput,
      is_error: !!isError,
    });
  }

  function emitWarning(message) {
    emit({ type: "warning", message: String(message || "") });
  }

  function emitError(message) {
    emit({ type: "error", message: String(message || "") });
  }

  function log(message) {
    process.stderr.write(`[copilot-host] ${message}\n`);
  }

  emit({ type: "run_started", message: `copilot sidecar starting: ${safeString(workOrder.task)}` });

  const ctx = {
    run_id: runId,
    workOrder,
    sdkOptions: buildRequestOptions(workOrder),
    policy: workOrder.policy || {},
    policyEngine,
    emitAssistantDelta,
    emitAssistantMessage,
    emitToolCall,
    emitToolResult,
    emitWarning,
    emitError,
    writeArtifact,
    log,
  };

  let usageRaw = {};
  let usage = {};
  let outcome = "complete";

  try {
    const result = (await adapter.run(ctx)) || {};
    usageRaw = result.usageRaw && typeof result.usageRaw === "object" ? result.usageRaw : {};
    usage = result.usage && typeof result.usage === "object" ? result.usage : parseUsage(usageRaw);
    if (typeof result.outcome === "string") {
      outcome = result.outcome;
    }
  } catch (err) {
    outcome = "failed";
    emitError(err && err.stack ? err.stack : safeString(err));
  }

  emit({ type: "run_completed", message: `copilot sidecar run completed: ${outcome}` });

  const finishedAt = nowIso();
  const receipt = {
    meta: {
      run_id: runId,
      work_order_id: workOrder.id,
      contract_version: CONTRACT_VERSION,
      started_at: startedAt,
      finished_at: finishedAt,
      duration_ms: Math.max(
        0,
        new Date(finishedAt).getTime() - new Date(startedAt).getTime()
      ),
    },
    backend: {
      id: "github_copilot_sdk",
      backend_version: adapter.version || ADAPTER_VERSION,
      adapter_version: ADAPTER_VERSION,
    },
    capabilities: backendCaps,
    mode,
    usage_raw: usageRaw,
    usage,
    trace,
    artifacts,
    verification: {
      git_diff: null,
      git_status: null,
      harness_ok: true,
    },
    outcome,
    receipt_sha256: null,
  };

  if (Array.isArray(toolCalls) && toolCalls.length > 0) {
    receipt.tool_calls = toolCalls;
  }

  receipt.receipt_sha256 = computeReceiptHash(receipt);

  write({ t: "final", ref_id: runId, receipt });
}

async function main() {
  const adapter = loadAdapter();
  const manifest = getCapabilityManifest();
  const backendCaps = manifest.capabilities || {};

  write({
    t: "hello",
    contract_version: CONTRACT_VERSION,
    backend: {
      id: "github_copilot_sdk",
      backend_version: adapter.version || ADAPTER_VERSION,
      adapter_version: ADAPTER_VERSION,
    },
    capabilities: {
      ...backendCaps,
    },
    mode: ExecutionMode.Mapped,
  });

  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  for await (const line of rl) {
    if (!line.trim()) {
      continue;
    }

    let envelope;
    try {
      envelope = JSON.parse(line);
    } catch (err) {
      write({
        t: "fatal",
        ref_id: null,
        error: `invalid json: ${safeString(err)}`,
      });
      continue;
    }

    if (envelope.t !== "run") {
      write({
        t: "fatal",
        ref_id: null,
        error: `expected run envelope, got '${safeString(envelope.t)}'`,
      });
      continue;
    }

    const runId = envelope.id;
    const workOrder = envelope.work_order || {};
    const mode = getExecutionMode(workOrder);
    try {
      await handleRun(runId, workOrder, adapter, backendCaps, mode);
    } catch (err) {
      write({
        t: "fatal",
        ref_id: runId,
        error: safeString(err),
      });
    }
  }
}

main().catch((err) => {
  console.error(`copilot host failed: ${safeString(err)}`);
  process.exitCode = 1;
});
