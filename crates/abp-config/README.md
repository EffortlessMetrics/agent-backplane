# abp-config

Configuration loading, validation, and merging for Agent Backplane.

## Features

- **TOML configuration** — Load `BackplaneConfig` from TOML files
- **Validation** — Check config values for correctness (timeouts, paths, etc.)
- **Merge** — Layer multiple configs (file + env + CLI overrides)
- **Defaults** — Sensible defaults for all settings

## Usage

```rust,no_run
use abp_config::{load_config, validate_config, merge_configs, BackplaneConfig};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let config = load_config(Some("backplane.toml".as_ref()))?;
validate_config(&config)?;
# Ok(())
# }
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT license](../../LICENSE-MIT) at your option.
