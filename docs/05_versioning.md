# Versioning

Backplane has two separate version surfaces:

1) **Contract version** (`abp-core::CONTRACT_VERSION`)
2) **Implementation version** (crate/package versions)

## Contract versioning

- The contract version changes only when the serialized schema changes.
- Additive changes (new optional fields, new enum variants behind feature gates) can stay within the same contract version.
- Breaking changes require a new contract version.

## Adapter compatibility

Sidecars declare the contract version in `hello`.

Control plane behavior:

- If sidecar contract version matches: proceed.
- If sidecar is older: attempt to downgrade/omit fields (best-effort) or fail.
- If sidecar is newer: fail (control plane cannot safely interpret receipts).

## Receipt hashing

Receipts are canonicalized and hashed.

This enables:

- dedupe
- auditability
- offline verification

