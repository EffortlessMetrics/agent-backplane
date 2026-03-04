# abp-config-env

Focused helper crate for reading Agent Backplane configuration environment overrides.

This crate centralizes:

- Environment variable names used for runtime config override behavior.
- Parsing those values from the process environment.

It is intentionally minimal so config-producing crates can share one source of truth
for env keys without duplicating parsing logic.
