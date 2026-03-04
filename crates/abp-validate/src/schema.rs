// SPDX-License-Identifier: MIT OR Apache-2.0
//! JSON schema validation for contract types.

use crate::{ValidationErrorKind, ValidationErrors, Validator};

/// Validates a [`serde_json::Value`] against structural expectations of a
/// contract type (work order, receipt, event).
///
/// This is a lightweight structural check — it verifies required fields,
/// types, and nesting without relying on a compiled JSON Schema registry.
#[derive(Debug)]
pub struct SchemaValidator {
    required_fields: Vec<(String, JsonType)>,
}

/// Expected JSON type for a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonType {
    /// JSON string.
    String,
    /// JSON number.
    Number,
    /// JSON boolean.
    Bool,
    /// JSON object.
    Object,
    /// JSON array.
    Array,
    /// Any non-null value.
    Any,
}

impl SchemaValidator {
    /// Create a validator with the given required fields and their expected types.
    #[must_use]
    pub fn new(required_fields: Vec<(String, JsonType)>) -> Self {
        Self { required_fields }
    }

    /// Create a validator for work order JSON.
    #[must_use]
    pub fn work_order() -> Self {
        Self::new(vec![
            ("id".into(), JsonType::String),
            ("task".into(), JsonType::String),
            ("lane".into(), JsonType::String),
            ("workspace".into(), JsonType::Object),
            ("context".into(), JsonType::Object),
            ("policy".into(), JsonType::Object),
            ("config".into(), JsonType::Object),
        ])
    }

    /// Create a validator for receipt JSON.
    #[must_use]
    pub fn receipt() -> Self {
        Self::new(vec![
            ("meta".into(), JsonType::Object),
            ("backend".into(), JsonType::Object),
            ("outcome".into(), JsonType::String),
            ("trace".into(), JsonType::Array),
            ("artifacts".into(), JsonType::Array),
        ])
    }

    /// Create a validator for agent event JSON.
    #[must_use]
    pub fn agent_event() -> Self {
        Self::new(vec![
            ("ts".into(), JsonType::String),
            ("type".into(), JsonType::String),
        ])
    }
}

impl Validator<serde_json::Value> for SchemaValidator {
    fn validate(&self, value: &serde_json::Value) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        let obj = match value.as_object() {
            Some(o) => o,
            None => {
                errs.add(
                    "",
                    ValidationErrorKind::InvalidFormat,
                    "value must be a JSON object",
                );
                return errs.into_result();
            }
        };

        for (field, expected_type) in &self.required_fields {
            match obj.get(field.as_str()) {
                None => {
                    errs.add(
                        field,
                        ValidationErrorKind::Required,
                        format!("missing required field '{field}'"),
                    );
                }
                Some(serde_json::Value::Null) => {
                    errs.add(
                        field,
                        ValidationErrorKind::Required,
                        format!("field '{field}' must not be null"),
                    );
                }
                Some(val) => {
                    let type_ok = match expected_type {
                        JsonType::String => val.is_string(),
                        JsonType::Number => val.is_number(),
                        JsonType::Bool => val.is_boolean(),
                        JsonType::Object => val.is_object(),
                        JsonType::Array => val.is_array(),
                        JsonType::Any => true,
                    };
                    if !type_ok {
                        errs.add(
                            field,
                            ValidationErrorKind::InvalidFormat,
                            format!(
                                "field '{field}' expected type {expected_type:?}, got {}",
                                json_type_name(val),
                            ),
                        );
                    }
                }
            }
        }

        errs.into_result()
    }
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
