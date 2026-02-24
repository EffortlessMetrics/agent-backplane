class MockKimiClient {
  constructor(options = {}) {
    this.options = options;
  }

  async runTask(payload = {}) {
    return {
      stream: this.stream(payload),
      usage: {
        input_tokens: 11,
        output_tokens: 7,
      },
    };
  }

  async *stream(payload = {}) {
    yield {
      type: "assistant_delta",
      text: "hello ",
    };
    yield {
      type: "assistant_delta",
      text: "from kimi",
    };
    yield {
      type: "tool_call",
      tool_name: "read_file",
      tool_use_id: "toolu_mock_1",
      input: {
        file_path: "README.md",
      },
    };
    yield {
      type: "tool_result",
      tool_name: "read_file",
      tool_use_id: "toolu_mock_1",
      output: "ok",
      is_error: false,
    };
    yield {
      type: "usage",
      usage: {
        input_tokens: 11,
        output_tokens: 7,
      },
    };
    yield {
      type: "assistant_message",
      text: `final: ${payload.prompt || payload.input || ""}`.trim(),
    };
  }
}

module.exports = {
  KimiClient: MockKimiClient,
};
