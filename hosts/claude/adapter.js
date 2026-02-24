const path = require("node:path");
const { pathToFileURL } = require("node:url");

const ADAPTER_NAME = "claude_sdk_adapter";
const ADAPTER_VERSION = "0.2.0";
const DEFAULT_SDK_MODULES = ["@anthropic-ai/claude-agent-sdk", "claude-agent-sdk"];
const DEFAULT_RETRY_COUNT = 1;
const DEFAULT_RETRY_DELAY_MS = 1000;
const DEFAULT_CLIENT_TIMEOUT_MS = 0;

const ExecutionMode = {
  Mapped: "mapped",
  Passthrough: "passthrough",
};

let cachedSdk = null;
const clientSessions = new Map();
let cleanupHooksBound = false;

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

function pickBoolean(obj, keys) {
  const value = pickValue(obj, keys);
  if (typeof value === "boolean") {
    return value;
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

function getClientConfig(workOrder) {
  const abpCfg = getVendorNamespace(workOrder, "abp");
  const claudeCfg = getVendorNamespace(workOrder, "claude");
  const enabled =
    pickBoolean(abpCfg, ["client_mode", "clientMode"]) ??
    pickBoolean(claudeCfg, ["client_mode", "clientMode"]) ??
    false;
  const persist =
    pickBoolean(abpCfg, ["client_persist", "clientPersist"]) ??
    pickBoolean(claudeCfg, ["client_persist", "clientPersist"]) ??
    false;
  const sessionKey =
    pickString(abpCfg, ["client_session_key", "clientSessionKey"]) ||
    pickString(claudeCfg, ["client_session_key", "clientSessionKey"]) ||
    undefined;
  const parsedTimeoutMs = parseInt(
    process.env.ABP_CLAUDE_CLIENT_TIMEOUT_MS || String(DEFAULT_CLIENT_TIMEOUT_MS),
    10
  );
  const timeoutMs =
    pickNumber(abpCfg, [
      "client_timeout_ms",
      "clientTimeoutMs",
      "timeoutMs",
      "timeout_ms",
    ]) ??
    pickNumber(claudeCfg, ["client_timeout_ms", "clientTimeoutMs"]) ??
    (Number.isFinite(parsedTimeoutMs) ? parsedTimeoutMs : DEFAULT_CLIENT_TIMEOUT_MS);

  return {
    enabled,
    persist,
    sessionKey,
    timeoutMs: Math.max(0, timeoutMs || 0),
  };
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

function stableJsonValue(value) {
  if (Array.isArray(value)) {
    return value.map(stableJsonValue);
  }
  if (!value || typeof value !== "object") {
    return value;
  }

  const out = {};
  for (const key of Object.keys(value).sort()) {
    out[key] = stableJsonValue(value[key]);
  }
  return out;
}

function stableSerialize(value) {
  try {
    return JSON.stringify(stableJsonValue(value));
  } catch (_) {
    return safeString(value);
  }
}

function isIterableLike(value) {
  if (!value) {
    return false;
  }
  if (typeof value[Symbol.asyncIterator] === "function") {
    return true;
  }
  return typeof value[Symbol.iterator] === "function" && !ArrayBuffer.isView(value);
}

function resolveMethod(target, methodNames) {
  if (!target || typeof target !== "object") {
    return null;
  }
  for (const methodName of methodNames) {
    if (typeof target[methodName] === "function") {
      return target[methodName].bind(target);
    }
  }
  return null;
}

async function withTimeout(promise, timeoutMs, onTimeout) {
  if (!timeoutMs || timeoutMs <= 0) {
    return promise;
  }

  let timer = null;
  let timeoutTriggered = false;
  const timeoutPromise = new Promise((_, reject) => {
    timer = setTimeout(() => {
      timeoutTriggered = true;
      Promise.resolve()
        .then(async () => {
          if (typeof onTimeout === "function") {
            await onTimeout();
          }
        })
        .finally(() => {
          reject(new Error(`Claude SDK client query timed out after ${timeoutMs}ms`));
        });
    }, timeoutMs);
  });

  try {
    return await Promise.race([promise, timeoutPromise]);
  } finally {
    if (!timeoutTriggered && timer) {
      clearTimeout(timer);
    }
  }
}

function resolveClientSessionKey(workOrder, request, clientConfig) {
  if (clientConfig.sessionKey) {
    return clientConfig.sessionKey;
  }

  const requestOptions = asObject(request?.options);
  const explicitSessionId = pickString(requestOptions, ["sessionId", "session_id"]);
  if (explicitSessionId) {
    return `session:${explicitSessionId}`;
  }

  const workspaceRoot = pickString(asObject(workOrder?.workspace), ["root"]);
  if (workspaceRoot) {
    return `workspace:${workspaceRoot}`;
  }

  return "default";
}

function withSessionLock(session, fn) {
  const runPromise = session.lock.then(fn);
  session.lock = runPromise.catch(() => {});
  return runPromise;
}

async function disconnectClient(client) {
  const disconnectFn = resolveMethod(client, ["disconnect", "close", "end"]);
  if (!disconnectFn) {
    return;
  }
  await disconnectFn();
}

async function connectClient(client) {
  const connectFn = resolveMethod(client, ["connect"]);
  if (!connectFn) {
    return;
  }
  await connectFn();
}

async function interruptClient(client) {
  const interruptFn = resolveMethod(client, ["interrupt", "cancel", "abort"]);
  if (!interruptFn) {
    return;
  }
  await interruptFn();
}

async function closeClientSession(session) {
  if (!session) {
    return;
  }
  if (session.key && clientSessions.get(session.key) === session) {
    clientSessions.delete(session.key);
  }
  await disconnectClient(session.client);
}

async function closeAllClientSessions() {
  const sessions = Array.from(clientSessions.values());
  clientSessions.clear();
  await Promise.all(
    sessions.map((session) =>
      closeClientSession(session).catch(() => {})
    )
  );
}

function bindCleanupHooks() {
  if (cleanupHooksBound) {
    return;
  }
  cleanupHooksBound = true;
  process.once("beforeExit", () => {
    void closeAllClientSessions();
  });
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

function resolveAgentOptionsCtor(mod) {
  if (!mod) {
    return null;
  }

  const candidates = [
    mod.ClaudeAgentOptions,
    mod.default?.ClaudeAgentOptions,
    mod.AgentOptions,
    mod.default?.AgentOptions,
  ];
  for (const candidate of candidates) {
    if (typeof candidate === "function") {
      return candidate;
    }
  }
  return null;
}

function resolveClientFactory(mod) {
  if (!mod) {
    return null;
  }

  const directFactoryCandidates = [
    mod.createClient,
    mod.default?.createClient,
  ];
  for (const candidate of directFactoryCandidates) {
    if (typeof candidate === "function") {
      return async (options) => {
        let lastError = null;
        const attempts = [
          () => candidate({ options }),
          () => candidate(options),
        ];
        for (const attempt of attempts) {
          try {
            const client = await attempt();
            if (client && typeof client === "object") {
              return client;
            }
          } catch (err) {
            lastError = err;
          }
        }
        throw new Error(`failed to create SDK client: ${safeString(lastError)}`);
      };
    }
  }

  const constructorCandidates = [
    mod.ClaudeSDKClient,
    mod.default?.ClaudeSDKClient,
    mod.SDKClient,
    mod.default?.SDKClient,
  ];
  for (const Candidate of constructorCandidates) {
    if (typeof Candidate !== "function") {
      continue;
    }
    return async (options) => {
      let lastError = null;
      const attempts = [
        () => new Candidate({ options }),
        () => new Candidate(options),
        () => Candidate({ options }),
        () => Candidate(options),
      ];
      for (const attempt of attempts) {
        try {
          const client = await attempt();
          if (client && typeof client === "object") {
            return client;
          }
        } catch (err) {
          lastError = err;
        }
      }
      throw new Error(`failed to instantiate SDK client: ${safeString(lastError)}`);
    };
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
      const createClient = resolveClientFactory(mod);
      if (typeof query !== "function" && typeof createClient !== "function") {
        throw new Error(`module '${candidate}' does not export query() or ClaudeSDKClient`);
      }
      cachedSdk = {
        moduleName: candidate,
        module: mod,
        query,
        createClient,
        agentOptionsCtor: resolveAgentOptionsCtor(mod),
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
  if (typeof queryFn !== "function") {
    throw new Error("claude SDK query() is unavailable");
  }
  try {
    return await queryFn(request);
  } catch (firstErr) {
    if (request && typeof request === "object" && Object.prototype.hasOwnProperty.call(request, "prompt")) {
      try {
        return await queryFn(request.prompt, request.options);
      } catch (_) {
        return queryFn(request.prompt);
      }
    }
    throw firstErr;
  }
}

function buildClientOptions(sdk, request) {
  const rawOptions = asObject(request?.options);
  if (typeof sdk.agentOptionsCtor === "function") {
    try {
      return new sdk.agentOptionsCtor(rawOptions);
    } catch (_) {
      // Fall through to plain options object.
    }
  }
  return rawOptions;
}

async function acquireClientSession(sdk, workOrder, request, clientConfig) {
  bindCleanupHooks();

  const key = clientConfig.persist
    ? resolveClientSessionKey(workOrder, request, clientConfig)
    : null;
  const optionFingerprint = stableSerialize(buildClientOptions(sdk, request));

  if (key && clientSessions.has(key)) {
    const existing = clientSessions.get(key);
    if (existing.optionFingerprint === optionFingerprint) {
      return existing;
    }
    await closeClientSession(existing);
  }

  const clientOptions = buildClientOptions(sdk, request);
  if (typeof sdk.createClient !== "function") {
    throw new Error("client_mode was enabled but SDK client constructor is unavailable");
  }
  const client = await sdk.createClient(clientOptions);
  await connectClient(client);

  const session = {
    key,
    client,
    optionFingerprint,
    lock: Promise.resolve(),
  };

  if (key) {
    clientSessions.set(key, session);
  }

  return session;
}

async function invokeClientQuery(client, request) {
  const queryFn = resolveMethod(client, ["query"]);
  if (!queryFn) {
    throw new Error("client_mode was enabled but client.query() is unavailable");
  }

  try {
    return await queryFn(request);
  } catch (firstErr) {
    if (request && typeof request === "object" && Object.prototype.hasOwnProperty.call(request, "prompt")) {
      try {
        return await queryFn(request.prompt, request.options);
      } catch (_) {
        try {
          return await queryFn(request.prompt);
        } catch (_) {
          throw firstErr;
        }
      }
    }
    throw firstErr;
  }
}

async function resolveClientResponseSource(client, queryResult) {
  if (isIterableLike(queryResult)) {
    return queryResult;
  }

  const receiveResponse = resolveMethod(client, ["receiveResponse", "receive_response"]);
  if (receiveResponse) {
    const received = await receiveResponse();
    if (isIterableLike(received)) {
      return received;
    }
    if (received != null) {
      return [received];
    }
  }

  if (queryResult != null) {
    return [queryResult];
  }
  return [];
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

async function runOnceWithClientSession(
  ctx,
  clientSession,
  request,
  passthroughMode,
  timeoutMs
) {
  const state = {
    usageRaw: {},
    lastAssistantText: "",
    sawAssistantDelta: false,
    sawAssistantMessage: false,
  };

  const queryResult = await withTimeout(
    invokeClientQuery(clientSession.client, request),
    timeoutMs,
    () => interruptClient(clientSession.client)
  );
  const response = await resolveClientResponseSource(clientSession.client, queryResult);

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
  const clientConfig = getClientConfig(workOrder);

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

  let useClientMode = clientConfig.enabled;
  if (useClientMode && typeof sdk.createClient !== "function") {
    ctx.emitWarning(
      "abp.client_mode=true requested, but this Claude SDK module does not expose a stateful client; falling back to query()."
    );
    useClientMode = false;
  }

  if (!useClientMode && typeof sdk.query !== "function") {
    ctx.emitError(
      "Claude SDK module does not expose query(). Enable client_mode with a compatible SDK or install a query()-compatible SDK."
    );
    return {
      usageRaw: {
        sdk_module: sdk.moduleName,
        error: "sdk_missing_query",
      },
      usage: {},
      outcome: "failed",
    };
  }

  const retryConfig = getRetryConfig(workOrder);
  let lastError = null;
  let clientSession = null;
  for (let attempt = 1; attempt <= retryConfig.maxAttempts; attempt += 1) {
    try {
      if (useClientMode && !clientSession) {
        clientSession = await acquireClientSession(sdk, workOrder, request, clientConfig);
      }

      const result = useClientMode
        ? await withSessionLock(clientSession, () =>
          runOnceWithClientSession(
            ctx,
            clientSession,
            request,
            usePassthrough,
            clientConfig.timeoutMs
          ))
        : await runOnce(ctx, sdk.query, request, usePassthrough);

      result.usageRaw = {
        sdk_module: sdk.moduleName,
        transport: useClientMode ? "client" : "query",
        client_mode: useClientMode,
        ...(useClientMode
          ? {
            client_session_key: clientSession?.key || null,
            client_persist: clientConfig.persist,
          }
          : {}),
        ...asObject(result.usageRaw),
      };
      result.usage = normalizeUsage(result.usageRaw);
      if (useClientMode && clientSession && !clientConfig.persist) {
        await closeClientSession(clientSession);
        clientSession = null;
      }
      return result;
    } catch (err) {
      lastError = err;
      if (clientSession) {
        await closeClientSession(clientSession).catch(() => {});
        clientSession = null;
      }
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
      transport: useClientMode ? "client" : "query",
      client_mode: useClientMode,
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
