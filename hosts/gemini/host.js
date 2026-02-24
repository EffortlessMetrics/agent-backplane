#!/usr/bin/env node

// Gemini sidecar for Agent Backplane (ABP).
//
// This process speaks JSONL envelopes over stdio:
// - hello (with mode: "mapped")
// - run
// - event*
// - final
//
// This sidecar operates in MAPPED mode, translating Claude-style dialect
// to Gemini engine. It implements:
// - Two-stage validation (facade + runtime capabilities)
// - Opinionated mapping with early failures
// - Emulation layer for non-native features
//
// A custom adapter can be provided via:
//   ABP_GEMINI_ADAPTER_MODULE=./path/to/adapter.js
//
// Adapter contract:
//   module.exports = {
//     name: "gemini_adapter_name",
//     version: "x.y.z",
//     async run(ctx) { ... }
//   }

const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const crypto = require("node:crypto");

const {
  SupportLevel,
  GeminiCapabilities,
  getCapabilityManifest,
} = require("./capabilities");

const {
  ErrorCodes,
  createError,
  validateFacade,
  validateCapabilities,
  mapClaudeToGemini,
  createMappedReceiptAdditions,
} = require("./mapper");

const CONTRACT_VERSION = "abp/v0.1";
const ADAPTER_VERSION = "0.1";
const MAX_INLINE_OUTPUT_BYTES = parseInt(
  process.env.ABP_GEMINI_MAX_INLINE_OUTPUT_BYTES || "8192",
  10
);

// Execution mode is always mapped for Gemini sidecar
const ExecutionMode = {
  Mapped: "mapped",
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
  return value
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+/, "")
    .replace(/-+$/, "")
    .slice(0, 64);
}

// ============================================================================
// Policy Engine (similar to Claude host)
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
        reason: `network host '${hostname}' is not in allow_network`,
      };
    }
    return { allowed: true };
  }

  function preTool(toolName, input) {
    const decision = canUseTool(toolName);
    if (!decision.allowed) {
      return decision;
    }
    if (requiresApproval(toolName)) {
      return {
        allowed: false,
        reason: `tool '${toolName}' requires approval (approval callbacks are not configured in abp/v0.1)`,
        code: ErrorCodes.REQUIRES_INTERACTIVE_APPROVAL,
      };
    }

    const lower = toolName.toLowerCase();
    const paths = collectPathValues(input);
    if (paths.length > 0) {
      for (const rawPath of paths) {
        const rel = canonicalWithin(workspaceRoot, rawPath);
        if (!rel) {
          return {
            allowed: false,
            reason: `path escapes workspace root: '${rawPath}'`,
          };
        }

        if (
          lower.includes("read") ||
          lower.includes("grep") ||
          lower.includes("glob")
        ) {
          const readDecision = canReadPath(rel);
          if (!readDecision.allowed) {
            return readDecision;
          }
        }

        if (
          lower.includes("write") ||
          lower.includes("edit") ||
          lower.includes("patch")
        ) {
          const writeDecision = canWritePath(rel);
          if (!writeDecision.allowed) {
            return writeDecision;
          }
        }
      }
    }

    if (lower.includes("web") || lower.includes("fetch") || lower.includes("search")) {
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

function extractHostname(input) {
  if (!input || typeof input !== "object") {
    return null;
  }
  for (const [k, v] of Object.entries(input)) {
    const key = k.toLowerCase();
    if (key.includes("url") || key.includes("host") || key.includes("uri")) {
      try {
        const url = new URL(v);
        return url.hostname;
      } catch (_) {
        // Not a valid URL
      }
    }
  }
  return null;
}

// ============================================================================
// Receipt Generation
// ============================================================================

function sha256Of(value) {
  const json = typeof value === "string" ? value : JSON.stringify(value);
  return crypto.createHash("sha256").update(json).digest("hex");
}

function computeReceiptHash(receipt) {
  // Null out the hash field before hashing (self-referential prevention)
  const v = JSON.parse(JSON.stringify(receipt));
  v.receipt_sha256 = null;
  return sha256Of(v);
}

function buildReceipt(runId, workOrder, startedAt, status, options = {}) {
  const completedAt = nowIso();
  const receipt = {
    id: runId,
    contract_version: CONTRACT_VERSION,
    started_at: startedAt,
    completed_at: completedAt,
    status,
    task: workOrder.task || "",
    backend: {
      name: "gemini",
      adapter: options.adapterName || "gemini_adapter",
      adapter_version: options.adapterVersion || ADAPTER_VERSION,
      mode: "mapped",
    },
    workspace_root: workOrder.workspace_root || null,
    usage: options.usage || {
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
    tool_calls: options.toolCalls || [],
    artifacts: options.artifacts || [],
    files_modified: options.filesModified || [],
    // Mapped mode additions
    ...createMappedReceiptAdditions({
      mappingWarnings: options.mappingWarnings || [],
      capabilitiesUsed: options.capabilitiesUsed || {
        native: [],
        emulated: [],
        unsupported: [],
      },
    }),
    receipt_sha256: null, // Will be computed
  };

  receipt.receipt_sha256 = computeReceiptHash(receipt);
  return receipt;
}

// ============================================================================
// Adapter Loading
// ============================================================================

function loadAdapter() {
  const customPath = process.env.ABP_GEMINI_ADAPTER_MODULE;
  if (customPath) {
    try {
      const resolved = path.resolve(customPath);
      const adapter = require(resolved);
      if (!adapter.name || !adapter.version || typeof adapter.run !== "function") {
        console.error(`Custom adapter at ${resolved} does not conform to the adapter contract`);
        process.exit(1);
      }
      return adapter;
    } catch (err) {
      console.error(`Failed to load custom adapter from ${customPath}: ${err.message}`);
      process.exit(1);
    }
  }

  // Default adapter
  try {
    return require("./adapter.js");
  } catch (err) {
    console.error(`Failed to load default adapter: ${err.message}`);
    process.exit(1);
  }
}

// ============================================================================
// Main Sidecar Logic
// ============================================================================

async function main() {
  const adapter = loadAdapter();

  // Send hello envelope with mapped mode
  write({
    t: "hello",
    contract_version: CONTRACT_VERSION,
    backend: {
      name: "gemini",
      version: adapter.version,
    },
    capabilities: getCapabilityManifest(),
    mode: ExecutionMode.Mapped,
  });

  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  for await (const line of rl) {
    if (!line.trim()) continue;

    let envelope;
    try {
      envelope = JSON.parse(line);
    } catch (err) {
      write({
        t: "fatal",
        ref_id: null,
        error: `Invalid JSON: ${err.message}`,
      });
      continue;
    }

    if (envelope.t !== "run") {
      write({
        t: "fatal",
        ref_id: null,
        error: `Expected 'run' envelope, got '${envelope.t}'`,
      });
      continue;
    }

    const runId = envelope.id;
    const workOrder = envelope.work_order;

    try {
      await handleRun(runId, workOrder, adapter);
    } catch (err) {
      write({
        t: "fatal",
        ref_id: runId,
        error: err.message,
      });
    }
  }
}

async function handleRun(runId, workOrder, adapter) {
  const startedAt = nowIso();
  const workspaceRoot = workOrder.workspace_root || process.cwd();
  const policy = workOrder.policy || {};
  const policyEngine = buildPolicyEngine(policy, workspaceRoot);

  // Events collector
  const events = [];
  const toolCalls = [];
  const artifacts = [];
  const mappingWarnings = [];

  // Event emitters
  function emitEvent(event) {
    events.push(event);
    write({
      t: "event",
      ref_id: runId,
      event,
    });
  }

  function emitAssistantDelta(text) {
    emitEvent({
      type: "assistant_delta",
      text,
      timestamp: nowIso(),
    });
  }

  function emitAssistantMessage(text) {
    emitEvent({
      type: "assistant_message",
      text,
      timestamp: nowIso(),
    });
  }

  function emitToolCall({ toolName, toolUseId, parentToolUseId, input }) {
    const id = toolUseId || `toolu_${crypto.randomUUID().replace(/-/g, "")}`;
    toolCalls.push({
      tool_name: toolName,
      tool_use_id: id,
      parent_tool_use_id: parentToolUseId || null,
      input,
      timestamp: nowIso(),
    });
    emitEvent({
      type: "tool_call",
      tool_name: toolName,
      tool_use_id: id,
      parent_tool_use_id: parentToolUseId || null,
      input,
      timestamp: nowIso(),
    });
    return id;
  }

  function emitToolResult({ toolName, toolUseId, output, isError }) {
    emitEvent({
      type: "tool_result",
      tool_name: toolName,
      tool_use_id: toolUseId,
      output: typeof output === "string" ? output : JSON.stringify(output),
      is_error: isError || false,
      timestamp: nowIso(),
    });
  }

  function emitWarning(message) {
    emitEvent({
      type: "warning",
      message,
      timestamp: nowIso(),
    });
  }

  function emitError(message) {
    emitEvent({
      type: "error",
      message,
      timestamp: nowIso(),
    });
  }

  function writeArtifact(kind, suggestedName, content) {
    const artifactId = `artifact_${crypto.randomUUID().replace(/-/g, "")}`;
    const artifact = {
      id: artifactId,
      kind,
      suggested_name: suggestedName,
      created_at: nowIso(),
    };

    // Store content based on size
    if (Buffer.byteLength(content) <= MAX_INLINE_OUTPUT_BYTES) {
      artifact.content = content;
    } else {
      const artifactsDir = path.join(workspaceRoot, ".abp", "artifacts");
      fs.mkdirSync(artifactsDir, { recursive: true });
      const fileName = `${sanitizeFilePart(suggestedName) || artifactId}.bin`;
      const filePath = path.join(artifactsDir, fileName);
      fs.writeFileSync(filePath, content);
      artifact.path = filePath;
    }

    artifacts.push(artifact);
    emitEvent({
      type: "artifact",
      artifact,
      timestamp: nowIso(),
    });
    return artifact;
  }

  function log(message) {
    // Log to stderr for debugging
    process.stderr.write(`[gemini-host] ${message}\n`);
  }

  // ========================================================================
  // Stage 1: Facade Validation
  // ========================================================================

  // Extract Claude-style request from work order
  const claudeRequest = {
    prompt: workOrder.task,
    cwd: workOrder.workspace_root,
    allowed_tools: policy.allowed_tools,
    permission_mode: workOrder.config?.permission_mode,
    setting_sources: workOrder.config?.setting_sources,
    mcp_servers: workOrder.config?.mcp_servers,
    thinking: workOrder.config?.thinking,
    extended_thinking: workOrder.config?.extended_thinking,
    agent_teams: workOrder.config?.agent_teams,
    model: workOrder.config?.model,
    temperature: workOrder.config?.temperature,
    max_tokens: workOrder.config?.max_tokens,
    hooks: workOrder.config?.hooks,
    memory: workOrder.config?.memory,
    checkpointing: workOrder.config?.checkpointing,
    checkpoint_interval: workOrder.config?.checkpoint_interval,
    session_id: workOrder.session_id,
    session_resume: workOrder.session_resume,
  };

  const facadeResult = validateFacade(claudeRequest);
  if (!facadeResult.valid) {
    // Early failure with typed error
    const firstError = facadeResult.errors[0];
    write({
      t: "fatal",
      ref_id: runId,
      error: JSON.stringify(firstError),
    });
    return;
  }

  // Collect warnings
  mappingWarnings.push(...facadeResult.warnings);

  // ========================================================================
  // Stage 2: Map to Gemini
  // ========================================================================

  const { geminiRequest, mappingWarnings: mapWarnings, capabilitiesUsed } = mapClaudeToGemini(claudeRequest);
  mappingWarnings.push(...mapWarnings);

  // ========================================================================
  // Stage 3: Runtime Capability Check
  // ========================================================================

  const capabilityResult = validateCapabilities(geminiRequest, GeminiCapabilities);
  if (!capabilityResult.valid) {
    const firstError = capabilityResult.errors[0];
    write({
      t: "fatal",
      ref_id: runId,
      error: JSON.stringify(firstError),
    });
    return;
  }

  // Merge capabilities
  capabilitiesUsed.native = [...new Set([...capabilitiesUsed.native, ...capabilityResult.capabilities_used.native])];
  capabilitiesUsed.emulated = [...new Set([...capabilitiesUsed.emulated, ...capabilityResult.capabilities_used.emulated])];

  // ========================================================================
  // Stage 4: Execute via Adapter
  // ========================================================================

  const ctx = {
    workOrder,
    sdkOptions: geminiRequest,
    policy,
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

  let status = "success";
  let usage = { input_tokens: 0, output_tokens: 0, total_tokens: 0 };

  try {
    await adapter.run(ctx);
  } catch (err) {
    status = "failed";
    emitError(err.message);
  }

  // ========================================================================
  // Final Receipt
  // ========================================================================

  const receipt = buildReceipt(runId, workOrder, startedAt, status, {
    adapterName: adapter.name,
    adapterVersion: adapter.version,
    usage,
    toolCalls,
    artifacts,
    mappingWarnings,
    capabilitiesUsed,
  });

  write({
    t: "final",
    ref_id: runId,
    receipt,
  });
}

// Run main
main().catch((err) => {
  console.error(`Fatal error: ${err.message}`);
  process.exit(1);
});
