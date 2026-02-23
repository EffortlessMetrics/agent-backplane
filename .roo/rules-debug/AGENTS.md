# Agent Backplane - Debug Mode Rules

## Protocol Debugging
- Enable tracing: `RUST_LOG=abp.sidecar.stderr=debug,abp.runtime=trace`
- Sidecar stderr captured to `abp.sidecar.stderr` target - see [`abp-host/src/lib.rs:125`](crates/abp-host/src/lib.rs:125)

## Common Failure Modes

### "expected hello, got X"
- Sidecar wrote output before hello envelope
- Check sidecar startup logs for early errors/prints

### "sidecar exited unexpectedly"
- Sidecar crashed before sending hello
- Check stderr via `abp.sidecar.stderr` tracing target

### Receipt Hash Mismatch
- Hash computed with `receipt_sha256` not null - see [`receipt_hash()`](crates/abp-core/src/lib.rs:380)
- Use `Receipt::with_hash()` to ensure correct computation

## JSONL Protocol Issues
- Each line must be valid JSON - use `jq` to validate: `cat output.jsonl | jq .`
- Envelope type field is `"t"` not `"type"` - common parsing error

## Workspace Staging Failures
- Check glob patterns in `PolicyProfile` - invalid globs fail at runtime
- Staged mode requires write access to temp dir

## Test Debugging
```bash
cargo test -p abp-core receipt_hash    # Run specific test
RUST_LOG=debug cargo test -p <crate>   # Debug logging in tests
```
