# Mutation Testing with cargo-mutants

[cargo-mutants](https://github.com/sourcefrog/cargo-mutants) systematically modifies source code and checks whether the test suite catches each mutation. Surviving mutants reveal gaps in test coverage that line-coverage metrics miss.

## Quick Start

```bash
# Install (once)
cargo install cargo-mutants

# Run mutation testing on the focused crates
cargo mutants

# Run on a single crate
cargo mutants -p abp-core

# Show only surviving mutants
cargo mutants -- --survived
```

## Configuration

The repository ships a `mutants.toml` in the repo root that:

| Setting | Purpose |
|---------|---------|
| `timeout = 60` | Kills any single mutation test run after 60 seconds |
| `examine_re` | Limits mutations to the core crates: `abp-core`, `abp-protocol`, `abp-glob`, `abp-policy`, `abp-workspace` |
| `exclude_re` | Skips test files, `fuzz/`, and `xtask/` directories |

## Mutation Survival Tests

`tests/mutation_survival.rs` contains tests specifically designed to kill common mutant patterns:

- **Off-by-one boundaries** (`>` vs `>=`, `<` vs `<=`): e.g. timestamp comparisons in receipt validation, version range checks, trace ordering
- **Boolean logic** (`&&` vs `||`, `!` flips): e.g. `SupportLevel::satisfies()`, `MatchDecision::is_allowed()`, policy allow/deny precedence
- **Default values** (`0` vs `1`, empty vs non-empty): e.g. `duration_ms` clamping, empty policy permits everything, `ConfigDefaults` values
- **Error path coverage**: e.g. invalid globs return errors, malformed JSON returns parse errors, empty backend ID is caught

### Targeted Areas

| Crate | What is tested |
|-------|---------------|
| `abp-core` | `receipt_hash` determinism and self-referential prevention, `validate_receipt` boundary conditions, `SupportLevel::satisfies` truth table, `ConfigValidator` boundary checks, `ReceiptBuilder` duration clamping |
| `abp-glob` | Empty patterns allow everything, include/exclude precedence, `MatchDecision::is_allowed` boolean, invalid glob error handling |
| `abp-policy` | Empty policy permits all, deny overrides allow, `Decision` constructors, path-based read/write deny checks |
| `abp-protocol` | JSONL encode/decode round-trips, `parse_version` edge cases, `is_compatible_version` boundaries, `ProtocolVersion::is_compatible` minor version comparison, `VersionRange::contains` inclusive boundaries, `EnvelopeValidator` field checks, sequence validation |

## Interpreting Results

After a run, cargo-mutants produces a report in `mutants.out/`:

```
mutants.out/
├── caught.txt      # Mutants killed by tests (good)
├── survived.txt    # Mutants not caught (investigate these)
├── timeout.txt     # Mutants that timed out
└── unviable.txt    # Mutants that failed to compile
```

Focus on `survived.txt`. Each entry shows the mutation and where it was applied. Add or strengthen tests to kill surviving mutants.

## CI Integration

To run mutation testing in CI (e.g. as a scheduled job):

```yaml
- name: Mutation testing
  run: |
    cargo install cargo-mutants
    cargo mutants --no-shuffle -j 2
    # Fail if any mutants survived
    test ! -s mutants.out/survived.txt
```

> **Note**: Mutation testing is CPU-intensive. Run it on a schedule (e.g. weekly) rather than on every PR.
