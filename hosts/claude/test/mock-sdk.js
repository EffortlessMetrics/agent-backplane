function normalizeRequest(requestOrPrompt, maybeOptions) {
  if (requestOrPrompt && typeof requestOrPrompt === "object") {
    return requestOrPrompt;
  }
  return {
    prompt: String(requestOrPrompt || ""),
    options: maybeOptions || {},
  };
}

async function* query(requestOrPrompt, maybeOptions) {
  const request = normalizeRequest(requestOrPrompt, maybeOptions);
  const prompt = String(request.prompt || "");

  yield {
    type: "assistant_delta",
    text: "Mapped ",
  };

  yield {
    type: "assistant_delta",
    text: "response.",
  };

  yield {
    type: "tool_call",
    tool_name: "Read",
    tool_use_id: "toolu_mock_read",
    input: {
      file_path: "README.md",
    },
  };

  yield {
    type: "tool_result",
    tool_name: "Read",
    tool_use_id: "toolu_mock_read",
    output: `read ok for prompt: ${prompt}`,
    is_error: false,
  };

  yield {
    type: "assistant_message",
    text: "Mapped response.",
  };

  yield {
    type: "usage",
    usage: {
      input_tokens: 42,
      output_tokens: 7,
    },
  };
}

module.exports = {
  query,
};
