# abp-error

Unified error taxonomy with stable error codes for the
[Agent Backplane](https://github.com/paiml/agent-backplane) project.

Every ABP error carries a machine-readable `ErrorCode` (a stable
`SCREAMING_SNAKE_CASE` string tag), a human-readable message, an optional
cause chain, and arbitrary key-value diagnostic context. Errors are grouped
into categories (protocol, backend, capability, policy, workspace, IR,
receipt, dialect, config, internal) for coarse-grained handling.

## Quick start

```rust
use abp_error::{AbpError, ErrorCode};

let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30 s")
    .with_context("backend", "openai")
    .with_context("timeout_ms", 30_000);

assert_eq!(err.code, ErrorCode::BackendTimeout);
println!("{err}"); // [BACKEND_TIMEOUT] timed out after 30 s {"backend":"openai","timeout_ms":30000}
```

## License

Dual-licensed under MIT OR Apache-2.0.
