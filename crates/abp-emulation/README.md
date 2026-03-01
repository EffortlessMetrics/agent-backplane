# abp-emulation

Labeled capability emulation for the
[Agent Backplane](https://github.com/paiml/agent-backplane) project.

When a backend does not natively support a capability (e.g. `ExtendedThinking`),
ABP can emulate it through system-prompt injection or post-processing — but
**only** with explicit labeling so the caller knows the result is emulated,
never silently degraded. Every emulation action is recorded in an
`EmulationReport`.

## Quick start

```rust
use abp_emulation::{EmulationEngine, EmulationConfig};
use abp_core::Capability;
use abp_core::ir::{IrConversation, IrMessage, IrRole};

let engine = EmulationEngine::with_defaults();

let mut conv = IrConversation::new()
    .push(IrMessage::text(IrRole::System, "You are helpful."))
    .push(IrMessage::text(IrRole::User, "Explain monads."));

let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
assert_eq!(report.applied.len(), 1);
assert!(report.warnings.is_empty());
```

## License

Dual-licensed under MIT OR Apache-2.0.
