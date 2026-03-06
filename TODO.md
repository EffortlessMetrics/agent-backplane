# TODO / Open questions

Open questions and remaining work items. See CHANGELOG.md for completed work.

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
- [x] Full 6×6 cross-dialect mapping matrix (OpenAI, Claude, Gemini, Codex, Kimi, Copilot)
- [x] IR layer for vendor-neutral intermediate representation
- [x] Projection matrix wired for capability-based routing
- [x] CLI `translate`, `health`, `schema`, `inspect`, `status` subcommands
- [x] HTTP daemon with full REST API + WebSocket streaming
- [x] Receipt store with persistence and retrieval (abp-receipt-store)
- [x] Rate limiting with token bucket and sliding window (abp-ratelimit)
- [x] Retry logic with circuit-breaker middleware (abp-retry)
- [x] Validation crate for work orders, receipts, events (abp-validate)
- [x] Git repository helpers (abp-git)
- [x] Agent event stream processing and multiplexing (abp-stream)
- [x] Labeled metrics with runtime integration and Prometheus export (abp-telemetry)
- [x] Middleware system with 8 built-in middlewares (logging, retry, timeout, auth, rate-limit, metrics, cache, circuit-breaker)
- [x] All 6 SDK shims complete (abp-shim-openai, -claude, -gemini, -codex, -kimi, -copilot)
- [x] All 6 SDK adapters complete (abp-openai-sdk, -claude-sdk, -gemini-sdk, -codex-sdk, -kimi-sdk, -copilot-sdk)
- [x] All 6 bridges complete (openai-bridge, claude-bridge, gemini-bridge, codex-bridge, copilot-bridge, kimi-bridge)
- [x] 52 fuzz targets, 32 benchmark suites

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
- LangChain/LangGraph adapters (future/aspirational)
- Vercel AI SDK adapters (future/aspirational)

## Implementation TODOs

### Production Hardening

- [ ] Add OpenTelemetry export in abp-telemetry (currently in-process only)
- [ ] Add credential management for API keys
- [ ] Add TLS/mTLS for daemon API

### Testing & Quality

- [ ] Add live integration tests against real vendor APIs (behind feature flags)
- [ ] Expand fuzz corpus with real-world protocol traces
- [ ] Add mutation testing baseline with cargo-mutants (`mutants.yml` workflow exists but baseline not yet established)
- [ ] Add load/stress testing for daemon endpoints
- [x] Add cargo-deny enforcement to CI (`deny` job in `ci.yml`)
