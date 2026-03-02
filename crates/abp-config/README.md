# abp-config

Configuration loading, validation, and merging for Agent Backplane.

## Features

- **TOML configuration** — Load `BackplaneConfig` from TOML files
- **Validation** — Check config values for correctness (timeouts, paths, etc.)
- **Merge** — Layer multiple configs (file + env + CLI overrides)
- **Defaults** — Sensible defaults for all settings

## Usage

```rust
use abp_config::{load_config, validate_config, merge_configs, BackplaneConfig};

let config = load_config(Some("backplane.toml".as_ref()))?;
validate_config(&config)?;
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT license](../../LICENSE-MIT) at your option.
