# abp-validate

Validation utilities for Agent Backplane work orders, receipts, events, and protocol envelopes.

Ensures contract compliance before execution by running typed validators against
each ABP data structure. Validators produce structured `ValidationErrors` with
machine-readable error kinds. The `CompositeValidator` chains multiple validators,
and `RuleBuilder` allows defining custom validation rules.

## Key Types

| Type | Description |
|------|-------------|
| `Validator<T>` | Core trait for validating a value of type `T` |
| `WorkOrderValidator` | Validates work order structure and required fields |
| `ReceiptValidator` | Validates receipt integrity and hash consistency |
| `EventValidator` | Validates agent event structure |
| `EnvelopeValidator` | Validates JSONL protocol envelopes |
| `CompositeValidator` | Chains multiple validators into a single pass |
| `ValidationErrors` | Accumulated error collection from a validation run |
| `ValidationReport` | Structured report with severity levels |
| `RuleBuilder` | Fluent builder for custom validation rules |

## Usage

```rust,no_run
use abp_validate::{WorkOrderValidator, Validator};

let validator = WorkOrderValidator;
// validator.validate(&work_order)?;
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
