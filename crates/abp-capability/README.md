# abp-capability

Capability negotiation between work-order requirements and backend manifests for the
[Agent Backplane](https://github.com/paiml/agent-backplane) project.

This crate compares a `CapabilityManifest` (what a backend advertises) against
`CapabilityRequirements` (what a work order needs) and produces structured
`NegotiationResult`s and human-readable `CompatibilityReport`s. Each capability
is classified as **native**, **emulatable**, or **unsupported**.

## Quick start

```rust
use abp_capability::{negotiate, generate_report};
use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements,
    MinSupport, SupportLevel,
};

// Build a manifest advertising native streaming support
let manifest = [(Capability::Streaming, SupportLevel::Native)]
    .into_iter()
    .collect();

// Require streaming
let requirements = CapabilityRequirements {
    required: vec![CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    }],
};

let result = negotiate(&manifest, &requirements);
assert!(result.is_compatible());

let report = generate_report(&result);
println!("{}", report.summary);
```

## License

Dual-licensed under MIT OR Apache-2.0.
