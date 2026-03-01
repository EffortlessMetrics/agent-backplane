# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-rc1] - Unreleased

### New Crates

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
- Projection matrix for capability mapping across dialects
- Capability negotiation with native/emulated/unsupported levels
- Structured error taxonomy with error catalog and consistency checks
- Receipt chain verification for audit trails
- Protocol utilities: batch, builder, codec, compress, router, stream, validate, version
- Emulation engine with system-prompt injection and post-processing strategies

### Testing

- BDD scenarios with cucumber (7 feature files: capability, policy, receipt, work order)
- Property tests with proptest (core invariants, cross-crate properties)
- Snapshot tests with insta (JSON schemas, SDK types, module types)
- E2E tests (pipeline, roundtrip, scenario-based)
- 20 fuzz targets covering envelopes, receipts, work orders, policies, and globs
- Conformance suites for contract and sidecar protocol
- Cross-SDK fidelity and IR roundtrip tests
- Criterion benchmarks (receipt hash, serde roundtrip, policy eval, projection)

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
