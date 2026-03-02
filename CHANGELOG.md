# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-rc1] - Unreleased

### New Crates

- **abp-projection** — Projection matrix that routes work orders to the best-fit backend based on capability negotiation
- **abp-stream** — Agent event stream processing, filtering, transformation, and multiplexing
- **abp-shim-openai** — Drop-in OpenAI SDK shim that routes through ABP's intermediate representation
- **abp-shim-claude** — Drop-in Anthropic Claude SDK shim that routes through ABP
- **abp-shim-gemini** — Drop-in Gemini SDK shim that routes through the Agent Backplane
- **abp-capability** — Capability negotiation between work-order requirements and backend manifests
- **abp-error** — Unified error taxonomy with stable machine-readable error codes and context
- **abp-receipt** — Receipt canonicalization, SHA-256 hashing, chain verification, and field-level diffing
- **abp-mapping** — Cross-dialect feature mapping validation between AI provider dialects
- **abp-config** — TOML configuration loading, validation, and merging with advisory warnings
- **abp-sidecar-proto** — Sidecar-side utilities for implementing services that speak ABP's JSONL protocol
- **abp-emulation** — Labeled capability emulation engine for missing backend features
- **abp-telemetry** — Structured metrics and telemetry collection (durations, tokens, error rates)
- **abp-dialect** — Dialect detection, validation, and metadata for known agent protocols
- **abp-backend-core** — Shared backend abstractions and policy helpers
- **abp-backend-mock** — Mock backend for local testing with emulated capabilities
- **abp-backend-sidecar** — Generic sidecar backend for JSONL protocol adapters
- **abp-sidecar-sdk** — Shared sidecar registration helpers for vendor SDK microcrates
- **abp-git** — Git repository helpers for workspace staging and verification
- **abp-claude-sdk** — Claude sidecar: ABP ↔ Anthropic Messages API translation
- **abp-codex-sdk** — Codex sidecar: ABP ↔ OpenAI Codex/Responses API translation
- **abp-copilot-sdk** — Copilot sidecar: ABP ↔ GitHub Copilot agent protocol translation
- **abp-gemini-sdk** — Gemini sidecar: ABP ↔ Gemini generateContent API translation
- **abp-kimi-sdk** — Kimi sidecar: ABP ↔ Moonshot Kimi chat completions translation
- **abp-openai-sdk** — OpenAI sidecar: ABP ↔ OpenAI Chat Completions API translation

### Features

- IR layer for vendor-neutral intermediate representation of agent events
- SDK lowering from IR to vendor-specific wire formats
- Projection matrix for capability mapping across dialects (abp-projection)
- Drop-in SDK shims for OpenAI, Claude, and Gemini that route through ABP transparently
- Capability negotiation with native/emulated/unsupported levels (abp-capability)
- Structured error taxonomy with error catalog and consistency checks (abp-error)
- Receipt canonicalization, chain verification, and field-level diffing (abp-receipt)
- Cross-dialect mapping validation with fidelity tracking (abp-mapping)
- TOML configuration loading and validation with layered merging (abp-config)
- Sidecar-side protocol handler utilities for building JSONL services (abp-sidecar-proto)
- Protocol utilities: batch, builder, codec, compress, router, stream, validate, version
- Emulation engine with system-prompt injection and post-processing strategies
- Enhanced emulation strategies: per-capability overrides, labeled injection, post-processing
- Full 6×6 mapping matrix covering OpenAI, Claude, Gemini, Codex, Kimi, and Copilot dialects
- CLI subcommands: `validate` (JSON file validation), `schema` (print JSON schemas),
  `inspect` (receipt inspection), `config check` (TOML validation),
  `receipt verify` (hash integrity), `receipt diff` (structured receipt comparison)
- Daemon HTTP routes: `/health`, `/metrics`, `/backends`, `/capabilities`, `/config`,
  `/validate`, `/schema/{type}`, `/run`, `/runs`, `/runs/{id}`, `/runs/{id}/receipt`,
  `/runs/{id}/cancel`, `/runs/{id}/events`, `/receipts`, `/receipts/{id}`, `/ws`
- Security hardening: policy engine deny-overrides-allow, network access control,
  glob-based path restrictions, require-approval-for patterns

### Testing

- BDD scenarios with cucumber (7 feature files: capability, policy, receipt, work order)
- Property tests with proptest (core invariants, cross-crate properties)
- Snapshot tests with insta (JSON schemas, SDK types, module types)
- E2E tests (pipeline, roundtrip, scenario-based)
- 20 fuzz targets covering envelopes, receipts, work orders, policies, and globs
- Conformance suites for contract and sidecar protocol
- Cross-SDK fidelity and IR roundtrip tests
- Benchmark suite: receipt hash, serde roundtrip, policy eval, projection, IR lowering,
  dialect detection, capability negotiation, mapping validation, protocol, and more

## [Unreleased]

### Added
- Core contract types: WorkOrder, Receipt, AgentEvent, Capability, PolicyProfile (abp-core)
- JSONL wire protocol with typed Envelope variants (abp-protocol)
- Sidecar process supervision and JSONL handshake (abp-host)
- Include/exclude glob matching utilities (abp-glob)
- Policy engine with tool/read/write access control (abp-policy)
- Workspace staging with git diff/status capture (abp-workspace)
- Backend trait with MockBackend and SidecarBackend implementations (abp-integrations)
- Runtime orchestration layer (abp-runtime)
- CLI with `run` and `backends` subcommands (abp-cli)
- HTTP daemon with REST API (abp-daemon)
- Low-level sidecar transport kit (sidecar-kit)
- Claude bridge with config discovery (claude-bridge)
- Sidecar hosts: Node, Python, Claude, Copilot, Gemini, Codex, Kimi
- GitHub Actions CI/CD pipeline
- JSON schema generation via xtask
- backplane.toml configuration support
- Per-crate README files and crates.io metadata
- Rustdoc documentation for all public APIs

### Changed
- Improved error types with RuntimeError and ProtocolError::UnexpectedMessage
- Made ensure_capability_requirements public for pre-flight checks

### Fixed
- Runtime race condition in tokio::select event loop
- Axum 0.8 route syntax (/receipts/{run_id})

## [0.1.0] - 2024-XX-XX

### Added
- Initial scaffold with contract types and sidecar protocol
