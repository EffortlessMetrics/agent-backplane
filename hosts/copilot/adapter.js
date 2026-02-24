const { spawn } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const readline = require("node:readline");
const { pathToFileURL } = require("node:url");

const ADAPTER_NAME = "copilot_sdk_adapter";
const ADAPTER_VERSION = "0.2.0";
const SDK_MODULE = process.env.ABP_COPILOT_SDK_MODULE || "@github/copilot-sdk";
const TRANSPORT_MODE = String(process.env.ABP_COPILOT_TRANSPORT || "auto").toLowerCase();
const SDK_RETRY_ATTEMPTS = parseInt(process.env.ABP_COPILOT_RETRY_ATTEMPTS || "3", 10);
const SDK_RETRY_BASE_DELAY_MS = parseInt(
  process.env.ABP_COPILOT_RETRY_BASE_DELAY_MS || "1000",
  10
);

const DEFAULT_COPILOT_CMD = process.env.ABP_COPILOT_CLI_PATH || process.env.ABP_COPILOT_CMD || "copilot";
const DEFAULT_COPILOT_ARGS = parseArgList(process.env.ABP_COPILOT_ARGS);
const RUNNER_CMD = process.env.ABP_COPILOT_RUNNER || "";
const RUNNER_ARGS = parseArgList(process.env.ABP_COPILOT_RUNNER_ARGS);
const ACP_URL = process.env.ABP_COPILOT_ACP_URL || "";
const ACP_PORT = parseInt(process.env.ABP_COPILOT_ACP_PORT || "", 10);
const ACP_ARGS = parseArgList(process.env.ABP_COPILOT_ACP_ARGS);

const MODE = String(process.env.ABP_COPILOT_PROTOCOL || "acp").toLowerCase();
const AUTO_APPROVE_ALWAYS = parseBool(process.env.ABP_COPILOT_PERMISSION_ALLOW_ALWAYS, false);
const AUTO_APPROVE_TOOLS = toLowerSet(parseArgList(process.env.ABP_COPILOT_PERMISSION_ALLOW_TOOLS));
const DENY_TOOLS = toLowerSet(parseArgList(process.env.ABP_COPILOT_PERMISSION_DENY_TOOLS));
const AUTO_ALLOW_ALWAYS_TOOLS = toLowerSet(
  parseArgList(process.env.ABP_COPILOT_PERMISSION_ALLOW_ALWAYS_TOOLS)
);
const AUTO_DENY_ALWAYS_TOOLS = toLowerSet(
  parseArgList(process.env.ABP_COPILOT_PERMISSION_DENY_ALWAYS_TOOLS)
);

let cachedCopilotClientCtor = null;
let cachedSdkLoadError = null;
let cachedSdkVersion = null;

function safeString(value) {
  if (value == null) {
    return "";
  }
  if (value instanceof Error) {
    return value.message || String(value);
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "object" && typeof value.message === "string") {
    return value.message;
  }
  try {
    return JSON.stringify(value);
  } catch (_) {
    return String(value);
  }
}

function parseBool(value, fallback = false) {
  if (value == null) {
    return fallback;
  }
  const normalized = String(value).trim().toLowerCase();
  if (["1", "true", "yes", "on", "enabled", "allow"].includes(normalized)) {
    return true;
  }
  if (["0", "false", "no", "off", "disabled", "deny"].includes(normalized)) {
    return false;
  }
  return fallback;
}

function parseArgList(raw) {
  if (!raw) {
    return [];
  }
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return parsed.map(String);
    }
  } catch (_) {
    return String(raw)
      .trim()
      .split(/\s+/)
      .filter(Boolean);
  }
  return [];
}

function asObject(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return value;
}

function parsePositiveInt(value, fallback) {
  const parsed = Number.parseInt(String(value || ""), 10);
  if (Number.isFinite(parsed) && parsed > 0) {
    return parsed;
  }
  return fallback;
}

function isRunnablePath(candidate) {
  try {
    return fs.statSync(candidate).isFile();
  } catch (_) {
    return false;
  }
}

function resolveCommandPath(command) {
  const cmd = String(command || "").trim();
  if (!cmd) {
    return null;
  }

  if (path.isAbsolute(cmd) || cmd.includes(path.sep)) {
    const absolute = path.resolve(cmd);
    return isRunnablePath(absolute) ? absolute : null;
  }

  const pathVar = process.env.PATH;
  if (!pathVar) {
    return null;
  }

  for (const dir of pathVar.split(path.delimiter)) {
    if (!dir) {
      continue;
    }

    if (process.platform === "win32") {
      for (const ext of [".exe", ".cmd", ".bat", ".com", ".ps1"]) {
        const candidate = path.join(dir, `${cmd}${ext}`);
        if (isRunnablePath(candidate)) {
          return candidate;
        }
      }
      continue;
    }

    const candidate = path.join(dir, cmd);
    if (isRunnablePath(candidate)) {
      return candidate;
    }
  }

  return null;
}

function resolveSdkVersion() {
  const candidates = [
    path.resolve(process.cwd(), "hosts/copilot/node_modules/@github/copilot-sdk/package.json"),
    path.resolve(__dirname, "node_modules/@github/copilot-sdk/package.json"),
  ];

  for (const candidate of candidates) {
    if (!isRunnablePath(candidate)) {
      continue;
    }
    try {
      const pkg = JSON.parse(fs.readFileSync(candidate, "utf8"));
      if (typeof pkg.version === "string" && pkg.version.length > 0) {
        return pkg.version;
      }
    } catch (_) {
      return null;
    }
  }

  return null;
}

async function loadCopilotClientCtor() {
  if (cachedCopilotClientCtor) {
    return cachedCopilotClientCtor;
  }
  if (cachedSdkLoadError) {
    throw cachedSdkLoadError;
  }

  try {
    const mod = await import(normalizeSdkImportTarget(SDK_MODULE));
    const ctor =
      mod?.CopilotClient ||
      mod?.default?.CopilotClient ||
      mod?.default;
    if (!ctor || typeof ctor !== "function") {
      throw new Error(`module '${SDK_MODULE}' does not export CopilotClient`);
    }
    cachedCopilotClientCtor = ctor;
    cachedSdkVersion = resolveSdkVersion();
    return cachedCopilotClientCtor;
  } catch (err) {
    cachedSdkLoadError = new Error(
      `failed to load Copilot SDK module '${SDK_MODULE}': ${safeString(err)}`
    );
    throw cachedSdkLoadError;
  }
}

function normalizeSdkImportTarget(target) {
  const raw = String(target || "").trim();
  if (!raw) {
    return raw;
  }
  if (
    raw.startsWith("file://") ||
    raw.startsWith("node:") ||
    raw.startsWith("data:")
  ) {
    return raw;
  }

  if (path.isAbsolute(raw) || raw.startsWith(".") || raw.startsWith("..")) {
    const resolved = path.resolve(raw);
    return pathToFileURL(resolved).href;
  }

  return raw;
}

function toLowerSet(values) {
  return Array.isArray(values)
    ? values
        .filter((value) => typeof value === "string" && value.trim())
        .map((value) => value.toLowerCase())
    : [];
}

function normalizeToolList(values) {
  if (!Array.isArray(values)) {
    return [];
  }
  const out = [];
  const seen = new Set();
  for (const value of values) {
    if (typeof value !== "string") {
      continue;
    }
    const item = value.trim();
    const key = item.toLowerCase();
    if (!item || seen.has(key)) {
      continue;
    }
    seen.add(key);
    out.push(item);
  }
  return out;
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
      const camel = String(key).replace(/_([a-z])/g, (_, ch) => ch.toUpperCase());
      if (typeof usage[camel] === "number") {
        return usage[camel];
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

function mergeUsage(base, raw) {
  if (!raw || typeof raw !== "object") {
    return base;
  }
  return {
    ...base,
    ...raw,
  };
}

function parseContextText(context) {
  if (!context || typeof context !== "object") {
    return "";
  }

  const lines = [];
  const files = Array.isArray(context.files) ? context.files : [];
  const snippets = Array.isArray(context.snippets) ? context.snippets : [];

  if (files.length > 0) {
    lines.push("### Context files");
    for (const file of files) {
      if (typeof file === "string") {
        lines.push(`- ${file}`);
        continue;
      }
      if (!file || typeof file !== "object") {
        continue;
      }
      const path = file.path || file.name || "unknown";
      const snippet = file.snippet || file.preview || "";
      lines.push(`- ${path}`);
      if (snippet) {
        lines.push(`  ${String(snippet).slice(0, 800)}`);
      }
    }
  }

  if (snippets.length > 0) {
    lines.push("### Snippets");
    for (const snippet of snippets) {
      const source = snippet?.path || snippet?.name || "snippet";
      const text = snippet?.text || snippet?.content || "";
      lines.push(`- ${source}`);
      if (text) {
        lines.push(`  ${String(text).slice(0, 800)}`);
      }
    }
  }

  return lines.join("\n");
}

function pickContextForRequest(ctx) {
  const workOrder = ctx.workOrder || {};
  const vendor = asObject(workOrder.config && workOrder.config.vendor);
  const copilotVendor = asObject(vendor.copilot);
  const abpVendor = asObject(vendor.abp);
  const requestText = `${workOrder.task || ""}`.trim();
  const contextText = parseContextText(workOrder.context || {});

  return {
    request_id: workOrder.id || null,
    prompt: requestText,
    prompt_ctx: contextText,
    workspace_root: (workOrder.workspace && workOrder.workspace.root) || process.cwd(),
    model: (workOrder.config && workOrder.config.model) || copilotVendor.model || null,
    reasoningEffort: copilotVendor.reasoningEffort || null,
    systemMessage: copilotVendor.systemMessage || null,
    copilotToken:
      copilotVendor.token ||
      copilotVendor.apiToken ||
      process.env.GH_TOKEN ||
      process.env.GITHUB_TOKEN ||
      null,
    timeoutMs: parsePositiveInt(copilotVendor.timeoutMs || copilotVendor.timeout_ms, 120000),
    retryAttempts: parsePositiveInt(copilotVendor.retryAttempts || copilotVendor.retry_attempts, parsePositiveInt(SDK_RETRY_ATTEMPTS, 3)),
    retryBaseDelayMs: parsePositiveInt(copilotVendor.retryBaseDelayMs || copilotVendor.retry_base_delay_ms, parsePositiveInt(SDK_RETRY_BASE_DELAY_MS, 1000)),
    availableTools: normalizeToolList([
      ...(normalizeToolList(copilotVendor.availableTools) || []),
      ...(normalizeToolList(copilotVendor.available_tools) || []),
      ...(normalizeToolList(abpVendor.availableTools) || []),
      ...(normalizeToolList(abpVendor.available_tools) || []),
    ]),
    excludedTools: normalizeToolList([
      ...(normalizeToolList(copilotVendor.excludedTools) || []),
      ...(normalizeToolList(copilotVendor.excluded_tools) || []),
      ...(normalizeToolList(abpVendor.excludedTools) || []),
      ...(normalizeToolList(abpVendor.excluded_tools) || []),
    ]),
    mcpServers: parseMcpServers(
      copilotVendor.mcpServers ||
        copilotVendor.mcp_servers ||
        abpVendor.mcpServers ||
        abpVendor.mcp_servers
    ),
    sessionId:
      copilotVendor.sessionId ||
      copilotVendor.session_id ||
      abpVendor.sessionId ||
      abpVendor.session_id ||
      null,
    mode: abpVendor.mode || "mapped",
    raw_request: abpVendor.request || null,
    policy: ctx.policy || {},
    env: (workOrder.config && workOrder.config.env) || {},
    policyEngine: ctx.policyEngine || null,
    vendor,
  };
}

function parseMcpServers(raw) {
  if (!raw) {
    return {};
  }
  if (Array.isArray(raw)) {
    const out = {};
    for (const item of raw) {
      if (!item || typeof item !== "object") {
        continue;
      }
      const name = item.name || item.id;
      if (!name || typeof item !== "object") {
        continue;
      }
      out[String(name)] = item;
    }
    return out;
  }
  if (typeof raw === "object") {
    return raw;
  }
  return {};
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isAuthError(err) {
  const msg = safeString(err).toLowerCase();
  return (
    msg.includes("401") ||
    msg.includes("unauthorized") ||
    msg.includes("authentication") ||
    msg.includes("auth failed") ||
    msg.includes("token")
  );
}

function isRetriableError(err) {
  const msg = safeString(err).toLowerCase();
  return (
    msg.includes("429") ||
    msg.includes("rate limit") ||
    msg.includes("503") ||
    msg.includes("504") ||
    msg.includes("temporar") ||
    msg.includes("timeout") ||
    msg.includes("timed out") ||
    msg.includes("econnreset")
  );
}

function extractText(value, depth = 0) {
  if (value == null || depth > 5) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value.text === "string") {
    return value.text;
  }
  if (typeof value.delta === "string") {
    return value.delta;
  }
  if (typeof value.message === "string") {
    return value.message;
  }
  if (value.message && typeof value.message === "object") {
    const nested = extractText(value.message, depth + 1);
    if (nested) {
      return nested;
    }
  }
  if (Array.isArray(value.content)) {
    const joined = value.content
      .map((item) => {
        if (typeof item === "string") {
          return item;
        }
        if (item && typeof item.text === "string") {
          return item.text;
        }
        if (item && typeof item.content === "string") {
          return item.content;
        }
        return "";
      })
      .filter(Boolean)
      .join("");
    if (joined) {
      return joined;
    }
  }
  if (Array.isArray(value.choices)) {
    for (const choice of value.choices) {
      const choiceText =
        extractText(choice?.delta, depth + 1) ||
        extractText(choice?.message, depth + 1) ||
        extractText(choice, depth + 1);
      if (choiceText) {
        return choiceText;
      }
    }
  }
  if (value.response) {
    return extractText(value.response, depth + 1);
  }
  return "";
}

function extractUsage(value) {
  if (!value || typeof value !== "object") {
    return {};
  }
  if (value.usage && typeof value.usage === "object") {
    return value.usage;
  }
  if (value.usageMetadata && typeof value.usageMetadata === "object") {
    return value.usageMetadata;
  }
  if (
    typeof value.input_tokens === "number" ||
    typeof value.output_tokens === "number" ||
    typeof value.prompt_tokens === "number"
  ) {
    return value;
  }
  if (value.response && typeof value.response === "object") {
    return extractUsage(value.response);
  }
  return {};
}

function normalizeToolEvent(payload) {
  const type = String(payload?.type || payload?.kind || "").toLowerCase();
  const toolName = String(
    payload?.tool_name ||
      payload?.toolName ||
      payload?.name ||
      payload?.tool ||
      payload?.function_name ||
      "tool"
  );
  const toolUseId =
    payload?.tool_use_id ||
    payload?.toolUseId ||
    payload?.id ||
    payload?.tool_call_id ||
    null;
  const parentToolUseId =
    payload?.parent_tool_use_id ||
    payload?.parentToolUseId ||
    null;
  const input = payload?.input || payload?.arguments || payload?.params || {};
  const output = payload?.output || payload?.result || payload?.value || {};
  const isError = !!(payload?.is_error || payload?.isError || payload?.error);

  if (type.includes("tool") && type.includes("call")) {
    return {
      kind: "call",
      toolName,
      toolUseId,
      parentToolUseId,
      input,
    };
  }

  if (type.includes("tool") && type.includes("result")) {
    return {
      kind: "result",
      toolName,
      toolUseId,
      output,
      isError,
    };
  }

  if (payload?.tool_calls && Array.isArray(payload.tool_calls)) {
    const call = payload.tool_calls[0] || {};
    return {
      kind: "call",
      toolName: String(call.name || call.tool_name || toolName),
      toolUseId: call.id || toolUseId,
      parentToolUseId,
      input: call.input || call.arguments || input,
    };
  }

  return null;
}

function toAsyncIterable(value) {
  if (!value) {
    return null;
  }
  if (typeof value[Symbol.asyncIterator] === "function") {
    return value;
  }
  if (value.stream && typeof value.stream[Symbol.asyncIterator] === "function") {
    return value.stream;
  }
  if (value.events && typeof value.events[Symbol.asyncIterator] === "function") {
    return value.events;
  }
  return null;
}

async function maybeAwait(value) {
  if (value && typeof value.then === "function") {
    return value;
  }
  return value;
}

async function closeCopilotResources(client, session) {
  const closers = [
    [session, "close"],
    [session, "stop"],
    [client, "close"],
    [client, "stop"],
    [client, "shutdown"],
    [client, "destroy"],
  ];
  for (const [target, method] of closers) {
    if (!target || typeof target[method] !== "function") {
      continue;
    }
    try {
      await target[method]();
    } catch (_) {
      // best-effort cleanup
    }
  }
}

function createSdkClientOptions(request) {
  const options = {};
  if (request.copilotToken) {
    options.token = request.copilotToken;
    options.authToken = request.copilotToken;
  }
  if (request.workspace_root) {
    options.cwd = request.workspace_root;
    options.workingDirectory = request.workspace_root;
  }
  if (request.timeoutMs) {
    options.timeoutMs = request.timeoutMs;
  }
  return options;
}

async function createSdkSession(client, request) {
  const opts = {
    model: request.model || null,
    cwd: request.workspace_root,
    workingDirectory: request.workspace_root,
    systemMessage: request.systemMessage || null,
    reasoningEffort: request.reasoningEffort || null,
    mcpServers: request.mcpServers || {},
    sessionId: request.sessionId || null,
  };

  const methodCandidates = [
    client && client.createSession,
    client && client.startSession,
    client && client.newSession,
  ];

  for (const method of methodCandidates) {
    if (typeof method !== "function") {
      continue;
    }
    try {
      const session = await method.call(client, opts);
      if (session) {
        return session;
      }
    } catch (err) {
      if (safeString(err).toLowerCase().includes("not implemented")) {
        continue;
      }
      throw err;
    }
  }

  if (client && client.session && typeof client.session === "object") {
    const nested = client.session;
    if (typeof nested.create === "function") {
      return nested.create(opts);
    }
    if (typeof nested.start === "function") {
      return nested.start(opts);
    }
  }

  throw new Error("Copilot SDK client does not expose a compatible session API");
}

async function sendPromptWithSdkSession(session, request, ctx) {
  const prompt = collectPromptText(request);
  const payload = {
    prompt,
    input: prompt,
    message: {
      role: "user",
      content: [{ type: "text", text: prompt }],
    },
    model: request.model || null,
    systemMessage: request.systemMessage || null,
    reasoningEffort: request.reasoningEffort || null,
    mcpServers: request.mcpServers || {},
    tools: {
      available: request.availableTools || [],
      excluded: request.excludedTools || [],
    },
    stream: true,
  };

  const state = {
    sawDelta: false,
    usageRaw: {},
  };

  const applyChunk = (chunk) => {
    if (!chunk) {
      return;
    }

    const usage = extractUsage(chunk);
    if (usage && Object.keys(usage).length > 0) {
      state.usageRaw = mergeUsage(state.usageRaw, usage);
    }

    const toolEvent = normalizeToolEvent(chunk);
    if (toolEvent && toolEvent.kind === "call") {
      ctx.emitToolCall({
        toolName: toolEvent.toolName,
        toolUseId: toolEvent.toolUseId,
        parentToolUseId: toolEvent.parentToolUseId,
        input: toolEvent.input,
      });
      return;
    }
    if (toolEvent && toolEvent.kind === "result") {
      ctx.emitToolResult({
        toolName: toolEvent.toolName,
        toolUseId: toolEvent.toolUseId,
        output: toolEvent.output,
        isError: toolEvent.isError,
      });
      return;
    }

    const type = String(chunk.type || chunk.kind || "").toLowerCase();
    if (type.includes("warning")) {
      ctx.emitWarning(safeString(chunk.message || chunk.text || chunk.warning || ""));
      return;
    }
    if (type.includes("error")) {
      ctx.emitError(safeString(chunk.error || chunk.message || chunk));
      return;
    }

    const text = extractText(chunk);
    if (text) {
      state.sawDelta = true;
      ctx.emitAssistantDelta(text);
    }
  };

  let response = null;
  let stream = null;
  if (typeof session.sendAndStream === "function") {
    response = await session.sendAndStream(payload);
    stream = toAsyncIterable(response);
  } else if (typeof session.send === "function") {
    response = await session.send(payload);
    stream = toAsyncIterable(response);
  } else if (typeof session.prompt === "function") {
    response = await session.prompt(payload);
    stream = toAsyncIterable(response);
  } else if (typeof session.run === "function") {
    response = await session.run(payload);
    stream = toAsyncIterable(response);
  } else {
    throw new Error("Copilot SDK session does not expose a compatible prompt method");
  }

  if (stream) {
    for await (const chunk of stream) {
      applyChunk(chunk);
    }
  } else if (response) {
    applyChunk(response);
  }

  if (response && response.response) {
    const finalResponse = await maybeAwait(response.response);
    applyChunk(finalResponse);
    if (!state.sawDelta) {
      const finalText = extractText(finalResponse);
      if (finalText) {
        ctx.emitAssistantMessage(finalText);
      }
    }
  } else if (!state.sawDelta) {
    const finalText = extractText(response);
    if (finalText) {
      ctx.emitAssistantMessage(finalText);
    }
  }

  return {
    usageRaw: state.usageRaw,
    usage: parseUsage(state.usageRaw),
    outcome: "complete",
  };
}

async function runSdkOnce(request, ctx) {
  const CopilotClient = await loadCopilotClientCtor();
  const clientOptions = createSdkClientOptions(request);
  const client = new CopilotClient(clientOptions);
  let session = null;
  try {
    session = await createSdkSession(client, request);
    const result = await sendPromptWithSdkSession(session, request, ctx);
    return {
      ...result,
      usageRaw: {
        ...asObject(result.usageRaw),
        sdk_transport: "github_copilot_sdk",
        sdk_version: cachedSdkVersion,
      },
    };
  } finally {
    await closeCopilotResources(client, session);
  }
}

async function runSdkWithRetry(request, ctx) {
  const attempts = parsePositiveInt(request.retryAttempts, 3);
  const baseDelayMs = parsePositiveInt(request.retryBaseDelayMs, 1000);
  let lastError = null;

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await runSdkOnce(request, ctx);
    } catch (err) {
      lastError = err;
      if (isAuthError(err)) {
        throw new Error(`Copilot authentication failed: ${safeString(err)}`);
      }

      const canRetry = attempt < attempts && isRetriableError(err);
      if (!canRetry) {
        break;
      }

      const jitter = Math.floor(Math.random() * 250);
      const delayMs = Math.min(10000, baseDelayMs * 2 ** (attempt - 1) + jitter);
      ctx.emitWarning(
        `Copilot SDK call failed (attempt ${attempt}/${attempts}), retrying in ${delayMs}ms: ${safeString(err)}`
      );
      await sleep(delayMs);
    }
  }

  throw lastError || new Error("Copilot SDK run failed");
}

function createJsonRpcTransport(writeLine, onNotification) {
  const pending = new Map();
  let nextId = 1;

  function send(method, params, timeoutMs = 120000) {
    const id = `${nextId++}`;
    const request = {
      jsonrpc: "2.0",
      id,
      method,
      params: params || {},
    };

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`json-rpc timeout for ${method}`));
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      writeLine(JSON.stringify(request) + "\n");
    });
  }

  async function handleLine(line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch (_) {
      return;
    }

    if (message && typeof message === "object" && message.id != null && (Object.prototype.hasOwnProperty.call(message, "result") || Object.prototype.hasOwnProperty.call(message, "error"))) {
      const id = String(message.id);
      const call = pending.get(id);
      if (!call) {
        return;
      }
      clearTimeout(call.timer);
      pending.delete(id);
      if (message.error) {
        const err = new Error(message.error.message || "json-rpc error");
        err.code = message.error.code;
        err.data = message.error.data;
        call.reject(err);
      } else {
        call.resolve(message.result || null);
      }
      return;
    }

    if (message && typeof message === "object" && typeof message.method === "string") {
      const method = message.method;
      const params = message.params || {};
      const id = Object.prototype.hasOwnProperty.call(message, "id") ? message.id : null;
      try {
        await Promise.resolve(onNotification(method, params, id));
      } catch (err) {
        if (id != null && id !== undefined && respond) {
          respond(id, null, err);
        }
      }
    }
  }

  function respond(id, result, error) {
    const response = {
      jsonrpc: "2.0",
      id,
    };
    if (error) {
      response.error = {
        code: Number(error.code) || -32000,
        message: safeString(error.message || error),
      };
    } else {
      response.result = result || {};
    }
    writeLine(JSON.stringify(response) + "\n");
  }

  function close(reason) {
    const err = reason || new Error("json-rpc client closed");
    for (const call of pending.values()) {
      clearTimeout(call.timer);
      call.reject(err);
    }
    pending.clear();
  }

  return { send, respond, handleLine, close };
}

function isMethodNotFound(err) {
  if (!err) {
    return false;
  }
  if (err.code === -32601) {
    return true;
  }
  const msg = String(err.message || "").toLowerCase();
  return msg.includes("method not found") || msg.includes("unknown method");
}

async function callWithFallback(rpc, candidates) {
  let lastErr = null;
  for (const item of candidates) {
    try {
      const result = await rpc.send(item.method, item.params);
      return result;
    } catch (err) {
      if (!isMethodNotFound(err)) {
        throw err;
      }
      lastErr = err;
    }
  }
  throw lastErr || new Error("no suitable JSON-RPC method found");
}

function collectPromptText(request) {
  if (request.prompt_ctx) {
    return [request.prompt, request.prompt_ctx].filter(Boolean).join("\n\n");
  }
  return request.prompt || "";
}

function buildInitializePayload(request) {
  return {
    protocol_version: "2025-03-26",
    capabilities: {
      fs: {
        readTextFile: true,
        writeTextFile: true,
      },
      terminal: {
        execute: true,
      },
      mcp: {
        client: true,
      },
      tools: true,
    },
    client_info: {
      name: ADAPTER_NAME,
      version: ADAPTER_VERSION,
    },
    workspace_root: request.workspace_root,
    cwd: request.workspace_root,
  };
}

function buildNewSessionPayload(request) {
  return {
    cwd: request.workspace_root,
    working_directory: request.workspace_root,
    mcp_servers: request.mcpServers,
  };
}

function buildSessionIdPayload(sessionId) {
  return {
    sessionId,
    session_id: sessionId,
  };
}

function buildPromptPayload(request) {
  const text = collectPromptText(request);
  const content = [{ type: "text", text }];
  const message = {
    role: "user",
    content,
  };
  return {
    model: request.model || null,
    reasoningEffort: request.reasoningEffort || null,
    systemMessage: request.systemMessage || null,
    prompt: text,
    message,
    content,
    tools: {
      available: request.availableTools,
      excluded: request.excludedTools,
    },
    mcpServers: request.mcpServers,
    streaming: true,
  };
}

function parseToolFromPayload(payload) {
  const toolName = String(
    payload.tool_name ||
      payload.toolName ||
      payload.name ||
      payload.tool ||
      payload.function ||
      "tool"
  );
  return {
    toolName,
    toolUseId: payload.tool_use_id || payload.toolUseId || payload.id || payload.tool_id || null,
    parentToolUseId: payload.parent_tool_use_id || payload.parentToolUseId || null,
    input: payload.input || payload.arguments || payload.params || {},
    output: payload.output || payload.result || payload.tool_output || payload.value || "",
    isError: !!(payload.is_error || payload.isError || payload.error),
  };
}

function isDestructiveTool(toolName, input) {
  const lower = String(toolName).toLowerCase();
  if (lower.includes("bash") || lower.includes("shell") || lower.includes("write") || lower.includes("edit") || lower.includes("rm") || lower.includes("delete")) {
    return true;
  }
  if (input && typeof input === "object") {
    const raw = safeString(input).toLowerCase();
    if (raw.includes("git push") || raw.includes("delete") || raw.includes("remove")) {
      return true;
    }
  }
  return false;
}

function evaluatePermission(ctx, params = {}) {
  const tool = String(
    params.tool_name ||
      params.toolName ||
      params.name ||
      params.tool ||
      params.function ||
      "tool"
  );
  const lowerTool = tool.toLowerCase();
  const input = params.input || params.arguments || params.params || {};

  if (AUTO_DENY_ALWAYS_TOOLS.includes(lowerTool) || hasPrefixMatch(DENY_TOOLS, lowerTool)) {
    return { decision: "reject", reason: `tool '${tool}' is blocked by denylist` };
  }

  if (ctx.policyEngine && typeof ctx.policyEngine.requiresApproval === "function") {
    const policyDecision = ctx.policyEngine.requiresApproval(tool);
    if (policyDecision) {
      return {
        decision: AUTO_APPROVE_ALWAYS ? "allow_always" : "reject",
        reason: `tool '${tool}' requires policy approval`,
      };
    }
  }

  if (AUTO_APPROVE_TOOLS.includes(lowerTool)) {
    return {
      decision: AUTO_APPROVE_ALWAYS ? "allow_always" : "allow_once",
      reason: `tool '${tool}' on allow-tools list`,
    };
  }

  if (AUTO_ALLOW_ALWAYS_TOOLS.includes(lowerTool)) {
    return { decision: "allow_always", reason: `tool '${tool}' on allow-always list` };
  }

  if (isDestructiveTool(tool, input) && !AUTO_APPROVE_ALWAYS) {
    return { decision: "allow_once", reason: `destructive tool '${tool}' allowed once` };
  }

  return { decision: AUTO_APPROVE_ALWAYS ? "allow_always" : "allow_once", reason: "default policy permit" };
}

function hasPrefixMatch(list, candidate) {
  if (!Array.isArray(list) || !candidate) {
    return false;
  }
  const value = String(candidate).toLowerCase();
  return list.some((item) => value === item || value.startsWith(`${item}:`));
}

function normalizeAcpPayload(ctx, payload) {
  if (payload == null) {
    return;
  }
  if (typeof payload === "string") {
    ctx.emitAssistantDelta(payload);
    return;
  }
  if (Array.isArray(payload)) {
    for (const item of payload) {
      normalizeAcpPayload(ctx, item);
    }
    return;
  }
  if (typeof payload !== "object") {
    return;
  }

  const type = String(payload.type || payload.kind || payload.event || "").toLowerCase();
  const text = safeString(payload.text || payload.message || payload.delta || "").trim();
  const tool = parseToolFromPayload(payload);

  if (type.includes("assistant") && type.includes("delta")) {
    if (text) {
      ctx.emitAssistantDelta(text);
    }
    return;
  }

  if (type.includes("assistant") && text) {
    ctx.emitAssistantMessage(text);
    return;
  }

  if (type.includes("tool") && type.includes("call")) {
    ctx.emitToolCall({
      toolName: tool.toolName,
      toolUseId: tool.toolUseId,
      parentToolUseId: tool.parentToolUseId,
      input: tool.input,
    });
    return;
  }

  if (type.includes("tool") && type.includes("result")) {
    ctx.emitToolResult({
      toolName: tool.toolName,
      toolUseId: tool.toolUseId,
      output: tool.output,
      isError: tool.isError,
    });
    return;
  }

  if (type.includes("warning")) {
    if (text) {
      ctx.emitWarning(text);
    }
    return;
  }

  if (type.includes("error")) {
    ctx.emitError(text || safeString(payload.error || payload));
    return;
  }

  if (type.includes("usage")) {
    ctx.__collectUsage(payload.usage || payload);
    return;
  }

  if (payload.content && Array.isArray(payload.content)) {
    for (const item of payload.content) {
      normalizeAcpPayload(ctx, item);
    }
    return;
  }

  if (payload.tool_calls && Array.isArray(payload.tool_calls)) {
    for (const call of payload.tool_calls) {
      normalizeAcpPayload(ctx, { ...call, type: call.type || "tool_call" });
    }
    return;
  }

  if (text) {
    ctx.emitAssistantDelta(text);
  }
}

function pickSessionId(result) {
  if (!result || typeof result !== "object") {
    return null;
  }
  return result.sessionId || result.session_id || result.id || null;
}

function resolveTransportMode() {
  if (["sdk", "acp", "legacy", "auto"].includes(TRANSPORT_MODE)) {
    return TRANSPORT_MODE;
  }
  if (MODE === "legacy") {
    return "legacy";
  }
  if (MODE === "acp") {
    return "acp";
  }
  return "auto";
}

function resolveLegacyRunner() {
  if (RUNNER_CMD && RUNNER_CMD.trim()) {
    const resolved = resolveCommandPath(RUNNER_CMD) || RUNNER_CMD;
    return {
      command: resolved,
      args: RUNNER_ARGS,
    };
  }

  if (process.env.ABP_COPILOT_CMD || DEFAULT_COPILOT_ARGS.length > 0) {
    const resolved = resolveCommandPath(DEFAULT_COPILOT_CMD);
    if (!resolved) {
      return null;
    }
    return {
      command: resolved,
      args: DEFAULT_COPILOT_ARGS,
    };
  }

  const resolved = resolveCommandPath(DEFAULT_COPILOT_CMD);
  if (!resolved) {
    return null;
  }

  return {
    command: resolved,
    args: [],
  };
}

async function runFromCommand(command, args, request, ctx) {
  return new Promise((resolve) => {
    if (!command) {
      resolve({
        usageRaw: {
          mode: "legacy_runner_not_configured",
        },
        usage: {},
        outcome: "partial",
      });
      return;
    }

    let settled = false;
    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      resolve(result);
    };

    const child = spawn(command, args, {
      cwd: request.workspace_root || process.cwd(),
      env: {
        ...process.env,
        ...request.env,
      },
      stdio: ["pipe", "pipe", "pipe"],
      shell: false,
    });

    let usageRaw = {};
    const out = readline.createInterface({
      input: child.stdout,
      crlfDelay: Infinity,
    });
    out.on("line", (line) => {
      if (!line || !line.trim()) {
        return;
      }
      let parsed = null;
      try {
        parsed = JSON.parse(line);
      } catch (_) {
        ctx.emitAssistantDelta(line);
        return;
      }

      const msgType = String(parsed.type || "").toLowerCase();
      if (msgType.includes("tool_call") || msgType.includes("toolcall") || msgType.includes("tool_call")) {
        const tool = parseToolFromPayload(parsed);
        ctx.emitToolCall({
          toolName: tool.toolName,
          toolUseId: tool.toolUseId,
          parentToolUseId: tool.parentToolUseId,
          input: tool.input,
        });
        return;
      }
      if (msgType.includes("tool_result") || msgType.includes("toolresult")) {
        const tool = parseToolFromPayload(parsed);
        ctx.emitToolResult({
          toolName: tool.toolName,
          toolUseId: tool.toolUseId,
          output: tool.output,
          isError: tool.isError,
        });
        return;
      }
      if (msgType.includes("usage")) {
        usageRaw = mergeUsage(usageRaw, parsed.usage || parsed);
        return;
      }
      if (parsed.type === "assistant_delta" || parsed.kind === "assistant_delta" || msgType.includes("delta")) {
        ctx.emitAssistantDelta(safeString(parsed.text || parsed.message || parsed.delta || ""));
        return;
      }
      if (msgType.includes("warning")) {
        ctx.emitWarning(safeString(parsed.message || parsed.text || ""));
        return;
      }
      if (msgType.includes("error")) {
        ctx.emitError(safeString(parsed.message || parsed.text || ""));
        return;
      }
      if (msgType.includes("assistant_message") || msgType.includes("assistant")) {
        ctx.emitAssistantMessage(safeString(parsed.text || parsed.message || ""));
        return;
      }
      ctx.emitWarning(`unhandled legacy payload: ${safeString(parsed)}`);
    });

    child.stderr.on("data", (chunk) => {
      ctx.emitWarning(`[copilot legacy] ${safeString(chunk.toString())}`);
    });

    child.on("error", (err) => {
      ctx.emitError(`legacy command failed to start: ${safeString(err)}`);
      finish({
        usageRaw: {
          mode: "legacy_runner_start_failed",
          error: safeString(err),
        },
        usage: {},
        outcome: "failed",
      });
    });

    child.on("close", (code) => {
      finish({
        usageRaw,
        usage: parseUsage(usageRaw),
        outcome: code === 0 ? "complete" : "failed",
      });
    });

    child.stdin.end(safeString(request) + "\n", "utf8");
  });
}

async function openTcpTransport(endpoint, onLine) {
  const parsed = parseTcpAddress(endpoint);
  if (!parsed) {
    throw new Error(`invalid TCP endpoint '${endpoint}'`);
  }

  return new Promise((resolve, reject) => {
    const socket = net.createConnection(parsed);
    const state = { buf: "" };
    socket.once("connect", () => {
      socket.on("data", (chunk) => {
        state.buf += chunk.toString();
        let index;
        while ((index = state.buf.indexOf("\n")) >= 0) {
          const line = state.buf.slice(0, index);
          state.buf = state.buf.slice(index + 1);
          if (line.trim()) {
            onLine(line.trim()).catch(() => {});
          }
        }
      });

      resolve({
        writeLine: (line) => socket.write(line),
        close: () => socket.end(),
        raw: socket,
      });
    });

    socket.once("error", (err) => {
      reject(err);
    });
  });
}

function parseTcpAddress(raw) {
  if (!raw) {
    return null;
  }
  if (/^\d+$/.test(String(raw))) {
    return { host: "127.0.0.1", port: Number(raw) };
  }
  if (/^[^:]+:\d+$/.test(String(raw))) {
    const [host, port] = String(raw).split(":");
    const parsed = Number(port);
    if (!Number.isNaN(parsed)) {
      return { host, port: parsed };
    }
  }
  try {
    const u = new URL(raw);
    const port = Number(u.port);
    if (Number.isNaN(port) || port <= 0) {
      return null;
    }
    return { host: u.hostname || "127.0.0.1", port };
  } catch (_) {
    return null;
  }
}

function waitForTcpOpen(address, timeoutMs = 12000) {
  const parsed = parseTcpAddress(address);
  if (!parsed) {
    return Promise.reject(new Error(`invalid endpoint '${address}'`));
  }

  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.createConnection(parsed);
      const onError = () => {
        if (Date.now() >= deadline) {
          reject(new Error(`timed out waiting for ACP endpoint ${address}`));
          return;
        }
        setTimeout(attempt, 200);
      };
      socket.once("error", onError);
      socket.once("connect", () => {
        socket.destroy();
        resolve();
      });
    };
    attempt();
  });
}

function spawnAcpServer(request) {
  const args = ACP_ARGS.length > 0 ? [...ACP_ARGS] : ["agent", "--acp", "--stdio"];
  if (typeof request.acpPort === "number" && request.acpPort > 0) {
    if (!args.includes("--port") && !args.includes("-p")) {
      args.push("--port", `${request.acpPort}`);
    }
  }
  const command = resolveCommandPath(DEFAULT_COPILOT_CMD) || DEFAULT_COPILOT_CMD;
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: request.workspace_root || process.cwd(),
      env: {
        ...process.env,
        ...request.env,
      },
      stdio: ["pipe", "pipe", "pipe"],
      shell: false,
    });

    child.once("error", (err) => {
      reject(err);
    });
    child.once("spawn", () => {
      resolve(child);
    });
  });
}

async function runAcp(request, ctx) {
  let usageRaw = {};
  let transport = null;
  let endpointServerChild = null;
  let rpc = null;
  const collectUsage = (raw) => {
    usageRaw = mergeUsage(usageRaw, raw);
  };

  const context = {
    ...ctx,
    __collectUsage: collectUsage,
  };

  try {
    const onNotification = async (method, params, id) => {
      const m = String(method || "").toLowerCase();
      if (m === "session/request_permission") {
        const decision = evaluatePermission(context, params || {});
        if (id != null && id !== undefined) {
          rpc.respond(id, {
            decision: decision.decision,
            approved: decision.decision === "allow_once" || decision.decision === "allow_always",
            approval: decision.decision,
            action: decision.decision,
            reason: decision.reason || null,
          });
        }
        if (decision.decision === "reject") {
          context.emitWarning(`permission denied: ${decision.reason}`);
        } else {
          context.emitWarning(`permission granted (${decision.decision})`);
        }
        return;
      }

      if (m === "session/update" || m.includes("update")) {
        normalizeAcpPayload(context, params);
        return;
      }

      if (m.includes("usage")) {
        if (params && typeof params === "object") {
          collectUsage(params.usage || params);
        }
        return;
      }

      if (id != null && id !== undefined && params) {
        rpc.respond(id, {});
      }
    };

    const endpoint = ACP_URL || (Number.isInteger(ACP_PORT) && ACP_PORT > 0 ? `127.0.0.1:${ACP_PORT}` : "");
    let lineReader = null;

    if (endpoint) {
      const shouldSpawnForPort = !ACP_URL && Number.isInteger(ACP_PORT) && ACP_PORT > 0;
      if (shouldSpawnForPort) {
        request = { ...request, acpPort: ACP_PORT };
        endpointServerChild = await spawnAcpServer(request);
      }
      await waitForTcpOpen(endpoint);
      transport = await openTcpTransport(endpoint, async (line) => {
        if (rpc) {
          await rpc.handleLine(line);
        }
      });
    } else {
      const child = await spawnAcpServer(request);
      transport = {
        writeLine: (line) => child.stdin.write(line),
        close: () => {
          try {
            child.kill();
          } catch (_) {}
        },
      };
      lineReader = readline.createInterface({
        input: child.stdout,
        crlfDelay: Infinity,
      });
      lineReader.on("line", (line) => {
        if (rpc) {
          rpc.handleLine(line).catch(() => {});
        }
      });
      child.stderr.on("data", (chunk) => {
        context.emitWarning(`[copilot acp cli] ${safeString(chunk.toString())}`);
      });
    }

    rpc = createJsonRpcTransport(transport.writeLine, onNotification);

    const initializeResult = await callWithFallback(rpc, [
      { method: "initialize", params: buildInitializePayload(request) },
      { method: "initializeClient", params: buildInitializePayload(request) },
    ]);
    if (initializeResult && initializeResult.protocol_version) {
      context.emitWarning(`copilot protocol ${initializeResult.protocol_version}`);
    }

    let sessionResult = null;
    if (request.sessionId) {
      sessionResult = await callWithFallback(rpc, [
        { method: "session/loadClient", params: buildSessionIdPayload(request.sessionId) },
        { method: "session/load", params: buildSessionIdPayload(request.sessionId) },
      ]);
      if (!sessionResult) {
        context.emitWarning(`unable to resume session ${request.sessionId}; creating new session`);
      }
    }
    if (!sessionResult) {
      sessionResult = await callWithFallback(rpc, [
        { method: "session/newClient", params: buildNewSessionPayload(request) },
        { method: "session/new", params: buildNewSessionPayload(request) },
      ]);
    }

    const sessionId = pickSessionId(sessionResult);
    const promptPayload = buildPromptPayload(request);

    if (!sessionId) {
      throw new Error("no session id from Copilot ACP session creation");
    }

    const promptResult = await callWithFallback(rpc, [
      {
        method: "session/promptClient",
        params: {
          sessionId,
          message: promptPayload.message,
          prompt: promptPayload.prompt,
          content: promptPayload.content,
          model: promptPayload.model,
          reasoningEffort: promptPayload.reasoningEffort,
          systemMessage: promptPayload.systemMessage,
        },
      },
      {
        method: "session/prompt",
        params: {
          session_id: sessionId,
          message: promptPayload.message,
          prompt: promptPayload.prompt,
          content: promptPayload.content,
          model: promptPayload.model,
          reasoningEffort: promptPayload.reasoningEffort,
          systemMessage: promptPayload.systemMessage,
        },
      },
    ]);

    if (promptResult && typeof promptResult === "object" && promptResult.usage) {
      usageRaw = mergeUsage(usageRaw, promptResult.usage);
    }

    if (promptResult && (promptResult.status || promptResult.result)) {
      const status = String(promptResult.status || promptResult.result).toLowerCase();
      if (status.includes("fail") || status.includes("error")) {
        return {
          usageRaw,
          usage: parseUsage(usageRaw),
          outcome: "failed",
        };
      }
    }

    return {
      usageRaw,
      usage: parseUsage(usageRaw),
      outcome: "complete",
    };
  } finally {
    if (transport && typeof transport.close === "function") {
      transport.close();
    }
    if (endpointServerChild && !endpointServerChild.killed) {
      try {
        endpointServerChild.kill();
      } catch (_) {}
    }
  }
}

async function runFromLegacy(request, ctx) {
  const selected = resolveLegacyRunner();
  if (!selected) {
    ctx.emitWarning("No legacy runner configured");
    return {
      usageRaw: {
        mode: "copilot_adapter_fallback",
        message: "Configure ABP_COPILOT_RUNNER or ABP_COPILOT_CMD",
      },
      usage: {},
      outcome: "partial",
    };
  }

  return runFromCommand(selected.command, selected.args, request, ctx);
}

async function run(ctx) {
  const request = pickContextForRequest(ctx);
  const transportMode = resolveTransportMode();

  if (request.mode === "passthrough" && request.raw_request) {
    const passthrough = {
      request_id: request.request_id,
      prompt: request.raw_request,
      workspace_root: request.workspace_root,
      model: request.model,
      env: request.env,
    };

    if (transportMode === "sdk") {
      return runSdkWithRetry(passthrough, ctx);
    }
    if (transportMode === "acp") {
      try {
        return await runAcp(passthrough, ctx);
      } catch (err) {
        ctx.emitWarning(
          `ACP passthrough failed, falling back to legacy runner: ${safeString(err)}`
        );
        return runFromLegacy(passthrough, ctx);
      }
    }
    if (transportMode === "legacy") {
      return runFromLegacy(passthrough, ctx);
    }

    // auto
    try {
      return await runSdkWithRetry(passthrough, ctx);
    } catch (sdkErr) {
      ctx.emitWarning(`Copilot SDK passthrough failed, trying ACP: ${safeString(sdkErr)}`);
    }
    try {
      return await runAcp(passthrough, ctx);
    } catch (acpErr) {
      ctx.emitWarning(
        `ACP passthrough failed, falling back to legacy runner: ${safeString(acpErr)}`
      );
      return runFromLegacy(passthrough, ctx);
    }
  }

  if (transportMode === "sdk") {
    return runSdkWithRetry(request, ctx);
  }
  if (transportMode === "acp") {
    try {
      return await runAcp(request, ctx);
    } catch (err) {
      ctx.emitWarning(`ACP mode failed, falling back to legacy command: ${safeString(err)}`);
      return runFromLegacy(request, ctx);
    }
  }
  if (transportMode === "legacy") {
    return runFromLegacy(request, ctx);
  }

  // auto
  try {
    return await runSdkWithRetry(request, ctx);
  } catch (sdkErr) {
    ctx.emitWarning(`Copilot SDK mode failed, trying ACP: ${safeString(sdkErr)}`);
  }
  try {
    return await runAcp(request, ctx);
  } catch (acpErr) {
    ctx.emitWarning(`ACP mode failed, falling back to legacy command: ${safeString(acpErr)}`);
    return runFromLegacy(request, ctx);
  }
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
};
