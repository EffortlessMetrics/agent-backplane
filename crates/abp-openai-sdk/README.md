# abp-openai-sdk

OpenAI Chat Completions SDK adapter for Agent Backplane.

## Features

- **Complete API types** — `ChatCompletionRequest`, `ChatCompletionResponse`, `Message`, `Tool`, `ToolCall`, `FunctionCall`, `FunctionDefinition`, `FinishReason`, `Usage` with detailed token breakdowns
- **Streaming types** — `StreamChunk`, `StreamChoice`, `Delta` (SSE `chat.completion.chunk` format), plus `ToolCallAccumulator` for reassembling fragmented tool calls
- **Model listing types** — `Model`, `ModelList`, `ModelDeleted` for the `/v1/models` endpoint
- **Structured output** — `ResponseFormat` with `text`, `json_object`, and `json_schema` variants
- **Stream options** — `StreamOptions` with `include_usage` for token counting on streaming responses
- **Dialect module** — Wire types (`OpenAIMessage`, `OpenAIToolCall`, etc.), model name canonicalization, capability manifest, and `WorkOrder`/`Receipt` mapping
- **IR lowering** — Bidirectional conversion between OpenAI messages and ABP's intermediate representation
- **Validation** — Mapped-mode validation for early failure on unmappable parameters
- **`From`/`Into` conversions** — `From<ChatCompletionRequest> for WorkOrder` and `From<Receipt> for ChatCompletionResponse`
- **JSON Schema** — All public types derive `schemars::JsonSchema` for schema generation
- **Serde** — All types derive `Serialize`/`Deserialize` with proper `#[serde(rename_all = "snake_case")]` and `#[serde(skip_serializing_if = "Option::is_none")]`

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
