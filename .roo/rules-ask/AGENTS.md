# Agent Backplane - Ask Mode Rules

## Project Overview
Agent Backplane is a contract-driven orchestration layer for agent SDKs (Claude, Codex, Copilot, Gemini). It provides a unified interface for running AI agent tasks with policy enforcement and receipt generation.

## Key Documentation Files
- [`docs/sidecar_protocol.md`](docs/sidecar_protocol.md) - JSONL wire format specification
- [`docs/02_architecture.md`](docs/02_architecture.md) - System architecture
- [`contracts/schemas/`](contracts/schemas/) - JSON schemas for WorkOrder and Receipt

## Core Concepts

### Work Order
A single unit of work - intentionally NOT a chat session. See [`WorkOrder`](crates/abp-core/src/lib.rs:20) struct.

### Execution Lanes
- `PatchFirst`: Agent proposes diffs, no direct mutation
- `WorkspaceFirst`: Agent can mutate a staged workspace

### Receipt
Immutable record of execution with hash verification. Always use `.with_hash()` before finalizing.

## Contract Stability
- `abp-core` is the stable contract - other crates may change
- `CONTRACT_VERSION = "abp/v0.1"` must match across all components

## Backend Types
- `mock`: Local development/testing backend
- `sidecar:<name>`: External sidecar process via JSONL
