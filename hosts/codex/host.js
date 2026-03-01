#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");

const CONTRACT_VERSION = "abp/v0.1";
const ADAPTER_VERSION = "0.2.0";
const BACKEND_ID = "codex";
const DEFAULT_MODE = "mapped";

const capabilities = {
  streaming: "native",
  tool_read: "native",
  tool_write: "native",
  tool_edit: "native",
  tool_bash: "native",
  tool_glob: "native",
  tool_grep: "native",
  tool_web_search: "native",
  session_resume: "native",
  structured_output_json_schema: "native",
  mcp_client: "native",
};

let cachedCodexClass = null;
let cachedCodexError = null;

function nowIso() {
  return new Date().toISOString();
}

function write(obj) {
  process.stdout.write(`${JSON.stringify(obj)}\n`);
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
  if (typeof value === "string") {
    try {
      const parsed = JSON.parse(value);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed;
      }
    } catch (_) {
      return undefined;
    }
  }
  return undefined;
}

function getVendorNamespace(workOrder, namespace) {
  const vendor = asObject(workOrder?.config?.vendor);
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

function getExecutionMode(workOrder) {
  const abp = getVendorNamespace(workOrder, "abp");
  const mode = pickString(abp, ["mode"]);
  if (mode === "passthrough") {
    return "passthrough";
  }
  return DEFAULT_MODE;
}

function getPassthroughRequest(workOrder) {
  const abp = getVendorNamespace(workOrder, "abp");
  if (Object.prototype.hasOwnProperty.call(abp, "request")) {
    return abp.request;
  }
  return null;
}

function promptInputForWorkOrder(workOrder) {
  const mode = getExecutionMode(workOrder);
  const rawRequest = mode === "passthrough" ? getPassthroughRequest(workOrder) : null;
  if (rawRequest == null) {
    return buildPrompt(workOrder);
  }

  const text =
    typeof rawRequest === "string"
      ? rawRequest
      : rawRequest && typeof rawRequest.prompt === "string"
        ? rawRequest.prompt
        : "";
  return text && text.trim().length > 0 ? text : safeString(rawRequest);
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

function normalizeUsage(usageRaw) {
  const usage = asObject(usageRaw);
  const inputTokens = pickNumber(usage, ["input_tokens", "inputTokens"]);
  const outputTokens = pickNumber(usage, ["output_tokens", "outputTokens"]);
  const cacheReadTokens = pickNumber(usage, [
    "cached_input_tokens",
    "cache_read_tokens",
    "cacheReadTokens",
  ]);

  const out = {};
  if (inputTokens !== undefined) {
    out.input_tokens = inputTokens;
  }
  if (outputTokens !== undefined) {
    out.output_tokens = outputTokens;
  }
  if (cacheReadTokens !== undefined) {
    out.cache_read_tokens = cacheReadTokens;
  }
  return out;
}

function addArtifact(artifacts, kind, artifactPath) {
  if (!artifactPath || typeof artifactPath !== "string") {
    return;
  }
  artifacts.push({
    kind,
    path: artifactPath.replace(/\\/g, "/"),
  });
}


function makeEmitter(runId, trace) {
  return function emit(kind, extRawMessage) {
    const event = {
      ts: nowIso(),
      ...kind,
    };
    if (extRawMessage !== undefined) {
      event.ext = {
        raw_message: extRawMessage,
      };
    }
    trace.push(event);
    write({
      t: "event",
      ref_id: runId,
      event,
    });
  };
}

function isRetriableError(err) {
  const text = safeString(err).toLowerCase();
  return (
    text.includes("rate limit") ||
    text.includes("timeout") ||
    text.includes("timed out") ||
    text.includes("temporar") ||
    text.includes("econnreset") ||
    text.includes("eai_again") ||
    text.includes("503")
  );
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function resolveSdkVersion() {
  const candidates = [
    path.resolve(process.cwd(), "hosts/codex/node_modules/@openai/codex-sdk/package.json"),
    path.resolve(__dirname, "node_modules/@openai/codex-sdk/package.json"),
  ];

  for (const candidate of candidates) {
    if (!fs.existsSync(candidate)) {
      continue;
    }
    try {
      const parsed = JSON.parse(fs.readFileSync(candidate, "utf8"));
      if (typeof parsed.version === "string" && parsed.version.length > 0) {
        return parsed.version;
      }
    } catch (_) {
      return null;
    }
  }

  return null;
}

async function loadCodexClass() {
  if (cachedCodexClass) {
    return cachedCodexClass;
  }
  if (cachedCodexError) {
    throw cachedCodexError;
  }

  try {
    const mod = await import("@openai/codex-sdk");
    const codexClass =
      mod?.Codex ||
      mod?.default?.Codex ||
      mod?.default;
    if (!codexClass) {
      throw new Error("module '@openai/codex-sdk' does not export Codex");
    }
    cachedCodexClass = codexClass;
    return codexClass;
  } catch (err) {
    cachedCodexError = new Error(
      `failed to load @openai/codex-sdk: ${safeString(err)}`
    );
    throw cachedCodexError;
  }
}

function createCodexClient(Codex, workOrder, codexCfg) {
  const envOverrides = {};
  const rawEnvOverrides = {
    ...asObject(workOrder?.config?.env),
    ...asObject(codexCfg.env),
  };
  for (const [key, value] of Object.entries(rawEnvOverrides)) {
    if (value == null) {
      continue;
    }
    envOverrides[key] = String(value);
  }
  const mergedEnv =
    Object.keys(envOverrides).length > 0
      ? { ...process.env, ...envOverrides }
      : undefined;

  const apiKey =
    pickString(codexCfg, ["apiKey", "api_key"]) ||
    process.env.CODEX_API_KEY ||
    process.env.OPENAI_API_KEY;

  const baseUrl =
    pickString(codexCfg, ["baseUrl", "base_url"]) ||
    process.env.OPENAI_BASE_URL;

  const config =
    pickObject(codexCfg, ["config"]) ||
    undefined;

  const codexPathOverride = pickString(codexCfg, [
    "codexPathOverride",
    "codex_path_override",
  ]);

  const options = {};
  if (apiKey) {
    options.apiKey = apiKey;
  }
  if (baseUrl) {
    options.baseUrl = baseUrl;
  }
  if (config) {
    options.config = config;
  }
  if (mergedEnv) {
    options.env = mergedEnv;
  }
  if (codexPathOverride) {
    options.codexPathOverride = codexPathOverride;
  }

  return new Codex(options);
}

function buildThreadOptions(workOrder, codexCfg) {
  const options = {};
  const workspaceRoot = pickString(asObject(workOrder?.workspace), ["root"]);
  if (workspaceRoot) {
    options.workingDirectory = workspaceRoot;
  }

  const model =
    pickString(codexCfg, ["model"]) ||
    pickString(asObject(workOrder?.config), ["model"]);
  if (model) {
    options.model = model;
  }

  const sandboxMode = pickString(codexCfg, ["sandboxMode", "sandbox_mode"]);
  if (sandboxMode) {
    options.sandboxMode = sandboxMode;
  }

  const skipGitRepoCheck = pickBoolean(codexCfg, [
    "skipGitRepoCheck",
    "skip_git_repo_check",
  ]);
  if (skipGitRepoCheck !== undefined) {
    options.skipGitRepoCheck = skipGitRepoCheck;
  }

  const webSearchMode = pickString(codexCfg, ["webSearchMode", "web_search_mode"]);
  if (webSearchMode) {
    options.webSearchMode = webSearchMode;
  }

  const webSearchEnabled = pickBoolean(codexCfg, [
    "webSearchEnabled",
    "web_search_enabled",
  ]);
  if (webSearchEnabled !== undefined) {
    options.webSearchEnabled = webSearchEnabled;
  }

  const approvalPolicy = pickString(codexCfg, ["approvalPolicy", "approval_policy"]);
  if (approvalPolicy) {
    options.approvalPolicy = approvalPolicy;
  }

  const additionalDirectories = pickArray(codexCfg, [
    "additionalDirectories",
    "additional_directories",
  ]);
  if (additionalDirectories) {
    options.additionalDirectories = additionalDirectories.map((v) => safeString(v));
  }

  return options;
}

function buildTurnOptions(workOrder, codexCfg) {
  const options = {};
  const model =
    pickString(codexCfg, ["model"]) ||
    pickString(asObject(workOrder?.config), ["model"]);
  if (model) {
    options.model = model;
  }

  const outputSchema = pickObject(codexCfg, ["outputSchema", "output_schema"]);
  if (outputSchema) {
    options.outputSchema = outputSchema;
  }

  const modelReasoningEffort = pickString(codexCfg, [
    "modelReasoningEffort",
    "model_reasoning_effort",
  ]);
  if (modelReasoningEffort) {
    options.modelReasoningEffort = modelReasoningEffort;
  }

  return options;
}

function emitItemStarted(item, emit, state) {
  if (!item || typeof item !== "object") {
    return;
  }

  if (item.type === "command_execution") {
    const toolUseId = item.id || null;
    state.openToolCalls.add(toolUseId || `bash-${state.openToolCalls.size}`);
    emit({
      type: "tool_call",
      tool_name: "Bash",
      tool_use_id: toolUseId,
      parent_tool_use_id: null,
      input: {
        command: safeString(item.command || ""),
      },
    });
    return;
  }

  if (item.type === "mcp_tool_call") {
    const toolUseId = item.id || null;
    state.openToolCalls.add(toolUseId || `mcp-${state.openToolCalls.size}`);
    emit({
      type: "tool_call",
      tool_name: `mcp:${safeString(item.server)}.${safeString(item.tool)}`,
      tool_use_id: toolUseId,
      parent_tool_use_id: null,
      input: asObject(item.arguments),
    });
    return;
  }

  if (item.type === "web_search") {
    const toolUseId = item.id || null;
    state.openToolCalls.add(toolUseId || `web-${state.openToolCalls.size}`);
    emit({
      type: "tool_call",
      tool_name: "WebSearch",
      tool_use_id: toolUseId,
      parent_tool_use_id: null,
      input: {
        query: safeString(item.query || ""),
      },
    });
  }
}

function emitItemUpdated(item, emit, state) {
  if (!item || typeof item !== "object") {
    return;
  }

  if (item.type === "agent_message") {
    const itemId = item.id || "agent";
    const text = safeString(item.text || "");
    const previous = state.agentTextByItem.get(itemId) || "";
    const delta = text.startsWith(previous) ? text.slice(previous.length) : text;
    state.agentTextByItem.set(itemId, text);
    if (delta.length > 0) {
      emit(
        {
          type: "assistant_delta",
          text: delta,
        },
        item
      );
    }
  }
}

function emitItemCompleted(item, emit, state, artifacts) {
  if (!item || typeof item !== "object") {
    return;
  }

  if (item.type === "agent_message") {
    const itemId = item.id || "agent";
    const finalText = safeString(item.text || state.agentTextByItem.get(itemId) || "");
    state.agentTextByItem.delete(itemId);
    if (finalText.length > 0) {
      emit(
        {
          type: "assistant_message",
          text: finalText,
        },
        item
      );
    }
    return;
  }

  if (item.type === "command_execution") {
    const toolUseId = item.id || null;
    const command = safeString(item.command || "");
    const output = safeString(item.aggregated_output || "");
    const exitCode =
      typeof item.exit_code === "number" && Number.isFinite(item.exit_code)
        ? item.exit_code
        : null;
    const isError = item.status === "failed" || (typeof exitCode === "number" && exitCode !== 0);

    emit(
      {
        type: "command_executed",
        command,
        exit_code: exitCode,
        output_preview: output.length > 2048 ? output.slice(0, 2048) : output,
      },
      item
    );

    emit(
      {
        type: "tool_result",
        tool_name: "Bash",
        tool_use_id: toolUseId,
        output: output,
        is_error: isError,
      },
      item
    );
    return;
  }

  if (item.type === "file_change") {
    const changes = Array.isArray(item.changes) ? item.changes : [];
    if (changes.length === 0) {
      emit(
        {
          type: "file_changed",
          path: ".",
          summary: safeString(item.status || "file_change"),
        },
        item
      );
      return;
    }

    for (const change of changes) {
      const changedPath = safeString(change?.path || ".");
      const summary = safeString(change?.kind || item.status || "updated");
      emit(
        {
          type: "file_changed",
          path: changedPath,
          summary,
        },
        change
      );
      addArtifact(artifacts, "file_change", changedPath);
    }
    return;
  }

  if (item.type === "mcp_tool_call") {
    const toolUseId = item.id || null;
    const output =
      item.error != null
        ? { error: safeString(item.error) }
        : item.result != null
          ? item.result
          : {};
    emit(
      {
        type: "tool_result",
        tool_name: `mcp:${safeString(item.server)}.${safeString(item.tool)}`,
        tool_use_id: toolUseId,
        output,
        is_error: item.error != null || item.status === "failed",
      },
      item
    );
    return;
  }

  if (item.type === "web_search") {
    const toolUseId = item.id || null;
    emit(
      {
        type: "tool_result",
        tool_name: "WebSearch",
        tool_use_id: toolUseId,
        output: {
          query: safeString(item.query || ""),
          status: safeString(item.status || "completed"),
        },
        is_error: item.status === "failed",
      },
      item
    );
    return;
  }

  if (item.type === "error") {
    emit(
      {
        type: "error",
        message: safeString(item.message || "codex item error"),
      },
      item
    );
  }
}

function handleSdkEvent(sdkEvent, emit, state, artifacts) {
  if (!sdkEvent || typeof sdkEvent !== "object") {
    return;
  }

  if (sdkEvent.type === "thread.started") {
    if (typeof sdkEvent.thread_id === "string") {
      state.threadId = sdkEvent.thread_id;
    }
    return;
  }

  if (sdkEvent.type === "turn.completed") {
    state.usageRaw = asObject(sdkEvent.usage);
    return;
  }

  if (sdkEvent.type === "turn.failed") {
    const message = safeString(sdkEvent.error || "turn failed");
    state.turnFailedMessage = message;
    emit(
      {
        type: "error",
        message,
      },
      sdkEvent
    );
    return;
  }

  if (sdkEvent.type === "error") {
    emit(
      {
        type: "error",
        message: safeString(sdkEvent.error || sdkEvent.message || "codex error"),
      },
      sdkEvent
    );
    return;
  }

  if (sdkEvent.type === "item.started") {
    emitItemStarted(sdkEvent.item, emit, state);
    return;
  }

  if (sdkEvent.type === "item.updated") {
    emitItemUpdated(sdkEvent.item, emit, state);
    return;
  }

  if (sdkEvent.type === "item.completed") {
    emitItemCompleted(sdkEvent.item, emit, state, artifacts);
  }
}

async function executeTurn(codex, workOrder, codexCfg, emit, artifacts) {
  const threadOptions = buildThreadOptions(workOrder, codexCfg);
  const turnOptions = buildTurnOptions(workOrder, codexCfg);

  const threadId = pickString(codexCfg, ["threadId", "thread_id"]);
  const resumeFlag = pickBoolean(codexCfg, ["resume"]) === true;
  const shouldResume = !!threadId && (resumeFlag || threadId.length > 0);

  let thread;
  if (shouldResume) {
    thread = await codex.resumeThread(threadId, threadOptions);
  } else {
    if (resumeFlag && !threadId) {
      emit({
        type: "warning",
        message: "vendor.codex.resume=true was set without vendor.codex.threadId; starting a new thread",
      });
    }
    thread = codex.startThread(threadOptions);
  }

  const timeoutMs = pickNumber(codexCfg, ["timeoutMs", "timeout_ms"]);
  let timeoutHandle = null;
  let controller = null;
  if (timeoutMs !== undefined && timeoutMs > 0) {
    controller = new AbortController();
    timeoutHandle = setTimeout(() => {
      controller.abort();
    }, timeoutMs);
    turnOptions.signal = controller.signal;
  }

  const state = {
    threadId: typeof thread.threadId === "function" ? thread.threadId() : undefined,
    usageRaw: {},
    turnFailedMessage: null,
    agentTextByItem: new Map(),
    openToolCalls: new Set(),
  };

  const promptInput = promptInputForWorkOrder(workOrder);
  let streamed;
  try {
    streamed = await thread.runStreamed(promptInput, turnOptions);
    for await (const event of streamed.events) {
      handleSdkEvent(event, emit, state, artifacts);
    }
  } finally {
    if (timeoutHandle) {
      clearTimeout(timeoutHandle);
    }
  }

  if (state.turnFailedMessage) {
    throw new Error(state.turnFailedMessage);
  }

  const usageRaw = {
    ...asObject(state.usageRaw),
  };
  if (streamed.requestId) {
    usageRaw.request_id = streamed.requestId;
  }
  if (state.threadId) {
    usageRaw.thread_id = state.threadId;
  }

  return {
    usageRaw,
  };
}

async function executeWithRetry(codex, workOrder, codexCfg, emit, artifacts) {
  const retries = pickNumber(codexCfg, ["retryCount", "retry_count", "retries"]);
  const maxAttempts = Math.max(1, 1 + (retries !== undefined ? Math.floor(retries) : 1));
  let lastError = null;

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    try {
      return await executeTurn(codex, workOrder, codexCfg, emit, artifacts);
    } catch (err) {
      lastError = err;
      const canRetry = attempt < maxAttempts && isRetriableError(err);
      if (!canRetry) {
        break;
      }

      const delayMs = Math.min(5000, attempt * 1000);
      emit({
        type: "warning",
        message: `Codex turn failed (attempt ${attempt}/${maxAttempts}); retrying in ${delayMs}ms: ${safeString(err)}`,
      });
      await sleep(delayMs);
    }
  }

  throw lastError || new Error("codex turn failed");
}

async function runWorkOrder(envelope, backendInfo) {
  const runId =
    typeof envelope.id === "string" && envelope.id.length > 0
      ? envelope.id
      : crypto.randomUUID();
  const workOrder = asObject(envelope.work_order);
  const mode = getExecutionMode(workOrder);

  const startedAt = nowIso();
  const trace = [];
  const artifacts = [];
  const emit = makeEmitter(runId, trace);

  let usageRaw = {};
  let usage = {};
  let outcome = "complete";

  emit({
    type: "run_started",
    message: `codex run starting: ${safeString(workOrder.task || "")}`,
  });

  try {
    const Codex = await loadCodexClass();
    const codexCfg = getVendorNamespace(workOrder, "codex");

    const workspaceRoot = pickString(asObject(workOrder.workspace), ["root"]);
    if (workspaceRoot) {
      try {
        process.chdir(workspaceRoot);
      } catch (err) {
        emit({
          type: "warning",
          message: `unable to change cwd to workspace root '${workspaceRoot}': ${safeString(err)}`,
        });
      }
    }

    const codex = createCodexClient(Codex, workOrder, codexCfg);
    const runResult = await executeWithRetry(codex, workOrder, codexCfg, emit, artifacts);
    usageRaw = asObject(runResult.usageRaw);
    usage = normalizeUsage(usageRaw);
  } catch (err) {
    outcome = "failed";
    emit({
      type: "error",
      message: safeString(err),
    });
  }

  emit({
    type: "run_completed",
    message:
      outcome === "complete"
        ? "codex run completed"
        : `codex run failed: ${safeString(trace[trace.length - 1]?.message || "")}`,
  });

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
    backend: backendInfo,
    capabilities,
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

  write({
    t: "final",
    ref_id: runId,
    receipt,
  });
}

async function main() {
  const backend = {
    id: BACKEND_ID,
    backend_version: resolveSdkVersion(),
    adapter_version: ADAPTER_VERSION,
  };

  write({
    t: "hello",
    contract_version: CONTRACT_VERSION,
    backend,
    capabilities,
    mode: DEFAULT_MODE,
  });

  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  for await (const line of rl) {
    if (!line || !line.trim()) {
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

    await runWorkOrder(envelope, backend);
  }
}

main().catch((err) => {
  write({
    t: "fatal",
    ref_id: null,
    error: `codex host failed: ${safeString(err)}`,
  });
  process.exitCode = 1;
});
