/**
 * Gemini Adapter for Agent Backplane
 *
 * This module provides the interface between ABP and the Gemini CLI/SDK.
 * It handles:
 * - Spawning and communicating with the Gemini CLI process
 * - Translating Gemini events to ABP event format
 * - Managing the Gemini session lifecycle
 *
 * Adapter contract (same as Claude adapter):
 *   module.exports = {
 *     name: "gemini_adapter",
 *     version: "x.y.z",
 *     async run(ctx) { ... }
 *   }
 */

const { spawn } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const readline = require("node:readline");

const ADAPTER_NAME = "gemini_adapter";
const ADAPTER_VERSION = "0.1.0";

// Default Gemini CLI command
const DEFAULT_GEMINI_CMD = process.env.ABP_GEMINI_CMD || "gemini";
const DEFAULT_GEMINI_ARGS = [];

/**
 * Check if Gemini CLI is available
 * @returns {Promise<boolean>}
 */
async function isGeminiAvailable() {
  return new Promise((resolve) => {
    const check = spawn(DEFAULT_GEMINI_CMD, ["--version"], {
      shell: true,
      timeout: 5000,
    });

    let output = "";
    check.stdout.on("data", (data) => {
      output += data.toString();
    });

    check.on("close", (code) => {
      resolve(code === 0);
    });

    check.on("error", () => {
      resolve(false);
    });
  });
}

/**
 * Build Gemini CLI arguments from mapped request
 * @param {object} geminiRequest - Mapped Gemini request
 * @param {object} options - Additional options
 * @returns {string[]} Command line arguments
 */
function buildGeminiArgs(geminiRequest, options = {}) {
  const args = [];

  // Prompt (can be passed as argument or via stdin)
  if (geminiRequest.prompt) {
    args.push("--prompt", geminiRequest.prompt);
  }

  // Working directory
  if (geminiRequest.cwd) {
    args.push("--cwd", geminiRequest.cwd);
  }

  // Model selection
  if (geminiRequest.model) {
    args.push("--model", geminiRequest.model);
  }

  // Tools configuration
  if (geminiRequest.tools && geminiRequest.tools.length > 0) {
    args.push("--tools", geminiRequest.tools.join(","));
  }

  // Excluded tools
  if (geminiRequest.excludeTools && geminiRequest.excludeTools.length > 0) {
    args.push("--exclude-tools", geminiRequest.excludeTools.join(","));
  }

  // Sandbox settings
  if (geminiRequest.sandbox) {
    if (geminiRequest.sandbox.sandbox === false) {
      args.push("--no-sandbox");
    }
    if (geminiRequest.sandbox.autoApprove) {
      args.push("--auto-approve", geminiRequest.sandbox.autoApprove.join(","));
    }
  }

  // JSON output mode for structured responses
  if (options.jsonMode || geminiRequest.json_mode) {
    args.push("--json");
  }

  // Context files
  if (geminiRequest.context) {
    if (geminiRequest.context.loadClaudemd) {
      args.push("--context-file", "CLAUDE.md");
    }
    if (geminiRequest.context.ignoreProjectGeminimd) {
      args.push("--ignore-project-settings");
    }
  }

  // MCP servers (if supported by CLI)
  if (geminiRequest.mcpServers) {
    for (const [name, config] of Object.entries(geminiRequest.mcpServers)) {
      if (config.command) {
        args.push("--mcp-server", `${name}=${config.command}`);
      }
    }
  }

  // Non-interactive mode for automation
  args.push("--non-interactive");

  // Output format
  args.push("--output-format", "json");

  return args;
}

/**
 * Parse Gemini CLI output line into event structure
 * @param {string} line - Raw output line
 * @returns {object|null} Parsed event or null
 */
function parseGeminiOutput(line) {
  if (!line || !line.trim()) {
    return null;
  }

  try {
    const parsed = JSON.parse(line);

    // Handle different Gemini output types
    if (parsed.type === "text_delta" || parsed.text) {
      return {
        type: "assistant_delta",
        text: parsed.text || parsed.delta || "",
      };
    }

    if (parsed.type === "tool_call" || parsed.tool_use) {
      return {
        type: "tool_call",
        toolName: parsed.tool_name || parsed.tool_use?.name,
        toolUseId: parsed.tool_id || parsed.tool_use?.id,
        input: parsed.tool_input || parsed.tool_use?.input,
      };
    }

    if (parsed.type === "tool_result" || parsed.tool_result) {
      return {
        type: "tool_result",
        toolName: parsed.tool_name,
        toolUseId: parsed.tool_id,
        output: parsed.tool_output || parsed.tool_result,
        isError: parsed.is_error || false,
      };
    }

    if (parsed.type === "thinking" || parsed.thinking) {
      return {
        type: "thinking",
        text: parsed.thinking || parsed.text,
      };
    }

    if (parsed.type === "complete" || parsed.done) {
      return {
        type: "complete",
        status: parsed.status || "success",
        usage: parsed.usage,
      };
    }

    if (parsed.type === "error" || parsed.error) {
      return {
        type: "error",
        message: parsed.error || parsed.message,
        code: parsed.code,
      };
    }

    // Pass through unknown types
    return {
      type: "unknown",
      raw: parsed,
    };
  } catch (e) {
    // Not JSON, treat as plain text
    return {
      type: "assistant_delta",
      text: line,
    };
  }
}

/**
 * Run the Gemini adapter
 * @param {object} ctx - Adapter context from host.js
 * @returns {Promise<void>}
 */
async function run(ctx) {
  const { workOrder, sdkOptions, policy, emitAssistantDelta, emitAssistantMessage, emitToolCall, emitToolResult, emitWarning, emitError, writeArtifact } = ctx;

  // Check Gemini availability
  const available = await isGeminiAvailable();
  if (!available) {
    emitError("Gemini CLI is not available. Please install it: npm install -g @google/gemini-cli");
    return;
  }

  // Build Gemini request from work order
  const geminiRequest = {
    prompt: workOrder.task || sdkOptions?.prompt,
    cwd: workOrder.workspace_root || process.cwd(),
    model: sdkOptions?.model,
    tools: sdkOptions?.tools,
    excludeTools: sdkOptions?.excludeTools,
    sandbox: sdkOptions?.sandbox,
    context: sdkOptions?.context,
    mcpServers: sdkOptions?.mcpServers,
    json_mode: sdkOptions?.json_mode,
  };

  const args = buildGeminiArgs(geminiRequest, { jsonMode: true });

  ctx.log(`Starting Gemini CLI with args: ${args.join(" ")}`);

  // Spawn Gemini process
  const gemini = spawn(DEFAULT_GEMINI_CMD, args, {
    cwd: geminiRequest.cwd,
    env: {
      ...process.env,
      // Pass through any Gemini-specific env vars
      GEMINI_API_KEY: process.env.GEMINI_API_KEY,
      GOOGLE_APPLICATION_CREDENTIALS: process.env.GOOGLE_APPLICATION_CREDENTIALS,
    },
    shell: true,
    stdio: ["pipe", "pipe", "pipe"],
  });

  // Track state
  let assistantText = "";
  let toolCalls = [];
  let usage = null;
  let hadError = false;

  // Handle stdout (events stream)
  const rl = readline.createInterface({
    input: gemini.stdout,
    crlfDelay: Infinity,
  });

  rl.on("line", (line) => {
    const event = parseGeminiOutput(line);
    if (!event) return;

    switch (event.type) {
      case "assistant_delta":
        assistantText += event.text;
        emitAssistantDelta(event.text);
        break;

      case "assistant_message":
        emitAssistantMessage(event.text);
        break;

      case "tool_call":
        toolCalls.push({
          toolName: event.toolName,
          toolUseId: event.toolUseId,
          input: event.input,
        });
        emitToolCall({
          toolName: event.toolName,
          toolUseId: event.toolUseId,
          input: event.input,
        });
        break;

      case "tool_result":
        emitToolResult({
          toolName: event.toolName,
          toolUseId: event.toolUseId,
          output: event.output,
          isError: event.isError,
        });
        break;

      case "thinking":
        // Gemini thinking - could emit as special event
        ctx.log(`[thinking] ${event.text?.slice(0, 100)}...`);
        break;

      case "complete":
        usage = event.usage;
        break;

      case "error":
        hadError = true;
        emitError(event.message);
        break;

      case "unknown":
        ctx.log(`Unknown Gemini event: ${JSON.stringify(event.raw)}`);
        break;
    }
  });

  // Handle stderr (logs, warnings)
  gemini.stderr.on("data", (data) => {
    const text = data.toString();
    ctx.log(`[gemini stderr] ${text}`);
    // Could parse for specific warnings
    if (text.includes("warning") || text.includes("Warning")) {
      emitWarning(text);
    }
  });

  // Handle process errors
  gemini.on("error", (err) => {
    hadError = true;
    emitError(`Failed to start Gemini: ${err.message}`);
  });

  // Wait for completion
  return new Promise((resolve, reject) => {
    gemini.on("close", (code) => {
      if (code !== 0 && !hadError) {
        emitError(`Gemini exited with code ${code}`);
      }
      resolve();
    });
  });
}

// Adapter metadata
module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
  isGeminiAvailable,
  buildGeminiArgs,
  parseGeminiOutput,
};
