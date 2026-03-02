// SPDX-License-Identifier: MIT OR Apache-2.0
//! Guardrails for untrusted JSON payloads.

use serde_json::Value;

/// Limits used to validate JSON payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonGuardLimits {
    /// Maximum allowed depth of objects/arrays.
    pub max_depth: usize,
    /// Maximum allowed UTF-8 byte size after JSON serialization.
    pub max_size_bytes: usize,
}

impl JsonGuardLimits {
    /// Constructs new JSON guard limits.
    pub const fn new(max_depth: usize, max_size_bytes: usize) -> Self {
        Self {
            max_depth,
            max_size_bytes,
        }
    }
}

/// Validates that `value` is a JSON object and does not exceed depth/size constraints.
///
/// Returns a vector of validation errors; empty means the payload passed all checks.
pub fn validate_json_object(value: &Value, limits: JsonGuardLimits) -> Vec<String> {
    let mut errors = Vec::new();

    if !value.is_object() {
        errors.push("config must be a JSON object".into());
        return errors;
    }

    if exceeds_depth(value, limits.max_depth) {
        errors.push(format!(
            "config exceeds maximum nesting depth of {}",
            limits.max_depth
        ));
    }

    if value.to_string().len() > limits.max_size_bytes {
        let max_mb = limits.max_size_bytes / 1_000_000;
        if max_mb > 0 {
            errors.push(format!("config exceeds maximum size of {max_mb}MB"));
        } else {
            errors.push(format!(
                "config exceeds maximum size of {} bytes",
                limits.max_size_bytes
            ));
        }
    }

    errors
}

/// Returns `true` if `value` exceeds `max_depth` levels of nesting.
fn exceeds_depth(value: &Value, max_depth: usize) -> bool {
    if max_depth == 0 {
        return value.is_object() || value.is_array();
    }
    match value {
        Value::Object(map) => map.values().any(|v| exceeds_depth(v, max_depth - 1)),
        Value::Array(arr) => arr.iter().any(|v| exceeds_depth(v, max_depth - 1)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_objects() {
        let errors =
            validate_json_object(&serde_json::json!([1, 2, 3]), JsonGuardLimits::new(10, 10));
        assert_eq!(errors, vec!["config must be a JSON object"]);
    }

    #[test]
    fn rejects_nested_structures_that_exceed_depth() {
        let value = serde_json::json!({"a": {"b": {"c": 1}}});
        let errors = validate_json_object(&value, JsonGuardLimits::new(2, 1_000_000));
        assert!(errors.iter().any(|e| e.contains("maximum nesting depth")));
    }

    #[test]
    fn rejects_payloads_larger_than_limit() {
        let value = serde_json::json!({"data": "1234567890"});
        let errors = validate_json_object(&value, JsonGuardLimits::new(10, 5));
        assert!(errors.iter().any(|e| e.contains("maximum size")));
    }

    #[test]
    fn accepts_payloads_within_limits() {
        let value = serde_json::json!({"key": [1, 2, 3]});
        let errors = validate_json_object(&value, JsonGuardLimits::new(10, 1_000_000));
        assert!(errors.is_empty());
    }
}
