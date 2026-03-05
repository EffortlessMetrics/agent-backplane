# codex-bridge

Codex Responses API bridge for Agent Backplane — IR translation layer.

Translates between OpenAI Codex Responses API types (from `abp-codex-sdk`)
and the vendor-agnostic Intermediate Representation defined in `abp-dialect`.

## Features

- **`ir`** — enables the `ir_translate` module for bidirectional
  Codex ↔ IR conversion (depends on `abp-dialect`).

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
