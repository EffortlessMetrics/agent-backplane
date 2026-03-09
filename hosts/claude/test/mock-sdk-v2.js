/**
 * Mock SDK with V2 preview surface methods for testing adapter-v2.js.
 *
 * Exports:
 *   - unstable_v2_prompt(prompt, options)   — one-shot async generator
 *   - unstable_v2_createSession(options)     — returns a mock session
 *   - unstable_v2_resumeSession(id, options) — returns a mock resumed session
 *
 * Environment toggles:
 *   MOCK_V2_PROMPT_STRING=1   — prompt returns a plain string instead of stream
 *   MOCK_V2_PROMPT_ERROR=1    — prompt throws an error
 *   MOCK_V2_SESSION_ERROR=1   — session.send() throws an error
 *   MOCK_V2_NO_PROMPT=1       — omit unstable_v2_prompt (session-only SDK)
 *   MOCK_V2_NO_SESSION=1      — omit unstable_v2_createSession (prompt-only SDK)
 */

function buildPromptMessages(prompt) {
  return [
    { type: "assistant_delta", text: "V2 " },
    { type: "assistant_delta", text: "prompt " },
    { type: "assistant_delta", text: "response." },
    {
      type: "tool_call",
      tool_name: "Read",
      tool_use_id: "toolu_v2_read",
      input: { file_path: "README.md" },
    },
    {
      type: "tool_result",
      tool_name: "Read",
      tool_use_id: "toolu_v2_read",
      output: `v2 read ok for prompt: ${prompt}`,
      is_error: false,
    },
    {
      type: "assistant_message",
      text: "V2 prompt response.",
    },
    {
      type: "usage",
      usage: {
        input_tokens: 50,
        output_tokens: 12,
      },
    },
  ];
}

function buildSessionMessages(prompt) {
  return [
    { type: "assistant_delta", text: "V2 " },
    { type: "assistant_delta", text: "session " },
    { type: "assistant_delta", text: "response." },
    {
      type: "assistant_message",
      text: "V2 session response.",
    },
    {
      type: "usage",
      usage: {
        input_tokens: 60,
        output_tokens: 15,
      },
    },
  ];
}

function buildResumeMessages(prompt) {
  return [
    { type: "assistant_delta", text: "V2 " },
    { type: "assistant_delta", text: "resumed " },
    { type: "assistant_delta", text: "response." },
    {
      type: "assistant_message",
      text: "V2 resumed response.",
    },
    {
      type: "usage",
      usage: {
        input_tokens: 30,
        output_tokens: 8,
      },
    },
  ];
}

// ---------------------------------------------------------------------------
// V2 prompt transport
// ---------------------------------------------------------------------------

async function* unstable_v2_prompt(prompt, options) {
  if (process.env.MOCK_V2_PROMPT_ERROR === "1") {
    throw new Error("mock v2 prompt error");
  }

  if (process.env.MOCK_V2_PROMPT_STRING === "1") {
    // This codepath won't work with async generator — the caller tests
    // non-iterable responses. We yield a single string event instead.
    yield "V2 plain string response.";
    return;
  }

  for (const message of buildPromptMessages(String(prompt || ""))) {
    yield message;
  }
}

// Wrapper that returns a plain string when MOCK_V2_PROMPT_STRING=1
async function unstable_v2_prompt_wrapper(prompt, options) {
  if (process.env.MOCK_V2_PROMPT_ERROR === "1") {
    throw new Error("mock v2 prompt error");
  }

  if (process.env.MOCK_V2_PROMPT_STRING === "1") {
    return "V2 plain string response.";
  }

  // Track call count for retry testing
  if (process.env.MOCK_V2_PROMPT_RETRIABLE === "1") {
    unstable_v2_prompt_wrapper._callCount = (unstable_v2_prompt_wrapper._callCount || 0) + 1;
    if (unstable_v2_prompt_wrapper._callCount === 1) {
      const err = new Error("Service temporarily unavailable");
      err.status = 503;
      throw err;
    }
  }

  if (process.env.MOCK_V2_PROMPT_NON_RETRIABLE === "1") {
    const err = new Error("Bad request");
    err.status = 400;
    throw err;
  }

  return unstable_v2_prompt(prompt, options);
}

// ---------------------------------------------------------------------------
// V2 session transport
// ---------------------------------------------------------------------------

let sessionCounter = 0;

function createMockSession(sessionId, options, isResume) {
  const id = sessionId || `v2-session-${++sessionCounter}`;
  const closed = { value: false };

  return {
    id,
    isResume,
    options,
    get closed() { return closed.value; },

    async send(prompt) {
      if (process.env.MOCK_V2_SESSION_ERROR === "1") {
        throw new Error("mock v2 session send error");
      }

      const messages = isResume
        ? buildResumeMessages(String(prompt || ""))
        : buildSessionMessages(String(prompt || ""));

      async function* stream() {
        for (const msg of messages) {
          yield msg;
        }
      }
      return stream();
    },

    async close() {
      closed.value = true;
    },
  };
}

async function unstable_v2_createSession(options) {
  return createMockSession(options?.sessionId || null, options, false);
}

async function unstable_v2_resumeSession(sessionId, options) {
  return createMockSession(sessionId, options, true);
}

// ---------------------------------------------------------------------------
// Export with environment-based toggles
// ---------------------------------------------------------------------------

const exports_obj = {};

if (process.env.MOCK_V2_NO_PROMPT !== "1") {
  exports_obj.unstable_v2_prompt = unstable_v2_prompt_wrapper;
}

if (process.env.MOCK_V2_NO_SESSION !== "1") {
  exports_obj.unstable_v2_createSession = unstable_v2_createSession;
  exports_obj.unstable_v2_resumeSession = unstable_v2_resumeSession;
}

module.exports = exports_obj;
