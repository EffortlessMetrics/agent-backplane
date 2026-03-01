# Benchmarks

Criterion-based benchmarks for Agent Backplane core operations.

## Running benchmarks

```bash
# Run all benchmarks
cargo bench

# Run a specific benchmark suite
cargo bench --bench receipt_hash_bench
cargo bench --bench serde_roundtrip_bench
cargo bench --bench policy_eval_bench
cargo bench --bench projection_bench

# Run a specific benchmark function by name filter
cargo bench -- "receipt_hash_by_trace_size"
cargo bench -- "work_order_serialize"

# Compile without running (CI check)
cargo bench --no-run
```

## Benchmark suites

| File | What it measures |
|---|---|
| `receipt_hash_bench.rs` | `receipt_hash()` and `with_hash()` across varying trace sizes (0–500 events) |
| `serde_roundtrip_bench.rs` | JSON serialize/deserialize of `WorkOrder`, `Receipt`, `Envelope`, `AgentEvent` |
| `policy_eval_bench.rs` | `PolicyEngine` compilation and evaluation with 1–100 rules |
| `projection_bench.rs` | `ProjectionMatrix` construction, work-order translation, tool/message mapping |

## HTML reports

After running benchmarks, Criterion generates HTML reports in:

```
target/criterion/<group_name>/report/index.html
```

Open the report to view statistical analysis, throughput graphs, and regression detection.
