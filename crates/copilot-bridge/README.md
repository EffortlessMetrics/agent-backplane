# copilot-bridge

Standalone GitHub Copilot bridge using `sidecar-kit` transport.

Provides Copilot-specific types and translation to/from the ABP intermediate
representation (IR) defined in `abp-sdk-types`.

## Features

- `ir` — enables translation between Copilot types and `abp-sdk-types` IR types.
- `normalized` — enables mapping to `abp-core` `AgentEvent` / `Receipt`.
