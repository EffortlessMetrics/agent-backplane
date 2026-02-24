/**
 * Codex Adapter for Agent Backplane
 *
 * This module provides the interface between ABP and the OpenAI Codex SDK.
 * It handles:
 * - Spawning and communicating with the Codex SDK
 * - Translating Codex events to ABP event format
 * - Managing the Codex thread lifecycle
 *
 * Adapter contract (same as Claude/Gemini adapter):
 *   module.exports = {
 *     name: "codex_adapter",
 *     version: "x.y.z",
 *     async run(ctx) { ... }
 *   }
 */

const { spawn } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const readline = require("node:readline");

const ADAPTER_NAME = "codex_adapter";
const ADAPTER_VERSION = "0.1.0";

// Default Codex CLI command (if using CLI wrapper)
const DEFAULT_CODEX_CMD = process.env.ABP_CODEX_CMD || "codex";
const DEFAULT_CODEX_ARGS = [];

/**
 * Check if Codex CLI is available
 * @returns {Promise<boolean>}
 */
async function isCodexAvailable() {
  return new Promise((resolve) => {
    const check = spawn(DEFAULT_CODEX_CMD, ["--version"], {
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
 * Build Codex SDK options from work order
 * @param {object} workOrder - ABP work order
 * @param {object} sdkOptions - Additional SDK options
 * @returns {object} Codex SDK options
 */
function buildCodexOptions(workOrder, sdkOptions = {}) {
  const options = {
    // Model selection
    model: sdkOptions.model || workOrder.config?.vendor?.model || "gpt-4",
    
    // Working directory
    cwd: workOrder.workspace?.root || process.cwd(),
    
    // Thread management
    threadId: sdkOptions.thread_id || workOrder.config?.vendor?.thread_id,
    
    // Tools
    tools: sdkOptions.tools || [],
    
    // Execution settings
    temperature: sdkOptions.temperature,
    maxTokens: sdkOptions.max_tokens || sdkOptions.maxTokens,
    topP: sdkOptions.top_p || sdkOptions.topP,
    
    // Permission mode
    autoApprove: sdkOptions.auto_approve || [],
    
    // Output format
    jsonMode: sdkOptions.json_mode || sdkOptions.response_format?.type === "json_object",
  };

  // Merge in any additional options
  return { ...options, ...sdkOptions };
}

/**
 * Parse Codex SDK output line into event structure
 * @param {string} line - Raw output line
 * @returns {object|null} Parsed event or null
 */
function parseCodexOutput(line) {
  if (!line || !line.trim()) {
    return null;
  }

  try {
    const parsed = JSON.parse(line);

    // Handle different Codex output types
    if (parsed.type === "text_delta" || parsed.delta) {
      return {
        type: "assistant_delta",
        text: parsed.delta?.content || parsed.text || "",
      };
    }

    if (parsed.type === "tool_call" || parsed.function_call) {
      return {
        type: "tool_call",
        toolName: parsed.function_call?.name || parsed.name,
        toolUseId: parsed.id || `codex_toolu_${Date.now()}`,
        input: parsed.function_call?.arguments 
          ? JSON.parse(parsed.function_call.arguments)
          : parsed.input || {},
      };
    }

    if (parsed.type === "tool_result" || parsed.tool_output) {
      return {
        type: "tool_result",
        toolUseId: parsed.tool_use_id || parsed.id,
        output: parsed.tool_output || parsed.output || "",
        isError: parsed.is_error || false,
      };
    }

    if (parsed.type === "message" || parsed.role === "assistant") {
      return {
        type: "assistant_message",
        text: parsed.content?.[0]?.text || parsed.content || "",
      };
    }

    if (parsed.type === "error") {
      return {
        type: "error",
        message: parsed.message || "Unknown error",
        code: parsed.code,
      };
    }

    // Pass through unknown types
    return {
      type: "raw",
      data: parsed,
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
 * Run the Codex adapter
 * @param {object} ctx - Adapter context
 * @returns {Promise<void>}
 */
async function run(ctx) {
  const {
    workOrder,
    sdkOptions,
    policy,
    emitAssistantDelta,
    emitAssistantMessage,
    emitToolCall,
    emitToolResult,
    emitWarning,
    emitError,
    writeArtifact,
  } = ctx;

  // Build Codex options
  const codexOptions = buildCodexOptions(workOrder, sdkOptions);

  // Check if we're in passthrough mode
  const passthroughRequest = workOrder.config?.vendor?.abp?.request;

  if (passthroughRequest) {
    // Passthrough mode: send raw request to Codex
    return runPassthrough(passthroughRequest, codexOptions, ctx);
  }

  // Mapped mode: use the mapped request
  return runMapped(codexOptions, ctx);
}

/**
 * Run in passthrough mode (Codex → Codex)
 * @param {object} request - Raw Codex request
 * @param {object} options - Codex options
 * @param {object} ctx - Adapter context
 * @returns {Promise<void>}
 */
async function runPassthrough(request, options, ctx) {
  const { emitAssistantDelta, emitAssistantMessage, emitToolCall, emitToolResult, emitError, writeArtifact } = ctx;

  try {
    // Try to use real Codex SDK if available
    const codexSdk = await loadCodexSdk();

    if (codexSdk) {
      // Real SDK execution
      const result = await codexSdk.runThread(request, options);

      // Stream results
      if (result.stream) {
        for await (const chunk of result.stream) {
          const event = parseCodexOutput(JSON.stringify(chunk));
          if (event) {
            switch (event.type) {
              case "assistant_delta":
                emitAssistantDelta(event.text);
                break;
              case "assistant_message":
                emitAssistantMessage(event.text);
                break;
              case "tool_call":
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
              case "error":
                emitError(event.message);
                break;
            }
          }
        }
      }
    } else {
      // Fallback: CLI execution
      return runCliPassthrough(request, options, ctx);
    }
  } catch (error) {
    emitError(`Codex execution failed: ${error.message}`);
    throw error;
  }
}

/**
 * Run in mapped mode (Codex dialect → Claude backend)
 * This is handled by the host.js which calls the mapper
 * @param {object} options - Mapped options
 * @param {object} ctx - Adapter context
 * @returns {Promise<void>}
 */
async function runMapped(options, ctx) {
  const { emitAssistantDelta, emitAssistantMessage, emitError } = ctx;

  // In mapped mode, the host.js has already transformed the request
  // and will delegate to the Claude adapter
  // This function is called when we're using Codex SDK directly
  // (which shouldn't happen in mapped mode, but we handle it anyway)

  emitError("Mapped mode should delegate to Claude adapter");
  throw new Error("Mapped mode requires Claude backend delegation");
}

/**
 * Run using Codex CLI in passthrough mode
 * @param {object} request - Raw request
 * @param {object} options - CLI options
 * @param {object} ctx - Adapter context
 * @returns {Promise<void>}
 */
async function runCliPassthrough(request, options, ctx) {
  const { emitAssistantDelta, emitAssistantMessage, emitToolCall, emitToolResult, emitError, emitWarning } = ctx;

  return new Promise((resolve, reject) => {
    const args = buildCliArgs(request, options);
    
    const proc = spawn(DEFAULT_CODEX_CMD, args, {
      cwd: options.cwd || process.cwd(),
      env: {
        ...process.env,
        OPENAI_API_KEY: process.env.OPENAI_API_KEY || process.env.CODEX_API_KEY,
      },
      stdio: ["pipe", "pipe", "pipe"],
      shell: true,
    });

    let stderr = "";

    proc.stderr.on("data", (data) => {
      stderr += data.toString();
    });

    const rl = readline.createInterface({
      input: proc.stdout,
      crlfDelay: Infinity,
    });

    rl.on("line", (line) => {
      const event = parseCodexOutput(line);
      if (event) {
        switch (event.type) {
          case "assistant_delta":
            emitAssistantDelta(event.text);
            break;
          case "assistant_message":
            emitAssistantMessage(event.text);
            break;
          case "tool_call":
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
          case "error":
            emitError(event.message);
            break;
          case "raw":
            // Log raw output for debugging
            emitWarning(`Raw Codex output: ${JSON.stringify(event.data)}`);
            break;
        }
      }
    });

    rl.on("close", () => {
      // Stream ended
    });

    proc.on("close", (code) => {
      if (code !== 0) {
        emitError(`Codex CLI exited with code ${code}: ${stderr}`);
        reject(new Error(`Codex CLI failed: ${code}`));
      } else {
        resolve();
      }
    });

    proc.on("error", (err) => {
      emitError(`Failed to spawn Codex CLI: ${err.message}`);
      reject(err);
    });

    // Send request via stdin if needed
    if (request.prompt || request.input) {
      proc.stdin.write(JSON.stringify(request));
      proc.stdin.end();
    }
  });
}

/**
 * Build CLI arguments from request and options
 * @param {object} request - Codex request
 * @param {object} options - CLI options
 * @returns {string[]} CLI arguments
 */
function buildCliArgs(request, options) {
  const args = [];

  // Prompt
  if (request.prompt || request.input) {
    args.push("--prompt", request.prompt || request.input);
  }

  // Model
  if (request.model || options.model) {
    args.push("--model", request.model || options.model);
  }

  // Thread ID
  if (request.thread_id || options.threadId) {
    args.push("--thread", request.thread_id || options.threadId);
  }

  // Tools
  if (request.tools && request.tools.length > 0) {
    const toolNames = request.tools.map(t => typeof t === "string" ? t : t.name);
    args.push("--tools", toolNames.join(","));
  }

  // Auto-approve
  if (options.autoApprove && options.autoApprove.length > 0) {
    args.push("--auto-approve", options.autoApprove.join(","));
  }

  // JSON mode
  if (options.jsonMode) {
    args.push("--json");
  }

  // Non-interactive
  args.push("--non-interactive");

  return args;
}

/**
 * Try to load the Codex SDK
 * @returns {Promise<object|null>} SDK module or null
 */
async function loadCodexSdk() {
  try {
    // Try @openai/codex first
    const sdk = require("@openai/codex");
    return {
      runThread: async (request, options) => {
        const codex = new sdk.Codex({
          apiKey: process.env.OPENAI_API_KEY,
        });

        // Start or resume thread
        let thread;
        if (request.thread_id) {
          thread = await codex.resumeThread(request.thread_id);
        } else {
          thread = await codex.startThread();
        }

        // Run the thread
        const stream = await thread.run(request.prompt || request.input, {
          model: options.model,
          tools: options.tools,
          temperature: options.temperature,
          maxTokens: options.maxTokens,
        });

        return { stream, threadId: thread.id };
      },
    };
  } catch (e) {
    // SDK not available
    return null;
  }
}

/**
 * Get adapter capabilities
 * @returns {object} Capability manifest
 */
function getCapabilities() {
  return {
    name: ADAPTER_NAME,
    version: ADAPTER_VERSION,
    features: [
      "streaming",
      "tools",
      "thread_management",
      "json_mode",
      "code_execution",
    ],
    models: [
      "gpt-4",
      "gpt-4-turbo",
      "gpt-4o",
      "gpt-4o-mini",
      "gpt-3.5-turbo",
      "o1",
      "o1-preview",
      "o1-mini",
    ],
  };
}

module.exports = {
  name: ADAPTER_NAME,
  version: ADAPTER_VERSION,
  run,
  isCodexAvailable,
  buildCodexOptions,
  parseCodexOutput,
  getCapabilities,
  loadCodexSdk,
};
