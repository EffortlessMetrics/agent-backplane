const { spawn } = require("node:child_process");
const readline = require("node:readline");

const ADAPTER_NAME = "kimi_default_adapter";
const ADAPTER_VERSION = "0.1.0";

const DEFAULT_CMD = process.env.ABP_KIMI_CMD || "kimi";
const DEFAULT_CMD_ARGS = parseCommandArgs(process.env.ABP_KIMI_ARGS);
const RUNNER_CMD = process.env.ABP_KIMI_RUNNER || "";

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
  const kimiVendor = vendor.kimi || {};
  const abpVendor = vendor.abp || {};
  const workspaceRoot = (workOrder.workspace && workOrder.workspace.root) || process.cwd();

  return {
    request_id: workOrder.id,
    prompt: workOrder.task || "",
    workspace_root: workspaceRoot,
    model: workOrder.config && workOrder.config.model ? workOrder.config.model : kimiVendor.model,
    temperature: kimiVendor.temperature,
    top_p: kimiVendor.topP,
    thinking_mode: kimiVendor.thinkingMode,
    reasoning_effort: kimiVendor.reasoningEffort,
    agent_mode: kimiVendor.agentMode,
    agent_swarm: kimiVendor.agentSwarm,
    lane: workOrder.lane,
    context: {
      files: Array.isArray(workOrder.context && workOrder.context.files)
        ? workOrder.context.files
        : [],
      snippets: Array.isArray(workOrder.context && workOrder.context.snippets)
        ? workOrder.context.snippets
        : [],
    },
    policy: workOrder.policy || {},
    tool_policy: { yolo: !!(kimiVendor.yolo || abpVendor.mode === "yolo") },
    streaming: true,
    mode: abpVendor.mode || "mapped",
    raw_request: abpVendor.request || null,
    max_budget_usd: workOrder.config ? workOrder.config.max_budget_usd : null,
    max_turns: workOrder.config ? workOrder.config.max_turns : null,
    env: workOrder.config && workOrder.config.env ? workOrder.config.env : {},
    vendor,
  };
}

function emitFromParsedMessage(ctx, message) {
  if (!message || typeof message !== "object") {
    if (typeof message === "string" && message.length > 0) {
      ctx.emitAssistantDelta(message);
    }
    return;
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
      toolName: String(message.tool_name || message.toolName || message.tool || "kimi_tool"),
      toolUseId: message.tool_use_id || message.toolUseId || message.id || null,
      parentToolUseId: message.parent_tool_use_id || message.parentToolUseId || null,
      input: message.input || message.arguments || {},
    });
    return;
  }

  if (kind.includes("tool_result") || kind.includes("toolresult")) {
    ctx.emitToolResult({
      toolName: String(message.tool_name || message.toolName || message.tool || "kimi_tool"),
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
    ctx.emitError(String(message.error || message.message || text || "kimi error"));
  }
}

function pickNumericToken(raw, candidates) {
  if (!raw || typeof raw !== "object") {
    return undefined;
  }

  for (const key of candidates) {
    const direct = raw[key];
    if (typeof direct === "number") {
      return direct;
    }

    const camel = toCamel(key);
    const camelValue = raw[camel];
    if (typeof camelValue === "number") {
      return camelValue;
    }
  }

  return undefined;
}

function toCamel(value) {
  return String(value).replace(/_([a-z])/g, (_, ch) => ch.toUpperCase());
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

      if (parsed && parsed.type && String(parsed.type).toLowerCase() === "usage") {
        usageRaw = parsed.usage || parsed;
      }
      emitFromParsedMessage(ctx, parsed);
    });

    const errBuf = [];
    child.stderr.on("data", (chunk) => {
      const text = String(chunk);
      errBuf.push(text);
      ctx.emitWarning(`[kimi stderr] ${text}`);
    });

    child.on("error", (err) => {
      reject(new Error(`failed to start kimi command '${command}': ${safeString(err)}`));
    });

    child.stdin.end(JSON.stringify(request) + "\n", "utf8");

    child.on("close", (code) => {
      if (code !== 0 && errBuf.length > 0) {
        ctx.emitError(errBuf.join("\n"));
      }

      const usage = normalizeUsage(usageRaw || {});
      resolve({
        usageRaw: usageRaw || {},
        usage,
        outcome: code === 0 ? "Complete" : "Failed",
      });
    });
  });
}

function fallbackResult(ctx) {
  const task = ctx?.workOrder?.task || "";
  ctx.emitAssistantMessage("Kimi adapter fallback mode.");
  ctx.emitAssistantMessage("No external Kimi runner is configured.");
  ctx.emitAssistantMessage(`Task: ${safeString(task)}`);

  return {
    usageRaw: {
      mode: "kimi_adapter_fallback",
      note: "Configure ABP_KIMI_RUNNER or ABP_KIMI_CMD for real execution",
    },
    usage: {
      input_tokens: 0,
      output_tokens: 0,
    },
    outcome: "Partial",
  };
}

async function run(ctx) {
  const request = buildRequest(ctx);
  const hasRunner = typeof RUNNER_CMD === "string" && RUNNER_CMD.trim();
  const hasExplicitCmd = Object.prototype.hasOwnProperty.call(process.env, "ABP_KIMI_CMD");
  const hasCmd = hasExplicitCmd && typeof DEFAULT_CMD === "string" && DEFAULT_CMD.trim();

  if (!hasRunner && !hasCmd) {
    return fallbackResult(ctx);
  }

  const command = hasRunner ? RUNNER_CMD : DEFAULT_CMD;
  const args = hasRunner
    ? parseCommandArgs(process.env.ABP_KIMI_RUNNER_ARGS)
    : DEFAULT_CMD_ARGS;
  const finalArgs = command === DEFAULT_CMD && args.length === 0 ? [] : args;

  try {
    return await runFromCommand(command, finalArgs, request, ctx);
  } catch (err) {
    ctx.emitError(`kimi adapter execution failed: ${safeString(err)}`);
    return {
      usageRaw: {
        error: safeString(err),
        command,
        args: finalArgs,
      },
      usage: {},
      outcome: "Failed",
    };
  }
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
};
