# abp-receipt

Receipt canonicalization, hashing, chain verification, and diffing for the
[Agent Backplane](https://github.com/paiml/agent-backplane) project.

This crate extracts receipt-focused logic into a dedicated microcrate. It
provides canonical JSON serialization (with `receipt_sha256` set to `null`
before hashing to prevent self-referential hashes), SHA-256 hash computation
and verification, ordered receipt chain validation, a fluent `ReceiptBuilder`,
and field-level receipt diffing.

## Quick start

```rust
use abp_receipt::{ReceiptBuilder, Outcome, compute_hash, verify_hash};

let mut receipt = ReceiptBuilder::new("mock")
    .outcome(Outcome::Complete)
    .build();

// Compute and attach a canonical hash
receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());
assert!(verify_hash(&receipt));
```

## License

Dual-licensed under MIT OR Apache-2.0.
