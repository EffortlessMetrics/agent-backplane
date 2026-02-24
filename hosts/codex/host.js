#!/usr/bin/env node

// Codex sidecar for Agent Backplane (ABP).
//
// This process speaks JSONL envelopes over stdio:
// - hello (with mode: "passthrough" or "mapped")
// - run
// - event*
// - final
//
// This sidecar operates in TWO modes:
// - PASSTHROUGH: Codex dialect → Codex engine (no transformation)
// - MAPPED: Codex dialect → Claude engine (opinionated mapping)
//
// Mode is determined by config.vendor.abp.mode:
// - "passthrough" → Forward to Codex SDK unchanged
// - "mapped" (default) → Transform to Claude format
//
// A custom adapter can be provided via:
//   ABP_CODEX_ADAPTER_MODULE=./path/to/adapter.js
//
// Adapter contract:
//   module.exports = {
//     name: "codex_adapter_name",
//     version: "x.y.z",
//     async run(ctx) { ... }
//   }

const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const crypto = require("node:crypto");

const {
  SupportLevel,
  ClaudeCapabilities,
  getCapabilityManifest,
} = require("./capabilities");

const {
  ErrorCodes,
  ErrorNames,
  createError,
  validateFacade,
  validateCapabilities,
  extractRequiredCapabilities,
  mapCodexToClaude,
  mapModel,
  mapToolList,
  mapPermissionMode,
  createMappedReceiptAdditions,
  mapClaudeToCodexResponse,
} = require("./mapper");

const CONTRACT_VERSION = "abp/v0.1";
const ADAPTER_VERSION = "0.1";
const MAX_INLINE_OUTPUT_BYTES = parseInt(
  process.env.ABP_CODEX_MAX_INLINE_OUTPUT_BYTES || "8192",
  10
);

// Execution modes for ABP
const ExecutionMode = {
  Passthrough: "passthrough",
  Mapped: "mapped",
};

/**
 * Extract execution mode from WorkOrder config.vendor.abp.mode
 * @param {object} workOrder - The work order
 * @returns {string} - "passthrough" or "mapped" (default)
 */
function getExecutionMode(workOrder) {
  const vendor = workOrder.config && workOrder.config.vendor;
  if (!vendor || typeof vendor !== "object") {
    return ExecutionMode.Mapped;
  }
  const abp = vendor.abp;
  if (!abp || typeof abp !== "object") {
    return ExecutionMode.Mapped;
  }
  const mode = abp.mode;
  if (mode === ExecutionMode.Passthrough) {
    return ExecutionMode.Passthrough;
  }
  return ExecutionMode.Mapped;
}

/**
 * Get the passthrough SDK request from WorkOrder config.vendor.abp.request
 * @param {object} workOrder - The work order
 * @returns {object|null} - The raw SDK request or null if not in passthrough mode
 */
function getPassthroughRequest(workOrder) {
  const vendor = workOrder.config && workOrder.config.vendor;
  if (!vendor || typeof vendor !== "object") {
    return null;
  }
  const abp = vendor.abp;
  if (!abp || typeof abp !== "object") {
    return null;
  }
  return abp.request || null;
}

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
  return value
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+/, "")
    .replace(/-+$/, "")
    .slice(0, 64);
}

// ============================================================================
// Policy Engine (similar to Claude/Gemini host)
// ============================================================================

function compileGlob(pattern) {
  const normalized = String(pattern || "").replace(/\\/g, "/");
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
      return { allowed: false, reason: "disallowed" };
    }
    if (allowedTools.length > 0 && !matchesAny(allowedTools, toolName)) {
      return { allowed: false, reason: "not_in_allowlist" };
    }
    return { allowed: true };
  }

  function checkPathAccess(filePath, mode) {
    const posixPath = toPosixPath(filePath);
    const canonical = canonicalWithin(workspaceRoot, filePath);
    if (canonical === null) {
      return { allowed: false, reason: "escape_attempt" };
    }
    if (mode === "read" && matchesAny(denyRead, posixPath)) {
      return { allowed: false, reason: "deny_read" };
    }
    if (mode === "write" && matchesAny(denyWrite, posixPath)) {
      return { allowed: false, reason: "deny_write" };
    }
    return { allowed: true, canonical };
  }

  function needsApproval(toolName, input) {
    if (matchesAny(requireApprovalFor, toolName)) {
      return true;
    }
    return false;
  }

  function checkNetwork(url) {
    const urlStr = String(url || "");
    if (matchesAny(denyNetwork, urlStr)) {
      return { allowed: false, reason: "deny_network" };
    }
    if (allowNetwork.length > 0 && !matchesAny(allowNetwork, urlStr)) {
      return { allowed: false, reason: "not_in_allowlist" };
    }
    return { allowed: true };
  }

  return {
    canUseTool,
    checkPathAccess,
    needsApproval,
    checkNetwork,
    allowedTools,
    disallowedTools,
    denyRead,
    denyWrite,
    requireApprovalFor,
    allowNetwork,
    denyNetwork,
  };
}

// ============================================================================
// Receipt Generation
// ============================================================================

function hashReceipt(receipt) {
  const clone = JSON.parse(JSON.stringify(receipt));
  clone.receipt_sha256 = null;
  const canonical = JSON.stringify(clone);
  return crypto.createHash("sha256").update(canonical).digest("hex");
}

function createBaseReceipt(workOrder, runId, mode) {
  return {
    contract_version: CONTRACT_VERSION,
    run_id: runId,
    work_order_id: workOrder.id,
    mode: mode,
    source_dialect: "codex",
    target_engine: mode === ExecutionMode.Passthrough ? "codex" : "claude",
    started_at: nowIso(),
    status: "running",
    events: [],
    tool_calls: [],
    artifacts: [],
    usage: {
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
    ext: {},
  };
}

// ============================================================================
// Adapter Loading
// ============================================================================

function loadAdapter() {
  const adapterPath = process.env.ABP_CODEX_ADAPTER_MODULE;
  if (adapterPath) {
    try {
      const resolved = path.resolve(adapterPath);
      const adapter = require(resolved);
      if (adapter && typeof adapter.run === "function") {
        return adapter;
      }
    } catch (e) {
      console.error(`[codex-host] Failed to load adapter from ${adapterPath}:`, e.message);
    }
  }

  // Fall back to built-in adapter
  try {
    return require("./adapter");
  } catch (e) {
    console.error("[codex-host] Failed to load built-in adapter:", e.message);
    return null;
  }
}

// ============================================================================
// Claude Backend Adapter (for mapped mode)
// ============================================================================

function loadClaudeAdapter() {
  // For mapped mode, we need to load the Claude adapter
  const claudeAdapterPath = process.env.ABP_CLAUDE_ADAPTER_MODULE;
  if (claudeAdapterPath) {
    try {
      const resolved = path.resolve(claudeAdapterPath);
      return require(resolved);
    } catch (e) {
      console.error(`[codex-host] Failed to load Claude adapter from ${claudeAdapterPath}:`, e.message);
    }
  }

  // Try to load from relative path
  try {
    return require("../claude/adapter.template.js");
  } catch (e) {
    // Ignore
  }

  return null;
}

// ============================================================================
// Main Run Handler
// ============================================================================

async function handleRun(envelope, adapter) {
  const runId = envelope.id;
  const workOrder = envelope.work_order;

  if (!workOrder) {
    write({
      t: "final",
      id: runId,
      error: "Missing work_order in run envelope",
    });
    return;
  }

  const mode = getExecutionMode(workOrder);
  const receipt = createBaseReceipt(workOrder, runId, mode);

  // Prepare workspace
  const workspaceRoot = workOrder.workspace?.root || process.cwd();
  const policy = buildPolicyEngine(workOrder.policy || {}, workspaceRoot);

  // Event collection
  const events = [];
  const toolCalls = [];
  const artifacts = [];

  function emitEvent(event) {
    events.push(event);
    receipt.events.push({
      timestamp: nowIso(),
      ...event,
    });
    write({
      t: "event",
      id: runId,
      event,
    });
  }

  function emitAssistantDelta(text) {
    emitEvent({
      type: "assistant_delta",
      text,
    });
  }

  function emitAssistantMessage(text) {
    emitEvent({
      type: "assistant_message",
      text,
    });
  }

  function emitToolCall({ toolName, toolUseId, parentToolUseId, input }) {
    const tc = {
      tool_name: toolName,
      tool_use_id: toolUseId,
      parent_tool_use_id: parentToolUseId || null,
      input,
      started_at: nowIso(),
    };
    toolCalls.push(tc);
    receipt.tool_calls.push(tc);
    emitEvent({
      type: "tool_call",
      tool_name: toolName,
      tool_use_id: toolUseId,
      parent_tool_use_id: parentToolUseId || null,
      input,
    });
  }

  function emitToolResult({ toolName, toolUseId, output, isError }) {
    emitEvent({
      type: "tool_result",
      tool_name: toolName,
      tool_use_id: toolUseId,
      output,
      is_error: isError || false,
    });
  }

  function emitWarning(message) {
    emitEvent({
      type: "warning",
      message,
    });
  }

  function emitError(message) {
    emitEvent({
      type: "error",
      message,
    });
  }

  async function writeArtifact(kind, suggestedName, content) {
    const artifactId = crypto.randomUUID();
    const sanitizedName = sanitizeFilePart(suggestedName) || "artifact";
    const fileName = `${artifactId.slice(0, 8)}_${sanitizedName}`;

    // Store artifact reference
    const artifact = {
      id: artifactId,
      kind,
      file_name: fileName,
      suggested_name: suggestedName,
      content_size: Buffer.byteLength(content),
    };
    artifacts.push(artifact);
    receipt.artifacts.push(artifact);

    emitEvent({
      type: "artifact",
      artifact_id: artifactId,
      kind,
      file_name: fileName,
      suggested_name: suggestedName,
    });

    return artifactId;
  }

  try {
    if (mode === ExecutionMode.Passthrough) {
      // PASSTHROUGH MODE: Forward to Codex SDK unchanged
      await runPassthroughMode(workOrder, adapter, {
        receipt,
        policy,
        workspaceRoot,
        emitAssistantDelta,
        emitAssistantMessage,
        emitToolCall,
        emitToolResult,
        emitWarning,
        emitError,
        writeArtifact,
      });
    } else {
      // MAPPED MODE: Transform Codex → Claude
      await runMappedMode(workOrder, adapter, {
        receipt,
        policy,
        workspaceRoot,
        emitAssistantDelta,
        emitAssistantMessage,
        emitToolCall,
        emitToolResult,
        emitWarning,
        emitError,
        writeArtifact,
      });
    }

    receipt.status = "completed";
  } catch (error) {
    receipt.status = "failed";
    receipt.error = {
      message: error.message,
      code: error.code || "UNKNOWN",
    };
    emitError(error.message);
  }

  // Finalize receipt
  receipt.completed_at = nowIso();
  receipt.receipt_sha256 = hashReceipt(receipt);

  write({
    t: "final",
    id: runId,
    receipt,
  });
}

/**
 * Run in passthrough mode (Codex → Codex)
 */
async function runPassthroughMode(workOrder, adapter, ctx) {
  const { receipt, emitWarning, emitError } = ctx;

  // Get raw request
  const rawRequest = getPassthroughRequest(workOrder);
  if (!rawRequest) {
    throw new Error("Passthrough mode requires config.vendor.abp.request");
  }

  // Store raw request in ext
  receipt.ext.raw_request = rawRequest;
  receipt.ext.mode_detail = "passthrough_codex_to_codex";

  emitWarning("Execution mode: passthrough (Codex → Codex)");

  if (!adapter) {
    throw new Error("No Codex adapter available for passthrough mode");
  }

  // Build SDK options
  const sdkOptions = {
    model: rawRequest.model,
    tools: rawRequest.tools,
    temperature: rawRequest.temperature,
    max_tokens: rawRequest.max_tokens,
    thread_id: rawRequest.thread_id,
    cwd: workOrder.workspace?.root,
  };

  // Run adapter
  await adapter.run({
    workOrder,
    sdkOptions,
    policy: workOrder.policy || {},
    ...ctx,
  });
}

/**
 * Run in mapped mode (Codex → Claude)
 */
async function runMappedMode(workOrder, adapter, ctx) {
  const { receipt, policy, emitWarning, emitError } = ctx;

  // Get Codex request
  const codexRequest = getPassthroughRequest(workOrder) || workOrder.config?.vendor || {};

  // Stage 1: Facade validation
  const facadeValidation = validateFacade(codexRequest);
  if (!facadeValidation.valid) {
    // Early failure with typed errors
    const errors = facadeValidation.errors.map(e => ({
      code: e.code,
      name: e.name,
      message: e.message,
      feature: e.feature,
      suggestion: e.suggestion,
    }));

    receipt.mapping_errors = errors;
    receipt.status = "failed";

    throw new Error(`Mapping validation failed: ${errors.map(e => e.message).join("; ")}`);
  }

  // Log warnings
  for (const warning of facadeValidation.warnings) {
    emitWarning(warning.message);
  }

  // Map Codex request to Claude request
  const claudeRequest = mapCodexToClaude(codexRequest);

  // Stage 2: Runtime capability validation
  const backendCapabilities = {}; // Would be populated from actual Claude backend
  const capabilityValidation = validateCapabilities(claudeRequest, backendCapabilities);

  if (!capabilityValidation.valid) {
    const errors = capabilityValidation.errors.map(e => ({
      code: e.code,
      name: e.name,
      message: e.message,
      feature: e.feature,
    }));

    receipt.mapping_errors = errors;
    receipt.status = "failed";

    throw new Error(`Capability validation failed: ${errors.map(e => e.message).join("; ")}`);
  }

  // Store mapping metadata in receipt
  const sessionMapping = {
    codexThreadId: codexRequest.thread_id,
    claudeSessionId: claudeRequest.session_id,
  };

  const mappedReceiptAdditions = createMappedReceiptAdditions(
    codexRequest,
    claudeRequest,
    {
      warnings: facadeValidation.warnings,
      capabilities_used: capabilityValidation.capabilities_used,
    },
    sessionMapping
  );

  Object.assign(receipt, mappedReceiptAdditions);
  receipt.ext.mode_detail = "mapped_codex_to_claude";
  receipt.ext.original_request = codexRequest;
  receipt.ext.mapped_request = claudeRequest;

  emitWarning("Execution mode: mapped (Codex → Claude)");

  // Load Claude adapter for mapped mode
  const claudeAdapter = loadClaudeAdapter();
  if (!claudeAdapter) {
    // Fall back to explain-only mode
    emitWarning("Claude adapter not available, using explain-only mode");
    ctx.emitAssistantMessage(
      `Mapped request would be sent to Claude backend.\n` +
      `Original model: ${codexRequest.model || "default"}\n` +
      `Mapped model: ${claudeRequest.model}\n` +
      `Tools: ${(claudeRequest.allowed_tools || []).join(", ")}`
    );
    return;
  }

  // Build Claude SDK options
  const sdkOptions = {
    model: claudeRequest.model,
    tools: claudeRequest.allowed_tools,
    temperature: claudeRequest.temperature,
    max_tokens: claudeRequest.max_tokens,
    session_id: claudeRequest.session_id,
    resume: claudeRequest.resume,
    cwd: workOrder.workspace?.root,
  };

  // Create modified work order for Claude
  const claudeWorkOrder = {
    ...workOrder,
    config: {
      ...workOrder.config,
      vendor: {
        ...workOrder.config?.vendor,
        abp: {
          ...workOrder.config?.vendor?.abp,
          request: claudeRequest,
        },
      },
    },
  };

  // Run Claude adapter
  await claudeAdapter.run({
    workOrder: claudeWorkOrder,
    sdkOptions,
    policy: workOrder.policy || {},
    ...ctx,
  });
}

// ============================================================================
// Main Entry Point
// ============================================================================

async function main() {
  const adapter = loadAdapter();

  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  let sentHello = false;

  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;

    let envelope;
    try {
      envelope = JSON.parse(trimmed);
    } catch (e) {
      write({
        t: "error",
        error: "Invalid JSON envelope",
        raw: trimmed.slice(0, 100),
      });
      return;
    }

    // Handle hello handshake
    if (!sentHello) {
      // First message should be hello or run
      if (envelope.t === "hello") {
        // Respond to hello
        write({
          t: "hello",
          version: CONTRACT_VERSION,
          adapter: adapter ? adapter.name : "codex_host",
          adapter_version: adapter ? adapter.version : ADAPTER_VERSION,
          capabilities: getCapabilityManifest(),
        });
        sentHello = true;
        return;
      }

      if (envelope.t === "run") {
        // Auto-send hello first
        const mode = getExecutionMode(envelope.work_order || {});
        write({
          t: "hello",
          version: CONTRACT_VERSION,
          adapter: adapter ? adapter.name : "codex_host",
          adapter_version: adapter ? adapter.version : ADAPTER_VERSION,
          mode: mode,
          capabilities: getCapabilityManifest(),
        });
        sentHello = true;
      }
    }

    if (envelope.t === "run") {
      handleRun(envelope, adapter).catch((err) => {
        write({
          t: "final",
          id: envelope.id,
          error: err.message,
        });
      });
    } else if (envelope.t === "shutdown") {
      rl.close();
    }
  });

  rl.on("close", () => {
    process.exit(0);
  });
}

main().catch((err) => {
  console.error("[codex-host] Fatal error:", err);
  process.exit(1);
});
