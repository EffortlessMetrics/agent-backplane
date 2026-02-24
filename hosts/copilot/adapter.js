const { spawn } = require("node:child_process");
const readline = require("node:readline");

const ADAPTER_NAME = "copilot_default_adapter";
const ADAPTER_VERSION = "0.1.0";

const DEFAULT_CMD = process.env.ABP_COPILOT_CMD || "copilot";
const DEFAULT_CMD_ARGS = parseCommandArgs(process.env.ABP_COPILOT_ARGS);
const RUNNER_CMD = process.env.ABP_COPILOT_RUNNER || "";

function parseCommandArgs(raw) {
  if (!raw) {
    return [];
  }
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return parsed.map(String);
    }
  } catch (_) {
    // Keep simple, space separated fallback for convenience.
    return String(raw)
      .trim()
      .split(/\s+/)
      .filter(Boolean);
  }
  return [];
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

function buildRequest(ctx) {
  const workOrder = ctx.workOrder || {};
  const vendor = (workOrder.config && workOrder.config.vendor) || {};
  const copilotVendor = vendor.copilot || {};
  const abpVendor = vendor.abp || {};
  const workspaceRoot = (workOrder.workspace && workOrder.workspace.root) || process.cwd();
  const task = workOrder.task || "";

  return {
    request_id: workOrder.id,
    prompt: task,
    workspace_root: workspaceRoot,
    model: workOrder.config && workOrder.config.model ? workOrder.config.model : copilotVendor.model,
    reasoning_effort: copilotVendor.reasoningEffort,
    system_message: copilotVendor.systemMessage,
    lane: workOrder.lane,
    context: {
      files: workOrder.context && Array.isArray(workOrder.context.files) ? workOrder.context.files : [],
      snippets: workOrder.context && Array.isArray(workOrder.context.snippets) ? workOrder.context.snippets : [],
    },
    policy: workOrder.policy || {},
    tools: {
      available: Array.isArray(copilotVendor.availableTools)
        ? copilotVendor.availableTools
        : undefined,
      excluded: Array.isArray(copilotVendor.excludedTools)
        ? copilotVendor.excludedTools
        : undefined,
      ask_user: true,
    },
    mcp_servers: copilotVendor.mcpServers || copilotVendor.mcp_servers || {},
    streaming: true,
    mode: abpVendor.mode || "mapped",
    raw_request: abpVendor.request || null,
    env: workOrder.config && workOrder.config.env ? workOrder.config.env : {},
  };
}

function emitFromParsedMessage(ctx, message) {
  if (!message || typeof message !== "object") {
    if (typeof message === "string" && message.length > 0) {
      ctx.emitAssistantDelta(message);
    }
    return null;
  }

  const kind = String(message.type || message.kind || "").toLowerCase();
  const text = message.text || message.message || message.delta || message.output || "";

  if (kind.includes("assistant_delta") || kind.includes("delta")) {
    ctx.emitAssistantDelta(String(text));
    return;
  }
  if (kind.includes("assistant_message") || kind.includes("assistant")) {
    ctx.emitAssistantMessage(String(text || ""));
    return;
  }
  if (kind.includes("tool_call") || kind.includes("toolcall") || kind.includes("tool-use")) {
    ctx.emitToolCall({
      toolName: String(message.tool_name || message.toolName || message.tool || "copilot_tool"),
      toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
      parentToolUseId: message.parent_tool_use_id || message.parentToolUseId || null,
      input: message.input || message.arguments || {},
    });
    return;
  }
  if (kind.includes("tool_result") || kind.includes("toolresult")) {
    ctx.emitToolResult({
      toolName: String(message.tool_name || message.toolName || message.tool || "copilot_tool"),
      toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
      output: message.output || message.result || "",
      isError: !!(message.is_error || message.isError || message.error),
    });
    return;
  }
  if (kind.includes("warning")) {
    ctx.emitWarning(String(text || "warning"));
    return;
  }
  if (kind.includes("error")) {
    ctx.emitError(String(message.error || message.message || text || "copilot error"));
    return;
  }
}

function normalizeUsage(raw) {
  if (!raw || typeof raw !== "object") {
    return {};
  }
  const pick = (candidates) => {
    for (const key of candidates) {
      if (Number.isFinite(raw[key])) {
        return raw[key];
      }
    }
    const camel = keyCandidatesToCamel(raw);
    for (const key of candidates) {
      const camelKey = toCamel(key);
      if (Number.isFinite(camel[camelKey])) {
        return camel[camelKey];
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

function keyCandidatesToCamel(raw) {
  const out = {};
  for (const [k, v] of Object.entries(raw)) {
    out[toCamel(k)] = v;
    out[k] = v;
  }
  return out;
}

function toCamel(value) {
  return String(value).replace(/_([a-z])/g, (_, ch) => ch.toUpperCase());
}

function runFromCommand(command, args, request, ctx) {
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

    let usageRaw = {};
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
      if (parsed && parsed.type && parsed.type.toLowerCase() === "usage") {
        usageRaw = parsed.usage || parsed;
      }
      emitFromParsedMessage(ctx, parsed);
    });

    const errBuf = [];
    child.stderr.on("data", (chunk) => {
      const text = String(chunk);
      errBuf.push(text);
      ctx.emitWarning(`[copilot stderr] ${text}`);
    });

    child.on("error", (err) => {
      reject(new Error(`failed to start copilot command '${command}': ${safeString(err)}`));
    });

    child.stdin.end(JSON.stringify(request) + "\n", "utf8");

    child.on("close", (code) => {
      if (code !== 0 && errBuf.length > 0) {
        ctx.emitError(errBuf.join("\n"));
      }
      const normalized = normalizeUsage(usageRaw || {});
      const outcome = code === 0 ? "complete" : "failed";
      resolve({
        usageRaw: usageRaw || {},
        usage: normalized,
        outcome,
      });
    });
  });
}

function fallbackResult(ctx) {
  const task = ctx?.workOrder?.task || "";
  ctx.emitAssistantMessage("GitHub Copilot adapter fallback mode.");
  ctx.emitAssistantMessage("No external Copilot runner is configured.");
  ctx.emitAssistantMessage(`Task: ${safeString(task)}`);

  return {
    usageRaw: {
      mode: "copilot_adapter_fallback",
      note: "Configure ABP_COPILOT_RUNNER or ABP_COPILOT_CMD for real execution",
    },
    usage: {
      input_tokens: 0,
      output_tokens: 0,
    },
    outcome: "partial",
  };
}

async function run(ctx) {
  const request = buildRequest(ctx);
  const hasRunner = typeof RUNNER_CMD === "string" && RUNNER_CMD.trim();
  const hasExplicitCmd = Object.prototype.hasOwnProperty.call(
    process.env,
    "ABP_COPILOT_CMD"
  );
  const hasCmd = hasExplicitCmd && typeof DEFAULT_CMD === "string" && DEFAULT_CMD.trim();

  if (!hasRunner && !hasCmd) {
    return fallbackResult(ctx);
  }

  const command = hasRunner ? RUNNER_CMD : DEFAULT_CMD;
  const args = hasRunner ? parseCommandArgs(process.env.ABP_COPILOT_RUNNER_ARGS) : DEFAULT_CMD_ARGS;
  const finalArgs = command === DEFAULT_CMD && args.length === 0 ? [] : args;

  try {
    return await runFromCommand(command, finalArgs, request, ctx);
  } catch (err) {
    ctx.emitError(`copilot adapter execution failed: ${safeString(err)}`);
    return {
      usageRaw: {
        error: safeString(err),
        command,
        args: finalArgs,
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
