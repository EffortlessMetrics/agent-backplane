#!/usr/bin/env node

// Claude sidecar for Agent Backplane (ABP).
//
// This process speaks JSONL envelopes over stdio:
// - hello
// - run
// - event*
// - final
//
// The host intentionally separates:
// - ABP protocol + policy + receipt shaping (this file)
// - Claude SDK invocation details (adapter module)
//
// A custom adapter can be provided via:
//   ABP_CLAUDE_ADAPTER_MODULE=./path/to/adapter.js
//
// Adapter contract:
//   module.exports = {
//     name: "claude_adapter_name",
//     version: "x.y.z",
//     async run(ctx) { ... }
//   }
//
// Where ctx contains:
//   - workOrder
//   - sdkOptions
//   - policy
//   - emitAssistantDelta(text)
//   - emitAssistantMessage(text)
//   - emitToolCall({ toolName, toolUseId, parentToolUseId, input })
//   - emitToolResult({ toolName, toolUseId, output, isError })
//   - emitWarning(message)
//   - emitError(message)
//   - writeArtifact(kind, suggestedName, content)
//
// If no custom adapter is provided, this script attempts a best-effort
// integration with common Claude Agent SDK entry points. If unavailable, it
// falls back to a deterministic "explain-only" mode.

const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const crypto = require("node:crypto");

const CONTRACT_VERSION = "abp/v0.1";
const ADAPTER_VERSION = "0.1";
const MAX_INLINE_OUTPUT_BYTES = parseInt(
  process.env.ABP_CLAUDE_MAX_INLINE_OUTPUT_BYTES || "8192",
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

function defaultCapabilities() {
  return {
    streaming: "native",
    tool_read: "emulated",
    tool_write: "emulated",
    tool_edit: "emulated",
    tool_bash: "emulated",
    tool_glob: "emulated",
    tool_grep: "emulated",
    tool_web_search: "emulated",
    tool_web_fetch: "emulated",
    hooks_pre_tool_use: "native",
    hooks_post_tool_use: "native",
    session_resume: "emulated",
    checkpointing: "emulated",
    structured_output_json_schema: "emulated",
    mcp_client: "emulated",
  };
}

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

    if (lower.includes("web") || lower.includes("fetch") || lower.includes("http")) {
      const maybeUrl =
        (input && (input.url || input.uri || input.endpoint)) || null;
      if (typeof maybeUrl === "string") {
        try {
          const host = new URL(maybeUrl).hostname;
          const netDecision = canAccessNetwork(host);
          if (!netDecision.allowed) {
            return netDecision;
          }
        } catch (_) {
          // Ignore parse errors and let the adapter/tool surface actual URL errors.
        }
      }
    }

    return { allowed: true };
  }

  return {
    canUseTool,
    canReadPath,
    canWritePath,
    canAccessNetwork,
    requiresApproval,
    preTool,
  };
}

function trimToolOutput(runCtx, toolName, output) {
  const text = safeString(output);
  const size = Buffer.byteLength(text, "utf8");
  if (size <= MAX_INLINE_OUTPUT_BYTES) {
    return output;
  }

  const stamp = Date.now();
  const baseName = `${sanitizeFilePart(toolName || "tool")}-${stamp}.txt`;
  const artifactPath = runCtx.writeArtifact("tool_output", baseName, text);
  const preview = text.slice(0, Math.min(text.length, 2048));
  return {
    output_preview: preview,
    output_truncated: true,
    bytes: size,
    artifact_path: artifactPath,
  };
}

function redactOutput(output) {
  const text = safeString(output);
  const redacted = text
    .replace(/\b(sk|api|token|secret)[_-]?[a-z0-9]{12,}\b/gi, "[REDACTED]")
    .replace(/(authorization:\s*bearer\s+)[a-z0-9._-]+/gi, "$1[REDACTED]");

  if (typeof output === "string") {
    return redacted;
  }
  if (redacted === text) {
    return output;
  }
  return {
    redacted: true,
    text: redacted,
  };
}

function permissionModeForLane(lane) {
  if (lane === "patch_first") {
    return "plan";
  }
  return "acceptEdits";
}

function buildPrompt(workOrder) {
  let prompt = String(workOrder.task || "").trim();
  const files = workOrder.context && Array.isArray(workOrder.context.files)
    ? workOrder.context.files
    : [];
  const snippets = workOrder.context && Array.isArray(workOrder.context.snippets)
    ? workOrder.context.snippets
    : [];

  if (files.length > 0) {
    prompt += "\n\nContext files:\n";
    for (const f of files) {
      prompt += `- ${f}\n`;
    }
  }

  if (snippets.length > 0) {
    prompt += "\nContext snippets:\n";
    for (const snippet of snippets) {
      const name = snippet && snippet.name ? snippet.name : "snippet";
      const content = snippet && snippet.content ? snippet.content : "";
      prompt += `\n[${name}]\n${content}\n`;
    }
  }

  return prompt;
}

function buildSdkOptions(workOrder) {
  const options = {
    cwd: workOrder.workspace && workOrder.workspace.root,
    env: workOrder.config && workOrder.config.env ? workOrder.config.env : {},
    model: workOrder.config && workOrder.config.model ? workOrder.config.model : undefined,
    permissionMode: permissionModeForLane(workOrder.lane),
    settingSources: ["project"],
    allowedTools:
      workOrder.policy && Array.isArray(workOrder.policy.allowed_tools)
        ? workOrder.policy.allowed_tools
        : undefined,
    disallowedTools:
      workOrder.policy && Array.isArray(workOrder.policy.disallowed_tools)
        ? workOrder.policy.disallowed_tools
        : undefined,
    vendor:
      workOrder.config && workOrder.config.vendor ? workOrder.config.vendor : {},
    maxTurns:
      workOrder.config && typeof workOrder.config.max_turns === "number"
        ? workOrder.config.max_turns
        : undefined,
  };
  return options;
}

function parseUsage(raw) {
  if (!raw || typeof raw !== "object") {
    return {};
  }
  const usage = raw.usage && typeof raw.usage === "object" ? raw.usage : raw;
  const pick = (keys) => {
    for (const k of keys) {
      if (typeof usage[k] === "number") {
        return usage[k];
      }
    }
    return undefined;
  };
  return {
    input_tokens: pick(["input_tokens", "inputTokens", "prompt_tokens", "promptTokens"]),
    output_tokens: pick(["output_tokens", "outputTokens", "completion_tokens", "completionTokens"]),
    cache_read_tokens: pick(["cache_read_tokens", "cacheReadTokens"]),
    cache_write_tokens: pick(["cache_write_tokens", "cacheWriteTokens"]),
  };
}

async function* toAsyncIterable(value) {
  const resolved = await value;
  if (!resolved) {
    return;
  }

  if (typeof resolved[Symbol.asyncIterator] === "function") {
    for await (const item of resolved) {
      yield item;
    }
    return;
  }

  if (typeof resolved[Symbol.iterator] === "function" && !ArrayBuffer.isView(resolved)) {
    for (const item of resolved) {
      yield item;
    }
    return;
  }

  yield resolved;
}

function tryRequire(moduleName) {
  try {
    // eslint-disable-next-line global-require, import/no-dynamic-require
    const mod = require(moduleName);
    let version = null;
    try {
      // eslint-disable-next-line global-require, import/no-dynamic-require
      const pkg = require(`${moduleName}/package.json`);
      version = pkg && pkg.version ? String(pkg.version) : null;
    } catch (_) {
      version = null;
    }
    return { moduleName, module: mod, version };
  } catch (_) {
    return null;
  }
}

function emitFromHeuristicMessage(runCtx, message, state) {
  if (message == null) {
    return;
  }

  if (typeof message === "string") {
    state.lastAssistantText = message;
    runCtx.emitAssistantDelta(message);
    return;
  }

  if (typeof message !== "object") {
    return;
  }

  if (message.usage && typeof message.usage === "object") {
    state.usageRaw = message;
  }

  const type = String(message.type || message.kind || message.event || "").toLowerCase();
  const text =
    typeof message.text === "string"
      ? message.text
      : typeof message.delta === "string"
        ? message.delta
        : typeof message.content === "string"
          ? message.content
          : null;

  if (text && (type.includes("delta") || type.includes("stream"))) {
    state.lastAssistantText = (state.lastAssistantText || "") + text;
    runCtx.emitAssistantDelta(text);
  } else if (text && (type.includes("assistant") || type.includes("message") || !type)) {
    state.lastAssistantText = text;
    runCtx.emitAssistantMessage(text);
  }

  const toolName =
    message.tool_name ||
    message.toolName ||
    message.name ||
    (message.tool && message.tool.name) ||
    null;

  if (toolName) {
    const toolUseId = message.tool_use_id || message.toolUseId || message.id || null;
    const input = message.input || message.arguments || message.args || {};
    const output = message.output || message.result || null;

    if (type.includes("result") || Object.prototype.hasOwnProperty.call(message, "output")) {
      runCtx.emitToolResult({
        toolName: String(toolName),
        toolUseId: toolUseId ? String(toolUseId) : null,
        output,
        isError: !!message.is_error || !!message.isError,
      });
    } else {
      runCtx.emitToolCall({
        toolName: String(toolName),
        toolUseId: toolUseId ? String(toolUseId) : null,
        parentToolUseId: null,
        input,
      });
    }
  }

  if (type.includes("error")) {
    const err =
      message.error ||
      message.message ||
      message.details ||
      "adapter message reported error";
    runCtx.emitError(safeString(err));
  }
}

function createFallbackAdapter(sdkProbe) {
  const probeText = sdkProbe
    ? `Detected package '${sdkProbe.moduleName}' but no supported entry point was found.`
    : "Claude Agent SDK package was not found.";

  return {
    name: "claude_fallback",
    version: ADAPTER_VERSION,
    capabilities: defaultCapabilities(),
    async run(ctx) {
      ctx.emitWarning(`${probeText} Running fallback adapter.`);
      ctx.emitAssistantMessage(
        "Claude sidecar is active. Provide ABP_CLAUDE_ADAPTER_MODULE with your exact Claude SDK binding to execute real agent runs."
      );
      ctx.emitAssistantMessage(`Task: ${ctx.workOrder.task}`);
      return {
        usageRaw: {
          mode: "fallback",
          sdk_detected: sdkProbe ? sdkProbe.moduleName : null,
        },
        outcome: "partial",
      };
    },
  };
}

function createHeuristicSdkAdapter(sdkProbe) {
  return {
    name: sdkProbe.moduleName,
    version: sdkProbe.version || null,
    capabilities: defaultCapabilities(),
    async run(ctx) {
      const sdk = sdkProbe.module;
      if (typeof sdk.query !== "function") {
        return createFallbackAdapter(sdkProbe).run(ctx);
      }

      const prompt = buildPrompt(ctx.workOrder);
      const options = ctx.sdkOptions;
      let response;
      let usedSignature = null;

      try {
        response = sdk.query({ prompt, options });
        usedSignature = "object";
      } catch (_) {
        try {
          response = sdk.query(prompt, options);
          usedSignature = "positional";
        } catch (err) {
          throw new Error(`failed to invoke sdk.query: ${safeString(err)}`);
        }
      }

      const state = {
        usageRaw: { query_signature: usedSignature },
        lastAssistantText: "",
      };

      for await (const message of toAsyncIterable(response)) {
        emitFromHeuristicMessage(ctx, message, state);
      }

      if (state.lastAssistantText && state.lastAssistantText.length > 0) {
        // Ensure at least one complete assistant message exists for non-streaming UIs.
        ctx.emitAssistantMessage(state.lastAssistantText);
      }

      return {
        usageRaw: state.usageRaw,
        usage: parseUsage(state.usageRaw),
        outcome: "complete",
      };
    },
  };
}

/**
 * Create a passthrough adapter that forwards requests directly to the SDK.
 *
 * Passthrough invariants:
 * - No request rewriting: SDK sees exactly what caller sent
 * - Stream equivalence: After removing ABP framing, stream is bitwise-equivalent to direct SDK call
 * - Observer-only governance: Log/record but don't modify tool calls or outputs
 * - Receipt out-of-band: Receipt doesn't appear in the stream
 */
function createPassthroughAdapter(sdkProbe) {
  return {
    name: sdkProbe ? `${sdkProbe.moduleName}_passthrough` : "claude_passthrough",
    version: sdkProbe ? sdkProbe.version : null,
    capabilities: {
      ...defaultCapabilities(),
      passthrough: "native",
      stream_equivalent: "native",
    },
    async run(ctx) {
      // Get the raw SDK request from the work order
      const rawRequest = getPassthroughRequest(ctx.workOrder);
      if (!rawRequest) {
        ctx.emitWarning("Passthrough mode requested but no abp.request provided. Using fallback.");
        return createFallbackAdapter(sdkProbe).run(ctx);
      }

      const sdk = sdkProbe ? sdkProbe.module : null;
      if (!sdk || typeof sdk.query !== "function") {
        ctx.emitWarning("Claude SDK not available for passthrough mode. Using fallback behavior.");
        ctx.emitAssistantMessage("Passthrough mode: SDK not available.");
        return {
          usageRaw: { mode: "passthrough_fallback", reason: "sdk_unavailable" },
          outcome: "partial",
        };
      }

      // PASSTHROUGH INVARIANT: Use the request exactly as provided
      // No modifications, no additions - the SDK sees exactly what the caller sent
      let response;
      try {
        // Pass the raw request directly to the SDK
        response = sdk.query(rawRequest);
      } catch (err) {
        throw new Error(`passthrough sdk.query failed: ${safeString(err)}`);
      }

      const state = {
        usageRaw: {},
        lastAssistantText: "",
      };

      // Process the response stream, preserving raw messages
      for await (const rawMessage of toAsyncIterable(response)) {
        // Store raw message for lossless reconstruction
        // The ext.raw_message field contains the verbatim SDK message
        ctx.emitPassthroughEvent(rawMessage);

        // Also extract usage info if present
        if (rawMessage && typeof rawMessage === "object" && rawMessage.usage) {
          state.usageRaw = rawMessage;
        }
      }

      return {
        usageRaw: state.usageRaw,
        usage: parseUsage(state.usageRaw),
        outcome: "complete",
        stream_equivalent: true, // Guarantee: stream is bitwise-equivalent after removing ABP framing
      };
    },
  };
}

function resolveAdapterModulePath(rawPath) {
  const fromCwd = path.resolve(process.cwd(), rawPath);
  if (fs.existsSync(fromCwd)) {
    return fromCwd;
  }
  return path.resolve(rawPath);
}

function loadAdapter(mode = ExecutionMode.Mapped) {
  const customPath = process.env.ABP_CLAUDE_ADAPTER_MODULE;
  if (customPath) {
    const resolved = resolveAdapterModulePath(customPath);
    // eslint-disable-next-line global-require, import/no-dynamic-require
    const loaded = require(resolved);
    const adapter = loaded && loaded.default ? loaded.default : loaded;
    if (!adapter || typeof adapter.run !== "function") {
      throw new Error(
        `custom adapter '${resolved}' must export an object with async run(ctx)`
      );
    }
    return {
      name: adapter.name || "custom_claude_adapter",
      version: adapter.version || null,
      capabilities: {
        ...defaultCapabilities(),
        ...(adapter.capabilities || {}),
      },
      run: adapter.run,
    };
  }

  const probe =
    tryRequire("@anthropic-ai/claude-agent-sdk") ||
    tryRequire("claude-agent-sdk");

  if (!probe) {
    return createFallbackAdapter(null);
  }

  // Select adapter based on execution mode
  if (mode === ExecutionMode.Passthrough) {
    return createPassthroughAdapter(probe);
  }
  return createHeuristicSdkAdapter(probe);
}

function createRunContext(runId, workOrder, trace, artifacts, emitEvent) {
  const workspaceRoot =
    (workOrder.workspace && workOrder.workspace.root) || process.cwd();
  const policy = buildPolicyEngine(workOrder.policy || {}, workspaceRoot);

  const artifactRoot = path.join(
    workspaceRoot,
    ".agent-backplane",
    "artifacts",
    runId
  );
  fs.mkdirSync(artifactRoot, { recursive: true });

  function emit(kind) {
    const ev = { ts: nowIso(), ...kind };
    trace.push(ev);
    emitEvent(ev);
  }

  function writeArtifact(kind, suggestedName, content) {
    const baseName = sanitizeFilePart(suggestedName || kind || "artifact") || "artifact";
    let fileName = baseName;
    if (!path.extname(fileName)) {
      fileName += ".txt";
    }
    const absPath = path.join(artifactRoot, fileName);
    fs.writeFileSync(absPath, safeString(content), "utf8");
    const relPath = path.relative(workspaceRoot, absPath);
    const relPosix = toPosixPath(relPath);
    artifacts.push({ kind, path: relPosix });
    return relPosix;
  }

  function emitToolCall(payload) {
    const toolName = String(payload.toolName || "unknown_tool");
    const input = payload.input || {};
    const pre = policy.preTool(toolName, input);
    if (!pre.allowed) {
      emit({
        type: "warning",
        message: `Denied ${toolName}: ${pre.reason}`,
      });
      emit({
        type: "tool_result",
        tool_name: toolName,
        tool_use_id: payload.toolUseId || null,
        output: { denied: true, reason: pre.reason },
        is_error: true,
      });
      return false;
    }

    emit({
      type: "tool_call",
      tool_name: toolName,
      tool_use_id: payload.toolUseId || null,
      parent_tool_use_id: payload.parentToolUseId || null,
      input,
    });
    return true;
  }

  function emitToolResult(payload) {
    const toolName = String(payload.toolName || "unknown_tool");
    const redacted = redactOutput(payload.output);
    const output = trimToolOutput(
      {
        writeArtifact,
      },
      toolName,
      redacted
    );
    emit({
      type: "tool_result",
      tool_name: toolName,
      tool_use_id: payload.toolUseId || null,
      output,
      is_error: !!payload.isError,
    });
  }

  return {
    workOrder,
    policy,
    sdkOptions: buildSdkOptions(workOrder),
    emitRaw(kind) {
      emit(kind);
    },
    emitAssistantDelta(text) {
      emit({ type: "assistant_delta", text: String(text || "") });
    },
    emitAssistantMessage(text) {
      emit({ type: "assistant_message", text: String(text || "") });
    },
    emitToolCall,
    emitToolResult,
    emitWarning(message) {
      emit({ type: "warning", message: String(message || "") });
    },
    emitError(message) {
      emit({ type: "error", message: String(message || "") });
    },
    writeArtifact,
    /**
     * Emit a passthrough event with the raw SDK message.
     * This is used in passthrough mode to preserve the original
     * SDK message for lossless reconstruction.
     *
     * @param {object} rawMessage - The verbatim SDK message
     */
    emitPassthroughEvent(rawMessage) {
      // Determine event kind from raw message
      const msgType = String(
        rawMessage.type || rawMessage.kind || rawMessage.event || ""
      ).toLowerCase();

      let kind = "assistant_delta"; // default
      let payload = {};

      if (rawMessage.usage) {
        kind = "usage";
        payload = { usage: rawMessage.usage };
      } else if (msgType.includes("delta") || msgType.includes("stream")) {
        kind = "assistant_delta";
        const text =
          typeof rawMessage.text === "string"
            ? rawMessage.text
            : typeof rawMessage.delta === "string"
              ? rawMessage.delta
              : typeof rawMessage.content === "string"
                ? rawMessage.content
                : "";
        payload = { text };
      } else if (msgType.includes("assistant") || msgType.includes("message") || !msgType) {
        kind = "assistant_message";
        const text =
          typeof rawMessage.text === "string"
            ? rawMessage.text
            : typeof rawMessage.content === "string"
              ? rawMessage.content
              : "";
        payload = { text };
      } else if (msgType.includes("tool") && (msgType.includes("result") || rawMessage.output !== undefined)) {
        kind = "tool_result";
        payload = {
          tool_name: String(
            rawMessage.tool_name ||
            rawMessage.toolName ||
            rawMessage.name ||
            "unknown_tool"
          ),
          tool_use_id: rawMessage.tool_use_id || rawMessage.toolUseId || rawMessage.id || null,
          output: rawMessage.output || rawMessage.result || {},
          is_error: !!rawMessage.is_error || !!rawMessage.isError,
        };
      } else if (msgType.includes("tool")) {
        kind = "tool_call";
        payload = {
          tool_name: String(
            rawMessage.tool_name ||
            rawMessage.toolName ||
            rawMessage.name ||
            "unknown_tool"
          ),
          tool_use_id: rawMessage.tool_use_id || rawMessage.toolUseId || rawMessage.id || null,
          parent_tool_use_id: null,
          input: rawMessage.input || rawMessage.arguments || rawMessage.args || {},
        };
      } else if (msgType.includes("error")) {
        kind = "error";
        payload = {
          message: safeString(
            rawMessage.error ||
            rawMessage.message ||
            rawMessage.details ||
            "unknown error"
          ),
        };
      }

      // Emit with ext field containing the raw message for lossless reconstruction
      const ev = {
        ts: nowIso(),
        type: kind,
        ...payload,
        ext: { raw_message: rawMessage },
      };
      trace.push(ev);
      emitEvent(ev);
    },
  };
}

async function handleRun(runMsg, adapter, backend, capabilities, mode = ExecutionMode.Mapped) {
  const runId = runMsg.id || crypto.randomUUID();
  const workOrder = runMsg.work_order || {};
  const startedAt = nowIso();
  const trace = [];
  const artifacts = [];

  function emitEvent(event) {
    write({ t: "event", ref_id: runId, event });
  }

  const ctx = createRunContext(runId, workOrder, trace, artifacts, emitEvent);

  let usageRaw = {};
  let usage = {};
  let outcome = "complete";
  let streamEquivalent = false;

  ctx.emitRaw({
    type: "run_started",
    message: `claude sidecar starting: ${safeString(workOrder.task)}`,
  });

  ctx.emitAssistantMessage(`Using adapter: ${adapter.name}`);
  if (adapter.version) {
    ctx.emitAssistantMessage(`Adapter version: ${adapter.version}`);
  }
  ctx.emitAssistantMessage(`Execution mode: ${mode}`);

  try {
    const result = (await adapter.run(ctx)) || {};
    if (result.usageRaw && typeof result.usageRaw === "object") {
      usageRaw = result.usageRaw;
    }
    if (result.usage && typeof result.usage === "object") {
      usage = result.usage;
    } else {
      usage = parseUsage(usageRaw);
    }
    if (typeof result.outcome === "string") {
      outcome = result.outcome;
    }
    if (result.stream_equivalent === true) {
      streamEquivalent = true;
    }
  } catch (err) {
    outcome = "failed";
    ctx.emitError(`adapter error: ${safeString(err && err.stack ? err.stack : err)}`);
  }

  ctx.emitRaw({
    type: "run_completed",
    message: `claude sidecar run completed with outcome=${outcome}`,
  });

  const finishedAt = nowIso();
  const durationMs = Math.max(
    0,
    new Date(finishedAt).getTime() - new Date(startedAt).getTime()
  );

  const receipt = {
    meta: {
      run_id: runId,
      work_order_id: workOrder.id,
      contract_version: CONTRACT_VERSION,
      started_at: startedAt,
      finished_at: finishedAt,
      duration_ms: durationMs,
    },
    backend,
    capabilities,
    mode,
    usage_raw: usageRaw,
    usage,
    trace,
    artifacts,
    verification: { git_diff: null, git_status: null, harness_ok: true },
    outcome,
    receipt_sha256: null,
  };

  // In passthrough mode, include stream_equivalent guarantee
  if (mode === ExecutionMode.Passthrough && streamEquivalent) {
    receipt.stream_equivalent = true;
  }

  write({ t: "final", ref_id: runId, receipt });
}

function main() {
  // Pre-load adapters for both modes
  let mappedAdapter;
  let passthroughAdapter;
  let defaultAdapter;

  try {
    mappedAdapter = loadAdapter(ExecutionMode.Mapped);
    passthroughAdapter = loadAdapter(ExecutionMode.Passthrough);
    defaultAdapter = mappedAdapter;
  } catch (err) {
    const backend = {
      id: "claude_agent_sdk",
      backend_version: null,
      adapter_version: ADAPTER_VERSION,
    };
    write({
      t: "hello",
      contract_version: CONTRACT_VERSION,
      backend,
      capabilities: defaultCapabilities(),
      mode: ExecutionMode.Mapped,
    });
    write({
      t: "fatal",
      ref_id: null,
      error: `failed to load adapter: ${safeString(err)}`,
    });
    process.exitCode = 1;
    return;
  }

  const backend = {
    id: "claude_agent_sdk",
    backend_version: defaultAdapter.version || null,
    adapter_version: ADAPTER_VERSION,
  };

  const capabilities = {
    ...defaultCapabilities(),
    ...(defaultAdapter.capabilities || {}),
  };

  // Send hello with default mode (mapped)
  // The actual mode is determined per-work-order
  write({
    t: "hello",
    contract_version: CONTRACT_VERSION,
    backend,
    capabilities,
    mode: ExecutionMode.Mapped,
  });

  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  rl.on("line", (line) => {
    if (!line || !line.trim()) {
      return;
    }

    let msg;
    try {
      msg = JSON.parse(line);
    } catch (err) {
      write({
        t: "fatal",
        ref_id: null,
        error: `invalid json: ${safeString(err)}`,
      });
      return;
    }

    if (msg.t !== "run") {
      return;
    }

    // Detect execution mode from work order
    const workOrder = msg.work_order || {};
    const mode = getExecutionMode(workOrder);

    // Select adapter based on mode
    const adapter = mode === ExecutionMode.Passthrough ? passthroughAdapter : mappedAdapter;

    handleRun(msg, adapter, backend, capabilities, mode).catch((err) => {
      write({
        t: "fatal",
        ref_id: msg.id || null,
        error: `run failed: ${safeString(err && err.stack ? err.stack : err)}`,
      });
    });
  });
}

main();
