# abp-dialect

Dialect detection, validation, and metadata for the Agent Backplane.

Analyzes raw JSON messages, HTTP headers, and API endpoints to determine which
vendor dialect (OpenAI, Claude, Gemini, Codex, Kimi, Copilot) produced them.
Returns confidence-scored results with human-readable evidence. Also provides
structural validation against each dialect's expected schema.

## Key Types

| Type | Description |
|------|-------------|
| `Dialect` | Enum of known agent-protocol dialects (OpenAI, Claude, Gemini, Codex, Kimi, Copilot) |
| `DialectDetector` | Scores JSON payloads, headers, and endpoints to identify the source dialect |
| `DetectionResult` | Detection outcome with dialect, confidence score, and evidence strings |
| `DialectValidator` | Validates a JSON message against a specific dialect's structural rules |
| `ValidationResult` | Validation outcome with errors and warnings |
| `ValidationError` | A single error with JSON-pointer path and message |

## Usage

```rust
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use serde_json::json;

let detector = DialectDetector::new();
let msg = json!({
    "model": "claude-3-opus-20240229",
    "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}]
});

let result = detector.detect(&msg).unwrap();
assert_eq!(result.dialect, Dialect::Claude);

let validator = DialectValidator::new();
let validation = validator.validate(&msg, Dialect::Claude);
assert!(validation.valid);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.