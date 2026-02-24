# Gemini Sidecar for Agent Backplane

This directory contains the Gemini sidecar implementation for Agent Backplane (ABP). It operates in **mapped mode**, translating Claude-style dialect requests to the Gemini engine.

## Overview

The Gemini sidecar implements the opinionated mapping from Claude-style SDK conventions to Google's Gemini CLI/backend. It provides:

- **Two-stage validation**: Facade validation before backend interaction, then runtime capability checking
- **Early failure guarantee**: Unsupported features fail immediately with typed error codes
- **Emulation layer**: Non-native features are emulated via ABP infrastructure
- **Structured receipts**: Complete execution records with mapping metadata

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Gemini Sidecar                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐      │
│  │   host.js    │───▶│   mapper.js  │───▶│  adapter.js  │      │
│  │              │    │              │    │              │      │
│  │ - JSONL      │    │ - Claude→    │    │ - Gemini CLI │      │
│  │   protocol   │    │   Gemini     │    │   spawning   │      │
│  │ - Policy     │    │ - Error      │    │ - Event      │      │
│  │   engine     │    │   taxonomy   │    │   parsing    │      │
│  │ - Receipt    │    │ - Capability │    │ - Stream     │      │
│  │   generation │    │   validation │    │   handling   │      │
│  └──────────────┘    └──────────────┘    └──────────────┘      │
│         │                   │                   │               │
│         └───────────────────┴───────────────────┘               │
│                             │                                   │
│                    ┌────────▼────────┐                         │
│                    │ capabilities.js │                         │
│                    │                 │                         │
│                    │ - Support levels│                         │
│                    │ - Tool mappings │                         │
│                    │ - Feature list  │                         │
│                    └─────────────────┘                         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Files

| File | Purpose |
|------|---------|
| [`host.js`](host.js) | Main sidecar entry point - handles JSONL protocol, policy, receipts |
| [`mapper.js`](mapper.js) | Claude→Gemini mapping logic with error taxonomy |
| [`adapter.js`](adapter.js) | Gemini CLI/SDK interface |
| [`capabilities.js`](capabilities.js) | Gemini capability manifest and tool mappings |
| [`test/mapped.test.js`](test/mapped.test.js) | Conformance tests for mapped mode |
| [`test/mock-adapter.js`](test/mock-adapter.js) | Mock adapter for testing |

## Mapping Strategy

### Direct Mapping (Native)

These Claude features map directly to Gemini equivalents:

| Claude | Gemini | Notes |
|--------|--------|-------|
| `Read` | `read_file` | Direct equivalent |
| `Write` | `write_file` | Direct equivalent |
| `Edit` | `edit_file` | Direct equivalent |
| `Bash` | `shell` | Direct equivalent |
| `Glob` | `glob` | Direct equivalent |
| `Grep` | `grep` | Direct equivalent |
| `WebSearch` | `web_search` | Uses Gemini grounding |
| `WebFetch` | `web_fetch` | Direct equivalent |

### Emulated Mapping

These features are emulated via ABP infrastructure:

| Feature | Emulation Method | Fidelity |
|---------|------------------|----------|
| `hooks` | ABP policy enforcement layer | High |
| `checkpointing` | ABP workspace snapshots | Medium |
| `memory` | ABP-owned jailed memory server | High |
| `session_resume` | Checkpoint file restoration | Medium |
| `mcp_client` | ABP MCP gateway | High |

### Unsupported (Fail Early)

These features have no equivalent and will cause immediate failure:

| Feature | Reason | Suggestion |
|---------|--------|------------|
| `extended_thinking` | Claude-specific reasoning | Use Gemini's native reasoning |
| `agent_teams` | Different subagent model | Use Gemini subagents |
| `context_compaction` | Different mechanism | Rely on 1M context window |
| `claude_session_semantics` | Different session model | Use ABP session emulation |

## Error Taxonomy

The mapper uses structured error codes from the dialect×engine matrix:

| Code | Name | HTTP Status | Retryable | Description |
|------|------|-------------|-----------|-------------|
| `E001` | UnsupportedFeature | 400 | No | Feature exists in dialect but not in engine |
| `E002` | UnsupportedTool | 400 | No | Tool not available in target engine |
| `E003` | AmbiguousMapping | 400 | No | Cannot uniquely map the request |
| `E004` | RequiresInteractiveApproval | 403 | Yes | Operation requires user approval |
| `E005` | UnsafeByPolicy | 403 | No | Operation blocked by policy |
| `E006` | BackendCapabilityMissing | 501 | No | Backend missing required capability |
| `E007` | BackendUnavailable | 503 | Yes | Backend unavailable |

### Error Response Format

```json
{
  "code": "E001",
  "name": "UnsupportedFeature",
  "message": "Extended thinking is not supported by Gemini backend",
  "feature": "extended_thinking",
  "dialect": "claude",
  "engine": "gemini",
  "suggestion": "Use Gemini's native reasoning capabilities",
  "documentation_url": "https://docs.abp.dev/errors/E001",
  "timestamp": "2024-01-15T10:30:00Z"
}
```

## Usage

### Starting the Sidecar

```bash
# Using Node.js directly
node hosts/gemini/host.js

# With custom adapter
ABP_GEMINI_ADAPTER_MODULE=./my-adapter.js node hosts/gemini/host.js

# With debug logging
DEBUG=abp:* node hosts/gemini/host.js
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ABP_GEMINI_ADAPTER_MODULE` | Path to custom adapter | `./adapter.js` |
| `ABP_GEMINI_CMD` | Gemini CLI command | `gemini` |
| `ABP_GEMINI_MAX_INLINE_OUTPUT_BYTES` | Max inline artifact size | `8192` |
| `GEMINI_API_KEY` | Gemini API key | - |
| `GOOGLE_APPLICATION_CREDENTIALS` | GCP credentials path | - |

### Protocol Handshake

The sidecar sends a `hello` envelope on startup:

```json
{
  "t": "hello",
  "contract_version": "abp/v0.1",
  "backend": {
    "name": "gemini",
    "version": "0.1.0"
  },
  "capabilities": {
    "backend": "gemini",
    "version": "1.0.0",
    "capabilities": { ... }
  },
  "mode": "mapped"
}
```

### Running a Task

Send a `run` envelope:

```json
{
  "t": "run",
  "id": "run_abc123",
  "work_order": {
    "task": "Read the main.rs file and explain its structure",
    "workspace_root": "/path/to/project",
    "policy": {
      "allowed_tools": ["Read", "Glob", "Grep"]
    }
  }
}
```

### Receipt Structure

The final receipt includes mapped mode metadata:

```json
{
  "t": "final",
  "ref_id": "run_abc123",
  "receipt": {
    "id": "run_abc123",
    "status": "success",
    "mode": "mapped",
    "source_dialect": "claude",
    "target_engine": "gemini",
    "mapping_warnings": [...],
    "capabilities_used": {
      "native": ["streaming", "tools", "tool_read"],
      "emulated": ["hooks_pre_tool_use", "hooks_post_tool_use"],
      "unsupported": []
    },
    "receipt_sha256": "..."
  }
}
```

## Two-Stage Validation

### Stage 1: Facade Validation

Occurs before any backend interaction:

```javascript
const result = validateFacade(claudeRequest);
if (!result.valid) {
  // Return early error - no backend call made
  return result.errors[0];
}
```

Checks:
- Request format validity
- Unsupported features (extended_thinking, agent_teams, etc.)
- Unsupported tools
- Claude-specific session semantics

### Stage 2: Runtime Capability Check

Occurs after mapping, before execution:

```javascript
const result = validateCapabilities(geminiRequest, backendCapabilities);
if (!result.valid) {
  // Return typed error
  return result.errors[0];
}
```

Checks:
- Backend capability availability
- Native vs emulated support levels
- Required native capabilities

## Testing

Run the conformance tests:

```bash
# Run all tests
node --test hosts/gemini/test/

# Run specific test file
node --test hosts/gemini/test/mapped.test.js

# Run with verbose output
node --test --test-reporter=tap hosts/gemini/test/
```

### Test Categories

1. **Facade Validation Tests**: Verify early failure for unsupported features
2. **Mapping Tests**: Verify correct translation of Claude→Gemini
3. **Capability Tests**: Verify capability detection and validation
4. **Receipt Tests**: Verify receipt structure and hash computation

## Custom Adapters

Create a custom adapter by implementing the adapter contract:

```javascript
// my-gemini-adapter.js
module.exports = {
  name: "my_gemini_adapter",
  version: "1.0.0",
  
  async run(ctx) {
    const { workOrder, sdkOptions, emitAssistantDelta, emitToolCall, emitToolResult } = ctx;
    
    // Your implementation here
    // - Spawn Gemini process or use SDK
    // - Parse events and emit via callbacks
    // - Handle errors gracefully
  }
};
```

### Adapter Context

The `ctx` object provides:

| Property | Type | Description |
|----------|------|-------------|
| `workOrder` | object | Original work order |
| `sdkOptions` | object | Mapped Gemini options |
| `policy` | object | Policy configuration |
| `policyEngine` | object | Policy checking functions |
| `emitAssistantDelta(text)` | function | Emit text delta |
| `emitAssistantMessage(text)` | function | Emit complete message |
| `emitToolCall({...})` | function | Emit tool invocation |
| `emitToolResult({...})` | function | Emit tool result |
| `emitWarning(message)` | function | Emit warning event |
| `emitError(message)` | function | Emit error event |
| `writeArtifact(kind, name, content)` | function | Write artifact |
| `log(message)` | function | Log to stderr |

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
