# Overview

Agent Backplane is a **translation layer** for agent SDKs.

- Developers already picked an SDK (OpenAI Agents SDK, Anthropic, LangChain/LangGraph, Vercel AI, etc.).
- You want them to be able to **swap providers or runtimes** without rewriting the application.
- You do that by shipping **drop‑in shims** (one per SDK) which preserve that SDK’s public API and map each call to an internal contract.

The internal contract is:

- **Stable** (versioned, schema-driven)
- **Expressive enough** to represent the common denominator for agents
- **Strict about receipts** (observability + governance)
- **Best-effort** on semantics where providers differ

This repo is the “spine”:

- Contract types (`abp-core`)
- Transport (`abp-protocol`)
- Sidecar supervision (`abp-host`)
- Workspace staging + git harness (`abp-workspace`)
- Policy compilation utilities (`abp-policy`)
- Backend trait + implementations (`abp-integrations`)
- Orchestration runtime (`abp-runtime`)
- CLI (`abp`)

Real value comes from the SDK shims and adapters you build on top.

## Key product promise

> **Minimal changes to existing code** to gain the ability to route the same SDK calls to different agent backends.

Not “semantic equivalence”; rather:

- **Semantic intent preservation**: keep the *meaning* where possible.
- **Explicit fallbacks**: when a feature cannot be represented, fail loudly and predictably.
- **Receipts**: always produce a structured execution record.

