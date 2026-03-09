/**
 * Claude SDK V2 Preview Adapter for Agent Backplane (ABP).
 *
 * This is a SEPARATE module from the V1 adapter (adapter.js). It is selected
 * per-run when the work order specifies:
 *   - claude.sdk_surface = "ts_v2_preview"
 *   - claude.v2_preview = true
 *
 * V2 surface transports:
 *   1. Prompt  — one-shot via sdk.unstable_v2_prompt()
 *   2. Session — stateful via sdk.unstable_v2_createSession() / session.send()
 *   3. Resume  — session resume via sdk.unstable_v2_resumeSession()
 *
 * All V2 receipts carry { sdk_surface: "ts_v2_preview", api_version: "v2_preview", preview: true }.
 */

const path = require("node:path");
const { pathToFileURL } = require("node:url");

const ADAPTER_NAME = "claude_sdk_v2_preview_adapter";
const ADAPTER_VERSION = "0.1.0";
const DEFAULT_SDK_MODULES = ["@anthropic-ai/claude-agent-sdk", "claude-agent-sdk"];

// V1-only options that must not be forwarded to V2 transports.
const V1_ONLY = ["forkSession", "cliPath", "extraArgs", "stderr", "maxBufferSize"];

// ---------------------------------------------------------------------------
// Utility helpers (shared patterns from V1, kept local for isolation)
// ---------------------------------------------------------------------------

function safeString(value) {
  if (value == null) return "";
  if (value instanceof Error) return value.stack || value.message || String(value);
  if (typeof value === "string") return value;
  if (typeof value === "object" && typeof value.message === "string") return value.message;
  try { return JSON.stringify(value); } catch (_) { return String(value); }
}

function asObject(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return value;
}

function pickValue(obj, keys) {
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(obj, key)) return obj[key];
  }
  return undefined;
}

function pickString(obj, keys) {
  const value = pickValue(obj, keys);
  if (typeof value === "string" && value.trim().length > 0) return value.trim();
  return undefined;
}

function pickBoolean(obj, keys) {
  const value = pickValue(obj, keys);
  return typeof value === "boolean" ? value : undefined;
}

function pickNumber(obj, keys) {
  const value = pickValue(obj, keys);
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function pickArray(obj, keys) {
  const value = pickValue(obj, keys);
  return Array.isArray(value) ? value : undefined;
}

function pickObject(obj, keys) {
  const value = pickValue(obj, keys);
  return value && typeof value === "object" && !Array.isArray(value) ? value : undefined;
}

function compactObject(value) {
  const out = {};
  for (const [key, entry] of Object.entries(asObject(value))) {
    if (entry !== undefined) out[key] = entry;
  }
  return out;
}

function getVendorNamespace(workOrder, namespace) {
  const vendor = asObject(workOrder?.config?.vendor);
  const out = {};
  Object.assign(out, asObject(vendor[namespace]));
  const prefix = `${namespace}.`;
  for (const [key, value] of Object.entries(vendor)) {
    if (key.startsWith(prefix)) out[key.slice(prefix.length)] = value;
  }
  return out;
}

function lowerType(value) {
  return String(value || "").toLowerCase();
}

function normalizeUsage(raw) {
  const usage = asObject(raw?.usage || raw);
  return compactObject({
    input_tokens: pickNumber(usage, ["input_tokens", "inputTokens", "prompt_tokens", "promptTokens"]),
    output_tokens: pickNumber(usage, ["output_tokens", "outputTokens", "completion_tokens", "completionTokens"]),
    cache_read_tokens: pickNumber(usage, ["cache_read_tokens", "cacheReadTokens"]),
    cache_write_tokens: pickNumber(usage, ["cache_write_tokens", "cacheWriteTokens"]),
  });
}

function mergeUsage(target, source) {
  if (!source || typeof source !== "object") return target;
  return { ...asObject(target), ...asObject(source) };
}

// ---------------------------------------------------------------------------
// V1-only option rejection
// ---------------------------------------------------------------------------

function rejectV1OnlyOptions(ctx, options) {
  const warnings = [];
  for (const key of V1_ONLY) {
    if (Object.prototype.hasOwnProperty.call(options, key) && options[key] !== undefined) {
      warnings.push(key);
      ctx.emitWarning(
        `V2 preview: option '${key}' is V1-only and will be ignored in the ts_v2_preview surface.`
      );
    }
  }
  // Return a copy with V1-only keys stripped
  if (warnings.length === 0) return options;
  const cleaned = { ...options };
  for (const key of V1_ONLY) {
    delete cleaned[key];
  }
  return cleaned;
}

// ---------------------------------------------------------------------------
// Preview labeling
// ---------------------------------------------------------------------------

function v2UsageRaw(extra) {
  return {
    sdk_surface: "ts_v2_preview",
    api_version: "v2_preview",
    preview: true,
    ...asObject(extra),
  };
}

// ---------------------------------------------------------------------------
// Prompt builder (reuses V1 pattern)
// ---------------------------------------------------------------------------

function buildPrompt(workOrder) {
  let prompt = String(workOrder?.task || "").trim();
  const context = asObject(workOrder?.context);
  const files = Array.isArray(context.files) ? context.files : [];
  const snippets = Array.isArray(context.snippets) ? context.snippets : [];

  if (files.length > 0) {
    prompt += "\n\nContext files:\n";
    for (const file of files) {
      prompt += `- ${safeString(file)}\n`;
    }
  }

  if (snippets.length > 0) {
    prompt += "\nContext snippets:\n";
    for (const snippet of snippets) {
      const name = safeString(snippet?.name || "snippet");
      const content = safeString(snippet?.content || "");
      prompt += `\n[${name}]\n${content}\n`;
    }
  }

  return prompt;
}

// ---------------------------------------------------------------------------
// V2 event emission
// ---------------------------------------------------------------------------

function emitV2Event(ctx, event, state) {
  if (typeof event === "string") {
    state.lastAssistantText += event;
    state.sawAssistantDelta = true;
    ctx.emitAssistantDelta(event);
    return;
  }

  if (!event || typeof event !== "object") return;

  // Collect usage from any event that carries it
  if (event.usage && typeof event.usage === "object") {
    state.usageRaw = mergeUsage(state.usageRaw, event.usage);
  }

  const rawType = lowerType(event.type || event.kind || event.event);

  // Hook events: pre_tool_use, post_tool_use
  if (rawType.includes("pre_tool_use") || rawType.includes("pretooluse")) {
    const toolName = String(event.tool_name || event.toolName || event.name || "unknown_tool");
    const toolUseId = event.tool_use_id || event.toolUseId || event.id || null;
    const decision = String(event.decision || event.action || "allow").toLowerCase();
    ctx.emitToolCall({
      toolName,
      toolUseId,
      parentToolUseId: event.parent_tool_use_id || event.parentToolUseId || null,
      input: event.input || event.arguments || event.args || {},
      ext: { hook: "pre_tool_use", decision },
    });
    return;
  }

  if (rawType.includes("post_tool_use") || rawType.includes("posttooluse")) {
    const isFailure = rawType.includes("failure") || rawType.includes("error");
    const toolName = String(event.tool_name || event.toolName || event.name || "unknown_tool");
    const toolUseId = event.tool_use_id || event.toolUseId || event.id || null;
    const hookName = isFailure ? "post_tool_use_failure" : "post_tool_use";
    ctx.emitToolResult({
      toolName,
      toolUseId,
      output: Object.prototype.hasOwnProperty.call(event, "output") ? event.output : (event.result || null),
      isError: isFailure || !!event.is_error || !!event.isError,
      ext: { hook: hookName },
    });
    return;
  }

  // Notification -> warning with notification ext
  if (rawType.includes("notification")) {
    const level = String(event.level || event.severity || "info").toLowerCase();
    const message = safeString(event.message || event.text || event.notification || "");
    ctx.emitWarning(message);
    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "warning",
        message,
        ext: { notification: true, level },
      });
    }
    return;
  }

  // Subagent start/stop
  if (rawType.includes("subagent_start") || rawType.includes("subagentstart")) {
    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "subagent_spawned",
        agent_id: String(event.agent_id || event.agentId || event.id || ""),
        task: String(event.task || event.description || ""),
      });
    }
    return;
  }

  if (rawType.includes("subagent_stop") || rawType.includes("subagentstop") || rawType.includes("subagent_complete")) {
    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "subagent_completed",
        agent_id: String(event.agent_id || event.agentId || event.id || ""),
        success: !!(event.success ?? event.ok ?? true),
      });
    }
    return;
  }

  // Permission request -> permission_requested + auto-resolved
  if (rawType.includes("permission_request") || rawType.includes("permissionrequest")) {
    const toolName = String(event.tool_name || event.toolName || event.name || "unknown_tool");
    const toolInput = event.input || event.arguments || event.args || {};
    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "permission_requested",
        tool_name: toolName,
        input: toolInput,
      });
      ctx.emitRaw({
        type: "permission_resolved",
        tool_name: toolName,
        granted: true,
        reason: "auto-approved (non-interactive sidecar)",
      });
    }
    return;
  }

  // Checkpoint events
  if (rawType.includes("checkpoint")) {
    const checkpointId = String(event.checkpoint_id || event.checkpointId || event.id || "");
    ctx.emitAssistantMessage(
      checkpointId ? `Checkpoint created: ${checkpointId}` : "Checkpoint created"
    );
    // Also emit as raw with ext metadata if available
    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "assistant_message",
        text: checkpointId ? `Checkpoint created: ${checkpointId}` : "Checkpoint created",
        ext: checkpointId ? { checkpoint_id: checkpointId } : { checkpoint: true },
      });
    }
    return;
  }

  // Text deltas
  if (rawType.includes("delta") || rawType.includes("stream")) {
    const text = event.text || event.delta || event.content || "";
    if (text) {
      state.lastAssistantText += text;
      state.sawAssistantDelta = true;
      ctx.emitAssistantDelta(text);
    }
    return;
  }

  // Complete assistant message
  if (rawType.includes("message") || rawType.includes("assistant")) {
    const text = event.text || event.content || "";
    if (text) {
      state.lastAssistantText = text;
      state.sawAssistantMessage = true;
      ctx.emitAssistantMessage(text);
    }
    return;
  }

  // Tool call
  if (rawType.includes("tool_use") || rawType.includes("tool_call")) {
    ctx.emitToolCall({
      toolName: String(event.tool_name || event.name || "unknown_tool"),
      toolUseId: event.tool_use_id || event.id || null,
      parentToolUseId: event.parent_tool_use_id || null,
      input: event.input || event.arguments || {},
    });
    return;
  }

  // Tool result
  if (rawType.includes("tool_result") || rawType.includes("result")) {
    ctx.emitToolResult({
      toolName: String(event.tool_name || event.name || "unknown_tool"),
      toolUseId: event.tool_use_id || event.id || null,
      output: event.output || event.result || null,
      isError: !!event.is_error || !!event.isError,
    });
    return;
  }

  // Error
  if (rawType.includes("error")) {
    ctx.emitError(safeString(event.error || event.message || "v2 sdk error"));
    return;
  }

  // Usage-only event
  if (rawType === "usage" || rawType === "usage_update") {
    // Already merged above
    return;
  }

  // Fallback: try to extract text from anything that looks like a message
  const fallbackText = event.text || event.delta || event.content || "";
  if (fallbackText) {
    state.lastAssistantText += fallbackText;
    state.sawAssistantDelta = true;
    ctx.emitAssistantDelta(fallbackText);
  }
}

// ---------------------------------------------------------------------------
// SDK loader (mirrors V1 pattern but looks for V2 methods)
// ---------------------------------------------------------------------------

let cachedSdk = null;

function normalizeSpecifier(raw) {
  const value = String(raw || "").trim();
  if (!value) return null;
  const looksLikePath =
    value.startsWith(".") ||
    value.startsWith("/") ||
    value.startsWith("\\") ||
    /^[a-z]:\\/i.test(value);
  if (looksLikePath) return path.resolve(process.cwd(), value);
  return value;
}

function canRequireAsCjs(err) {
  if (!err || typeof err !== "object") return false;
  return (
    err.code === "ERR_REQUIRE_ESM" ||
    err.code === "ERR_REQUIRE_ASYNC_MODULE" ||
    String(err.message || "").includes("Must use import")
  );
}

async function importModule(specifier) {
  try {
    return require(specifier);
  } catch (err) {
    if (!canRequireAsCjs(err)) throw err;
    const looksLikePath =
      specifier.startsWith("/") ||
      specifier.startsWith("\\") ||
      /^[a-z]:\\/i.test(specifier);
    if (looksLikePath) return import(pathToFileURL(specifier).href);
    return import(specifier);
  }
}

function moduleNotFound(err) {
  if (!err || typeof err !== "object") return false;
  if (err.code === "MODULE_NOT_FOUND" || err.code === "ERR_MODULE_NOT_FOUND") return true;
  return String(err.message || "").includes("Cannot find module");
}

function hasV2Methods(mod) {
  if (!mod) return false;
  const target = mod.default || mod;
  return (
    typeof target.unstable_v2_prompt === "function" ||
    typeof target.unstable_v2_createSession === "function"
  );
}

async function loadSdk() {
  if (cachedSdk) return cachedSdk;

  const candidates = [];
  if (process.env.ABP_CLAUDE_SDK_MODULE && process.env.ABP_CLAUDE_SDK_MODULE.trim()) {
    candidates.push(normalizeSpecifier(process.env.ABP_CLAUDE_SDK_MODULE));
  }
  for (const moduleName of DEFAULT_SDK_MODULES) {
    candidates.push(moduleName);
  }

  let lastError = null;
  for (const candidate of candidates) {
    if (!candidate) continue;
    try {
      const mod = await importModule(candidate);
      const target = mod.default || mod;
      if (!hasV2Methods(target)) {
        throw new Error(
          `module '${candidate}' does not export unstable_v2_prompt() or unstable_v2_createSession()`
        );
      }
      cachedSdk = { moduleName: candidate, module: mod, target };
      return cachedSdk;
    } catch (err) {
      lastError = err;
      if (!moduleNotFound(err)) {
        // Keep trying candidates
      }
    }
  }

  throw new Error(`unable to load Claude SDK with V2 surface: ${safeString(lastError)}`);
}

// ---------------------------------------------------------------------------
// V2 transport: one-shot prompt
// ---------------------------------------------------------------------------

async function runV2Prompt(ctx, sdk, request) {
  const state = {
    usageRaw: {},
    lastAssistantText: "",
    sawAssistantDelta: false,
    sawAssistantMessage: false,
  };

  const target = sdk.target;
  const response = await target.unstable_v2_prompt(request.prompt, request.options);

  // The response could be a string, an object, or an async iterable
  if (typeof response === "string") {
    state.lastAssistantText = response;
    state.sawAssistantMessage = true;
    ctx.emitAssistantMessage(response);
  } else if (response && typeof response[Symbol.asyncIterator] === "function") {
    for await (const event of response) {
      emitV2Event(ctx, event, state);
    }
  } else if (response && typeof response[Symbol.iterator] === "function" && !ArrayBuffer.isView(response)) {
    for (const event of response) {
      emitV2Event(ctx, event, state);
    }
  } else if (response && typeof response === "object") {
    emitV2Event(ctx, response, state);
  }

  // Ensure at least one complete assistant message
  if (state.sawAssistantDelta && !state.sawAssistantMessage && state.lastAssistantText.length > 0) {
    ctx.emitAssistantMessage(state.lastAssistantText);
  }

  return {
    usageRaw: v2UsageRaw({
      sdk_module: sdk.moduleName,
      transport: "v2_prompt",
      ...asObject(state.usageRaw),
    }),
    usage: normalizeUsage(state.usageRaw),
    outcome: "complete",
  };
}

// ---------------------------------------------------------------------------
// V2 transport: session-based
// ---------------------------------------------------------------------------

async function runV2Session(ctx, sdk, request) {
  const state = {
    usageRaw: {},
    lastAssistantText: "",
    sawAssistantDelta: false,
    sawAssistantMessage: false,
  };

  const target = sdk.target;
  const options = request.options || {};

  let session;
  let isResume = false;

  if (options.resume && options.sessionId) {
    // Resume an existing session
    if (typeof target.unstable_v2_resumeSession !== "function") {
      throw new Error("V2 session resume requested but unstable_v2_resumeSession() is unavailable");
    }
    session = await target.unstable_v2_resumeSession(options.sessionId, options);
    isResume = true;

    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "session_resumed",
        session_id: session.id || options.sessionId,
        resumed_from: String(options.resume),
      });
    }
  } else {
    // Create a new session
    if (typeof target.unstable_v2_createSession !== "function") {
      throw new Error("V2 session requested but unstable_v2_createSession() is unavailable");
    }
    session = await target.unstable_v2_createSession(options);

    if (typeof ctx.emitRaw === "function") {
      ctx.emitRaw({
        type: "session_started",
        session_id: session.id,
      });
    }
  }

  // Send the prompt and stream back events
  const stream = await session.send(request.prompt);

  if (stream && typeof stream[Symbol.asyncIterator] === "function") {
    for await (const event of stream) {
      emitV2Event(ctx, event, state);
    }
  } else if (stream && typeof stream[Symbol.iterator] === "function" && !ArrayBuffer.isView(stream)) {
    for (const event of stream) {
      emitV2Event(ctx, event, state);
    }
  } else if (stream && typeof stream === "object") {
    emitV2Event(ctx, stream, state);
  } else if (typeof stream === "string") {
    state.lastAssistantText = stream;
    state.sawAssistantMessage = true;
    ctx.emitAssistantMessage(stream);
  }

  // Ensure at least one complete assistant message
  if (state.sawAssistantDelta && !state.sawAssistantMessage && state.lastAssistantText.length > 0) {
    ctx.emitAssistantMessage(state.lastAssistantText);
  }

  // Close the session
  if (typeof session.close === "function") {
    await session.close();
  }

  if (typeof ctx.emitRaw === "function") {
    ctx.emitRaw({
      type: "session_completed",
      session_id: session.id,
    });
  }

  return {
    usageRaw: v2UsageRaw({
      sdk_module: sdk.moduleName,
      transport: isResume ? "v2_session_resume" : "v2_session",
      session_id: session.id,
      ...asObject(state.usageRaw),
    }),
    usage: normalizeUsage(state.usageRaw),
    outcome: "complete",
  };
}

// ---------------------------------------------------------------------------
// Transport selection
// ---------------------------------------------------------------------------

function selectTransport(sdk, options) {
  const target = sdk.target;
  const wantsSession =
    options.sessionId || options.session_id || options.resume;

  if (wantsSession && typeof target.unstable_v2_createSession === "function") {
    return "session";
  }
  if (typeof target.unstable_v2_prompt === "function") {
    return "prompt";
  }
  if (typeof target.unstable_v2_createSession === "function") {
    return "session";
  }
  return null;
}

// ---------------------------------------------------------------------------
// Build V2 request from work order
// ---------------------------------------------------------------------------

function buildV2Request(ctx) {
  const workOrder = asObject(ctx?.workOrder);
  const claudeCfg = getVendorNamespace(workOrder, "claude");
  const base = asObject(ctx?.sdkOptions);
  const optionOverrides = asObject(claudeCfg.options);
  const merged = { ...base, ...optionOverrides };

  const options = compactObject({
    cwd: pickString(merged, ["cwd", "workingDirectory", "working_directory"]),
    model: pickString(merged, ["model"]),
    env: pickObject(merged, ["env"]),
    permissionMode: pickString(merged, ["permissionMode", "permission_mode"]),
    sessionId: pickString(merged, ["sessionId", "session_id"]),
    resume: pickValue(merged, ["resume", "resume_session", "resume_session_id"]),
    settingSources: pickArray(merged, ["settingSources", "setting_sources"]),
    allowedTools: pickArray(merged, ["allowedTools", "allowed_tools"]),
    disallowedTools: pickArray(merged, ["disallowedTools", "disallowed_tools"]),
    maxTurns: pickNumber(merged, ["maxTurns", "max_turns"]),
    systemPrompt: pickString(merged, ["systemPrompt", "system_prompt"]),
    mcpServers: pickObject(merged, ["mcpServers", "mcp_servers"]),
    tools: pickArray(merged, ["tools"]),
    continue: pickBoolean(merged, ["continue"]),
    maxBudgetUsd: pickNumber(merged, ["maxBudgetUsd", "max_budget_usd"]),
    outputFormat: pickValue(merged, ["outputFormat", "output_format"]),
    thinking: pickValue(merged, ["thinking"]),
    effort: pickString(merged, ["effort"]),
    // V1-only (will be filtered by rejectV1OnlyOptions)
    forkSession: pickBoolean(merged, ["forkSession", "fork_session"]),
    cliPath: pickString(merged, ["cliPath", "cli_path"]),
    extraArgs: pickArray(merged, ["extraArgs", "extra_args"]),
    stderr: pickValue(merged, ["stderr"]),
    maxBufferSize: pickNumber(merged, ["maxBufferSize", "max_buffer_size"]),
  });

  return {
    prompt: buildPrompt(workOrder),
    options,
  };
}

// ---------------------------------------------------------------------------
// Retry helpers
// ---------------------------------------------------------------------------

function isRetriableError(err) {
  const text = safeString(err).toLowerCase();
  const status = Number(err?.status || err?.statusCode || err?.code);
  if (Number.isFinite(status) && [408, 409, 425, 429, 500, 502, 503, 504].includes(status)) {
    return true;
  }
  return (
    text.includes("timeout") ||
    text.includes("timed out") ||
    text.includes("temporar") ||
    text.includes("rate limit") ||
    text.includes("econnreset") ||
    text.includes("eai_again") ||
    text.includes("503") ||
    text.includes("429")
  );
}

function sleep(ms) {
  return new Promise((resolve) => { setTimeout(resolve, ms); });
}

function getRetryConfig(workOrder) {
  const claudeCfg = getVendorNamespace(workOrder, "claude");
  const DEFAULT_RETRY_COUNT = 1;
  const DEFAULT_RETRY_DELAY_MS = 1000;
  const retryCount =
    pickNumber(claudeCfg, ["retryCount", "retry_count", "retries"]) ??
    parseInt(process.env.ABP_CLAUDE_RETRY_COUNT || String(DEFAULT_RETRY_COUNT), 10);
  const retryDelayMs =
    pickNumber(claudeCfg, ["retryDelayMs", "retry_delay_ms"]) ??
    parseInt(process.env.ABP_CLAUDE_RETRY_DELAY_MS || String(DEFAULT_RETRY_DELAY_MS), 10);
  return {
    maxAttempts: Math.max(1, 1 + Math.max(0, Math.floor(retryCount || 0))),
    retryDelayMs: Math.max(0, Number.isFinite(retryDelayMs) ? retryDelayMs : DEFAULT_RETRY_DELAY_MS),
  };
}

// ---------------------------------------------------------------------------
// Main run entry point
// ---------------------------------------------------------------------------

async function run(ctx) {
  const workOrder = asObject(ctx?.workOrder);

  let sdk;
  try {
    sdk = await loadSdk();
  } catch (err) {
    ctx.emitWarning(safeString(err));
    ctx.emitAssistantMessage(
      "Claude SDK V2 preview surface is unavailable. Install a compatible @anthropic-ai/claude-agent-sdk version."
    );
    return {
      usageRaw: v2UsageRaw({ mode: "fallback", reason: "sdk_unavailable" }),
      usage: {},
      outcome: "partial",
    };
  }

  const request = buildV2Request(ctx);

  // Reject V1-only options with warnings
  request.options = rejectV1OnlyOptions(ctx, request.options);

  if (!request.prompt || String(request.prompt).trim().length === 0) {
    ctx.emitWarning("work order task is empty; running V2 preview with an empty prompt");
  }

  // Select transport
  const transport = selectTransport(sdk, request.options);
  if (!transport) {
    ctx.emitError(
      "Claude SDK V2 surface has no available transport (unstable_v2_prompt or unstable_v2_createSession)."
    );
    return {
      usageRaw: v2UsageRaw({ sdk_module: sdk.moduleName, error: "no_v2_transport" }),
      usage: {},
      outcome: "failed",
    };
  }

  const retryConfig = getRetryConfig(workOrder);
  let lastError = null;
  for (let attempt = 1; attempt <= retryConfig.maxAttempts; attempt += 1) {
    try {
      if (transport === "session") {
        return await runV2Session(ctx, sdk, request);
      }
      return await runV2Prompt(ctx, sdk, request);
    } catch (err) {
      lastError = err;
      const shouldRetry = attempt < retryConfig.maxAttempts && isRetriableError(err);
      if (!shouldRetry) {
        break;
      }
      ctx.emitWarning(
        `V2 preview attempt ${attempt}/${retryConfig.maxAttempts} failed; retrying in ${retryConfig.retryDelayMs}ms: ${safeString(err)}`
      );
      await sleep(retryConfig.retryDelayMs);
    }
  }

  ctx.emitError(`V2 preview execution failed: ${safeString(lastError)}`);
  return {
    usageRaw: v2UsageRaw({ sdk_module: sdk.moduleName, error: safeString(lastError) }),
    usage: {},
    outcome: "failed",
  };
}

// ---------------------------------------------------------------------------
// Reset cached SDK (for testing)
// ---------------------------------------------------------------------------

function resetSdkCache() {
  cachedSdk = null;
}

// ---------------------------------------------------------------------------
// Module exports
// ---------------------------------------------------------------------------

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  capabilities: {
    streaming: "native",
    hooks_pre_tool_use: "native",
    hooks_post_tool_use: "native",
    mcp_client: "native",
    tool_ask_user: "native",
    permission_callback: "native",
    v2_preview: "native",
  },
  run,
  // Exported for testing
  _test: {
    buildV2Request,
    emitV2Event,
    rejectV1OnlyOptions,
    selectTransport,
    v2UsageRaw,
    V1_ONLY,
    resetSdkCache,
    loadSdk,
    isRetriableError,
    getRetryConfig,
    sleep,
  },
};
