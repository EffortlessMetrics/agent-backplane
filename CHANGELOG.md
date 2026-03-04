# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### New Crates

#### Core & Contract

- **abp-core** — Stable contract types: WorkOrder, Receipt, AgentEvent, Capability, PolicyProfile
- **abp-protocol** — JSONL wire protocol with typed Envelope variants (`#[serde(tag = "t")]`)
- **abp-ir** — Vendor-neutral intermediate representation for cross-dialect agent event mapping
- **abp-sdk-types** — Shared SDK type definitions and conversion utilities across all vendor dialects
- **abp-receipt** — Receipt canonicalization, SHA-256 hashing, chain verification, and field-level diffing
- **abp-error** — Unified error taxonomy with stable machine-readable error codes and context
- **abp-error-taxonomy** — Deep error classification with codes, severity levels, and recommended actions

#### Dialect & Mapping

- **abp-dialect** — Dialect detection, validation, and metadata for known agent protocols
- **abp-mapper** — Cross-dialect mapping engine with role, tool, and content normalization
- **abp-mapping** — Cross-dialect feature mapping validation with fidelity tracking
- **abp-projection** — Dialect×Backend projection matrix that routes work orders to best-fit backends
- **abp-capability** — Capability registry and negotiation (native/emulated/unsupported levels)
- **abp-emulation** — Labeled capability emulation engine with system-prompt injection and post-processing

#### SDK Shims (drop-in clients that route through ABP)

- **abp-shim-openai** — OpenAI ChatCompletion drop-in shim with `to_work_order`/`from_receipt`/`from_agent_event` converters
- **abp-shim-claude** — Anthropic Claude Messages API shim with full converter pipeline
- **abp-shim-gemini** — Google Gemini GenerateContent shim with converter pipeline
- **abp-shim-codex** — OpenAI Codex/Responses shim with file change support and converters
- **abp-shim-kimi** — Moonshot Kimi chat completions shim with search support and converters
- **abp-shim-copilot** — GitHub Copilot agent protocol shim with references and converters

#### Sidecar SDKs (ABP ↔ vendor API translation)

- **abp-openai-sdk** — OpenAI sidecar: ABP ↔ OpenAI Chat Completions API translation
- **abp-claude-sdk** — Claude sidecar: ABP ↔ Anthropic Messages API translation
- **abp-gemini-sdk** — Gemini sidecar: ABP ↔ Gemini generateContent API translation
- **abp-codex-sdk** — Codex sidecar: ABP ↔ OpenAI Codex/Responses API translation with file changes
- **abp-kimi-sdk** — Kimi sidecar: ABP ↔ Moonshot Kimi chat completions translation with search
- **abp-copilot-sdk** — Copilot sidecar: ABP ↔ GitHub Copilot agent protocol translation with references

#### Backend & Sidecar Infrastructure

- **abp-backend-core** — Shared backend trait, abstractions, and policy helpers
- **abp-backend-mock** — Mock backend for local testing with emulated capabilities
- **abp-backend-sidecar** — Generic sidecar backend for JSONL protocol adapters
- **abp-integrations** — Backend trait with MockBackend and SidecarBackend implementations
- **abp-sidecar-proto** — Sidecar-side utilities for implementing services that speak ABP's JSONL protocol
- **abp-sidecar-sdk** — Shared sidecar registration helpers for vendor SDK microcrates
- **abp-sidecar-utils** — Reusable sidecar protocol utilities
- **sidecar-kit** — Low-level sidecar transport construction kit
- **claude-bridge** — Claude sidecar bridge with config discovery
- **gemini-bridge** — Standalone Gemini SDK bridge using sidecar-kit transport (raw passthrough + optional normalized mode)
- **openai-bridge** — Standalone OpenAI Chat Completions bridge using sidecar-kit transport (raw/mapped-raw/normalized modes)

#### Policy, Workspace & Tooling

- **abp-glob** — Include/exclude glob compilation using `globset`
- **abp-policy** — Policy engine with tool/read/write access control via deny-overrides-allow
- **abp-workspace** — Staged workspace creation with git diff/status capture
- **abp-git** — Git repository helpers for workspace staging and verification
- **abp-stream** — Agent event stream processing, filtering, transformation, and multiplexing
- **abp-telemetry** — Structured metrics and telemetry collection (durations, tokens, error rates)
- **abp-config** — TOML configuration loading, validation, and merging with advisory warnings
- **abp-ratelimit** — Rate limiting primitives (token bucket, sliding window) for backend calls

#### Applications

- **abp-cli** — CLI binary with `run`, `backends`, `validate`, `schema`, `inspect`, `config check`,
  `receipt verify`, and `receipt diff` subcommands
- **abp-daemon** — HTTP daemon scaffold with axum REST API and WebSocket support
- **abp-host** — Sidecar process supervision and JSONL handshake over stdio
- **abp-runtime** — Orchestration layer: workspace prep, backend selection, event multiplexing, receipt hashing

### Added

- All 6 SDK shim converters (`to_work_order`, `from_receipt`, `from_agent_event`) for
  OpenAI, Claude, Gemini, Codex, Kimi, and Copilot dialects
- IR layer for vendor-neutral intermediate representation of agent events
- SDK lowering from IR to vendor-specific wire formats
- Full 6×6 mapping matrix covering OpenAI, Claude, Gemini, Codex, Kimi, and Copilot dialects
- Projection matrix for capability-based routing across dialects
- Emulation engine with per-capability overrides, labeled system-prompt injection,
  and post-processing strategies
- Protocol utilities: batch, builder, codec, compress, router, stream, validate, version
- Daemon HTTP routes: `/health`, `/metrics`, `/backends`, `/capabilities`, `/config`,
  `/validate`, `/schema/{type}`, `/run`, `/runs`, `/runs/{id}`, `/runs/{id}/receipt`,
  `/runs/{id}/cancel`, `/runs/{id}/events`, `/receipts`, `/receipts/{id}`, `/ws`
- Security hardening: policy engine deny-overrides-allow, network access control,
  glob-based path restrictions, require-approval-for patterns
- Sidecar hosts: Node, Python, Claude, Copilot, Gemini, Codex, Kimi
- GitHub Actions CI/CD pipeline
- JSON schema generation via xtask
- `backplane.toml` configuration support with layered merging
- Per-crate README files and crates.io metadata
- Rustdoc documentation for all public APIs

### Changed

- Improved error types with RuntimeError and ProtocolError::UnexpectedMessage
- Made `ensure_capability_requirements` public for pre-flight checks

### Fixed

- Runtime race condition in `tokio::select` event loop
- Axum 0.8 route syntax (`/receipts/{run_id}`)

### Testing

- **Unit tests**: deep unit tests for every crate (~700+ test functions across 267 test files)
- **Cross-SDK integration tests**: roundtrip fidelity tests between all dialect pairs
- **Property-based tests**: proptest for core invariants, IR roundtrip, and cross-crate properties
- **BDD scenario tests**: cucumber with 7 feature files (capability checking, capability negotiation,
  policy enforcement, receipt validation, receipt verification, work order, work order routing)
- **Snapshot tests**: insta snapshots for JSON schemas, SDK types, and module types
- **Sidecar conformance tests**: contract and protocol conformance suites
- **E2E scenario tests**: full pipeline, roundtrip, and multi-backend scenario tests
- **CI hardening tests**: build verification, lint checks, cross-platform validation
- **42 fuzz targets**: envelopes, receipts, work orders, policies, globs, config, capabilities,
  dialect detection, IR roundtrip/lowering, mapping validation, protocol streams, JSONL parsing
- **21 benchmark suites**: receipt hashing, serde roundtrip, policy evaluation, projection matrix,
  IR lowering/roundtrip, dialect detection, capability negotiation, mapping validation,
  protocol encoding, glob matching, envelope serde, work order serde, core benchmarks

## [0.1.0] - 2024-XX-XX

### Added

- Initial scaffold with contract types and sidecar protocol
