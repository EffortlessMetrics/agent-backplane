# abp-glob

Include/exclude glob pattern compilation for Agent Backplane.

Compiles include and exclude glob patterns into a single evaluator that
classifies paths as allowed or denied. Used by both workspace staging and
policy enforcement.

## Key Types

| Type | Description |
|------|-------------|
| `IncludeExcludeGlobs` | Compiled include/exclude glob pair for path filtering |
| `MatchDecision` | Result of evaluation: `Allowed`, `DeniedByExclude`, or `DeniedByMissingInclude` |

## Usage

```rust
use abp_glob::{IncludeExcludeGlobs, MatchDecision};

let globs = IncludeExcludeGlobs::new(
    &["src/**".into()],
    &["src/generated/**".into()],
).unwrap();

assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
assert_eq!(globs.decide_str("src/generated/out.rs"), MatchDecision::DeniedByExclude);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
