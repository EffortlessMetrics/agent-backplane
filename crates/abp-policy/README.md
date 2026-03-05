# abp-policy

Policy engine with tool/read/write allow/deny checks for Agent Backplane.

Compiles a `PolicyProfile` (from `abp-core`) into a `PolicyEngine` that
evaluates tool use, file read, and file write requests against allow/deny
rules using glob patterns.

## Key Types

| Type | Description |
|------|-------------|
| `PolicyEngine` | Compiled policy evaluator with tool and path checks |
| `Decision` | Result of a policy check — allowed or denied with optional reason |

## Usage

```rust
use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

let profile = PolicyProfile::default(); // empty = permit everything
let engine = PolicyEngine::new(&profile).unwrap();

let decision = engine.can_use_tool("bash");
assert!(decision.allowed);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
