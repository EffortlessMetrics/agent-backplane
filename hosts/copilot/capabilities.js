const SupportLevel = {
  Native: "native",
  Emulated: "emulated",
  Unsupported: "unsupported",
};

const CopilotCapabilities = {
  streaming: SupportLevel.Native,
  tool_read: SupportLevel.Native,
  tool_write: SupportLevel.Native,
  tool_edit: SupportLevel.Native,
  tool_bash: SupportLevel.Native,
  tool_glob: SupportLevel.Native,
  tool_grep: SupportLevel.Native,
  tool_web_search: SupportLevel.Native,
  tool_web_fetch: SupportLevel.Native,
  structured_output_json_schema: SupportLevel.Native,
  hooks_pre_tool_use: SupportLevel.Emulated,
  hooks_post_tool_use: SupportLevel.Emulated,
  session_resume: SupportLevel.Native,
  session_fork: SupportLevel.Native,
  checkpointing: SupportLevel.Emulated,
  mcp_client: SupportLevel.Native,
  mcp_server: SupportLevel.Emulated,
  tool_ask_user: SupportLevel.Native,
};

const TOOL_HINTS = {
  Read: "tool_read",
  Write: "tool_write",
  Edit: "tool_edit",
  Bash: "tool_bash",
  Glob: "tool_glob",
  Grep: "tool_grep",
  WebSearch: "tool_web_search",
  WebFetch: "tool_web_fetch",
  MCP: "mcp_client",
};

function getCapabilityManifest() {
  return {
    backend: "github_copilot_sdk",
    version: "0.2.0",
    capabilities: { ...CopilotCapabilities },
    tool_hints: { ...TOOL_HINTS },
  };
}

module.exports = {
  SupportLevel,
  CopilotCapabilities,
  TOOL_HINTS,
  getCapabilityManifest,
};

