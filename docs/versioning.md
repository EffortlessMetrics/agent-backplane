# Versioning

> Current contract version: `abp/v0.1`

This document describes the versioning scheme, compatibility rules, and
evolution strategy for the Agent Backplane wire protocol.

---

## Versioning Scheme

Contract versions follow the format **`abp/v{major}.{minor}`**.

| Component | Meaning |
|-----------|---------|
| `abp/v` | Fixed prefix — identifies an Agent Backplane contract version |
| `{major}` | Incremented for breaking changes |
| `{minor}` | Incremented for backward-compatible additions |

The contract version is defined in `abp-core` as `CONTRACT_VERSION` and is
embedded in every `hello` envelope and every `Receipt.meta.contract_version`
field.

**Implementation versions** (crate / package versions) are independent of
the contract version. A crate may ship many releases while the contract stays
at the same version.

---

## Compatibility Rules

Two versions are **compatible** when they share the same **major** number.

| Control Plane | Sidecar | Compatible? | Reason |
|---------------|---------|-------------|--------|
| `abp/v0.1` | `abp/v0.1` | ✓ | Exact match |
| `abp/v0.1` | `abp/v0.2` | ✓ | Same major version |
| `abp/v0.2` | `abp/v0.1` | ✓ | Same major version |
| `abp/v0.1` | `abp/v1.0` | ✗ | Different major version |
| `abp/v1.0` | `abp/v2.0` | ✗ | Different major version |

Compatibility is checked during the JSONL handshake: the sidecar sends its
`contract_version` in the `hello` envelope, and the control plane compares
it against its own version using `is_compatible_version()`. If incompatible,
the control plane rejects the sidecar with a protocol error.

---

## Wire Format Stability Guarantees

Within a major version the following are guaranteed:

1. **Existing fields are never removed or renamed.** A message serialized by
   an older minor version will always deserialize under a newer minor version
   within the same major.

2. **Enum variant strings are stable.** Once a variant (e.g. `"patch_first"`,
   `"complete"`) is published in a release, its serialized form will not change
   within the major version.

3. **Discriminator tags are stable.** The protocol envelope uses `"t"` and
   `AgentEventKind` uses `"type"` — these will not change within a major
   version.

4. **Deterministic serialization.** All maps use `BTreeMap` for sorted key
   order, ensuring canonical JSON output suitable for hashing.

---

## Schema Evolution Strategy

### Minor version bumps (additive only)

The following changes are permitted without bumping the major version:

- Adding **new optional fields** to existing structs (with `#[serde(default)]`
  so older consumers can ignore them).
- Adding **new enum variants** to open enums (consumers must tolerate unknown
  variants gracefully).
- Adding **new envelope types** (older control planes / sidecars can report
  an `UnexpectedMessage` error for unknown types).
- Adding **new capability keys** to `CapabilityManifest`.

### Major version bumps (breaking changes)

The following changes require a new major version:

- Removing or renaming an existing field.
- Changing the type of an existing field.
- Changing the serialized string of an enum variant.
- Changing a discriminator tag name (`"t"`, `"type"`).
- Changing the semantics of an existing field in an incompatible way.
- Removing an envelope type or enum variant.

---

## Breaking Change Policy

1. **No silent breakage.** Any change that alters the serialized wire format
   in a way that would cause existing consumers to fail **must** bump the
   major version.

2. **Contract stability tests gate merges.** The `contract_stability` test
   suite in `abp-core` and the `version_compat_tests` suite in `abp-protocol`
   contain blessed fixtures and assertions. CI failures in these tests indicate
   a breaking wire-format change that must be handled deliberately.

3. **Dual-version support window.** When a new major version is introduced,
   the previous major version remains supported for at least one release
   cycle. Sidecars may advertise the older version and the control plane
   should attempt best-effort translation or clearly report incompatibility.

---

## Deprecation Process

1. **Announce.** The field or feature is marked as deprecated in documentation
   and, where possible, with Rust `#[deprecated]` attributes.

2. **Retain.** The deprecated item continues to function within the current
   major version. It is serialized/deserialized normally.

3. **Remove.** The item is removed in the next major version bump. A
   CHANGELOG entry documents the removal and migration path.

Deprecation applies to contract-level constructs (fields, enum variants,
envelope types). Internal API deprecation follows standard Rust semver
conventions.

---

## Version Parsing

The `abp-protocol` crate provides two helper functions:

```rust
/// Parse "abp/v{MAJOR}.{MINOR}" into (MAJOR, MINOR).
pub fn parse_version(version: &str) -> Option<(u32, u32)>;

/// Returns true if two version strings share the same major number.
pub fn is_compatible_version(a: &str, b: &str) -> bool;
```

Invalid strings (wrong prefix, non-numeric components, extra segments)
return `None` / `false` respectively.
