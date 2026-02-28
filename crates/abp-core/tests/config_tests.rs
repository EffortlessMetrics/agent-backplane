use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::{PolicyProfile, RuntimeConfig, WorkOrderBuilder};
use std::collections::BTreeMap;

fn validator() -> ConfigValidator {
    ConfigValidator::new()
}

// ---------- task checks ----------

#[test]
fn valid_work_order_no_warnings() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn empty_task_is_error() {
    let wo = WorkOrderBuilder::new("").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task" && w.severity == WarningSeverity::Error));
}

#[test]
fn whitespace_only_task_is_error() {
    let wo = WorkOrderBuilder::new("   ").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task"));
}

// ---------- max_turns checks ----------

#[test]
fn zero_max_turns_is_error() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "config.max_turns" && w.severity == WarningSeverity::Error));
}

#[test]
fn positive_max_turns_ok() {
    let wo = WorkOrderBuilder::new("task").max_turns(5).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().all(|w| w.field != "config.max_turns"));
}

#[test]
fn none_max_turns_ok() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_turns.is_none());
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().all(|w| w.field != "config.max_turns"));
}

// ---------- budget checks ----------

#[test]
fn zero_budget_is_error() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.0).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "config.max_budget_usd" && w.severity == WarningSeverity::Error));
}

#[test]
fn negative_budget_is_error() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-1.0).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn positive_budget_ok() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.5).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .all(|w| w.field != "config.max_budget_usd"));
}

// ---------- duplicate tools ----------

#[test]
fn duplicate_allowed_tools_warning() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into(), "read".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "policy.allowed_tools" && w.severity == WarningSeverity::Warning));
}

#[test]
fn unique_allowed_tools_ok() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .all(|w| w.field != "policy.allowed_tools"));
}

// ---------- model checks ----------

#[test]
fn empty_model_is_error() {
    let wo = WorkOrderBuilder::new("task").model("").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "config.model" && w.severity == WarningSeverity::Error));
}

#[test]
fn whitespace_model_is_error() {
    let wo = WorkOrderBuilder::new("task").model("  ").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.model"));
}

#[test]
fn valid_model_ok() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().all(|w| w.field != "config.model"));
}

#[test]
fn no_model_set_ok() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.model.is_none());
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().all(|w| w.field != "config.model"));
}

// ---------- policy empty globs ----------

#[test]
fn empty_deny_read_glob_is_error() {
    let policy = PolicyProfile {
        deny_read: vec!["*.log".into(), "".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "policy.deny_read" && w.severity == WarningSeverity::Error));
}

#[test]
fn empty_deny_write_glob_is_error() {
    let policy = PolicyProfile {
        deny_write: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_write"));
}

#[test]
fn empty_disallowed_tools_glob_is_error() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into(), " ".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "policy.disallowed_tools"));
}

#[test]
fn valid_policy_globs_ok() {
    let policy = PolicyProfile {
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/**".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().all(|w| w.field != "policy.deny_read"));
    assert!(warnings.iter().all(|w| w.field != "policy.deny_write"));
}

// ---------- vendor config keys ----------

#[test]
fn empty_vendor_key_is_error() {
    let mut vendor = BTreeMap::new();
    vendor.insert("".into(), serde_json::json!("val"));
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings
        .iter()
        .any(|w| w.field == "config.vendor" && w.severity == WarningSeverity::Error));
}

#[test]
fn valid_vendor_keys_ok() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), serde_json::json!({"key": "val"}));
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.iter().all(|w| w.field != "config.vendor"));
}

// ---------- ConfigDefaults ----------

#[test]
fn default_max_turns_is_25() {
    assert_eq!(ConfigDefaults::default_max_turns(), 25);
}

#[test]
fn default_max_budget_is_1() {
    assert!((ConfigDefaults::default_max_budget() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn default_model_is_gpt4() {
    assert_eq!(ConfigDefaults::default_model(), "gpt-4");
}

#[test]
fn apply_defaults_fills_missing() {
    let mut wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.model.is_none());

    ConfigDefaults::apply_defaults(&mut wo);

    assert_eq!(wo.config.max_turns, Some(25));
    assert!((wo.config.max_budget_usd.unwrap() - 1.0).abs() < f64::EPSILON);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn apply_defaults_preserves_existing() {
    let mut wo = WorkOrderBuilder::new("task")
        .model("claude-3")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();

    ConfigDefaults::apply_defaults(&mut wo);

    assert_eq!(wo.config.max_turns, Some(10));
    assert!((wo.config.max_budget_usd.unwrap() - 5.0).abs() < f64::EPSILON);
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
}

// ---------- combined / edge cases ----------

#[test]
fn multiple_errors_reported() {
    let policy = PolicyProfile {
        allowed_tools: vec!["a".into(), "a".into()],
        deny_read: vec!["".into()],
        ..Default::default()
    };
    let mut vendor = BTreeMap::new();
    vendor.insert(" ".into(), serde_json::json!(1));
    let config = RuntimeConfig {
        model: Some("".into()),
        max_turns: Some(0),
        max_budget_usd: Some(-1.0),
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("").policy(policy).config(config).build();
    let warnings = validator().validate_work_order(&wo);
    // task + max_turns + budget + duplicate tool + model + deny_read + vendor key = 7
    assert!(warnings.len() >= 7);
}

#[test]
fn apply_defaults_then_validate_passes() {
    let mut wo = WorkOrderBuilder::new("Fix the bug").build();
    ConfigDefaults::apply_defaults(&mut wo);
    let warnings = validator().validate_work_order(&wo);
    assert!(warnings.is_empty());
}
