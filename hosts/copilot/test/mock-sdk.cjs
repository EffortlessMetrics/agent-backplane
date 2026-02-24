class MockSession {
  constructor() {
    this.closed = false;
  }

  async sendAndStream(payload) {
    this.payload = payload;
    const events = [
      { type: "assistant_delta", text: "hello " },
      { type: "assistant_delta", text: "world" },
      {
        type: "tool_call",
        tool_name: "Read",
        tool_use_id: "toolu_mock_1",
        input: { path: "README.md" },
      },
      {
        type: "tool_result",
        tool_name: "Read",
        tool_use_id: "toolu_mock_1",
        output: "ok",
        is_error: false,
      },
      {
        type: "usage",
        usage: {
          input_tokens: 5,
          output_tokens: 7,
        },
      },
    ];

    return {
      async *[Symbol.asyncIterator]() {
        for (const item of events) {
          yield item;
        }
      },
      response: Promise.resolve({
        text: "hello world",
        usage: {
          input_tokens: 5,
          output_tokens: 7,
        },
      }),
    };
  }

  async close() {
    this.closed = true;
  }
}

class CopilotClient {
  constructor(options) {
    this.options = options;
  }

  async createSession(options) {
    this.sessionOptions = options;
    return new MockSession();
  }

  async close() {
    return;
  }
}

module.exports = {
  CopilotClient,
};
