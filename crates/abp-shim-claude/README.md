# abp-shim-claude

A drop-in-compatible Anthropic Claude client that routes through the Agent Backplane.

Provides `AnthropicClient`, `MessageRequest`, `MessageResponse`, and streaming types
that mirror the Anthropic Messages API surface, with internal conversion through
ABP's intermediate representation.

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
