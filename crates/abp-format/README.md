# abp-format

Shared formatting utilities for Agent Backplane contract objects (`Receipt`, `AgentEvent`, and `WorkOrder`).

This crate provides:

- `OutputFormat` for selecting `json`, `json-pretty`, `text`, `table`, or `compact` output
- `Formatter` for formatting receipts, events, work orders, and error messages

`abp-cli` re-exports these APIs from `abp_cli::format` for compatibility.
