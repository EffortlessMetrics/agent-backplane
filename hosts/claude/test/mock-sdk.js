function normalizeRequest(requestOrPrompt, maybeOptions) {
  if (requestOrPrompt && typeof requestOrPrompt === "object") {
    return requestOrPrompt;
  }
  return {
    prompt: String(requestOrPrompt || ""),
    options: maybeOptions || {},
  };
}

function buildHookMessages() {
  return [
    {
      type: "pre_tool_use",
      tool_name: "Write",
      tool_use_id: "toolu_hook_write",
      input: { file_path: "test.txt", content: "hello" },
    },
    {
      type: "post_tool_use",
      tool_name: "Write",
      tool_use_id: "toolu_hook_write",
      output: "write ok",
      is_error: false,
    },
  ];
}

function buildTypedSdkMessages() {
  return [
    {
      type: "message_start",
      message: { usage: { input_tokens: 10 } },
    },
    {
      type: "content_block_start",
      content_block: { type: "text", text: "Typed " },
    },
    {
      type: "content_block_delta",
      delta: { type: "text_delta", text: "hello" },
    },
    {
      type: "content_block_stop",
    },
    {
      type: "message_delta",
      delta: { stop_reason: "end_turn" },
      usage: { output_tokens: 5 },
    },
    {
      type: "message_stop",
    },
  ];
}

function buildAskUserMessages() {
  return [
    {
      type: "tool_call",
      tool_name: "AskUserQuestion",
      tool_use_id: "toolu_ask_user",
      input: { question: "Should I proceed?" },
    },
  ];
}

function buildMappedMessages(prompt) {
  // Hook messages mode
  if (process.env.MOCK_CLAUDE_HOOK_MODE === "1") {
    return buildHookMessages();
  }
  // Typed SDK messages mode
  if (process.env.MOCK_CLAUDE_TYPED_MODE === "1") {
    return buildTypedSdkMessages();
  }
  // AskUser messages mode
  if (process.env.MOCK_CLAUDE_ASK_USER_MODE === "1") {
    return buildAskUserMessages();
  }
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
