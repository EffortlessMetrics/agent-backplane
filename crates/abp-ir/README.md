# abp-ir

Intermediate Representation types and property-based tests for the Agent Backplane.

Part of the [Agent Backplane](https://github.com/anthropics/agent-backplane) project.

## Overview

`abp-ir` provides a focused entry point for the cross-dialect intermediate
representation (IR) layer. It re-exports the IR types from `abp_core::ir`
and houses a comprehensive property-based test suite (via `proptest`) that
validates serde round-trip invariants without bloating the core crate.

## What It Provides

- **Re-exports of all IR types** — `IrRole`, `IrContentPart`, `IrMessage`,
  `IrToolDef`, `IrConversation`, `IrUsage`, and related types
- **Property-based round-trip tests** — randomized serde JSON encode/decode
  for every IR type, nested structures, metadata maps, tool definitions,
  conversations, and usage records

## Usage

Add `abp-ir` to your dependencies:

```toml
[dependencies]
abp-ir = { path = "../abp-ir" }
```

Then use the IR types directly:

```rust
use abp_ir::{IrMessage, IrRole, IrContentPart};

let msg = IrMessage {
    role: IrRole::User,
    content: vec![IrContentPart::Text {
        text: "Hello, agent!".into(),
    }],
    metadata: Default::default(),
};

let json = serde_json::to_string(&msg).unwrap();
let back: IrMessage = serde_json::from_str(&json).unwrap();
assert_eq!(back.role, IrRole::User);
```

## Running Tests

```bash
cargo test -p abp-ir
```

The property-based tests use `proptest` to generate randomized inputs and
verify that all IR types survive a JSON round-trip without data loss.

## Crate Structure

```
src/lib.rs                    — re-exports from abp_core::ir
tests/
  proptest_roundtrip.rs       — property-based serde round-trip tests
```

## License

Licensed under either of

- MIT license
- Apache License, Version 2.0

at your option.
