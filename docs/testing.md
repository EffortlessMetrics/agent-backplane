# Testing Guide

> Contract version: `abp/v0.1`

This document describes the test strategy, test categories, and how to run
each type of test in the Agent Backplane workspace.

---

## Table of Contents

- [Test Strategy](#test-strategy)
- [Test Pyramid](#test-pyramid)
- [Running Tests](#running-tests)
- [Test Categories](#test-categories)
  - [Unit Tests](#unit-tests)
  - [Property-Based Tests](#property-based-tests)
  - [Snapshot Tests](#snapshot-tests)
  - [Golden Tests](#golden-tests)
  - [Fuzz Tests](#fuzz-tests)
  - [Integration Tests](#integration-tests)
  - [End-to-End Tests](#end-to-end-tests)
  - [Conformance Tests](#conformance-tests)
  - [Host-Specific Tests](#host-specific-tests)
- [Test Coverage Goals](#test-coverage-goals)
- [Mutation Testing](#mutation-testing)
- [Writing New Tests](#writing-new-tests)

---

## Test Strategy

ABP's test strategy is driven by two priorities:

1. **Contract stability**: the core types (`WorkOrder`, `Receipt`,
   `AgentEvent`, capabilities) are the product. Any serialization change,
   hash behavior change, or validation regression is a breaking change.
2. **Protocol correctness**: the JSONL protocol between control plane and
   sidecars must round-trip correctly and handle edge cases gracefully.

The strategy uses a multi-layered testing pyramid with property-based testing
and fuzzing at the foundation, snapshot tests for regression detection, and
conformance tests for cross-language protocol compliance.

---

## Test Pyramid

```
                    ╱╲
                   ╱  ╲           Conformance + E2E
                  ╱ C  ╲         Cross-language protocol compliance,
                 ╱──────╲        full CLI → sidecar round-trips.
                ╱        ╲
               ╱   E2E    ╲      End-to-end with mock backend,
              ╱─────────────╲    runtime orchestration flows.
             ╱               ╲
            ╱   Integration   ╲   Multi-crate interactions,
           ╱───────────────────╲  sidecar spawn + handshake.
          ╱                     ╲
         ╱  Snapshot + Golden    ╲  Serialization format regression,
        ╱─────────────────────────╲ wire format stability.
       ╱                           ╲
      ╱  Property-Based + Fuzz      ╲  Arbitrary inputs, round-trip
     ╱───────────────────────────────╲ invariants, edge cases.
    ╱                                 ╲
   ╱         Unit Tests                ╲  Individual functions,
  ╱─────────────────────────────────────╲ validation logic, policy rules.
```

---

## Running Tests

### All tests

```bash
cargo test                              # Run all Rust tests
```

### Single crate

```bash
cargo test -p abp-core                  # All tests in abp-core
cargo test -p abp-protocol              # All tests in abp-protocol
cargo test -p abp-runtime               # All tests in abp-runtime
cargo test -p abp-policy                # All tests in abp-policy
cargo test -p abp-glob                  # All tests in abp-glob
cargo test -p abp-workspace             # All tests in abp-workspace
cargo test -p abp-host                  # All tests in abp-host
cargo test -p abp-integrations          # All tests in abp-integrations
cargo test -p sidecar-kit               # All tests in sidecar-kit
cargo test -p claude-bridge             # All tests in claude-bridge
cargo test -p abp-cli                   # All tests in abp-cli
cargo test -p abp-daemon                # All tests in abp-daemon
```

### Single test by name

```bash
cargo test -p abp-core receipt_hash     # Run test matching "receipt_hash"
cargo test -p abp-policy combination    # Run test matching "combination"
```

### Property-based tests only

```bash
cargo test -p abp-core proptest         # Property tests in abp-core
cargo test -p abp-protocol proptest     # Property tests in abp-protocol
cargo test -p abp-runtime proptest      # Property tests in abp-runtime
cargo test -p abp-workspace proptest    # Property tests in abp-workspace
cargo test -p abp-policy proptest       # Property tests in abp-policy
cargo test -p abp-glob proptest         # Property tests in abp-glob
cargo test -p sidecar-kit proptest      # Property tests in sidecar-kit
cargo test -p claude-bridge proptest    # Property tests in claude-bridge
```

### Snapshot tests

```bash
cargo test -p abp-core snapshot         # Snapshot tests in abp-core
cargo test -p abp-runtime snapshot      # Snapshot tests in abp-runtime
cargo test -p abp-policy snapshot       # Snapshot tests in abp-policy
cargo test -p abp-workspace snapshot    # Snapshot tests in abp-workspace
cargo test -p sidecar-kit snapshot      # Snapshot tests in sidecar-kit
```

To review or update snapshots:

```bash
cargo insta review                      # Interactive snapshot review
cargo insta accept                      # Accept all pending snapshots
```

### Fuzz tests

Requires `cargo-fuzz` (nightly only):

```bash
cargo install cargo-fuzz

cargo +nightly fuzz run fuzz_work_order   # Fuzz WorkOrder deserialization
cargo +nightly fuzz run fuzz_envelope     # Fuzz Envelope deserialization
cargo +nightly fuzz run fuzz_receipt      # Fuzz Receipt deserialization
```

### Conformance tests (JavaScript)

```bash
cd tests/conformance
node runner.js                          # Run all conformance tests
node passthrough.test.js                # Passthrough parity tests
node mapped.test.js                     # Mapped dialect tests
node matrix.test.js                     # Full dialect×engine matrix
```

### Host-specific tests

```bash
cd hosts/claude/test && node --test     # Claude sidecar tests
cd hosts/codex/test && node --test      # Codex sidecar tests
cd hosts/gemini/test && node --test     # Gemini sidecar tests
cd hosts/kimi/test && node --test       # Kimi sidecar tests
cd hosts/copilot/test && node --test    # Copilot sidecar tests
```

### Schema generation (validation)

```bash
cargo run -p xtask -- schema            # Generate JSON schemas to contracts/schemas/
```

---

## Test Categories

### Unit Tests

**Location:** inline `#[cfg(test)]` modules in source files and
`tests/` directories per crate.

**Purpose:** test individual functions, validation logic, policy evaluation
rules, and type construction in isolation.

**Examples:**
- Receipt field validation (`MissingField`, `EmptyBackendId`, etc.)
- Policy `can_use_tool()` / `can_read_path()` / `can_write_path()` decisions
- Glob pattern compilation and matching
- Capability `satisfies()` logic
- `WorkOrderBuilder` construction

**Crates with unit tests:**
- `abp-core`: contract validation, hashing, capabilities
- `abp-protocol`: envelope encoding/decoding
- `abp-policy`: allow/deny rule evaluation
- `abp-glob`: pattern compilation and matching
- `abp-workspace`: workspace mode logic
- `abp-cli`: config validation
- `abp-host`: sidecar registry

---

### Property-Based Tests

**Location:** `proptest_*.rs` files in each crate's `tests/` directory.

**Framework:** [proptest](https://crates.io/crates/proptest)

**Purpose:** generate arbitrary inputs to verify invariants hold across a
wide range of data. These tests catch edge cases that hand-written examples miss.

**Key invariants tested:**

| Crate | Invariant |
|-------|-----------|
| `abp-core` | Any valid `WorkOrder` round-trips through JSON serialize/deserialize. |
| `abp-core` | `receipt_hash()` is deterministic: same receipt always produces same hash. |
| `abp-core` | `with_hash()` produces a receipt that passes `validate_receipt()`. |
| `abp-protocol` | Any valid `Envelope` round-trips through `encode()` + `decode()`. |
| `abp-policy` | Deny rules always override conflicting allow rules. |
| `abp-glob` | Compiled glob patterns match the same paths as reference implementations. |
| `abp-workspace` | Staged workspace always excludes `.git`. |
| `abp-runtime` | Event trace in receipt contains all streamed events. |
| `sidecar-kit` | Frames round-trip through codec encode/decode. |
| `claude-bridge` | Bridge type mappings are reversible where applicable. |

**Custom strategies:** `abp-core` defines `Arbitrary` implementations (or
proptest strategies) for core types in `proptest_types.rs`, reused across
crate test suites.

---

### Snapshot Tests

**Location:** `tests/snapshots/` directories; test files named `*snapshot*`.

**Framework:** [insta](https://crates.io/crates/insta) with JSON feature.

**Purpose:** detect unintended serialization format changes. Snapshot files
record the expected JSON output and fail the test if the output changes.

**What is snapshot-tested:**
- `Envelope` wire format (all variants)
- `Receipt` JSON structure
- `WorkOrder` JSON structure
- `RuntimeError` display output
- `PolicyProfile` serialization
- `AgentEvent` serialization

**Workflow:**
1. Run tests: a snapshot mismatch shows a diff.
2. Review changes: `cargo insta review` presents an interactive diff.
3. Accept intentional changes: `cargo insta accept`.
4. Commit updated `.snap` files alongside the code change.

Snapshot tests are critical for contract stability — any change to the wire
format is immediately visible in code review.

---

### Golden Tests

**Location:** `golden_tests.rs` files in crate test directories.

**Purpose:** compare output against a fixed "golden" reference. Similar to
snapshots but typically used for format validation where the expected output
is manually curated rather than auto-generated.

**Used in:**
- `abp-core`: golden receipt format
- `abp-protocol`: golden envelope format

---

### Fuzz Tests

**Location:** `fuzz/fuzz_targets/`

**Framework:** [libfuzzer-sys](https://crates.io/crates/libfuzzer-sys)
(via `cargo-fuzz`)

**Purpose:** discover crashes, panics, and undefined behavior by feeding
random byte sequences to deserialization functions.

**Targets:**

| Target | What it fuzzes |
|--------|---------------|
| `fuzz_work_order` | `serde_json::from_slice::<WorkOrder>()` — ensures arbitrary JSON input never panics |
| `fuzz_envelope` | `serde_json::from_slice::<Envelope>()` — ensures arbitrary JSON input never panics |
| `fuzz_receipt` | `serde_json::from_slice::<Receipt>()` — ensures arbitrary JSON input never panics |

**Running:**

```bash
# Requires nightly Rust and cargo-fuzz
cargo +nightly fuzz run fuzz_work_order -- -max_total_time=300
cargo +nightly fuzz run fuzz_envelope -- -max_total_time=300
cargo +nightly fuzz run fuzz_receipt -- -max_total_time=300
```

Fuzz tests complement property-based tests: proptest generates structured
valid-ish data, while fuzzing generates completely arbitrary bytes.

---

### Integration Tests

**Location:** `tests/` directories in crates that depend on multiple other
crates.

**Purpose:** verify that crates work correctly together — sidecar spawn +
handshake, runtime orchestration with real (mock) backends, workspace staging
with policy enforcement.

**Key integration test files:**

| File | What it tests |
|------|--------------|
| `abp-host/tests/` | Sidecar spawn, hello handshake, event streaming, error recovery |
| `abp-integrations/tests/` | Backend trait implementations, projection, dialect integration |
| `abp-runtime/tests/` | Full runtime flow: workspace + policy + backend + receipt |
| `claude-bridge/tests/` | Bridge spawn, type conversion round-trips |
| `abp-cli/tests/` | CLI argument parsing, config loading |

---

### End-to-End Tests

**Location:** `abp-runtime/tests/` (e.g. `e2e_mock.rs`)

**Purpose:** exercise the full path from work order submission through
runtime orchestration to receipt generation, using the `MockBackend`.

**What is verified:**
- Complete `run_streaming()` flow succeeds.
- Events are received in order.
- Receipt contains expected fields.
- Receipt hash validates.
- Workspace verification (git diff/status) is populated.

---

### Conformance Tests

**Location:** `tests/conformance/`

**Language:** JavaScript (Node.js)

**Purpose:** verify cross-language protocol compliance. These tests run
actual sidecar host scripts from `hosts/` and verify that the JSONL protocol
is followed correctly — envelope ordering, field presence, `ref_id`
correlation, and receipt structure.

**Test files:**

| File | What it tests |
|------|--------------|
| `passthrough.test.js` | Passthrough mode: input passes through unmodified, stream events match |
| `mapped.test.js` | Mapped mode: dialect translation, capability checking, early failure |
| `matrix.test.js` | Full dialect×engine matrix: all dialect/engine combinations |
| `runner.js` | Shared test runner utilities |

---

### Host-Specific Tests

**Location:** `hosts/{host}/test/`

**Language:** JavaScript (Node.js)

**Purpose:** test individual sidecar implementations in isolation, verifying
their protocol compliance and mapping correctness.

| Host | Test files | What they test |
|------|-----------|---------------|
| Claude | `passthrough.test.js`, `mapped.test.js` | Claude adapter protocol compliance |
| Codex | `passthrough.test.js`, `mapped.test.js` | Codex adapter protocol compliance |
| Gemini | `mapped.test.js` | Gemini adapter mapping correctness |
| Kimi | `sdk-adapter.test.js` | Kimi SDK adapter contract |
| Copilot | `sdk-adapter.test.js` | Copilot SDK adapter contract |

---

## Test Coverage Goals

### Coverage Priorities (by crate)

| Crate | Priority | Rationale |
|-------|----------|-----------|
| `abp-core` | **Critical** | Contract types are the product. Any regression is a breaking change. |
| `abp-protocol` | **Critical** | Wire format must round-trip correctly across languages. |
| `abp-policy` | **High** | Policy decisions must be correct — false allows are security bugs. |
| `abp-glob` | **High** | Glob matching underlies both policy and workspace staging. |
| `abp-runtime` | **High** | Orchestration logic must handle all error paths gracefully. |
| `abp-workspace` | **Medium** | Staging logic has filesystem side effects; focus on invariants. |
| `abp-host` | **Medium** | Sidecar supervision; integration-heavy, harder to unit test. |
| `sidecar-kit` | **Medium** | Transport layer; well-tested via property tests and snapshots. |
| `abp-integrations` | **Medium** | Backend trait implementations; tested via runtime e2e. |
| `abp-cli` | **Low** | Thin CLI layer; tested via `assert_cmd` for arg parsing. |
| `abp-daemon` | **Low** | Stub; minimal functionality to test. |

### What Must Always Be Tested

- **Serialization round-trips**: every public type in `abp-core` and
  `abp-protocol` must survive `serialize → deserialize → serialize`
  without data loss.
- **Hash determinism**: `receipt_hash()` must produce identical output for
  identical input across runs and platforms.
- **Validation completeness**: `validate_receipt()` must catch all invalid
  states (missing fields, bad hashes, timestamp inversions).
- **Protocol ordering**: the JSONL protocol state machine (hello → run →
  events → final/fatal) must reject invalid sequences.
- **Policy deny-overrides-allow**: the `PolicyEngine` must never allow an
  operation that matches a deny rule, even if it also matches an allow rule.

---

## Mutation Testing

### What Is Mutation Testing?

Mutation testing measures test suite quality by introducing small changes
("mutants") into the source code — flipping comparisons, replacing return
values, deleting statements — and checking whether the tests catch each
change.  A mutant that is **killed** (tests fail) indicates the test suite
covers that logic path.  A mutant that **survives** (tests still pass) reveals
a gap.

We use [cargo-mutants](https://mutants.rs/) for Rust mutation testing.

### Running Mutation Tests

```bash
# Install cargo-mutants (one-time)
cargo install cargo-mutants --locked

# Run against a single crate (recommended starting point)
cargo mutants --package abp-core
cargo mutants --package abp-protocol

# Output goes to mutants-out/ by default
```

Configuration lives in `.cargo/mutants.toml`:
- Focuses on `abp-core` and `abp-protocol` (highest-value crates)
- Excludes test files, benchmarks, fuzz targets, and xtask
- Skips trivial trait impls (`Display`, `Debug`, `Default`, `From`)
- Uses a 3× timeout multiplier (test suite runs ~5 min)

### Interpreting Results

After a run, `mutants-out/` contains:
- `caught.txt` — mutants killed by the test suite ✅
- `unviable.txt` — mutants that don't compile (expected) ⚠️
- `missed.txt` — mutants that survived (**action needed**) ❌
- `timeout.txt` — mutants that timed out (may need investigation)

Focus on `missed.txt`.  Each entry describes the mutation and the source
location.  Write a targeted test that would fail under that mutation.  The
`mutation_guards.rs` test file in `abp-core` demonstrates this pattern.

### Coverage Targets

| Crate | Target | Notes |
|-------|--------|-------|
| `abp-core` | < 5% missed | Contract types are the product |
| `abp-protocol` | < 10% missed | Wire format correctness |
| Other crates | Advisory | Run periodically to find gaps |

### CI Integration

Mutation testing runs as an **advisory, manual-dispatch** workflow
(`.github/workflows/mutants.yml`).  It is not part of the regular CI gate
because runs are slow (minutes per mutant).  Trigger it manually when:

- Adding new public API surface
- Refactoring core logic
- Reviewing test coverage before a release

---

## Writing New Tests

### Conventions

- **Unit tests**: place in `#[cfg(test)]` module at the bottom of the source
  file for small, focused tests. Use `tests/` directory for larger test suites.
- **Proptest**: create a `proptest_*.rs` file in the crate's `tests/` directory.
  Reuse strategies from `abp-core/tests/proptest_types.rs` where possible.
- **Snapshots**: use `insta::assert_json_snapshot!()` for serialization tests.
  Place snapshot files in `tests/snapshots/`. Name snapshots descriptively.
- **Integration tests**: test public APIs only. Avoid reaching into private
  internals.

### Dev Dependencies

Available test dependencies (see individual `Cargo.toml` files):

| Dependency | Version | Purpose |
|-----------|---------|---------|
| `insta` | `1` (with `json` feature) | Snapshot testing |
| `proptest` | `1` | Property-based testing |
| `assert_cmd` | `2` | CLI binary testing |
| `predicates` | `3` | Assertion helpers |
| `criterion` | `0.5` (with `html_reports`) | Benchmarking |
| `tempfile` | `3` | Temporary file/directory creation |

### Adding a New Fuzz Target

1. Create `fuzz/fuzz_targets/fuzz_<name>.rs`:
   ```rust
   #![no_main]
   use libfuzzer_sys::fuzz_target;

   fuzz_target!(|data: &[u8]| {
       let _ = serde_json::from_slice::<YourType>(data);
   });
   ```
2. Add a `[[bin]]` entry to `fuzz/Cargo.toml`.
3. Run: `cargo +nightly fuzz run fuzz_<name>`.
