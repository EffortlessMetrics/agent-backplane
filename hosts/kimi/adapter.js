const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const { pathToFileURL } = require("node:url");

const ADAPTER_NAME = "kimi_sdk_adapter";
const ADAPTER_VERSION = "0.2.0";
const SDK_MODULE = process.env.ABP_KIMI_SDK_MODULE || "@moonshot-ai/kimi-agent-sdk";
const TRANSPORT_MODE = String(process.env.ABP_KIMI_TRANSPORT || "auto").toLowerCase();
const DEFAULT_MODEL = process.env.ABP_KIMI_MODEL || "kimi-for-coding";
const DEFAULT_KIMI_CMD = process.env.ABP_KIMI_CMD || "kimi";
const DEFAULT_KIMI_ARGS = parseArgList(process.env.ABP_KIMI_ARGS);
const RUNNER_CMD = process.env.ABP_KIMI_RUNNER || "";
const RUNNER_ARGS = parseArgList(process.env.ABP_KIMI_RUNNER_ARGS);
const DEFAULT_RETRY_ATTEMPTS = parsePositiveInt(process.env.ABP_KIMI_RETRY_ATTEMPTS, 3);
const DEFAULT_RETRY_BASE_DELAY_MS = parsePositiveInt(
  process.env.ABP_KIMI_RETRY_BASE_DELAY_MS,
  1000
);

let cachedSdkModule = null;
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

function asObject(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return value;
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

function parsePositiveInt(raw, fallback) {
  const parsed = Number.parseInt(String(raw || ""), 10);
  if (Number.isFinite(parsed) && parsed > 0) {
    return parsed;
  }
  return fallback;
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
  if (typeof value === "string") {
    const lowered = value.trim().toLowerCase();
    if (lowered === "true") {
      return true;
    }
    if (lowered === "false") {
      return false;
    }
  }
  return undefined;
}

function pickNumber(obj, keys) {
  const value = pickValue(obj, keys);
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string" && value.trim().length > 0) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }
  return undefined;
}

function getVendorNamespace(vendor, namespace) {
  const out = {};
  const nested = asObject(vendor[namespace]);
  Object.assign(out, nested);

  const prefix = `${namespace}.`;
  for (const [key, value] of Object.entries(vendor)) {
    if (key.startsWith(prefix)) {
      out[key.slice(prefix.length)] = value;
    }
  }

  return out;
}

function toCamel(value) {
  return String(value).replace(/_([a-z])/g, (_, ch) => ch.toUpperCase());
}

function pickNumericToken(raw, candidates) {
  if (!raw || typeof raw !== "object") {
    return undefined;
  }

  for (const key of candidates) {
    const direct = raw[key];
    if (typeof direct === "number" && Number.isFinite(direct)) {
      return direct;
    }

    const camel = toCamel(key);
    const camelValue = raw[camel];
    if (typeof camelValue === "number" && Number.isFinite(camelValue)) {
      return camelValue;
    }
  }

  return undefined;
}

function normalizeUsage(raw) {
  if (!raw || typeof raw !== "object") {
    return {};
  }

  return {
    input_tokens: pickNumericToken(raw, [
      "input_tokens",
      "inputTokens",
      "prompt_tokens",
      "promptTokens",
    ]),
    output_tokens: pickNumericToken(raw, [
      "output_tokens",
      "outputTokens",
      "completion_tokens",
      "completionTokens",
    ]),
    cache_read_tokens: pickNumericToken(raw, ["cache_read_tokens", "cacheReadTokens"]),
    cache_write_tokens: pickNumericToken(raw, ["cache_write_tokens", "cacheWriteTokens"]),
    request_units: pickNumericToken(raw, ["request_units", "requestUnits"]),
    estimated_cost_usd: pickNumericToken(raw, ["estimated_cost_usd", "estimatedCostUsd"]),
  };
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
      for (const ext of ["", ".exe", ".cmd", ".bat", ".com", ".ps1"]) {
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

function normalizeSdkImportTarget(target) {
  const raw = String(target || "").trim();
  if (!raw) {
    return raw;
  }
  if (raw.startsWith("file://") || raw.startsWith("node:") || raw.startsWith("data:")) {
    return raw;
  }
  if (path.isAbsolute(raw) || raw.startsWith(".") || raw.startsWith("..")) {
    return pathToFileURL(path.resolve(raw)).href;
  }
  return raw;
}

function resolveSdkVersion() {
  const candidates = [
    path.resolve(process.cwd(), "hosts/kimi/node_modules/@moonshot-ai/kimi-agent-sdk/package.json"),
    path.resolve(__dirname, "node_modules/@moonshot-ai/kimi-agent-sdk/package.json"),
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

async function loadKimiSdkModule() {
  if (cachedSdkModule) {
    return cachedSdkModule;
  }
  if (cachedSdkLoadError) {
    throw cachedSdkLoadError;
  }

  try {
    cachedSdkModule = await import(normalizeSdkImportTarget(SDK_MODULE));
    cachedSdkVersion = resolveSdkVersion();
    return cachedSdkModule;
  } catch (err) {
    cachedSdkLoadError = new Error(
      `failed to load Kimi SDK module '${SDK_MODULE}': ${safeString(err)}`
    );
    throw cachedSdkLoadError;
  }
}

function buildPrompt(workOrder) {
  let prompt = String(workOrder.task || "").trim();
  const context = asObject(workOrder.context);
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

function buildRequest(ctx) {
  const workOrder = asObject(ctx.workOrder);
  const runtimeConfig = asObject(workOrder.config);
  const vendor = asObject(runtimeConfig.vendor);
  const kimiVendor = getVendorNamespace(vendor, "kimi");
  const abpVendor = getVendorNamespace(vendor, "abp");

  const model =
    pickString(kimiVendor, ["model"]) ||
    pickString(runtimeConfig, ["model"]) ||
    DEFAULT_MODEL;
  const stream = pickBoolean(kimiVendor, ["stream", "streaming"]);
  const temperature = pickNumber(kimiVendor, ["temperature"]);
  const topP = pickNumber(kimiVendor, ["top_p", "topP"]);
  const timeoutMs = pickNumber(kimiVendor, ["timeout_ms", "timeoutMs"]);
  const retryAttempts =
    pickNumber(kimiVendor, ["retry_attempts", "retryAttempts", "retries"]) ||
    DEFAULT_RETRY_ATTEMPTS;
  const retryBaseDelayMs =
    pickNumber(kimiVendor, ["retry_base_delay_ms", "retryBaseDelayMs"]) ||
    DEFAULT_RETRY_BASE_DELAY_MS;

  return {
    request_id: workOrder.id || null,
    prompt: buildPrompt(workOrder),
    workspace_root: (workOrder.workspace && workOrder.workspace.root) || process.cwd(),
    model,
    stream: stream !== undefined ? stream : true,
    temperature,
    top_p: topP,
    thinking_mode: pickString(kimiVendor, ["thinking_mode", "thinkingMode"]),
    reasoning_effort: pickString(kimiVendor, ["reasoning_effort", "reasoningEffort"]),
    agent_mode: pickString(kimiVendor, ["agent_mode", "agentMode"]),
    agent_swarm: pickBoolean(kimiVendor, ["agent_swarm", "agentSwarm"]),
    yolo: pickBoolean(kimiVendor, ["yolo"]),
    max_tokens: pickNumber(kimiVendor, ["max_tokens", "maxTokens", "token_limit", "tokenLimit"]),
    timeout_ms: timeoutMs,
    retry_attempts: Math.max(1, Math.floor(retryAttempts)),
    retry_base_delay_ms: Math.max(1, Math.floor(retryBaseDelayMs)),
    api_key:
      pickString(kimiVendor, ["api_key", "apiKey"]) ||
      process.env.KIMI_API_KEY ||
      process.env.KIMI_API_CODE ||
      null,
    base_url:
      pickString(kimiVendor, ["base_url", "baseUrl"]) ||
      process.env.KIMI_BASE_URL ||
      process.env.KIMI_API_BASE_URL ||
      null,
    mode: pickString(abpVendor, ["mode"]) || "mapped",
    raw_request: abpVendor.request || null,
    max_budget_usd: runtimeConfig.max_budget_usd,
    max_turns: runtimeConfig.max_turns,
    env: asObject(runtimeConfig.env),
    vendor,
  };
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
  if (typeof value.output === "string") {
    return value.output;
  }
  if (typeof value.content === "string") {
    return value.content;
  }
  if (Array.isArray(value.content)) {
    return value.content
      .map((part) => {
        if (typeof part === "string") {
          return part;
        }
        if (part && typeof part.text === "string") {
          return part.text;
        }
        return "";
      })
      .join("");
  }
  if (Array.isArray(value.messages) && value.messages.length > 0) {
    return value.messages.map((msg) => extractText(msg, depth + 1)).filter(Boolean).join("\n");
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

function mergeUsage(base, raw) {
  if (!raw || typeof raw !== "object") {
    return base;
  }
  return {
    ...base,
    ...raw,
  };
}

function normalizeOutcome(value, fallback = "complete") {
  const raw = String(value || fallback).trim().toLowerCase();
  if (raw === "complete" || raw === "completed") {
    return "complete";
  }
  if (raw === "partial" || raw === "partially_complete" || raw === "partially-complete") {
    return "partial";
  }
  if (raw === "failed" || raw === "failure" || raw === "error") {
    return "failed";
  }
  return fallback;
}

function parseToolCall(message) {
  return {
    toolName: String(
      message.tool_name || message.toolName || message.name || message.tool || "kimi_tool"
    ),
    toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
    parentToolUseId: message.parent_tool_use_id || message.parentToolUseId || null,
    input: message.input || message.arguments || message.params || {},
  };
}

function parseToolResult(message) {
  return {
    toolName: String(
      message.tool_name || message.toolName || message.name || message.tool || "kimi_tool"
    ),
    toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
    output: message.output || message.result || message.value || {},
    isError: !!(message.is_error || message.isError || message.error),
  };
}

function emitFromParsedMessage(ctx, message, onUsage) {
  if (!message || typeof message !== "object") {
    if (typeof message === "string" && message.length > 0) {
      ctx.emitAssistantDelta(message);
    }
    return;
  }

  const kind = String(message.type || message.kind || message.event || "").toLowerCase();
  const text = extractText(message);

  if (
    kind.includes("assistant_delta") ||
    kind.includes("content_delta") ||
    kind.includes("stream") ||
    kind.includes("delta")
  ) {
    if (text) {
      ctx.emitAssistantDelta(text);
    }
    return;
  }

  if (kind.includes("assistant_message") || kind === "assistant" || kind === "message") {
    if (text) {
      ctx.emitAssistantMessage(text);
    }
    return;
  }

  if (kind.includes("tool") && kind.includes("call")) {
    ctx.emitToolCall(parseToolCall(message));
    return;
  }

  if (kind.includes("tool") && kind.includes("result")) {
    ctx.emitToolResult(parseToolResult(message));
    return;
  }

  if (kind.includes("warning")) {
    ctx.emitWarning(text || "kimi warning");
    return;
  }

  if (kind.includes("error")) {
    ctx.emitError(text || safeString(message.error || message));
    return;
  }

  if (kind.includes("usage")) {
    onUsage(message.usage || message);
    return;
  }

  if (message.usage && typeof message.usage === "object") {
    onUsage(message.usage);
  }
  if (text) {
    ctx.emitAssistantDelta(text);
  }
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
  if (value.chunks && typeof value.chunks[Symbol.asyncIterator] === "function") {
    return value.chunks;
  }
  return null;
}

function buildSdkClientOptions(request) {
  const options = {
    model: request.model,
  };

  if (request.api_key) {
    options.apiKey = request.api_key;
    options.api_key = request.api_key;
    options.token = request.api_key;
  }
  if (request.base_url) {
    options.baseUrl = request.base_url;
    options.base_url = request.base_url;
  }
  if (request.workspace_root) {
    options.cwd = request.workspace_root;
    options.workingDirectory = request.workspace_root;
  }
  if (typeof request.timeout_ms === "number") {
    options.timeoutMs = request.timeout_ms;
    options.timeout_ms = request.timeout_ms;
  }

  return options;
}

function findSdkFactory(mod) {
  const candidates = [
    mod?.KimiClient,
    mod?.KimiAgent,
    mod?.KimiAgentClient,
    mod?.Client,
    mod?.default?.KimiClient,
    mod?.default?.KimiAgent,
    mod?.default?.KimiAgentClient,
    mod?.default,
  ];

  for (const candidate of candidates) {
    if (typeof candidate === "function") {
      return candidate;
    }
  }

  return null;
}

async function createSdkClient(mod, request) {
  const options = buildSdkClientOptions(request);

  if (typeof mod?.createClient === "function") {
    return mod.createClient(options);
  }
  if (typeof mod?.default?.createClient === "function") {
    return mod.default.createClient(options);
  }

  const factory = findSdkFactory(mod);
  if (!factory) {
    throw new Error(`Kimi SDK module '${SDK_MODULE}' does not export a compatible client`);
  }

  try {
    return new factory(options);
  } catch (_) {
    const maybeClient = factory(options);
    if (maybeClient && typeof maybeClient === "object") {
      return maybeClient;
    }
    throw new Error(`Kimi SDK factory '${factory.name || "anonymous"}' is not constructible`);
  }
}

function buildSdkPromptPayload(request) {
  return {
    prompt: request.raw_request || request.prompt,
    input: request.raw_request || request.prompt,
    task: request.raw_request || request.prompt,
    model: request.model,
    stream: request.stream,
    temperature: request.temperature,
    top_p: request.top_p,
    thinking_mode: request.thinking_mode,
    reasoning_effort: request.reasoning_effort,
    agent_mode: request.agent_mode,
    agent_swarm: request.agent_swarm,
    yolo: request.yolo,
    max_tokens: request.max_tokens,
    timeout_ms: request.timeout_ms,
    max_budget_usd: request.max_budget_usd,
    max_turns: request.max_turns,
    workspace_root: request.workspace_root,
    vendor: request.vendor,
  };
}

async function invokeSdkMethod(client, request) {
  const payload = buildSdkPromptPayload(request);
  const prompt = request.raw_request || request.prompt;
  const options = {
    model: request.model,
    stream: request.stream,
    temperature: request.temperature,
    topP: request.top_p,
    reasoningEffort: request.reasoning_effort,
    agentMode: request.agent_mode,
    timeoutMs: request.timeout_ms,
    workspaceRoot: request.workspace_root,
  };

  const methods = [
    "run",
    "runTask",
    "execute",
    "chat",
    "complete",
    "generate",
    "query",
    "ask",
    "invoke",
    "sendMessage",
  ];

  const errors = [];
  for (const methodName of methods) {
    const method = client && client[methodName];
    if (typeof method !== "function") {
      continue;
    }

    try {
      return await method.call(client, payload);
    } catch (errPayload) {
      try {
        return await method.call(client, prompt, options);
      } catch (errPrompt) {
        errors.push(
          `${methodName}: payload=${safeString(errPayload)}; prompt=${safeString(errPrompt)}`
        );
      }
    }
  }

  if (errors.length > 0) {
    throw new Error(`Kimi SDK run attempts failed (${errors.join(" | ")})`);
  }

  throw new Error("Kimi SDK client does not expose a supported run method");
}

async function runSdkOnce(request, ctx) {
  const module = await loadKimiSdkModule();
  const client = await createSdkClient(module, request);
  const result = await invokeSdkMethod(client, request);

  let usageRaw = {};
  let sawDelta = false;
  let fullText = "";

  const iterable = toAsyncIterable(result);
  if (iterable) {
    for await (const chunk of iterable) {
      emitFromParsedMessage(ctx, chunk, (usage) => {
        usageRaw = mergeUsage(usageRaw, usage);
      });
      const chunkText = extractText(chunk);
      if (chunkText) {
        fullText += chunkText;
        sawDelta = true;
      }
      usageRaw = mergeUsage(usageRaw, extractUsage(chunk));
    }
  } else {
    emitFromParsedMessage(ctx, result, (usage) => {
      usageRaw = mergeUsage(usageRaw, usage);
    });
    fullText = extractText(result);
    usageRaw = mergeUsage(usageRaw, extractUsage(result));
  }

  if (!sawDelta && fullText) {
    ctx.emitAssistantMessage(fullText);
  }
  if (!sawDelta && !fullText) {
    ctx.emitWarning("Kimi SDK returned no text content");
  }

  return {
    usageRaw: {
      ...usageRaw,
      sdk_transport: "kimi_agent_sdk",
      sdk_version: cachedSdkVersion,
    },
    usage: normalizeUsage(usageRaw),
    outcome: "complete",
  };
}

function isRetriableError(err) {
  const text = safeString(err).toLowerCase();
  return (
    text.includes("429") ||
    text.includes("503") ||
    text.includes("504") ||
    text.includes("rate limit") ||
    text.includes("temporar") ||
    text.includes("timeout") ||
    text.includes("timed out") ||
    text.includes("econnreset") ||
    text.includes("eai_again")
  );
}

function isAuthError(err) {
  const text = safeString(err).toLowerCase();
  return (
    text.includes("401") ||
    text.includes("unauthorized") ||
    text.includes("authentication") ||
    text.includes("api key") ||
    text.includes("access terminated")
  );
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function runSdkWithRetry(request, ctx) {
  const attempts = Math.max(1, parsePositiveInt(request.retry_attempts, DEFAULT_RETRY_ATTEMPTS));
  const baseDelayMs = Math.max(
    1,
    parsePositiveInt(request.retry_base_delay_ms, DEFAULT_RETRY_BASE_DELAY_MS)
  );

  let lastError = null;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await runSdkOnce(request, ctx);
    } catch (err) {
      lastError = err;
      if (isAuthError(err)) {
        throw err;
      }
      const canRetry = attempt < attempts && isRetriableError(err);
      if (!canRetry) {
        break;
      }

      const jitter = Math.floor(Math.random() * 250);
      const delayMs = Math.min(10000, baseDelayMs * 2 ** (attempt - 1) + jitter);
      ctx.emitWarning(
        `Kimi SDK call failed (attempt ${attempt}/${attempts}), retrying in ${delayMs}ms: ${safeString(err)}`
      );
      await sleep(delayMs);
    }
  }

  throw lastError || new Error("Kimi SDK execution failed");
}

function substituteArgs(args, request) {
  let promptIncluded = false;
  const out = args.map((arg) => {
    let value = String(arg);
    if (value.includes("{prompt}")) {
      promptIncluded = true;
      value = value.split("{prompt}").join(request.prompt || "");
    }
    if (value.includes("{model}")) {
      value = value.split("{model}").join(request.model || "");
    }
    return value;
  });

  if (!promptIncluded) {
    out.push(request.prompt || "");
  }

  return out;
}

function resolveCliCommand() {
  if (RUNNER_CMD && RUNNER_CMD.trim()) {
    const resolvedRunner = resolveCommandPath(RUNNER_CMD) || RUNNER_CMD;
    return {
      command: resolvedRunner,
      args: RUNNER_ARGS,
      inputMode: "json-stdin",
    };
  }

  const resolvedCmd = resolveCommandPath(DEFAULT_KIMI_CMD);
  if (!resolvedCmd) {
    return null;
  }

  const args = DEFAULT_KIMI_ARGS.length > 0 ? DEFAULT_KIMI_ARGS : ["{prompt}"];
  return {
    command: resolvedCmd,
    args,
    inputMode: "prompt-arg",
  };
}

function runFromCommand(command, args, request, ctx, inputMode) {
  return new Promise((resolve, reject) => {
    let usageRaw = {};

    const child = spawn(command, args, {
      cwd: request.workspace_root || process.cwd(),
      env: {
        ...process.env,
        ...request.env,
      },
      stdio: ["pipe", "pipe", "pipe"],
      shell: false,
    });

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

      emitFromParsedMessage(ctx, parsed, (nextUsage) => {
        usageRaw = mergeUsage(usageRaw, nextUsage);
      });
      usageRaw = mergeUsage(usageRaw, extractUsage(parsed));
    });

    child.stderr.on("data", (chunk) => {
      ctx.emitWarning(`[kimi cli] ${safeString(chunk.toString())}`);
    });

    child.on("error", (err) => {
      reject(new Error(`failed to start Kimi command '${command}': ${safeString(err)}`));
    });

    if (inputMode === "json-stdin") {
      child.stdin.end(JSON.stringify(request) + "\n", "utf8");
    } else {
      child.stdin.end();
    }

    child.on("close", (code) => {
      resolve({
        usageRaw,
        usage: normalizeUsage(usageRaw),
        outcome: code === 0 ? "complete" : "failed",
      });
    });
  });
}

async function runViaCli(request, ctx) {
  const resolved = resolveCliCommand();
  if (!resolved) {
    return null;
  }

  const args =
    resolved.inputMode === "prompt-arg"
      ? substituteArgs(resolved.args, request)
      : resolved.args;

  const result = await runFromCommand(resolved.command, args, request, ctx, resolved.inputMode);
  return {
    ...result,
    usageRaw: {
      ...asObject(result.usageRaw),
      sdk_transport: "kimi_cli",
    },
  };
}

function fallbackResult(ctx, request) {
  ctx.emitAssistantMessage("Kimi adapter fallback mode.");
  ctx.emitAssistantMessage("No Kimi SDK module or CLI runner is available.");
  ctx.emitAssistantMessage(
    "Authenticate with 'kimi /login' or set KIMI_API_KEY/KIMI_API_CODE, then install '@moonshot-ai/kimi-agent-sdk' or configure ABP_KIMI_RUNNER."
  );
  ctx.emitAssistantMessage(`Task: ${safeString(request.prompt || ctx?.workOrder?.task || "")}`);

  return {
    usageRaw: {
      mode: "kimi_adapter_fallback",
      note:
        "Install @moonshot-ai/kimi-agent-sdk for SDK mode, or configure ABP_KIMI_RUNNER / ABP_KIMI_CMD for CLI mode",
    },
    usage: {
      input_tokens: 0,
      output_tokens: 0,
    },
    outcome: "partial",
  };
}

function resolveTransportMode() {
  if (["sdk", "cli", "auto"].includes(TRANSPORT_MODE)) {
    return TRANSPORT_MODE;
  }
  return "auto";
}

async function run(ctx) {
  const request = buildRequest(ctx);
  const transportMode = resolveTransportMode();

  if (transportMode === "sdk") {
    try {
      return await runSdkWithRetry(request, ctx);
    } catch (err) {
      const message = safeString(err);
      if (isAuthError(err)) {
        ctx.emitError(
          `Kimi authentication failed: ${message}. Run 'kimi /login' or set KIMI_API_KEY/KIMI_API_CODE.`
        );
      } else {
        ctx.emitError(`Kimi SDK execution failed: ${message}`);
      }
      return {
        usageRaw: {
          transport: "sdk",
          error: message,
        },
        usage: {},
        outcome: "failed",
      };
    }
  }

  if (transportMode === "cli") {
    try {
      const cliResult = await runViaCli(request, ctx);
      if (cliResult) {
        return {
          ...cliResult,
          outcome: normalizeOutcome(cliResult.outcome, "complete"),
        };
      }
    } catch (err) {
      ctx.emitError(`Kimi CLI execution failed: ${safeString(err)}`);
      return {
        usageRaw: {
          transport: "cli",
          error: safeString(err),
        },
        usage: {},
        outcome: "failed",
      };
    }

    return fallbackResult(ctx, request);
  }

  try {
    return await runSdkWithRetry(request, ctx);
  } catch (sdkErr) {
    ctx.emitWarning(`Kimi SDK unavailable, trying CLI mode: ${safeString(sdkErr)}`);
  }

  try {
    const cliResult = await runViaCli(request, ctx);
    if (cliResult) {
      return {
        ...cliResult,
        outcome: normalizeOutcome(cliResult.outcome, "complete"),
      };
    }
  } catch (cliErr) {
    ctx.emitWarning(`Kimi CLI unavailable: ${safeString(cliErr)}`);
  }

  return fallbackResult(ctx, request);
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
};
