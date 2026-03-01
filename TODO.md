# TODO / Open questions

This repo is a scaffold. The hard part is the mapping layer.

## Completed ✅

- [x] Core contract types: WorkOrder, Receipt, AgentEvent, Capability, PolicyProfile
- [x] JSONL wire protocol with Envelope variants and codec utilities
- [x] Sidecar process supervision and JSONL handshake
- [x] Policy engine with tool/read/write access control via globs
- [x] Workspace staging with git diff/status capture
- [x] Backend trait with MockBackend and SidecarBackend implementations
- [x] Runtime orchestration layer
- [x] CLI with `run` and `backends` subcommands
- [x] Scaffold SDK microcrates for Claude, Codex, Copilot, Gemini, Kimi, OpenAI
- [x] Dialect detection and metadata (abp-dialect)
- [x] Emulation engine for missing capabilities (abp-emulation)
- [x] Telemetry collection scaffolding (abp-telemetry)
- [x] Low-level sidecar transport kit (sidecar-kit)
- [x] Claude bridge with config discovery (claude-bridge)
- [x] Sidecar hosts: Node, Python, Claude, Copilot, Gemini, Codex, Kimi
- [x] BDD feature tests (7 scenarios), property tests, snapshot tests, fuzz targets
- [x] JSON schema generation via xtask
- [x] backplane.toml configuration support
- [x] Protocol utilities: batch, builder, compress, router, stream, validate, version
- [x] Receipt chain verification
- [x] Capability negotiation (native/emulated/unsupported)
- [x] Conformance tests for sidecars
- [x] Capability satisfiability checks (required vs provided)

## Cross-SDK questions we need precise answers for

1) **Streaming**
- Does the SDK stream raw text deltas, message objects, or structured events?
- How are tool calls streamed (if at all)?
- Are ordering guarantees documented?

2) **Tool calling**
- JSON schema subset and size limits
- How tool call IDs are generated and correlated
- How tool errors are represented

3) **File and workspace tools**
- Does the SDK include file tools natively?
- If not, what is the idiomatic pattern?

4) **Usage and billing**
- Token accounting fields
- Caching fields (read/write cache tokens)
- Cost reporting support (if any)

5) **Retries and idempotency**
- SDK-level retry configuration
- Request IDs / idempotency keys
- Failure modes (timeouts vs partial results)

6) **Sessions**
- Resume and fork semantics
- "Run" identifiers and traceability

## Target SDKs (initial list)

- OpenAI Agents SDK (Python/TypeScript)
- OpenAI Responses/Chat Completions (Python/TypeScript)
- Anthropic SDK (Python/TypeScript)
- Google Gemini SDK (Python/TypeScript)
- LangChain/LangGraph adapters (optional)
- Vercel AI SDK adapters (optional)

## Implementation TODOs

### Real SDK Adapters (scaffold → production)

- [ ] Implement real Claude adapter with live Anthropic API calls
- [ ] Implement real OpenAI adapter with Chat Completions + Responses API
- [ ] Implement real Gemini adapter with generateContent API
- [ ] Implement real Codex adapter with Codex CLI integration
- [ ] Implement real Kimi adapter with Moonshot API
- [ ] Implement real Copilot adapter with Copilot agent protocol
- [ ] Wire projection matrix for cross-dialect translation (not just scaffolded)

### HTTP Daemon

- [ ] Implement a real `abp-daemon` HTTP API (currently a stub)
- [ ] Add WebSocket streaming for live event feeds
- [ ] Add receipt store with query/replay API
- [ ] Add health check and readiness endpoints

### Production Hardening

- [ ] Wire `backplane.toml` into `abp` CLI (backend registry)
- [ ] Add rate limiting and backpressure handling
- [ ] Add retry logic with configurable policies
- [ ] Add OpenTelemetry export in abp-telemetry (currently in-process only)
- [ ] Add credential management for API keys
- [ ] Implement receipt store + replay tooling
- [ ] Add TLS/mTLS for daemon API

### Testing & Quality

- [ ] Add live integration tests against real vendor APIs (behind feature flags)
- [ ] Expand fuzz corpus with real-world protocol traces
- [ ] Add mutation testing baseline with cargo-mutants
- [ ] Add load/stress testing for daemon endpoints
