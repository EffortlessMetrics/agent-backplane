# abp-error-taxonomy

Deep tests for the Agent Backplane error taxonomy.

Part of the [Agent Backplane](https://github.com/anthropics/agent-backplane) project.

## Overview

`abp-error-taxonomy` re-exports everything from `abp-error` and provides an
exhaustive integration test suite that validates the error taxonomy across
multiple dimensions:

- **Unique codes** — every `ErrorCode` variant has a unique `as_str()` value
- **Stable strings** — snapshot tests lock the wire representation of each code
- **Display impls** — `ErrorCategory` and `ErrorCode` produce correct human-readable output
- **`AbpError` wrapping** — all codes can be wrapped, messages are preserved, context is deterministic
- **Error source chains** — single and multi-level `source()` chains walk correctly
- **Downcast paths** — `io::Error` and `AbpError` sources are downcastable
- **Serialization round-trips** — JSON encode/decode for codes, categories, and `AbpErrorDto`
- **`Send + Sync + 'static`** — all error types satisfy thread-safety bounds
- **Equality & hashing** — `ErrorCode`, `ErrorCategory`, and `AbpErrorDto` equality is consistent
- **`From` conversions** — wrapping `io::Error` and `serde_json::Error` sources
- **Custom messages** — empty, unicode, long, and special-character messages are preserved
- **Severity classification** — every code maps to `critical`, `error`, or `warning`
- **Recovery suggestions** — every category has a non-empty recovery hint

## Usage

This crate is a test-only crate. Add it as a dev-dependency or run its tests
directly:

```bash
cargo test -p abp-error-taxonomy
```

In code, the re-exports mirror `abp-error`:

```rust
use abp_error_taxonomy::{AbpError, ErrorCode, ErrorCategory};

let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
assert_eq!(err.code.category(), ErrorCategory::Backend);
```

## Crate Structure

```
src/lib.rs           — re-exports from abp-error
tests/
  taxonomy_deep.rs   — comprehensive taxonomy test suite
```

## License

Licensed under either of

- MIT license
- Apache License, Version 2.0

at your option.
