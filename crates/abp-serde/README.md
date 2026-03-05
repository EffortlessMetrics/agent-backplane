# abp-serde

Shared serde helpers for small common value encodings used across Agent Backplane crates.

Currently includes:

- `duration_millis`: serialize `std::time::Duration` as integer milliseconds.
- `option_duration_millis`: serialize `Option<std::time::Duration>` as optional integer milliseconds.
