# Google Gemini SDK Surface Area

> Mapping reference for the Gemini `generateContent` API as implemented by `abp-shim-gemini`.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1beta/models/{model}:generateContent` | POST | Non-streaming generation |
| `/v1beta/models/{model}:streamGenerateContent` | POST | Streaming generation |

## Request Format

### generateContent Request

```jsonc
{
  "model": "gemini-2.5-flash",              // set via URL path, not body
  "contents": [                               // required
    {
      "role": "user",                         // "user" | "model"
      "parts": [
        { "text": "Hello" }                   // text part
      ]
    },
    {
      "role": "model",
      "parts": [
        { "text": "Hi there!" },
        {
          "functionCall": {                   // camelCase (not snake_case)
            "name": "get_weather",
            "args": { "city": "NYC" }         // parsed JSON object
          }
        }
      ]
    },
    {
      "role": "user",
      "parts": [
        {
          "functionResponse": {
            "name": "get_weather",
            "response": { "temp": 72 }        // correlates by function name
          }
        }
      ]
    }
  ],
  "systemInstruction": {                      // optional, separate from contents
    "role": "user",
    "parts": [{ "text": "You are helpful." }]
  },
  "generationConfig": {                       // optional
    "maxOutputTokens": 4096,
    "temperature": 0.7,                       // 0.0–2.0
    "topP": 0.95,
    "topK": 40,
    "stopSequences": ["\n\n"],
    "responseMimeType": "application/json",   // for structured output
    "responseSchema": { /* JSON Schema */ }
  },
  "safetySettings": [                         // optional
    {
      "category": "HARM_CATEGORY_HATE_SPEECH",
      "threshold": "BLOCK_MEDIUM_AND_ABOVE"
    }
  ],
  "tools": [                                  // optional
    {
      "functionDeclarations": [
        {
          "name": "get_weather",
          "description": "Get current weather",
          "parameters": {
            "type": "object",
            "properties": {
              "city": { "type": "string" }
            },
            "required": ["city"]
          }
        }
      ]
    }
  ],
  "toolConfig": {                             // optional
    "functionCallingConfig": {
      "mode": "AUTO",                         // "AUTO" | "ANY" | "NONE"
      "allowedFunctionNames": ["get_weather"] // optional restriction
    }
  }
}
```

### ABP Shim Struct: `GenerateContentRequest`

| Field | Type | ABP Mapping |
|-------|------|-------------|
| `model` | `String` | → `WorkOrder.config.model` |
| `contents` | `Vec<Content>` | → `IrConversation` via `lowering::to_ir()` |
| `system_instruction` | `Option<Content>` | → System message in IR |
| `generation_config` | `Option<GenerationConfig>` | → vendor config fields |
| `safety_settings` | `Option<Vec<SafetySetting>>` | → vendor.safety_settings |
| `tools` | `Option<Vec<ToolDeclaration>>` | → `IrToolDefinition` list |
| `tool_config` | `Option<ToolConfig>` | → vendor.tool_config |

## Response Format

### generateContent Response

```jsonc
{
  "candidates": [
    {
      "content": {
        "role": "model",
        "parts": [
          { "text": "Hello! How can I help?" }
        ]
      },
      "finishReason": "STOP"
    }
  ],
  "usageMetadata": {
    "promptTokenCount": 25,
    "candidatesTokenCount": 10,
    "totalTokenCount": 35
  }
}
```

### ABP Shim Struct: `GenerateContentResponse`

| Field | Type | ABP Source |
|-------|------|-----------|
| `candidates` | `Vec<Candidate>` | Built from `Receipt.trace` events |
| `usage_metadata` | `Option<UsageMetadata>` | From `Receipt.usage` |

### Helper Methods

- `response.text()` — Extract text from the first candidate's first text part.
- `response.function_calls()` — Extract all function calls as `(&str, &Value)` tuples.

## Streaming Format

**Protocol:** JSONL (newline-delimited JSON), **not** SSE.

**Content-Type:** `application/json` (each line is a complete JSON object)

Unlike OpenAI and Claude which use SSE (`event:` + `data:` lines), Gemini streams individual JSON objects, each being a complete `GenerateContentResponse`:

```jsonc
{"candidates":[{"content":{"role":"model","parts":[{"text":"Hel"}]},"finishReason":null}],"usageMetadata":null}
{"candidates":[{"content":{"role":"model","parts":[{"text":"lo!"}]},"finishReason":null}],"usageMetadata":null}
{"candidates":[{"content":{"role":"model","parts":[{"text":""}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3,"totalTokenCount":8}}
```

### ABP Shim Struct: `StreamEvent`

Same structure as `GenerateContentResponse` — each chunk is a complete response object with candidates and optional usage metadata.

### ABP Event Mapping

| Gemini Stream Content | ABP `AgentEventKind` |
|----------------------|----------------------|
| `parts[].text` (non-empty) | `AssistantDelta { text }` |
| `parts[].functionCall` | `ToolCall { tool_name, input }` |
| `finishReason: "STOP"` | `RunCompleted` |
| Final chunk with `usageMetadata` | Usage recorded |

## Tool Calling Conventions

- **Invocation:** Model returns `functionCall` parts within the `model` content.
- **Correlation:** By **function name** (no unique call ID like OpenAI/Claude).
- **Result:** User sends a `functionResponse` part with the same function name.
- **Arguments:** A **parsed JSON object** (like Claude, unlike OpenAI).
- **Parallel calls:** Multiple `functionCall` parts in a single response are supported.
- **Calling modes:**
  - `"AUTO"` — model decides (default)
  - `"ANY"` — must call at least one function
  - `"NONE"` — no function calls

### Key Differences from OpenAI/Claude

```
OpenAI:   Correlation by tool_call.id (unique per call)
Claude:   Correlation by tool_use.id (unique per call)
Gemini:   Correlation by function name (must be unique in a turn)
```

```
OpenAI:   arguments = JSON string    → must parse
Claude:   input = JSON object        → already parsed
Gemini:   args = JSON object         → already parsed
```

### ABP IR Mapping

```
Gemini functionCall.name     → IrToolCall.name
Gemini functionCall.args     → IrToolCall.input (already parsed)
Gemini functionResponse      → IrToolResult { name, response }
```

## System Message Handling

- System instruction is a **separate top-level field** (`systemInstruction`).
- It is structured as a `Content` object with `role: "user"` and text parts.
- Only one system instruction per request.

### Key Difference from OpenAI/Claude

```
OpenAI:   messages: [{ role: "system", content: "..." }]
Claude:   system: "..."
Gemini:   systemInstruction: { role: "user", parts: [{ text: "..." }] }
```

### ABP Mapping

```
Gemini systemInstruction.parts[].text → IrMessage { role: IrRole::System, ... }
```

## Token Counting

| Gemini Field | ABP `UsageNormalized` Field |
|-------------|----------------------------|
| `usageMetadata.promptTokenCount` | `input_tokens` |
| `usageMetadata.candidatesTokenCount` | `output_tokens` |
| `usageMetadata.totalTokenCount` | `input_tokens + output_tokens` |

**Note:** Gemini uses `camelCase` for all field names. No cache token breakdown in the standard API.

## Finish Reasons

| Gemini `finishReason` | Meaning | ABP `Outcome` |
|----------------------|---------|----------------|
| `"STOP"` | Natural completion | `Complete` |
| `"MAX_TOKENS"` | Token limit reached | `Partial` |
| `"SAFETY"` | Safety filter triggered | `Failed` |
| `"RECITATION"` | Recitation detected | `Failed` |
| `"OTHER"` | Other reason | `Failed` |
| `null` | Still generating | N/A |

**Note:** Gemini uses UPPER_SNAKE_CASE for enum values, unlike OpenAI/Claude's lowercase.

## Content Types

### Part Types (`Part` enum)

| Type | Variant | Fields | Notes |
|------|---------|--------|-------|
| Text | `Text(String)` | — | Primary content |
| Inline data | `InlineData` | `mime_type, data` | Base64 images/files |
| Function call | `FunctionCall` | `name, args` | Model tool invocation |
| Function response | `FunctionResponse` | `name, response` | User tool result |

### Supported Media Types

| Type | Support | Notes |
|------|---------|-------|
| Text | ✅ | Primary |
| Images (input) | ✅ | Via `inlineData` (base64) or file URI |
| Images (output) | ✅ | Imagen integration (separate API) |
| Video (input) | ✅ | Via file URI |
| Audio (input) | ✅ | Via file URI |
| PDFs (input) | ✅ | Via file URI or inlineData |
| Code execution | ✅ | Built-in `code_execution` tool |
| Tool calls | ✅ | `functionCall` parts |
| Tool results | ✅ | `functionResponse` parts |
| Structured output | ✅ | `responseMimeType` + `responseSchema` |

## Safety Settings

Gemini has a unique safety filtering system not present in OpenAI/Claude:

| Category | Description |
|----------|-------------|
| `HARM_CATEGORY_HATE_SPEECH` | Hate speech filtering |
| `HARM_CATEGORY_SEXUALLY_EXPLICIT` | Sexual content filtering |
| `HARM_CATEGORY_DANGEROUS_CONTENT` | Dangerous content filtering |
| `HARM_CATEGORY_HARASSMENT` | Harassment filtering |

| Threshold | Behavior |
|-----------|----------|
| `BLOCK_NONE` | No blocking |
| `BLOCK_LOW_AND_ABOVE` | Block low+ probability |
| `BLOCK_MEDIUM_AND_ABOVE` | Block medium+ (default) |
| `BLOCK_ONLY_HIGH` | Block only high probability |

## Model Names

| Model | Context Window | Notes |
|-------|---------------|-------|
| `gemini-2.5-flash` | 1M | Default in ABP shim |
| `gemini-2.5-pro` | 1M | Most capable |
| `gemini-2.0-flash` | 1M | Previous gen fast |
| `gemini-1.5-pro` | 2M | Long context |
| `gemini-1.5-flash` | 1M | Previous gen flash |

## Error Codes

| HTTP Status | Error Code | Description |
|-------------|-----------|-------------|
| 400 | `INVALID_ARGUMENT` | Malformed request |
| 401 | `UNAUTHENTICATED` | Invalid API key |
| 403 | `PERMISSION_DENIED` | Insufficient permissions |
| 404 | `NOT_FOUND` | Unknown model |
| 429 | `RESOURCE_EXHAUSTED` | Rate limit / quota exceeded |
| 500 | `INTERNAL` | Internal server error |
| 503 | `UNAVAILABLE` | Service unavailable |

### ABP Shim Error Types (`GeminiError`)

| Variant | When |
|---------|------|
| `RequestConversion(String)` | Request conversion failed |
| `ResponseConversion(String)` | Response conversion failed |
| `BackendError(String)` | Backend returned failure |
| `Serde(serde_json::Error)` | Serialization error |

### ABP Error Mapping

| Gemini Error | ABP `ErrorCode` |
|-------------|-----------------|
| `INVALID_ARGUMENT` | `IR_LOWERING_FAILED` or `CONFIG_INVALID` |
| `UNAUTHENTICATED` | `BACKEND_CRASHED` |
| `RESOURCE_EXHAUSTED` | `BACKEND_TIMEOUT` |
| `INTERNAL` | `BACKEND_CRASHED` |

## Naming Convention Differences

Gemini uses `camelCase` throughout, unlike OpenAI/Claude's `snake_case`:

| Concept | OpenAI | Claude | Gemini |
|---------|--------|--------|--------|
| Max tokens | `max_tokens` | `max_tokens` | `maxOutputTokens` |
| Stop sequences | `stop` | `stop_sequences` | `stopSequences` |
| Finish reason | `finish_reason` | `stop_reason` | `finishReason` |
| Token usage | `prompt_tokens` | `input_tokens` | `promptTokenCount` |
| Tool args | `arguments` (string) | `input` (object) | `args` (object) |
| Model role | `"assistant"` | `"assistant"` | `"model"` |
| System msg | in messages array | top-level `system` | top-level `systemInstruction` |
