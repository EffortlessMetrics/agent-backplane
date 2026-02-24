# Codex Sidecar for Agent Backplane

This directory contains the Codex sidecar implementation for Agent Backplane (ABP). The sidecar supports two execution modes:

- **Passthrough Mode**: Codex dialect → Codex engine (no transformation)
- **Mapped Mode**: Codex dialect → Claude engine (opinionated mapping)

## Quick Start

```bash
# Run the sidecar (speaks JSONL over stdio)
node host.js

# With custom adapter
ABP_CODEX_ADAPTER_MODULE=./my-adapter.js node host.js
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Codex Sidecar                            │
├─────────────────────────────────────────────────────────────┤
│  host.js - Main entry point (JSONL protocol, policy, etc.)  │
├─────────────────────────────────────────────────────────────┤
│                    ┌──────────────┐                          │
│                    │  Mode Check  │                          │
│                    └──────┬───────┘                          │
│           ┌───────────────┴───────────────┐                  │
│           ▼                               ▼                  │
│   ┌───────────────┐               ┌───────────────┐         │
│   │  Passthrough  │               │    Mapped     │         │
│   │     Mode      │               │     Mode      │         │
│   └───────┬───────┘               └───────┬───────┘         │
│           │                               │                  │
│           ▼                               ▼                  │
│   ┌───────────────┐               ┌───────────────┐         │
│   │  adapter.js   │               │   mapper.js   │         │
│   │ (Codex SDK)   │               │ (Codex→Claude)│         │
│   └───────────────┘               └───────┬───────┘         │
│                                           │                  │
│                                           ▼                  │
│                                   ┌───────────────┐         │
│                                   │ Claude Adapter│         │
│                                   └───────────────┘         │
├─────────────────────────────────────────────────────────────┤
│  capabilities.js - Capability manifest and tool mappings    │
└─────────────────────────────────────────────────────────────┘
```

## Files

| File | Description |
|------|-------------|
| [`host.js`](host.js) | Main sidecar entry point - handles JSONL protocol, policy enforcement, and mode routing |
| [`adapter.js`](adapter.js) | Codex SDK adapter - handles passthrough mode execution |
| [`mapper.js`](mapper.js) | Codex→Claude mapping logic - transforms requests and validates capabilities |
| [`capabilities.js`](capabilities.js) | Capability manifest - defines supported features and tool mappings |
| [`test/mock-adapter.js`](test/mock-adapter.js) | Mock adapter for testing |
| [`test/passthrough.test.js`](test/passthrough.test.js) | Passthrough mode conformance tests |
| [`test/mapped.test.js`](test/mapped.test.js) | Mapped mode conformance tests |

## Execution Modes

### Passthrough Mode (Codex → Codex)

In passthrough mode, requests are forwarded to the Codex SDK unchanged:

- **No request rewriting**: SDK sees exactly what caller sent
- **Stream equivalence**: Response stream is unchanged after removing ABP framing
- **Observer-only governance**: Tool calls are logged but not modified
- **Receipt out-of-band**: Receipt doesn't appear in the stream

Enable passthrough mode by setting `config.vendor.abp.mode` to `"passthrough"`:

```json
{
  "t": "run",
  "id": "run-123",
  "work_order": {
    "config": {
      "vendor": {
        "abp": {
          "mode": "passthrough",
          "request": {
            "prompt": "Hello, Codex!",
            "model": "gpt-4",
            "tools": ["code_execution"]
          }
        }
      }
    }
  }
}
```

### Mapped Mode (Codex → Claude)

In mapped mode, Codex-style requests are transformed to Claude format:

- **Two-stage validation**: Facade validation + runtime capability check
- **Opinionated mapping**: Features map with explicit semantics
- **Early failures**: Unsupported features fail immediately with typed errors
- **Mapping metadata**: Receipt includes mapping details

Enable mapped mode by setting `config.vendor.abp.mode` to `"mapped"` (or omit for default):

```json
{
  "t": "run",
  "id": "run-456",
  "work_order": {
    "config": {
      "vendor": {
        "abp": {
          "mode": "mapped",
          "request": {
            "prompt": "Hello, Claude!",
            "model": "gpt-4",
            "tools": ["read_file", "shell"]
          }
        }
      }
    }
  }
}
```

## Tool Mappings

The following tool mappings are applied in mapped mode:

| Codex Tool | Claude Tool | Support Level |
|------------|-------------|---------------|
| `read_file` / `file_read` | `Read` | Native |
| `write_file` / `file_write` | `Write` | Native |
| `edit_file` | `Edit` | Native |
| `code_execution` / `shell` / `execute` | `Bash` | Native |
| `glob` | `Glob` | Native |
| `grep` / `search_files` | `Grep` | Native |
| `web_search` | `WebSearch` | Native |
| `web_fetch` / `browser` | `WebFetch` | Native |
| `memory` | `Memory` | Emulated |
| `subagent` / `delegate` | `Task` | Native |
| `todo_write` | `TodoWrite` | Native |

## Model Mappings

Codex models are mapped to Claude equivalents:

| Codex Model | Claude Model |
|-------------|--------------|
| `gpt-4` / `gpt-4-turbo` / `gpt-4o` | `claude-3-5-sonnet-20241022` |
| `gpt-4o-mini` / `gpt-3.5-turbo` | `claude-3-5-haiku-20241022` |
| `o1` / `o1-preview` | `claude-3-5-sonnet-20241022` |
| `o1-mini` | `claude-3-5-haiku-20241022` |
| (default) | `claude-3-5-sonnet-20241022` |

## Error Taxonomy

Mapped mode uses the following error codes:

| Code | Name | Description |
|------|------|-------------|
| E001 | UnsupportedFeature | Feature not supported by target engine |
| E002 | UnsupportedTool | Tool has no equivalent in target engine |
| E003 | AmbiguousMapping | Mapping is ambiguous, requires clarification |
| E004 | RequiresInteractiveApproval | Operation requires user approval |
| E005 | UnsafeByPolicy | Operation denied by policy |
| E006 | BackendCapabilityMissing | Backend lacks required capability |
| E007 | BackendUnavailable | Backend is not available |

### Unsupported Features

The following Codex features are **not supported** in mapped mode and will fail early:

| Feature | Reason | Suggestion |
|---------|--------|------------|
| `function_call` (deprecated) | Legacy format not supported | Use `tools` format |
| `assistant_id` / `run_id` | Assistants API not supported | Use Claude sessions |
| `retrieval` / `file_search` | Codex retrieval not available | Use MCP servers |
| `codex_thread_model` | Different session model | Use ABP session mapping |

### Emulated Features

The following features are **emulated** via ABP:

| Feature | Emulation | Fidelity |
|---------|-----------|----------|
| Thread resume | Session ID mapping | High |
| Code execution | Bash tool | High |
| JSON mode | Structured output | Medium |

## Receipt Structure

### Passthrough Mode Receipt

```json
{
  "contract_version": "abp/v0.1",
  "run_id": "run-123",
  "mode": "passthrough",
  "source_dialect": "codex",
  "target_engine": "codex",
  "status": "completed",
  "ext": {
    "raw_request": { ... },
    "mode_detail": "passthrough_codex_to_codex"
  }
}
```

### Mapped Mode Receipt

```json
{
  "contract_version": "abp/v0.1",
  "run_id": "run-456",
  "mode": "mapped",
  "source_dialect": "codex",
  "target_engine": "claude",
  "status": "completed",
  "mapping_warnings": [],
  "capabilities_used": {
    "native": ["streaming", "tool_read"],
    "emulated": ["session_resume"],
    "unsupported": []
  },
  "session_mapping": {
    "codex_thread_id": "thread_abc",
    "claude_session_id": "codex_thread:thread_abc"
  },
  "model_mapping": {
    "original": "gpt-4",
    "mapped": "claude-3-5-sonnet-20241022"
  },
  "tool_mappings": [
    {
      "codex_tool": "read_file",
      "claude_tool": "Read",
      "support_level": "native"
    }
  ]
}
```

## Testing

```bash
# Run passthrough mode tests
node hosts/codex/test/passthrough.test.js

# Run mapped mode tests (using Node.js test runner)
node --test hosts/codex/test/mapped.test.js

# Run with mock adapter
ABP_CODEX_ADAPTER_MODULE=hosts/codex/test/mock-adapter.js node hosts/codex/host.js
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ABP_CODEX_ADAPTER_MODULE` | Path to custom adapter module | (built-in) |
| `ABP_CODEX_MAX_INLINE_OUTPUT_BYTES` | Max inline output size | 8192 |
| `ABP_CODEX_CMD` | Codex CLI command | `codex` |
| `ABP_CLAUDE_ADAPTER_MODULE` | Path to Claude adapter (for mapped mode) | (built-in) |

## Adapter Contract

Custom adapters must export:

```javascript
module.exports = {
  name: "my_codex_adapter",
  version: "1.0.0",
  async run(ctx) {
    const {
      workOrder,      // ABP work order
      sdkOptions,     // Processed SDK options
      policy,         // Policy engine
      emitAssistantDelta,    // (text) => void
      emitAssistantMessage,  // (text) => void
      emitToolCall,          // ({toolName, toolUseId, input}) => void
      emitToolResult,        // ({toolName, toolUseId, output, isError}) => void
      emitWarning,           // (message) => void
      emitError,             // (message) => void
      writeArtifact,         // async (kind, name, content) => id
    } = ctx;
    
    // Execute and emit events
  }
};
```

## Thread → Session Mapping

In mapped mode, Codex thread IDs are mapped to Claude session IDs:

```
Codex: thread_abc123 → Claude: codex_thread:thread_abc123
```

This mapping is:
- **Tracked in receipt**: Both IDs are recorded
- **Reversible**: Responses include the original thread ID
- **Consistent**: Same thread ID always maps to same session ID

## Related Documentation

- [Dialect×Engine Matrix](../../docs/dialect_engine_matrix.md) - Design specification
- [Sidecar Protocol](../../docs/sidecar_protocol.md) - JSONL protocol reference
- [Claude Host](../claude/README.md) - Claude sidecar implementation
- [Gemini Host](../gemini/README.md) - Gemini sidecar implementation (Claude→Gemini mapping reference)
