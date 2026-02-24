module.exports = {
  name: "copilot_adapter_template",
  version: "0.1.0",

  /**
   * Implement this method with your real Copilot SDK binding.
   *
   * @param {object} ctx
   * @param {object} ctx.workOrder - ABP work order
   * @param {object} ctx.sdkOptions - normalized vendor options
   * @param {function(string)} ctx.emitAssistantDelta
   * @param {function(string)} ctx.emitAssistantMessage
   * @param {function(object)} ctx.emitToolCall
   * @param {function(object)} ctx.emitToolResult
   * @param {function(string)} ctx.emitWarning
   * @param {function(string)} ctx.emitError
   * @param {function(string,string,string)} ctx.writeArtifact
   */
  async run(ctx) {
    ctx.emitAssistantMessage("Using Copilot adapter template. Replace with real binding.");
    return {
      usageRaw: {
        template: true,
        model: ctx.sdkOptions?.model || null,
      },
      usage: {
        input_tokens: 0,
        output_tokens: 0,
      },
      outcome: "partial",
    };
  },
};

