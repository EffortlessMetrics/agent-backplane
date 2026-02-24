module.exports = {
  name: "gemini_mock_adapter",
  version: "0.1.0",
  async run(ctx) {
    const task = ctx?.workOrder?.task || "";
    const requestModel = ctx?.sdkOptions?.model || "gemini-mock-model";
    const requestVendor = ctx?.sdkOptions?.vendor || {};

    ctx.emitAssistantMessage(`mock gemini adapter: model=${requestModel}`);
    ctx.emitAssistantMessage(`task=${task}`);

    if (task.includes("requires tool")) {
      const toolUseId = ctx.emitToolCall({
        toolName: "Read",
        toolUseId: "mock-tool-1",
        input: {
          path: "README.md",
        },
      });

      ctx.emitToolResult({
        toolName: "Read",
        toolUseId,
        output: "mock tool output",
        isError: false,
      });
    }

    ctx.emitAssistantDelta(`vendor flags=${Object.keys(requestVendor).join(",") || "none"}`);

    const mode = ctx?.workOrder?.config?.vendor?.abp?.mode || "mapped";
    if (mode === "passthrough") {
      ctx.emitAssistantMessage("mock passthrough mode complete");
    } else {
      ctx.emitAssistantMessage("mock mapped mode complete");
    }

    return {
      usageRaw: {
        mode,
        model: requestModel,
      },
      usage: {
        input_tokens: 12,
        output_tokens: 34,
      },
      outcome: "complete",
    };
  },
};
