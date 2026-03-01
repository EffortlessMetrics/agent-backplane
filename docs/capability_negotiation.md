# Capability Negotiation

> How ABP matches work-order requirements against backend manifests.

Capability negotiation ensures a backend can satisfy a work order's requirements
**before** execution begins. The system classifies each capability as native,
emulated, or unsupported, and provides labeled emulation strategies for
capabilities that aren't natively available.

**Source crates:** `abp-capability`, `abp-emulation`, `abp-core`

---

## Table of Contents

- [Overview](#overview)
- [Support Levels](#support-levels)
- [Negotiation Flow](#negotiation-flow)
- [NegotiationResult](#negotiationresult)
- [CompatibilityReport](#compatibilityreport)
- [Decision Tree](#decision-tree)
- [Emulation Strategies](#emulation-strategies)
- [EmulationEngine](#emulationengine)
- [Default Strategy Table](#default-strategy-table)
- [JSON Examples](#json-examples)
- [API Reference](#api-reference)

---

## Overview

Every ABP backend advertises a **capability manifest** — a `BTreeMap<Capability,
SupportLevel>` — describing what features it supports and how well. Work orders
carry **capability requirements** declaring the minimum support level needed for
each feature. The negotiation system compares these two data structures and
produces a structured result.

The three-crate split:

| Crate | Purpose |
|-------|---------|
| `abp-core` | Defines `Capability`, `SupportLevel`, `CapabilityManifest`, `CapabilityRequirements` |
| `abp-capability` | Negotiation logic: `negotiate()`, `check_capability()`, `generate_report()` |
| `abp-emulation` | Emulation strategies: `EmulationEngine`, `EmulationConfig`, `apply_emulation()` |

---

## Support Levels

`SupportLevel` in `abp-core` describes how well a backend supports a capability:

| Level | Wire Value | Meaning |
|-------|------------|---------|
| **Native** | `"native"` | First-class support — no translation or emulation needed |
| **Emulated** | `"emulated"` | Supported via ABP translation layer with acceptable fidelity |
| **Restricted** | `{"restricted": {"reason": "..."}}` | Supported but limited by policy or environment |
| **Unsupported** | `"unsupported"` | Cannot be provided |

### MinSupport Thresholds

Work order requirements specify a **minimum** acceptable level:

| `MinSupport` | Satisfied by |
|--------------|-------------|
| `Native` | `Native` only |
| `Emulated` | `Native`, `Emulated`, or `Restricted` |

Note: `Restricted` satisfies `Emulated` but **not** `Native`.

### Negotiation-level SupportLevel

`abp-capability` uses its own `SupportLevel` enum for negotiation results:

| Level | Meaning |
|-------|---------|
| **Native** | Backend supports this natively |
| **Emulated { strategy }** | Can be provided through adapter/polyfill (includes description) |
| **Unsupported** | Cannot be provided |

`Restricted` from the core level maps to `Emulated { strategy: "restricted: <reason>" }`
in the negotiation result.

---

## Negotiation Flow

```
  WorkOrder                          Backend
  ────────                           ───────
  CapabilityRequirements             CapabilityManifest
  ┌─────────────────────┐            ┌─────────────────────┐
  │ required:            │            │ Streaming: Native    │
  │  - Streaming (native)│            │ ToolRead:  Emulated  │
  │  - ToolRead (emulated)│           │ ToolBash:  Restricted│
  │  - ToolEdit (emulated)│           └─────────────────────┘
  └──────────┬──────────┘
             │
             ▼
      ┌──────────────┐
      │  negotiate()  │    For each requirement:
      │               │    manifest.get(capability) → classify
      └──────┬───────┘
             │
             ▼
      NegotiationResult
      ┌─────────────────────────────┐
      │ native:      [Streaming]     │
      │ emulatable:  [ToolRead]      │
      │ unsupported: [ToolEdit]      │
      └──────────┬──────────────────┘
                 │
          ┌──────┴──────┐
          │              │
     is_compatible()   is_compatible()
       = false           = true
          │              │
          ▼              ▼
      REJECT          generate_report()
      dispatch          │
                        ▼
                 CompatibilityReport
                 ┌────────────────────────┐
                 │ compatible: true        │
                 │ native_count: 2         │
                 │ emulated_count: 1       │
                 │ unsupported_count: 0    │
                 │ summary: "2 native, …"  │
                 │ details: [...]          │
                 └────────────────────────┘
```

### Classification Rules

For each required capability:

| Manifest entry | Negotiation result |
|---------------|-------------------|
| `Native` | → **native** bucket |
| `Emulated` | → **emulatable** bucket |
| `Restricted { reason }` | → **emulatable** bucket (strategy includes reason) |
| `Unsupported` | → **unsupported** bucket |
| Not present in manifest | → **unsupported** bucket |

A `NegotiationResult` is **compatible** if and only if `unsupported` is empty.

---

## NegotiationResult

```rust
pub struct NegotiationResult {
    pub native: Vec<Capability>,      // natively supported
    pub emulatable: Vec<Capability>,  // can be emulated
    pub unsupported: Vec<Capability>, // cannot be provided
}

impl NegotiationResult {
    pub fn is_compatible(&self) -> bool;  // unsupported.is_empty()
    pub fn total(&self) -> usize;         // sum of all three
}
```

---

## CompatibilityReport

A human-readable summary produced by `generate_report()`:

```rust
pub struct CompatibilityReport {
    pub compatible: bool,
    pub native_count: usize,
    pub emulated_count: usize,
    pub unsupported_count: usize,
    pub summary: String,              // e.g. "2 native, 1 emulatable, 0 unsupported — fully compatible"
    pub details: Vec<(String, SupportLevel)>,
}
```

---

## Decision Tree

Use this decision tree to understand how a single capability is classified
and what actions follow:

```
  Required capability C with min_support M
  │
  ├─ Is C in the backend manifest?
  │   │
  │   ├─ NO
  │   │   └─ Result: UNSUPPORTED
  │   │       └─ Can C be emulated?
  │   │           ├─ YES → apply EmulationStrategy (see below)
  │   │           └─ NO  → fail pre-dispatch check
  │   │
  │   └─ YES → what is the manifest level?
  │       │
  │       ├─ Native
  │       │   └─ Result: NATIVE
  │       │       └─ Satisfies MinSupport::Native? YES
  │       │       └─ Satisfies MinSupport::Emulated? YES
  │       │
  │       ├─ Emulated
  │       │   └─ Result: EMULATABLE
  │       │       └─ Satisfies MinSupport::Native? NO
  │       │       └─ Satisfies MinSupport::Emulated? YES
  │       │
  │       ├─ Restricted { reason }
  │       │   └─ Result: EMULATABLE (with restriction note)
  │       │       └─ Satisfies MinSupport::Native? NO
  │       │       └─ Satisfies MinSupport::Emulated? YES
  │       │
  │       └─ Unsupported
  │           └─ Result: UNSUPPORTED
  │               └─ Same as "not in manifest" path above
  │
  └─ After classification:
      │
      ├─ ALL capabilities native or emulatable?
      │   └─ YES → NegotiationResult.is_compatible() = true
      │            → proceed to dispatch
      │            → for emulatable caps: apply EmulationEngine
      │
      └─ ANY capability unsupported?
          └─ YES → NegotiationResult.is_compatible() = false
                   → reject dispatch
                   → error: CAPABILITY_UNSUPPORTED
```

---

## Emulation Strategies

When a capability is not natively supported but can be approximated, ABP uses
labeled emulation. The key design principle: **never silently degrade**. Every
emulation is explicitly recorded in an `EmulationReport`.

**Source:** `crates/abp-emulation/src/lib.rs`

### EmulationStrategy Enum

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EmulationStrategy {
    SystemPromptInjection { prompt: String },
    PostProcessing { detail: String },
    Disabled { reason: String },
}
```

| Strategy | How it works | Mutates conversation? |
|----------|-------------|----------------------|
| **SystemPromptInjection** | Injects text into the system prompt to approximate the capability. If a system message exists, appends to it; otherwise prepends a new one. | Yes |
| **PostProcessing** | Applies post-processing on the assistant's response (e.g. JSON validation). Recorded in the report but applied after the response. | No |
| **Disabled** | Cannot be safely emulated. Produces a warning but does not modify anything. | No |

---

## EmulationEngine

The `EmulationEngine` applies strategies to an `IrConversation`:

```rust
let engine = EmulationEngine::with_defaults();
let report = engine.apply(&[Capability::ExtendedThinking], &mut conversation);

// Check results
assert_eq!(report.applied.len(), 1);
assert!(report.warnings.is_empty());
```

### Configuration

Use `EmulationConfig` to override default strategies per capability:

```rust
let mut config = EmulationConfig::new();
config.set(
    Capability::CodeExecution,
    EmulationStrategy::SystemPromptInjection {
        prompt: "Simulate code execution step by step.".into(),
    },
);
let engine = EmulationEngine::new(config);
```

The engine resolves strategies by checking config overrides first, then
falling back to `default_strategy()`.

### EmulationReport

Every call to `engine.apply()` returns an `EmulationReport`:

```rust
pub struct EmulationReport {
    pub applied: Vec<EmulationEntry>,  // successfully applied
    pub warnings: Vec<String>,         // disabled capabilities
}

pub struct EmulationEntry {
    pub capability: Capability,
    pub strategy: EmulationStrategy,
}
```

- `report.is_empty()` — no emulations applied and no warnings.
- `report.has_unemulatable()` — at least one requested capability could not be emulated.

---

## Default Strategy Table

| Capability | Default Strategy | Details |
|-----------|-----------------|---------|
| `ExtendedThinking` | `SystemPromptInjection` | Injects "Think step by step before answering." |
| `StructuredOutputJsonSchema` | `PostProcessing` | "Parse and validate JSON from text response" |
| `CodeExecution` | `Disabled` | "Cannot safely emulate sandboxed code execution" |
| All other capabilities | `Disabled` | "No emulation available for \<capability\>" |

Use `can_emulate(capability)` to check if a capability has a non-Disabled
default strategy.

---

## JSON Examples

### NegotiationResult

```json
{
  "native": ["streaming", "tool_read"],
  "emulatable": ["tool_write"],
  "unsupported": []
}
```

### CompatibilityReport

```json
{
  "compatible": true,
  "native_count": 2,
  "emulated_count": 1,
  "unsupported_count": 0,
  "summary": "2 native, 1 emulatable, 0 unsupported — fully compatible",
  "details": [
    ["Streaming", {"level": "native"}],
    ["ToolRead", {"level": "native"}],
    ["ToolWrite", {"level": "emulated", "strategy": "adapter"}]
  ]
}
```

### EmulationReport

```json
{
  "applied": [
    {
      "capability": "extended_thinking",
      "strategy": {
        "type": "system_prompt_injection",
        "prompt": "Think step by step before answering."
      }
    }
  ],
  "warnings": [
    "Capability CodeExecution not emulated: Cannot safely emulate sandboxed code execution"
  ]
}
```

---

## API Reference

### abp-capability

```rust
/// Classify a single capability against a manifest.
pub fn check_capability(manifest: &CapabilityManifest, cap: &Capability) -> SupportLevel;

/// Negotiate all required capabilities against a manifest.
pub fn negotiate(manifest: &CapabilityManifest, requirements: &CapabilityRequirements) -> NegotiationResult;

/// Produce a human-readable report from a negotiation result.
pub fn generate_report(result: &NegotiationResult) -> CompatibilityReport;
```

### abp-emulation

```rust
/// Return the default emulation strategy for a capability.
pub fn default_strategy(capability: &Capability) -> EmulationStrategy;

/// Returns true if the capability has a non-Disabled default strategy.
pub fn can_emulate(capability: &Capability) -> bool;

/// Apply emulations with a given config (free function).
pub fn apply_emulation(config: &EmulationConfig, capabilities: &[Capability], conv: &mut IrConversation) -> EmulationReport;
```
