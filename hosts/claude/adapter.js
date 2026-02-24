const path = require("node:path");
const { pathToFileURL } = require("node:url");

const ADAPTER_NAME = "claude_sdk_adapter";
const ADAPTER_VERSION = "0.1.0";
const DEFAULT_SDK_MODULES = ["@anthropic-ai/claude-agent-sdk", "claude-agent-sdk"];
const DEFAULT_RETRY_COUNT = 1;
const DEFAULT_RETRY_DELAY_MS = 1000;

const ExecutionMode = {
  Mapped: "mapped",
  Passthrough: "passthrough",
};

let cachedSdk = null;

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

function asObject(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return value;
}

function pickValue(obj, keys) {
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(obj, key)) {
      return obj[key];
    }
  }
  return undefined;
}

function pickString(obj, keys) {
  const value = pickValue(obj, keys);
  if (typeof value === "string" && value.trim().length > 0) {
    return value.trim();
  }
  return undefined;
}

function pickNumber(obj, keys) {
  const value = pickValue(obj, keys);
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  return undefined;
}

function pickArray(obj, keys) {
  const value = pickValue(obj, keys);
  if (Array.isArray(value)) {
    return value;
  }
  return undefined;
}

function pickObject(obj, keys) {
  const value = pickValue(obj, keys);
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value;
  }
  return undefined;
}

function compactObject(value) {
  const out = {};
  for (const [key, entry] of Object.entries(asObject(value))) {
    if (entry !== undefined) {
      out[key] = entry;
    }
  }
  return out;
}

function getVendorNamespace(workOrder, namespace) {
  const vendor = asObject(workOrder?.config?.vendor);
  const out = {};
  Object.assign(out, asObject(vendor[namespace]));

  const prefix = `${namespace}.`;
  for (const [key, value] of Object.entries(vendor)) {
    if (key.startsWith(prefix)) {
      out[key.slice(prefix.length)] = value;
    }
  }

  return out;
}

function getAbpVendorValue(workOrder, key) {
  const vendor = asObject(workOrder?.config?.vendor);
  const nested = asObject(vendor.abp);
  if (Object.prototype.hasOwnProperty.call(nested, key)) {
    return nested[key];
  }

  const dotted = `abp.${key}`;
  if (Object.prototype.hasOwnProperty.call(vendor, dotted)) {
    return vendor[dotted];
  }

  return null;
}

function getExecutionMode(workOrder) {
  const mode = getAbpVendorValue(workOrder, "mode");
  return mode === ExecutionMode.Passthrough
    ? ExecutionMode.Passthrough
    : ExecutionMode.Mapped;
}

function getPassthroughRequest(workOrder) {
  const request = getAbpVendorValue(workOrder, "request");
  if (request == null) {
    return null;
  }
  return request;
}

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

function buildMappedRequest(ctx) {
  const workOrder = asObject(ctx?.workOrder);
  const claudeCfg = getVendorNamespace(workOrder, "claude");
  const base = asObject(ctx?.sdkOptions);
  const optionOverrides = asObject(claudeCfg.options);
  const merged = {
    ...base,
    ...optionOverrides,
  };

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
  });

  return {
    prompt: buildPrompt(workOrder),
    options,
  };
}

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
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function getRetryConfig(workOrder) {
  const claudeCfg = getVendorNamespace(workOrder, "claude");
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

function normalizeUsage(raw) {
  const usage = asObject(raw?.usage || raw);
  const inputTokens = pickNumber(usage, ["input_tokens", "inputTokens", "prompt_tokens", "promptTokens"]);
  const outputTokens = pickNumber(usage, [
    "output_tokens",
    "outputTokens",
    "completion_tokens",
    "completionTokens",
  ]);
  const cacheReadTokens = pickNumber(usage, ["cache_read_tokens", "cacheReadTokens"]);
  const cacheWriteTokens = pickNumber(usage, ["cache_write_tokens", "cacheWriteTokens"]);

  return compactObject({
    input_tokens: inputTokens,
    output_tokens: outputTokens,
    cache_read_tokens: cacheReadTokens,
    cache_write_tokens: cacheWriteTokens,
  });
}

function mergeUsage(target, source) {
  if (!source || typeof source !== "object") {
    return target;
  }
  return {
    ...asObject(target),
    ...asObject(source),
  };
}

function lowerType(value) {
  return String(value || "").toLowerCase();
}

function extractText(rawMessage) {
  if (typeof rawMessage === "string") {
    return rawMessage;
  }
  if (!rawMessage || typeof rawMessage !== "object") {
    return "";
  }
  if (typeof rawMessage.text === "string") {
    return rawMessage.text;
  }
  if (typeof rawMessage.delta === "string") {
    return rawMessage.delta;
  }
  if (typeof rawMessage.content === "string") {
    return rawMessage.content;
  }

  const nested = rawMessage.message;
  if (nested && typeof nested.content === "string") {
    return nested.content;
  }
  return "";
}

function extractBlocks(rawMessage) {
  if (!rawMessage || typeof rawMessage !== "object") {
    return [];
  }
  if (Array.isArray(rawMessage.content)) {
    return rawMessage.content;
  }
  if (rawMessage.message && Array.isArray(rawMessage.message.content)) {
    return rawMessage.message.content;
  }
  return [];
}

function emitAssistant(ctx, state, text, isDelta) {
  const value = String(text || "");
  if (!value) {
    return;
  }
  if (isDelta) {
    state.lastAssistantText += value;
    state.sawAssistantDelta = true;
    ctx.emitAssistantDelta(value);
    return;
  }
  state.lastAssistantText = value;
  state.sawAssistantMessage = true;
  ctx.emitAssistantMessage(value);
}

function extractToolName(rawMessage) {
  return (
    rawMessage?.tool_name ||
    rawMessage?.toolName ||
    rawMessage?.name ||
    rawMessage?.tool?.name ||
    rawMessage?.call?.name ||
    null
  );
}

function extractToolUseId(rawMessage) {
  return (
    rawMessage?.tool_use_id ||
    rawMessage?.toolUseId ||
    rawMessage?.id ||
    rawMessage?.call_id ||
    null
  );
}

function extractToolInput(rawMessage) {
  return (
    rawMessage?.input ||
    rawMessage?.arguments ||
    rawMessage?.args ||
    rawMessage?.tool_input ||
    rawMessage?.call?.input ||
    {}
  );
}

function extractToolOutput(rawMessage) {
  if (Object.prototype.hasOwnProperty.call(asObject(rawMessage), "output")) {
    return rawMessage.output;
  }
  if (Object.prototype.hasOwnProperty.call(asObject(rawMessage), "result")) {
    return rawMessage.result;
  }
  if (Object.prototype.hasOwnProperty.call(asObject(rawMessage), "tool_output")) {
    return rawMessage.tool_output;
  }
  return null;
}

function isToolResult(rawType, rawMessage) {
  return (
    rawType.includes("tool_result") ||
    rawType.includes("toolresult") ||
    (rawType.includes("result") && extractToolName(rawMessage) != null) ||
    Object.prototype.hasOwnProperty.call(asObject(rawMessage), "output") ||
    Object.prototype.hasOwnProperty.call(asObject(rawMessage), "result")
  );
}

function emitToolEvent(ctx, rawType, rawMessage) {
  const toolName = extractToolName(rawMessage);
  if (!toolName) {
    return false;
  }

  const toolUseId = extractToolUseId(rawMessage);
  if (isToolResult(rawType, rawMessage)) {
    ctx.emitToolResult({
      toolName: String(toolName),
      toolUseId: toolUseId ? String(toolUseId) : null,
      output: extractToolOutput(rawMessage),
      isError: !!rawMessage?.is_error || !!rawMessage?.isError,
    });
    return true;
  }

  ctx.emitToolCall({
    toolName: String(toolName),
    toolUseId: toolUseId ? String(toolUseId) : null,
    parentToolUseId:
      rawMessage?.parent_tool_use_id || rawMessage?.parentToolUseId || null,
    input: extractToolInput(rawMessage),
  });
  return true;
}

function collectUsage(state, rawMessage) {
  if (!rawMessage || typeof rawMessage !== "object") {
    return;
  }
  state.usageRaw = mergeUsage(state.usageRaw, rawMessage.usage);
  if (rawMessage.message && typeof rawMessage.message === "object") {
    state.usageRaw = mergeUsage(state.usageRaw, rawMessage.message.usage);
  }
}

function emitMappedMessage(ctx, rawMessage, state) {
  if (typeof rawMessage === "string") {
    emitAssistant(ctx, state, rawMessage, true);
    return;
  }

  if (!rawMessage || typeof rawMessage !== "object") {
    return;
  }

  collectUsage(state, rawMessage);
  const rawType = lowerType(rawMessage.type || rawMessage.kind || rawMessage.event);

  let handledText = false;
  let handledTool = false;
  for (const block of extractBlocks(rawMessage)) {
    if (!block || typeof block !== "object") {
      continue;
    }
    collectUsage(state, block);
    const blockType = lowerType(block.type || block.kind || rawType);

    if (typeof block.text === "string") {
      emitAssistant(
        ctx,
        state,
        block.text,
        blockType.includes("delta") || rawType.includes("delta")
      );
      handledText = true;
      continue;
    }

    if (emitToolEvent(ctx, blockType, block)) {
      handledTool = true;
      continue;
    }
  }

  if (!handledText) {
    const text = extractText(rawMessage);
    if (text) {
      const isDelta = rawType.includes("delta") || rawType.includes("stream");
      const isAssistant = rawType.includes("assistant") || rawType.includes("message") || !rawType;
      if (isDelta || isAssistant) {
        emitAssistant(ctx, state, text, isDelta);
      }
    }
  }

  if (!handledTool) {
    emitToolEvent(ctx, rawType, rawMessage);
  }

  if (rawType.includes("error")) {
    const err = rawMessage.error || rawMessage.message || rawMessage.details || "claude sdk error";
    ctx.emitError(safeString(err));
  }
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

function normalizeSpecifier(raw) {
  const value = String(raw || "").trim();
  if (!value) {
    return null;
  }
  const looksLikePath =
    value.startsWith(".") ||
    value.startsWith("/") ||
    value.startsWith("\\") ||
    /^[a-z]:\\/i.test(value);
  if (looksLikePath) {
    return path.resolve(process.cwd(), value);
  }
  return value;
}

function canRequireAsCjs(err) {
  if (!err || typeof err !== "object") {
    return false;
  }
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
    if (!canRequireAsCjs(err)) {
      throw err;
    }

    const looksLikePath =
      specifier.startsWith("/") ||
      specifier.startsWith("\\") ||
      /^[a-z]:\\/i.test(specifier);
    if (looksLikePath) {
      return import(pathToFileURL(specifier).href);
    }
    return import(specifier);
  }
}

function moduleNotFound(err) {
  if (!err || typeof err !== "object") {
    return false;
  }
  if (err.code === "MODULE_NOT_FOUND" || err.code === "ERR_MODULE_NOT_FOUND") {
    return true;
  }
  return String(err.message || "").includes("Cannot find module");
}

function resolveQueryFunction(mod) {
  if (!mod) {
    return null;
  }
  if (typeof mod.query === "function") {
    return mod.query.bind(mod);
  }
  if (mod.default && typeof mod.default.query === "function") {
    return mod.default.query.bind(mod.default);
  }
  if (typeof mod.default === "function") {
    return mod.default;
  }
  if (typeof mod === "function") {
    return mod;
  }
  return null;
}

async function loadSdk() {
  if (cachedSdk) {
    return cachedSdk;
  }

  const candidates = [];
  if (process.env.ABP_CLAUDE_SDK_MODULE && process.env.ABP_CLAUDE_SDK_MODULE.trim()) {
    candidates.push(normalizeSpecifier(process.env.ABP_CLAUDE_SDK_MODULE));
  }
  for (const moduleName of DEFAULT_SDK_MODULES) {
    candidates.push(moduleName);
  }

  let lastError = null;
  for (const candidate of candidates) {
    if (!candidate) {
      continue;
    }

    try {
      const mod = await importModule(candidate);
      const query = resolveQueryFunction(mod);
      if (typeof query !== "function") {
        throw new Error(`module '${candidate}' does not export query()`);
      }
      cachedSdk = {
        moduleName: candidate,
        query,
      };
      return cachedSdk;
    } catch (err) {
      lastError = err;
      if (!moduleNotFound(err)) {
        // Keep trying candidates; non-fatal here.
      }
    }
  }

  throw new Error(`unable to load Claude SDK: ${safeString(lastError)}`);
}

async function invokeQuery(queryFn, request) {
  try {
    return queryFn(request);
  } catch (firstErr) {
    if (request && typeof request === "object" && Object.prototype.hasOwnProperty.call(request, "prompt")) {
      return queryFn(request.prompt, request.options);
    }
    throw firstErr;
  }
}

async function runOnce(ctx, queryFn, request, passthroughMode) {
  const state = {
    usageRaw: {},
    lastAssistantText: "",
    sawAssistantDelta: false,
    sawAssistantMessage: false,
  };

  const response = await invokeQuery(queryFn, request);
  for await (const rawMessage of toAsyncIterable(response)) {
    if (passthroughMode && typeof ctx.emitPassthroughEvent === "function") {
      ctx.emitPassthroughEvent(rawMessage);
      collectUsage(state, rawMessage);
      continue;
    }
    emitMappedMessage(ctx, rawMessage, state);
  }

  if (
    !passthroughMode &&
    state.sawAssistantDelta &&
    !state.sawAssistantMessage &&
    state.lastAssistantText.length > 0
  ) {
    ctx.emitAssistantMessage(state.lastAssistantText);
  }

  return {
    usageRaw: state.usageRaw,
    usage: normalizeUsage(state.usageRaw),
    outcome: "complete",
    ...(passthroughMode ? { stream_equivalent: true } : {}),
  };
}

async function run(ctx) {
  const workOrder = asObject(ctx?.workOrder);
  const mode = getExecutionMode(workOrder);
  const passthroughRequest = getPassthroughRequest(workOrder);
  const usePassthrough = mode === ExecutionMode.Passthrough && passthroughRequest != null;

  let sdk;
  try {
    sdk = await loadSdk();
  } catch (err) {
    ctx.emitWarning(safeString(err));
    ctx.emitAssistantMessage(
      "Claude SDK is unavailable. Install @anthropic-ai/claude-agent-sdk to enable real runs."
    );
    return {
      usageRaw: {
        mode: "fallback",
        reason: "sdk_unavailable",
      },
      usage: {},
      outcome: "partial",
    };
  }

  const request = usePassthrough ? passthroughRequest : buildMappedRequest(ctx);
  if (!usePassthrough && (!request.prompt || String(request.prompt).trim().length === 0)) {
    ctx.emitWarning("work order task is empty; running Claude SDK with an empty prompt");
  }

  const retryConfig = getRetryConfig(workOrder);
  let lastError = null;
  for (let attempt = 1; attempt <= retryConfig.maxAttempts; attempt += 1) {
    try {
      const result = await runOnce(ctx, sdk.query, request, usePassthrough);
      result.usageRaw = {
        sdk_module: sdk.moduleName,
        ...asObject(result.usageRaw),
      };
      result.usage = normalizeUsage(result.usageRaw);
      return result;
    } catch (err) {
      lastError = err;
      const shouldRetry = attempt < retryConfig.maxAttempts && isRetriableError(err);
      if (!shouldRetry) {
        break;
      }

      ctx.emitWarning(
        `Claude SDK attempt ${attempt}/${retryConfig.maxAttempts} failed; retrying in ${retryConfig.retryDelayMs}ms: ${safeString(err)}`
      );
      await sleep(retryConfig.retryDelayMs);
    }
  }

  ctx.emitError(`Claude SDK execution failed: ${safeString(lastError)}`);
  return {
    usageRaw: {
      sdk_module: sdk.moduleName,
      error: safeString(lastError),
    },
    usage: {},
    outcome: "failed",
  };
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  capabilities: {
    streaming: "native",
    hooks_pre_tool_use: "native",
    hooks_post_tool_use: "native",
    mcp_client: "native",
  },
  run,
};
