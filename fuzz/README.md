# Agent Backplane — Fuzz Tests

This directory contains [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for the
Agent Backplane contract types and protocol parsing.

## Prerequisites

```bash
cargo install cargo-fuzz   # requires nightly Rust
```

## Usage

List all available fuzz targets:

```bash
cd fuzz
cargo fuzz list
```

Run a specific target (uses nightly by default):

```bash
cargo fuzz run fuzz_envelope_parse          # JSONL envelope parsing
cargo fuzz run fuzz_work_order_json         # WorkOrder JSON deserialization
cargo fuzz run fuzz_receipt_json            # Receipt JSON deserialization + hashing
cargo fuzz run fuzz_glob_pattern            # Glob pattern compilation & matching
cargo fuzz run fuzz_policy_compile          # PolicyProfile → PolicyEngine compilation
cargo fuzz run fuzz_agent_event_json        # AgentEvent JSON deserialization
```

Limit run duration or iterations:

```bash
cargo fuzz run fuzz_envelope_parse -- -max_total_time=60    # 60 seconds
cargo fuzz run fuzz_work_order_json -- -runs=100000         # 100k iterations
```

## Target Summary

| Target                    | What it fuzzes                                              |
|---------------------------|-------------------------------------------------------------|
| `fuzz_envelope_parse`     | `JsonlCodec::decode` with arbitrary bytes                   |
| `fuzz_work_order_json`    | `serde_json::from_slice::<WorkOrder>` + round-trip          |
| `fuzz_receipt_json`       | `serde_json::from_slice::<Receipt>` + `receipt_hash`        |
| `fuzz_glob_pattern`       | `IncludeExcludeGlobs::new` + `decide_str` / `decide_path`  |
| `fuzz_policy_compile`     | `PolicyEngine::new` + `can_use_tool` / `can_read_path`      |
| `fuzz_agent_event_json`   | `serde_json::from_slice::<AgentEvent>` + `AgentEventKind`   |

See `fuzz_targets/` for the full set of targets (there are many more beyond the core six listed above).

## Corpus

Seed corpora live in `corpus/<target_name>/`. Cargo-fuzz auto-creates these directories on first
run and saves interesting inputs. You can also add seed files manually.

## Reproducing crashes

```bash
cargo fuzz run fuzz_envelope_parse artifacts/fuzz_envelope_parse/crash-<hash>
```
