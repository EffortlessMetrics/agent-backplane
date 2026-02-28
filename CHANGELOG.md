# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
- SDK adapters for Claude, Codex, Gemini, Kimi (abp-claude-sdk, abp-codex-sdk, abp-gemini-sdk, abp-kimi-sdk)
- Low-level sidecar transport kit (sidecar-kit)
- Claude bridge with config discovery (claude-bridge)
- Sidecar hosts: Node, Python, Claude, Copilot, Gemini, Codex, Kimi
- GitHub Actions CI/CD pipeline
- JSON schema generation via xtask
- backplane.toml configuration support
- Property-based tests with proptest
- Snapshot tests with insta
- Criterion benchmarks for hot paths
- Comprehensive test suite (300+ tests)
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
