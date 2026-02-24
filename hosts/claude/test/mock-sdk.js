function normalizeRequest(requestOrPrompt, maybeOptions) {
  if (requestOrPrompt && typeof requestOrPrompt === "object") {
    return requestOrPrompt;
  }
  return {
    prompt: String(requestOrPrompt || ""),
    options: maybeOptions || {},
  };
}

function buildMappedMessages(prompt) {
  return [
    {
      type: "assistant_delta",
      text: "Mapped ",
    },
    {
      type: "assistant_delta",
      text: "response.",
    },
    {
      type: "tool_call",
      tool_name: "Read",
      tool_use_id: "toolu_mock_read",
      input: {
        file_path: "README.md",
      },
    },
    {
      type: "tool_result",
      tool_name: "Read",
      tool_use_id: "toolu_mock_read",
      output: `read ok for prompt: ${prompt}`,
      is_error: false,
    },
    {
      type: "assistant_message",
      text: "Mapped response.",
    },
    {
      type: "usage",
      usage: {
        input_tokens: 42,
        output_tokens: 7,
      },
    },
  ];
}

function buildClientMessages(prompt) {
  return [
    {
      type: "assistant_delta",
      text: "Client ",
    },
    {
      type: "assistant_delta",
      text: "response.",
    },
    {
      type: "tool_call",
      tool_name: "Read",
      tool_use_id: "toolu_client_read",
      input: {
        file_path: "README.md",
      },
    },
    {
      type: "tool_result",
      tool_name: "Read",
      tool_use_id: "toolu_client_read",
      output: `client read ok for prompt: ${prompt}`,
      is_error: false,
    },
    {
      type: "assistant_message",
      text: "Client response.",
    },
    {
      type: "usage",
      usage: {
        input_tokens: 84,
        output_tokens: 14,
      },
    },
  ];
}

async function* query(requestOrPrompt, maybeOptions) {
  const request = normalizeRequest(requestOrPrompt, maybeOptions);
  const prompt = String(request.prompt || "");
  for (const message of buildMappedMessages(prompt)) {
    yield message;
  }
}

class ClaudeSDKClient {
  constructor(config = {}) {
    this.config = config;
    this.messages = [];
    this.connected = false;
  }

  async connect() {
    this.connected = true;
  }

  async disconnect() {
    this.connected = false;
  }

  async query(requestOrPrompt, maybeOptions) {
    const request = normalizeRequest(requestOrPrompt, maybeOptions);
    const prompt = String(request.prompt || "");
    this.messages = buildClientMessages(prompt);
    return null;
  }

  async *receiveResponse() {
    for (const message of this.messages) {
      yield message;
    }
  }
}

const disableClient = process.env.MOCK_CLAUDE_DISABLE_CLIENT === "1";

module.exports = disableClient
  ? {
    query,
  }
  : {
    query,
    ClaudeSDKClient,
  };
