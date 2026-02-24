// Copy this file and point ABP_CLAUDE_ADAPTER_MODULE at your copy.
//
// This template intentionally avoids hard-coding Claude SDK symbol names.
// Wire it to your installed SDK version and keep this adapter as the only
// vendor-specific layer.

module.exports = {
  name: "claude_custom_template",
  version: "0.1.0",
  capabilities: {
    streaming: "native",
    hooks_pre_tool_use: "native",
    hooks_post_tool_use: "native",
    mcp_client: "native",
  },
  async run(ctx) {
    ctx.emitAssistantMessage("Custom Claude adapter template loaded.");
    ctx.emitAssistantMessage(
      "Implement SDK session/query wiring in hosts/claude/adapter.template.js copy."
    );

    // Example policy gate before a synthetic tool call:
    const allowed = ctx.emitToolCall({
      toolName: "Bash",
      toolUseId: "example-1",
      input: { command: "echo hello" },
    });
    if (!allowed) {
      return {
        usageRaw: { note: "tool denied by policy" },
        outcome: "partial",
      };
    }

    ctx.emitToolResult({
      toolName: "Bash",
      toolUseId: "example-1",
      output: "hello",
      isError: false,
    });

    return {
      usageRaw: { note: "replace with real Claude SDK usage payload" },
      usage: {},
      outcome: "complete",
    };
  },
};
