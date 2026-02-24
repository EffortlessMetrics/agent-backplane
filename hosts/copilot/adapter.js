const { spawn } = require("node:child_process");
const net = require("node:net");
const readline = require("node:readline");

const ADAPTER_NAME = "copilot_acp_adapter";
const ADAPTER_VERSION = "0.1.0";

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
  const vendor = (workOrder.config && workOrder.config.vendor) || {};
  const copilotVendor = vendor.copilot || {};
  const abpVendor = vendor.abp || {};
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

function isAcpMode() {
  return MODE !== "legacy";
}

function shouldUseLegacyFallback() {
  return MODE === "legacy";
}

function resolveLegacyRunner() {
  if (RUNNER_CMD && RUNNER_CMD.trim()) {
    return {
      command: RUNNER_CMD,
      args: RUNNER_ARGS,
    };
  }

  if (process.env.ABP_COPILOT_CMD || DEFAULT_COPILOT_ARGS.length > 0) {
    return {
      command: DEFAULT_COPILOT_CMD,
      args: DEFAULT_COPILOT_ARGS,
    };
  }

  return {
    command: DEFAULT_COPILOT_CMD,
    args: [],
  };
}

async function runFromCommand(command, args, request, ctx) {
  return new Promise((resolve) => {
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
    });

    child.on("close", (code) => {
      resolve({
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
  const command = DEFAULT_COPILOT_CMD;
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

  if (request.mode === "passthrough" && request.raw_request) {
    const passthrough = {
      request_id: request.request_id,
      prompt: request.raw_request,
      workspace_root: request.workspace_root,
      model: request.model,
      env: request.env,
    };
    if (isAcpMode() && !shouldUseLegacyFallback()) {
      try {
        return runAcp(request, ctx);
      } catch (err) {
        ctx.emitWarning(`ACP passthrough failed, falling back to legacy runner: ${safeString(err)}`);
        return runFromCommand(...[resolveLegacyRunner().command, resolveLegacyRunner().args, passthrough, ctx]);
      }
    }
    return runFromCommand(...[resolveLegacyRunner().command, resolveLegacyRunner().args, passthrough, ctx]);
  }

  if (!isAcpMode() || shouldUseLegacyFallback()) {
    return runFromCommand(
      resolveLegacyRunner().command,
      resolveLegacyRunner().args,
      request,
      ctx
    );
  }

  try {
    return await runAcp(request, ctx);
  } catch (err) {
    ctx.emitWarning(`ACP mode failed, falling back to legacy command: ${safeString(err)}`);
    if (shouldUseLegacyFallback()) {
      return runFromLegacy(request, ctx);
    }
    const fallback = resolveLegacyRunner();
    if (fallback) {
      return runFromCommand(fallback.command, fallback.args, request, ctx);
    }
    return {
      usageRaw: {
        mode: "copilot_adapter_fallback",
        error: safeString(err),
      },
      usage: {},
      outcome: "failed",
    };
  }
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
};
