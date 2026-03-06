# kimi-bridge

Standalone Kimi SDK bridge built on `sidecar-kit` transport.

Provides Kimi (Moonshot) Chat Completions API types and optional IR translation
for use with the ABP sidecar protocol. The `ir` feature enables bidirectional
conversion between Kimi API types and `abp-dialect` intermediate representation.

## Modules

| Module | Description |
|--------|-------------|
| `kimi_types` | Kimi Chat Completions API types (request, response, streaming, tool calls) |
| `ir_translate` | Translation between Kimi API types and `abp-dialect` IR (feature-gated) |

## Features

| Feature | Description |
|---------|-------------|
| `normalized` | Enables ABP contract type mapping via `abp-core` |
| `ir` | Enables Kimi to `abp-dialect` IR translation |

## Usage

```rust
use kimi_bridge::kimi_types;
// Use Kimi API types for request/response serialization
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.