// SPDX-License-Identifier: MIT OR Apache-2.0
//! Request validation for the daemon API.

use abp_core::WorkOrder;
use abp_json_guard::{JsonGuardLimits, validate_json_object};
use uuid::Uuid;

/// Validates incoming API requests before processing.
pub struct RequestValidator;

/// Maximum allowed length for the task string.
const MAX_TASK_LENGTH: usize = 100_000;

/// Maximum allowed length for a backend name.
const MAX_BACKEND_NAME_LENGTH: usize = 256;

impl RequestValidator {
    /// Validate all fields of a [`WorkOrder`], accumulating every error found.
    pub fn validate_work_order(wo: &WorkOrder) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if let Err(e) = Self::validate_run_id(&wo.id.to_string()) {
            errors.push(e);
        }

        if wo.task.is_empty() {
            errors.push("task must not be empty".into());
        } else if wo.task.len() > MAX_TASK_LENGTH {
            errors.push(format!(
                "task exceeds maximum length of {MAX_TASK_LENGTH} characters"
            ));
        } else if wo.task.trim().is_empty() {
            errors.push("task must contain non-whitespace characters".into());
        }

        if wo.workspace.root.is_empty() {
            errors.push("workspace.root must not be empty".into());
        }

        if let Some(budget) = wo.config.max_budget_usd {
            if budget < 0.0 {
                errors.push("config.max_budget_usd must not be negative".into());
            }
            if budget.is_nan() || budget.is_infinite() {
                errors.push("config.max_budget_usd must be a finite number".into());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate that `id` is a well-formed UUID string.
    pub fn validate_run_id(id: &str) -> Result<(), String> {
        if id.is_empty() {
            return Err("run_id must not be empty".into());
        }
        id.parse::<Uuid>()
            .map(|_| ())
            .map_err(|_| format!("invalid UUID format: {id}"))
    }

    /// Validate that `name` refers to a known backend.
    pub fn validate_backend_name(name: &str, backends: &[String]) -> Result<(), String> {
        if name.is_empty() {
            return Err("backend name must not be empty".into());
        }
        if name.len() > MAX_BACKEND_NAME_LENGTH {
            return Err(format!(
                "backend name exceeds maximum length of {MAX_BACKEND_NAME_LENGTH}"
            ));
        }
        if !backends.iter().any(|b| b == name) {
            return Err(format!("unknown backend: {name}"));
        }
        Ok(())
    }

    /// Validate a JSON config value. The value must be an object (if present)
    /// and must not contain excessively nested structures.
    pub fn validate_config(config: &serde_json::Value) -> Result<(), Vec<String>> {
        const MAX_DEPTH: usize = 10;
        const MAX_SIZE_BYTES: usize = 1_000_000;

        let errors = validate_json_object(config, JsonGuardLimits::new(MAX_DEPTH, MAX_SIZE_BYTES));

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_uuid_accepted() {
        let id = Uuid::new_v4().to_string();
        assert!(RequestValidator::validate_run_id(&id).is_ok());
    }

    #[test]
    fn nil_uuid_accepted() {
        assert!(RequestValidator::validate_run_id(&Uuid::nil().to_string()).is_ok());
    }

    #[test]
    fn invalid_uuid_rejected() {
        assert!(RequestValidator::validate_run_id("not-a-uuid").is_err());
    }

    #[test]
    fn empty_uuid_rejected() {
        assert!(RequestValidator::validate_run_id("").is_err());
    }

    #[test]
    fn valid_backend_accepted() {
        let backends = vec!["mock".to_string(), "sidecar:node".to_string()];
        assert!(RequestValidator::validate_backend_name("mock", &backends).is_ok());
    }

    #[test]
    fn unknown_backend_rejected() {
        let backends = vec!["mock".to_string()];
        let err = RequestValidator::validate_backend_name("unknown", &backends).unwrap_err();
        assert!(err.contains("unknown backend"));
    }

    #[test]
    fn empty_backend_rejected() {
        let backends = vec!["mock".to_string()];
        assert!(RequestValidator::validate_backend_name("", &backends).is_err());
    }

    #[test]
    fn valid_config_accepted() {
        let config = serde_json::json!({"key": "value"});
        assert!(RequestValidator::validate_config(&config).is_ok());
    }

    #[test]
    fn non_object_config_rejected() {
        let config = serde_json::json!("not an object");
        assert!(RequestValidator::validate_config(&config).is_err());
    }

    #[test]
    fn array_config_rejected() {
        let config = serde_json::json!([1, 2, 3]);
        assert!(RequestValidator::validate_config(&config).is_err());
    }
}
