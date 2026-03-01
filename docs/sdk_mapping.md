# SDK Mapping Architecture

> Comprehensive reference for the Agent Backplane SDK translation layer.

## Overview

The Agent Backplane (ABP) SDK mapping layer is a **translation framework** that converts between vendor-specific AI agent API formats and a single canonical contract. This enables work orders to be authored once and routed to _any_ supported backend — Claude, Codex, Gemini, Kimi, or future vendors — without rewriting client code.

The mapping layer solves three problems:

1. **Format divergence** — each vendor uses different JSON shapes for requests, responses, tool calls, and streaming events.
2. **Semantic drift** — tool names, event lifecycle labels, and capability surface areas differ across SDKs.
3. **Capability heterogeneity** — not every vendor supports every feature; ABP tracks what's native, emulated, or unsupported per backend.

## Architecture Diagram

```
                              ┌─────────────────────────────┐
                              │       ABP Contract          │
                              │  WorkOrder · Receipt · Event│
                              └─────────────┬───────────────┘
                                            │
                             ┌──────────────┼──────────────┐
                             │    Projection Matrix         │
                             │  (abp-integrations/          │
                             │   projection.rs)             │
                             │                              │
                             │  WorkOrder translation       │
                             │  Tool name mapping           │
                             │  Event kind mapping          │
                             └──┬───────┬───────┬───────┬──┘
                                │       │       │       │
              ┌─────────────────┤       │       │       └──────────────────┐
              │                 │       │       │                          │
   ┌──────────▼──────────┐ ┌───▼───────▼──┐ ┌─▼──────────────┐ ┌─────────▼─────────┐
   │   abp-claude-sdk    │ │abp-codex-sdk │ │ abp-gemini-sdk │ │   abp-kimi-sdk    │
   │                     │ │              │ │                │ │                   │
   │ ClaudeRequest       │ │ CodexRequest │ │ GeminiRequest  │ │ KimiRequest       │
   │ ClaudeResponse      │ │ CodexResponse│ │ GeminiResponse │ │ KimiResponse      │
   │ ClaudeToolDef       │ │ CodexToolDef │ │ GeminiFuncDecl │ │ KimiToolDef       │
   │                     │ │              │ │                │ │                   │
   │ map_work_order()    │ │map_work_order│ │ map_work_order │ │ map_work_order()  │
   │ map_response()      │ │map_response()│ │ map_response() │ │ map_response()    │
   └──────────┬──────────┘ └──────┬───────┘ └───────┬────────┘ └─────────┬─────────┘
              │                   │                 │                    │
   ┌──────────▼──────────┐ ┌─────▼────────┐ ┌──────▼─────────┐ ┌───────▼──────────┐
   │  Anthropic Messages │ │ OpenAI       │ │ Gemini         │ │ Moonshot Kimi    │
   │  API                │ │ Responses API│ │ generateContent│ │ Chat Completions │
   └─────────────────────┘ └──────────────┘ └────────────────┘ └──────────────────┘
```

## Supported Vendors

| Vendor | SDK Crate | API Format | Default Model | Dialect Version | Mapping Completeness |
|--------|-----------|------------|---------------|-----------------|---------------------|
| **Anthropic Claude** | `abp-claude-sdk` | Messages API (`/v1/messages`) | `claude-sonnet-4-20250514` | `claude/v0.1` | ✅ Full |
| **OpenAI Codex** | `abp-codex-sdk` | Responses API (`/v1/responses`) | `codex-mini-latest` | `codex/v0.1` | ✅ Full |
| **OpenAI Chat** | `abp-openai-sdk` | Chat Completions (`/v1/chat/completions`) | `gpt-4` | `openai/v0.1` | ✅ Full |
| **Google Gemini** | `abp-gemini-sdk` | generateContent (`/v1beta`) | `gemini-2.5-flash` | `gemini/v0.1` | ✅ Full |
| **Moonshot Kimi** | `abp-kimi-sdk` | Chat Completions (`/v1/chat/completions`) | `moonshot-v1-8k` | `kimi/v0.1` | ✅ Full |
| **GitHub Copilot** | `abp-copilot-sdk` | Copilot Extensions API | — | — | 🚧 Scaffold |

## Contract Types

The canonical contract lives in `abp-core` and is the single source of truth for all data flowing through ABP.

### WorkOrder

A single unit of work (intentionally not a chat session). Key fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `Uuid` | Unique identifier |
| `task` | `String` | Human intent / prompt |
| `lane` | `ExecutionLane` | `patch_first` or `workspace_first` |
| `workspace` | `WorkspaceSpec` | Root path, staging mode, include/exclude globs |
| `context` | `ContextPacket` | Pre-loaded files and named snippets |
| `policy` | `PolicyProfile` | Tool allow/deny lists, path restrictions |
| `requirements` | `CapabilityRequirements` | What the backend must support |
| `config` | `RuntimeConfig` | Model, budget caps, vendor flags, env vars |

### Receipt

The outcome of a completed run. Includes timing metadata, backend identity, capability manifest, ordered trace of `AgentEvent`s, artifacts, verification data, and a canonical SHA-256 hash.

**Hashing gotcha**: `receipt_hash()` sets `receipt_sha256` to `null` before hashing to prevent self-referential hash. Always use `Receipt::with_hash()`.

### AgentEvent

A timestamped event emitted during a run. Discriminated by `AgentEventKind`:

| Kind | Description |
|------|-------------|
| `RunStarted` | Agent run has started |
| `RunCompleted` | Agent run has completed |
| `AssistantDelta` | Incremental streaming text token |
| `AssistantMessage` | Complete assistant message |
| `ToolCall` | Tool invocation (name, id, input) |
| `ToolResult` | Tool result (name, id, output, is_error) |
| `FileChanged` | Workspace file created/modified |
| `CommandExecuted` | Shell command executed |
| `Warning` | Non-fatal warning |
| `Error` | Fatal error |

Events use `#[serde(tag = "type")]` for discrimination (distinct from the protocol envelope which uses `#[serde(tag = "t")]`).

## Dialect Pattern

Each vendor SDK crate implements a consistent dialect pattern with these components:

### 1. Model Name Mapping

Bidirectional conversion between vendor model names and ABP canonical form:

```
to_canonical_model("claude-sonnet-4-20250514")  → "anthropic/claude-sonnet-4-20250514"
from_canonical_model("anthropic/claude-sonnet-4-20250514") → "claude-sonnet-4-20250514"
```

Each vendor uses its own prefix:

| Vendor | Canonical Prefix | Example |
|--------|-----------------|---------|
| Claude | `anthropic/` | `anthropic/claude-sonnet-4-20250514` |
| Codex | `openai/` | `openai/codex-mini-latest` |
| Gemini | `google/` | `google/gemini-2.5-flash` |
| Kimi | `moonshot/` | `moonshot/moonshot-v1-8k` |

### 2. Capability Manifest

Each dialect provides a `capability_manifest()` function returning a `BTreeMap<Capability, SupportLevel>` describing what the backend supports. Support levels are:

- **`Native`** — first-class built-in support
- **`Emulated`** — support via adapter or polyfill layer
- **`Unsupported`** — capability not available
- **`Restricted { reason }`** — supported but disabled by policy

### 3. Request Mapping (`map_work_order`)

Converts an ABP `WorkOrder` + vendor-specific `Config` into the vendor's native request format:

```rust
// Claude
fn map_work_order(wo: &WorkOrder, config: &ClaudeConfig) -> ClaudeRequest;

// Codex
fn map_work_order(wo: &WorkOrder, config: &CodexConfig) -> CodexRequest;

// Gemini
fn map_work_order(wo: &WorkOrder, config: &GeminiConfig) -> GeminiRequest;

// Kimi
fn map_work_order(wo: &WorkOrder, config: &KimiConfig) -> KimiRequest;
```

All implementations follow the same logic:
1. Use `wo.config.model` if set, otherwise fall back to the config's default model.
2. Build user content from `wo.task` plus any `wo.context.snippets`.
3. Apply vendor-specific config (max tokens, temperature, system prompt, etc.).

### 4. Response Mapping (`map_response`)

Converts a vendor response back into a sequence of canonical `AgentEvent`s:

```rust
fn map_response(resp: &ClaudeResponse) -> Vec<AgentEvent>;
fn map_response(resp: &CodexResponse) -> Vec<AgentEvent>;
fn map_response(resp: &GeminiResponse) -> Vec<AgentEvent>;
fn map_response(resp: &KimiResponse) -> Vec<AgentEvent>;
```

Text content blocks → `AssistantMessage`. Tool use/function call blocks → `ToolCall`.

### 5. Tool Definition Translation

Bidirectional conversion between the ABP `CanonicalToolDef` and each vendor's native format:

```rust
// ABP canonical form
struct CanonicalToolDef {
    name: String,
    description: String,
    parameters_schema: serde_json::Value,
}

// Convert to/from vendor format
fn tool_def_to_claude(def: &CanonicalToolDef) -> ClaudeToolDef;
fn tool_def_from_claude(def: &ClaudeToolDef) -> CanonicalToolDef;
```

## Projection Matrix

The `ProjectionMatrix` in `crates/abp-integrations/src/projection.rs` is the central routing engine. It operates at two levels:

### Level 1: WorkOrder Translation (`Dialect` enum)

Translates a `WorkOrder` from one `Dialect` to another, producing vendor-specific request JSON:

```rust
pub enum Dialect { Abp, Claude, Codex, Gemini, Kimi }

// Translate ABP → Claude
matrix.translate(Dialect::Abp, Dialect::Claude, &work_order)?;
// Returns: { "model": "...", "max_tokens": 4096, "messages": [...] }
```

**v0.1 supports:**
- Identity translations (same dialect in and out)
- ABP → vendor translations (ABP `WorkOrder` to vendor request JSON)

### Level 2: Tool & Event Translation (string-based dialect names)

Maps tool names and event kinds between dialects using string identifiers (`"abp"`, `"openai"`, `"anthropic"`, `"gemini"`):

```rust
// Tool call translation
matrix.translate_tool_call("anthropic", "openai", &tool_call)?;

// Tool result translation
matrix.translate_tool_result("openai", "gemini", &tool_result)?;

// Event translation
matrix.translate_event("gemini", "abp", &agent_event)?;
```

Tool names without an explicit mapping pass through unchanged.

## Tool Mapping

Tool names are translated bidirectionally between all dialect pairs. The full mapping table:

| ABP (canonical) | OpenAI (Codex) | Anthropic (Claude) | Gemini | Description |
|-----------------|----------------|-------------------|--------|-------------|
| `read_file` | `file_read` | `Read` | `readFile` | Read file contents |
| `write_file` | `file_write` | `Write` | `writeFile` | Write new file |
| `edit_file` | `apply_diff` | `Edit` | `editFile` | Edit existing file |
| `bash` | `shell` | `Bash` | `executeCommand` | Execute shell command |
| `glob` | `file_search` | `Glob` | `searchFiles` | Search files by pattern |

**Naming conventions by vendor:**
- **ABP**: `snake_case` — `read_file`, `write_file`
- **OpenAI**: `snake_case` — `file_read`, `file_write` (noun-first)
- **Anthropic**: `PascalCase` — `Read`, `Write`, `Bash`
- **Gemini**: `camelCase` — `readFile`, `writeFile`, `executeCommand`

### Tool Definition Format Differences

| Vendor | Wrapper | Schema Field | Example |
|--------|---------|-------------|---------|
| **ABP** | `CanonicalToolDef` | `parameters_schema` | `{ name, description, parameters_schema }` |
| **Claude** | `ClaudeToolDef` | `input_schema` | `{ name, description, input_schema }` |
| **Codex** | `CodexToolDef` | `function.parameters` | `{ type: "function", function: { name, description, parameters } }` |
| **Gemini** | `GeminiFunctionDeclaration` | `parameters` | `{ name, description, parameters }` |
| **Kimi** | `KimiToolDef` | `function.parameters` | `{ type: "function", function: { name, description, parameters } }` |

## Streaming Events

Streaming event kinds are mapped between dialects via the projection matrix's event mapping tables:

| ABP (canonical) | OpenAI | Anthropic | Gemini |
|----------------|--------|-----------|--------|
| `run_started` | `response.created` | `message_start` | `generate_content_start` |
| `run_completed` | `response.completed` | `message_stop` | `generate_content_end` |
| `assistant_message` | `response.output_text.done` | `content_block_stop` | `text` |
| `assistant_delta` | `response.output_text.delta` | `content_block_delta` | `text_delta` |
| `tool_call` | `function_call` | `tool_use` | `function_call` |
| `tool_result` | `function_call_output` | `tool_result` | `function_response` |

All dialect pairs have bidirectional mappings registered (ABP↔OpenAI, ABP↔Anthropic, ABP↔Gemini, OpenAI↔Anthropic, OpenAI↔Gemini, Anthropic↔Gemini).

## Capability Matrix

Comparison of capability support across vendors (as declared by each `capability_manifest()`):

| Capability | Claude | Codex | Gemini | Kimi |
|-----------|--------|-------|--------|------|
| **Streaming** | ✅ Native | ✅ Native | ✅ Native | ✅ Native |
| **ToolRead** | ✅ Native | ✅ Native | ✅ Native | ✅ Native |
| **ToolWrite** | ✅ Native | ✅ Native | ⚡ Emulated | ⚡ Emulated |
| **ToolEdit** | ✅ Native | ✅ Native | ⚡ Emulated | ❌ Unsupported |
| **ToolBash** | ✅ Native | ✅ Native | ⚡ Emulated | ❌ Unsupported |
| **ToolGlob** | ✅ Native | ⚡ Emulated | ❌ Unsupported | — |
| **ToolGrep** | ✅ Native | ⚡ Emulated | ❌ Unsupported | — |
| **ToolWebSearch** | ✅ Native | — | — | ✅ Native |
| **ToolWebFetch** | ✅ Native | — | — | — |
| **StructuredOutput** | ✅ Native | ✅ Native | ✅ Native | ⚡ Emulated |
| **Hooks (Pre/Post)** | ✅ Native | ⚡ Emulated | — | — |
| **MCP Client** | ✅ Native | ❌ Unsupported | ❌ Unsupported | ❌ Unsupported |
| **MCP Server** | ❌ Unsupported | ❌ Unsupported | ❌ Unsupported | ❌ Unsupported |
| **Checkpointing** | ⚡ Emulated | — | — | — |

Legend: ✅ Native · ⚡ Emulated · ❌ Unsupported · — Not declared

## Execution Modes

ABP supports two execution modes set via `work_order.config.vendor.abp.mode`:

| Mode | Description | Use Case |
|------|-------------|----------|
| **Passthrough** | Lossless wrapping — ABP acts as observer/recorder only. No request rewriting. Stream is bitwise-equivalent to direct SDK call after removing ABP framing. | Same-dialect routing (Claude→Claude) |
| **Mapped** (default) | Full dialect translation between different agent dialects. ABP translates requests and responses. | Cross-dialect routing (Claude→Gemini) |

## Adding a New Vendor

Follow these steps to add a new vendor SDK adapter:

### Step 1: Create the SDK Crate

```bash
cargo new crates/abp-<vendor>-sdk --lib
```

Add the crate to the workspace `Cargo.toml` and add `abp-core` as a dependency.

### Step 2: Implement the Dialect Module

Create `crates/abp-<vendor>-sdk/src/dialect.rs` with:

```rust
// Required constants
pub const DIALECT_VERSION: &str = "<vendor>/v0.1";
pub const DEFAULT_MODEL: &str = "<default-model>";

// Model name mapping
pub fn to_canonical_model(vendor_model: &str) -> String;
pub fn from_canonical_model(canonical: &str) -> String;
pub fn is_known_model(model: &str) -> bool;

// Capability manifest
pub fn capability_manifest() -> CapabilityManifest;

// Tool definition translation
pub fn tool_def_to_<vendor>(def: &CanonicalToolDef) -> <Vendor>ToolDef;
pub fn tool_def_from_<vendor>(def: &<Vendor>ToolDef) -> CanonicalToolDef;

// Request/response types
pub struct <Vendor>Config { ... }
pub struct <Vendor>Request { ... }
pub struct <Vendor>Response { ... }

// Mapping functions
pub fn map_work_order(wo: &WorkOrder, config: &<Vendor>Config) -> <Vendor>Request;
pub fn map_response(resp: &<Vendor>Response) -> Vec<AgentEvent>;
```

### Step 3: Register in the Projection Matrix

In `crates/abp-integrations/src/projection.rs`:

1. Add a variant to the `Dialect` enum:
   ```rust
   pub enum Dialect {
       // ...existing variants...
       <Vendor>,
   }
   ```

2. Add to `Dialect::ALL`.

3. Add an inline translation function (`wo_to_<vendor>`).

4. Register tool name mappings in `register_builtin_translations()`:
   ```rust
   // ABP ↔ <Vendor>
   self.register_tool_translation("abp", "<vendor>", &[
       ("read_file", "<vendor_read>"),
       ("write_file", "<vendor_write>"),
       // ...
   ]);
   ```

5. Register event kind mappings for ABP ↔ `<vendor>` and cross-vendor pairs.

### Step 4: Add a Sidecar Host (Optional)

Create `hosts/<vendor>/` with an entry-point script that speaks the JSONL sidecar protocol. Register it in the CLI's sidecar registry.

### Step 5: Add Tests

- Unit tests in the dialect module (config defaults, `map_work_order`, `map_response`)
- Snapshot tests for serialized request/response JSON
- Update projection matrix tests for the new dialect pair

### Step 6: Update Documentation

- Add the vendor to the table in this document
- Update the capability matrix
- Add tool name and event kind mappings
- Update the main `README.md` crate table

## Related Documentation

- [Sidecar Protocol](sidecar_protocol.md) — JSONL wire format specification
- [Dialect×Engine Matrix](dialect_engine_matrix.md) — passthrough vs mapped routing design
- [Mapping Matrix (Planning)](03_mapping_matrix.md) — original planning notes for SDK shims
- [Capabilities](capabilities.md) — capability model reference
