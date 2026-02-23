# TODO / Open questions

This repo is a scaffold. The hard part is the mapping layer.

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
- “Run” identifiers and traceability

## Target SDKs (initial list)

- OpenAI Agents SDK (Python/TypeScript)
- OpenAI Responses/Chat Completions (Python/TypeScript)
- Anthropic SDK (Python/TypeScript)
- Google Gemini SDK (Python/TypeScript)
- LangChain/LangGraph adapters (optional)
- Vercel AI SDK adapters (optional)

## Implementation TODOs in this repo

- Wire `backplane.toml` into `abp` CLI (backend registry)
- Add conformance tests for sidecars
- Add capability satisfiability checks (required vs provided)
- Implement receipt store + replay tooling
- Implement a real `abp-daemon` HTTP API

