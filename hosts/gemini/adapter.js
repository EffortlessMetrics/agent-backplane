const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");

const ADAPTER_NAME = "gemini_sdk_adapter";
const ADAPTER_VERSION = "0.2.0";

const DEFAULT_MODEL = process.env.ABP_GEMINI_MODEL || "gemini-2.5-flash";
const DEFAULT_CMD = process.env.ABP_GEMINI_CMD || "gemini";
const DEFAULT_CMD_ARGS = parseCommandArgs(process.env.ABP_GEMINI_ARGS);
const RUNNER_CMD = process.env.ABP_GEMINI_RUNNER || "";
const RUNNER_ARGS = parseCommandArgs(process.env.ABP_GEMINI_RUNNER_ARGS);
const TRANSPORT = String(process.env.ABP_GEMINI_TRANSPORT || "auto").toLowerCase();
const CLI_INPUT_MODE = String(process.env.ABP_GEMINI_CLI_INPUT || "prompt-arg").toLowerCase();

const DEFAULT_RETRY_ATTEMPTS = parsePositiveInt(process.env.ABP_GEMINI_RETRY_ATTEMPTS, 3);
const DEFAULT_RETRY_BASE_DELAY_MS = parsePositiveInt(
  process.env.ABP_GEMINI_RETRY_BASE_DELAY_MS,
  1000
);

let cachedGoogleGenAI = null;
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

function parsePositiveInt(raw, fallback) {
  const parsed = Number.parseInt(String(raw || ""), 10);
  if (Number.isFinite(parsed) && parsed > 0) {
    return parsed;
  }
  return fallback;
}

function parseCommandArgs(raw) {
  if (!raw) {
    return [];
  }

  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return parsed.map((v) => String(v));
    }
  } catch (_) {
    return String(raw)
      .trim()
      .split(/\s+/)
      .filter(Boolean);
  }

  return [];
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

function pickArray(obj, keys) {
  const value = pickValue(obj, keys);
  if (Array.isArray(value)) {
    return value;
  }
  return undefined;
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
      "prompt_token_count",
      "promptTokenCount",
    ]),
    output_tokens: pickNumericToken(raw, [
      "output_tokens",
      "outputTokens",
      "completion_tokens",
      "completionTokens",
      "candidate_token_count",
      "candidateTokenCount",
    ]),
    cache_read_tokens: pickNumericToken(raw, [
      "cache_read_tokens",
      "cacheReadTokens",
      "cached_content_token_count",
      "cachedContentTokenCount",
    ]),
    cache_write_tokens: pickNumericToken(raw, ["cache_write_tokens", "cacheWriteTokens"]),
    request_units: pickNumericToken(raw, ["request_units", "requestUnits"]),
    estimated_cost_usd: pickNumericToken(raw, ["estimated_cost_usd", "estimatedCostUsd"]),
  };
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
  const geminiVendor = getVendorNamespace(vendor, "gemini");
  const abpVendor = getVendorNamespace(vendor, "abp");
  const workspaceRoot = String(workOrder.workspace?.root || process.cwd());

  const stream = pickBoolean(geminiVendor, ["stream", "streaming"]);
  const vertex = pickBoolean(geminiVendor, [
    "vertex",
    "vertexai",
    "vertex_ai",
    "use_vertex_ai",
    "useVertexAi",
  ]);
  const model =
    pickString(geminiVendor, ["model"]) ||
    pickString(runtimeConfig, ["model"]) ||
    DEFAULT_MODEL;

  const maxOutputTokens = pickNumber(geminiVendor, [
    "max_output_tokens",
    "maxOutputTokens",
    "token_limit",
    "tokenLimit",
    "max_tokens",
    "maxTokens",
  ]);
  const topP = pickNumber(geminiVendor, ["top_p", "topP"]);
  const topK = pickNumber(geminiVendor, ["top_k", "topK"]);
  const temperature = pickNumber(geminiVendor, ["temperature"]);
  const candidateCount = pickNumber(geminiVendor, ["candidate_count", "candidateCount"]);
  const timeoutMs = pickNumber(geminiVendor, ["timeout_ms", "timeoutMs"]);
  const retryAttempts =
    pickNumber(geminiVendor, ["retry_attempts", "retryAttempts", "retries"]) ||
    DEFAULT_RETRY_ATTEMPTS;
  const retryBaseDelayMs =
    pickNumber(geminiVendor, ["retry_base_delay_ms", "retryBaseDelayMs"]) ||
    DEFAULT_RETRY_BASE_DELAY_MS;

  const safetySettings = pickArray(geminiVendor, ["safety_settings", "safetySettings"]);
  const stopSequences = pickArray(geminiVendor, ["stop_sequences", "stopSequences"]);

  const generationConfig = {};
  if (temperature !== undefined) {
    generationConfig.temperature = temperature;
  }
  if (topP !== undefined) {
    generationConfig.topP = topP;
  }
  if (topK !== undefined) {
    generationConfig.topK = topK;
  }
  if (maxOutputTokens !== undefined) {
    generationConfig.maxOutputTokens = maxOutputTokens;
  }
  if (candidateCount !== undefined) {
    generationConfig.candidateCount = candidateCount;
  }
  if (Array.isArray(stopSequences) && stopSequences.length > 0) {
    generationConfig.stopSequences = stopSequences.map((v) => safeString(v));
  }

  const envOverrides = asObject(runtimeConfig.env);

  return {
    requestId: workOrder.id || null,
    prompt: buildPrompt(workOrder),
    workspaceRoot,
    model,
    lane: workOrder.lane,
    stream: stream !== undefined ? stream : true,
    mode: pickString(abpVendor, ["mode"]) || "mapped",
    project:
      pickString(geminiVendor, ["project", "project_id", "projectId"]) ||
      process.env.GOOGLE_CLOUD_PROJECT,
    location:
      pickString(geminiVendor, ["location", "region"]) ||
      process.env.GOOGLE_CLOUD_LOCATION ||
      process.env.GOOGLE_CLOUD_REGION,
    apiKey:
      pickString(geminiVendor, ["api_key", "apiKey"]) ||
      process.env.GEMINI_API_KEY ||
      process.env.GOOGLE_API_KEY,
    useVertex:
      vertex !== undefined
        ? vertex
        : String(process.env.GOOGLE_GENAI_USE_VERTEXAI || "").toLowerCase() === "true",
    systemInstruction: pickString(geminiVendor, [
      "system_instruction",
      "systemInstruction",
      "instruction",
    ]),
    generationConfig,
    safetySettings: Array.isArray(safetySettings) ? safetySettings : undefined,
    timeoutMs: timeoutMs !== undefined ? timeoutMs : undefined,
    retryAttempts: Math.max(1, Math.floor(retryAttempts)),
    retryBaseDelayMs: Math.max(1, Math.floor(retryBaseDelayMs)),
    env: envOverrides,
    vendor,
  };
}

function commandExists(command) {
  return resolveCommandPath(command) !== null;
}

function resolveCommandPath(command) {
  const candidate = path.resolve(command);
  if (path.isAbsolute(command) || command.includes(path.sep)) {
    return isRunnablePath(candidate) ? candidate : null;
  }

  const pathVar = process.env.PATH;
  if (!pathVar) {
    return null;
  }

  const dirs = pathVar.split(path.delimiter);
  for (const dir of dirs) {
    if (!dir) {
      continue;
    }
    if (process.platform === "win32") {
      for (const ext of [".exe", ".cmd", ".bat", ".com"]) {
        if (isRunnablePath(path.join(dir, `${command}${ext}`))) {
          return path.join(dir, `${command}${ext}`);
        }
      }
      continue;
    }

    if (isRunnablePath(path.join(dir, command))) {
      return path.join(dir, command);
    }
  }

  return null;
}

function isRunnablePath(candidate) {
  try {
    const stat = fs.statSync(candidate);
    return stat.isFile();
  } catch (_) {
    return false;
  }
}

function resolveSdkVersion() {
  const candidates = [
    path.resolve(process.cwd(), "hosts/gemini/node_modules/@google/genai/package.json"),
    path.resolve(__dirname, "node_modules/@google/genai/package.json"),
  ];

  for (const candidate of candidates) {
    if (!fs.existsSync(candidate)) {
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

async function loadGoogleGenAI() {
  if (cachedGoogleGenAI) {
    return cachedGoogleGenAI;
  }
  if (cachedSdkLoadError) {
    throw cachedSdkLoadError;
  }

  try {
    const mod = await import("@google/genai");
    const ctor = mod?.GoogleGenAI || mod?.default?.GoogleGenAI || mod?.default;
    if (!ctor) {
      throw new Error("module '@google/genai' does not export GoogleGenAI");
    }
    cachedGoogleGenAI = ctor;
    cachedSdkVersion = resolveSdkVersion();
    return cachedGoogleGenAI;
  } catch (err) {
    cachedSdkLoadError = new Error(`failed to load @google/genai: ${safeString(err)}`);
    throw cachedSdkLoadError;
  }
}

function createSdkClient(GoogleGenAI, request) {
  const options = {};
  if (request.apiKey) {
    options.apiKey = request.apiKey;
  }
  if (request.useVertex) {
    options.vertexai = true;
    options.vertexAI = true;
  }
  if (request.project) {
    options.project = request.project;
  }
  if (request.location) {
    options.location = request.location;
  }

  return new GoogleGenAI(options);
}

function buildSdkPayload(request) {
  const payload = {
    model: request.model,
    contents: request.prompt,
  };

  if (request.systemInstruction) {
    payload.systemInstruction = request.systemInstruction;
  }
  if (request.safetySettings) {
    payload.safetySettings = request.safetySettings;
  }
  if (request.generationConfig && Object.keys(request.generationConfig).length > 0) {
    payload.config = request.generationConfig;
  }

  return payload;
}

function resolveStreamMethod(models) {
  if (!models || typeof models !== "object") {
    return null;
  }
  if (typeof models.generateContentStream === "function") {
    return "generateContentStream";
  }
  if (typeof models.streamGenerateContent === "function") {
    return "streamGenerateContent";
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
  return null;
}

async function maybeAwait(value) {
  if (value && typeof value.then === "function") {
    return value;
  }
  return value;
}

function extractText(value, depth = 0) {
  if (depth > 4 || value == null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }

  if (typeof value.text === "string") {
    return value.text;
  }
  if (typeof value.text === "function") {
    try {
      const out = value.text();
      if (typeof out === "string") {
        return out;
      }
    } catch (_) {
      // ignore text() errors and continue extraction attempts.
    }
  }

  const candidates = Array.isArray(value.candidates) ? value.candidates : [];
  const parts = candidates[0]?.content?.parts;
  if (Array.isArray(parts)) {
    const joined = parts
      .map((part) => {
        if (typeof part?.text === "string") {
          return part.text;
        }
        return "";
      })
      .filter(Boolean)
      .join("");
    if (joined.length > 0) {
      return joined;
    }
  }

  if (value.response) {
    return extractText(value.response, depth + 1);
  }
  return "";
}

function extractUsage(raw) {
  if (!raw || typeof raw !== "object") {
    return {};
  }
  if (raw.usageMetadata && typeof raw.usageMetadata === "object") {
    return raw.usageMetadata;
  }
  if (raw.usage && typeof raw.usage === "object") {
    return raw.usage;
  }
  if (
    typeof raw.promptTokenCount === "number" ||
    typeof raw.candidateTokenCount === "number" ||
    typeof raw.totalTokenCount === "number"
  ) {
    return raw;
  }
  if (raw.response && typeof raw.response === "object") {
    return extractUsage(raw.response);
  }
  return {};
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

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function runSdkOnce(request, ctx) {
  const GoogleGenAI = await loadGoogleGenAI();
  const ai = createSdkClient(GoogleGenAI, request);
  const payload = buildSdkPayload(request);
  const streamMethod = resolveStreamMethod(ai.models);

  if (request.stream && streamMethod) {
    const streamResult = await ai.models[streamMethod](payload);
    const iterable = toAsyncIterable(streamResult);
    if (!iterable) {
      throw new Error(`Gemini SDK ${streamMethod} did not return an async iterable stream`);
    }

    let fullText = "";
    let usageRaw = {};
    let emittedDeltaCount = 0;

    for await (const chunk of iterable) {
      const delta = extractText(chunk);
      if (delta) {
        emittedDeltaCount += 1;
        fullText += delta;
        ctx.emitAssistantDelta(delta);
      }

      const usage = extractUsage(chunk);
      if (Object.keys(usage).length > 0) {
        usageRaw = usage;
      }
    }

    if (streamResult && streamResult.response) {
      const finalResponse = await maybeAwait(streamResult.response);
      const finalText = extractText(finalResponse);
      if (finalText && fullText.length === 0) {
        fullText = finalText;
      }
      const usage = extractUsage(finalResponse);
      if (Object.keys(usage).length > 0) {
        usageRaw = usage;
      }
    }

    if (fullText.length > 0 && emittedDeltaCount === 0) {
      ctx.emitAssistantMessage(fullText);
    }
    if (fullText.length === 0) {
      ctx.emitWarning("Gemini SDK returned no text content");
    }

    return {
      usageRaw: {
        ...usageRaw,
        sdk_transport: "google_genai_sdk",
        sdk_version: cachedSdkVersion,
      },
      usage: normalizeUsage(usageRaw),
      outcome: "complete",
    };
  }

  if (!ai.models || typeof ai.models.generateContent !== "function") {
    throw new Error("Gemini SDK client does not expose models.generateContent");
  }

  const response = await ai.models.generateContent(payload);
  const text = extractText(response);
  if (text.length > 0) {
    ctx.emitAssistantMessage(text);
  } else {
    ctx.emitWarning("Gemini SDK returned no text content");
  }

  const usageRaw = extractUsage(response);
  return {
    usageRaw: {
      ...usageRaw,
      sdk_transport: "google_genai_sdk",
      sdk_version: cachedSdkVersion,
    },
    usage: normalizeUsage(usageRaw),
    outcome: "complete",
  };
}

async function runSdkWithRetry(request, ctx) {
  const attempts = Math.max(1, request.retryAttempts || DEFAULT_RETRY_ATTEMPTS);
  const baseDelayMs = Math.max(1, request.retryBaseDelayMs || DEFAULT_RETRY_BASE_DELAY_MS);

  let lastError = null;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await runSdkOnce(request, ctx);
    } catch (err) {
      lastError = err;
      const canRetry = attempt < attempts && isRetriableError(err);
      if (!canRetry) {
        break;
      }

      const jitter = Math.floor(Math.random() * 250);
      const delayMs = Math.min(10000, baseDelayMs * 2 ** (attempt - 1) + jitter);
      ctx.emitWarning(
        `Gemini SDK call failed (attempt ${attempt}/${attempts}), retrying in ${delayMs}ms: ${safeString(
          err
        )}`
      );
      await sleep(delayMs);
    }
  }

  throw lastError || new Error("Gemini SDK execution failed");
}

function emitFromParsedMessage(ctx, message, onUsage) {
  if (!message || typeof message !== "object") {
    if (typeof message === "string" && message.length > 0) {
      ctx.emitAssistantDelta(message);
    }
    return;
  }

  const kind = String(message.type || message.kind || "").toLowerCase();
  const text = message.text || message.message || message.delta || message.output || "";

  if (
    kind.includes("assistant_delta") ||
    kind.includes("delta") ||
    kind.includes("content_delta") ||
    kind.includes("stream_chunk")
  ) {
    ctx.emitAssistantDelta(text ? String(text) : "");
    return;
  }

  if (kind.includes("assistant_message") || kind === "assistant" || kind === "message") {
    ctx.emitAssistantMessage(String(text || ""));
    return;
  }

  if (kind.includes("tool_call") || kind.includes("toolcall") || kind.includes("tool-use")) {
    const toolName = String(
      message.tool_name || message.toolName || message.name || message.tool || "gemini_tool"
    );
    ctx.emitToolCall({
      toolName,
      toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
      parentToolUseId: message.parent_tool_use_id || message.parentToolUseId || null,
      input: message.input || message.arguments || message.params || {},
    });
    return;
  }

  if (kind.includes("tool_result") || kind.includes("toolresult")) {
    const toolName = String(
      message.tool_name || message.toolName || message.name || message.tool || "gemini_tool"
    );
    ctx.emitToolResult({
      toolName,
      toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
      output: message.output || message.result || message.value || "",
      isError: !!(message.is_error || message.isError || message.error),
    });
    return;
  }

  if (kind.includes("warning")) {
    ctx.emitWarning(String(text || "gemini warning"));
    return;
  }

  if (kind.includes("error")) {
    ctx.emitError(String(message.error || message.message || text || "gemini error"));
    return;
  }

  if (kind.includes("usage")) {
    if (message.usage && typeof message.usage === "object") {
      onUsage(message.usage);
      return;
    }
    onUsage(message);
    return;
  }

  ctx.emitWarning(`unhandled payload from Gemini command: ${safeString(message)}`);
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

function runFromCommand(command, args, request, ctx, inputMode) {
  return new Promise((resolve, reject) => {
    let usageRaw = {};
    const child = spawn(command, args, {
      cwd: request.workspaceRoot || process.cwd(),
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
      if (!line) {
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
        usageRaw = {
          ...usageRaw,
          ...asObject(nextUsage),
        };
      });
    });

    child.stderr.on("data", (chunk) => {
      ctx.emitWarning(`[gemini stderr] ${String(chunk)}`);
    });

    child.on("error", (err) => {
      reject(new Error(`failed to start Gemini command '${command}': ${safeString(err)}`));
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

function resolveCliCommand() {
  const hasRunner = typeof RUNNER_CMD === "string" && RUNNER_CMD.trim().length > 0;
  if (hasRunner) {
    return {
      command: RUNNER_CMD.trim(),
      args: RUNNER_ARGS,
      inputMode: "json-stdin",
    };
  }

  if (!commandExists(DEFAULT_CMD)) {
    return null;
  }

  const resolvedCommand = resolveCommandPath(DEFAULT_CMD) || DEFAULT_CMD;

  const args =
    DEFAULT_CMD_ARGS.length > 0
      ? DEFAULT_CMD_ARGS
      : ["--output-format", "stream-json", "--model", "{model}", "{prompt}"];

  return {
    command: resolvedCommand,
    args,
    inputMode: CLI_INPUT_MODE === "json-stdin" ? "json-stdin" : "prompt-arg",
  };
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
  return runFromCommand(resolved.command, args, request, ctx, resolved.inputMode);
}

function fallbackResult(ctx, request) {
  ctx.emitAssistantMessage("Gemini adapter fallback mode.");
  ctx.emitAssistantMessage("No Gemini SDK or CLI runner is available.");
  ctx.emitAssistantMessage(`Task: ${safeString(request.prompt || ctx?.workOrder?.task || "")}`);

  return {
    usageRaw: {
      mode: "gemini_fallback",
      note:
        "Install @google/genai for SDK mode, or configure ABP_GEMINI_RUNNER / ABP_GEMINI_CMD for CLI mode",
    },
    usage: {
      input_tokens: 0,
      output_tokens: 0,
    },
    outcome: "Partial",
  };
}

function shouldUseSdkFirst() {
  if (TRANSPORT === "sdk") {
    return true;
  }
  if (TRANSPORT === "cli") {
    return false;
  }
  // auto
  return true;
}

async function run(ctx) {
  const request = buildRequest(ctx);
  const sdkFirst = shouldUseSdkFirst();

  if (sdkFirst) {
    try {
      return await runSdkWithRetry(request, ctx);
    } catch (err) {
      if (TRANSPORT === "sdk") {
        ctx.emitError(`Gemini SDK execution failed: ${safeString(err)}`);
        return {
          usageRaw: { error: safeString(err), transport: "sdk" },
          usage: {},
          outcome: "failed",
        };
      }
      ctx.emitWarning(`Gemini SDK unavailable, falling back to CLI: ${safeString(err)}`);
    }
  }

  try {
    const cliResult = await runViaCli(request, ctx);
    if (cliResult) {
      return {
        ...cliResult,
        usageRaw: {
          ...asObject(cliResult.usageRaw),
          sdk_transport: "gemini_cli",
        },
      };
    }
  } catch (err) {
    ctx.emitError(`gemini CLI execution failed: ${safeString(err)}`);
    return {
      usageRaw: { error: safeString(err), transport: "cli" },
      usage: {},
      outcome: "failed",
    };
  }

  if (!sdkFirst) {
    try {
      return await runSdkWithRetry(request, ctx);
    } catch (err) {
      ctx.emitError(`Gemini SDK execution failed: ${safeString(err)}`);
      return {
        usageRaw: { error: safeString(err), transport: "sdk" },
        usage: {},
        outcome: "failed",
      };
    }
  }

  return fallbackResult(ctx, request);
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
};
