# abp-mapping

Cross-dialect mapping validation for the Agent Backplane.

Provides feature-level fidelity tracking when translating between agent dialects.
Each `MappingRule` pairs a source/target dialect with a feature name and a
`Fidelity` grade (lossless, lossy-labeled, or unsupported). The `MappingRegistry`
collects rules and produces `FidelityReport` summaries for any dialect pair.

## Key Types

| Type | Description |
|------|-------------|
| `MappingRule` | Describes how a single feature translates between two dialects |
| `Fidelity` | Grade of a mapping: `Lossless`, `LossyLabeled`, or `Unsupported` |
| `MappingError` | Error variants for unsupported features, fidelity loss, and dialect mismatches |
| `MappingValidation` | Per-feature validation result with fidelity and errors |
| `FidelityReport` | Aggregated fidelity summary across all features for a dialect pair |
| `BidirectionalReport` | Validates both directions (A to B and B to A) for symmetry |
| `RuleMetadata` | Documentation and versioning metadata attached to a mapping rule |

## Usage

```rust
use abp_mapping::{MappingRule, Fidelity};
use abp_dialect::Dialect;

let rule = MappingRule {
    source_dialect: Dialect::OpenAi,
    target_dialect: Dialect::Claude,
    feature: "streaming".into(),
    fidelity: Fidelity::Lossless,
};
assert!(rule.fidelity.is_lossless());
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
