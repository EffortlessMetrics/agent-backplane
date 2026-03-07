# abp-sdk-types

SDK-specific dialect type definitions for the Agent Backplane.

This crate contains **pure data model types** — no networking, no SDK calls.
Each supported vendor's request/response surface is mirrored here for use in
dialect translation, projection, and testing.

## Supported Dialects

| Dialect | Vendor | API Style |
|---------|--------|-----------|
| `OpenAi` | OpenAI | Chat Completions |
| `Claude` | Anthropic | Messages API |
| `Gemini` | Google | generateContent |
| `Kimi` | Moonshot | Chat Completions (extended) |
| `Codex` | OpenAI | Responses API |
| `Copilot` | GitHub | Copilot Extensions |

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Dual-licensed under MIT OR Apache-2.0.
