// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Parsing and normalization helpers for CLI `KEY=VALUE` flags.

use anyhow::{Context, Result};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;

/// Parse a `KEY=VALUE` style CLI flag payload.
///
/// Returns an error if `=` is missing or if the key portion is empty.
pub fn parse_key_value_flag(raw: &str, flag_name: &str) -> Result<(String, String)> {
    let (raw_key, raw_value) = raw
        .split_once('=')
        .with_context(|| format!("{flag_name} expects KEY=VALUE, got '{raw}'"))?;

    let key = raw_key.trim();
    if key.is_empty() {
        anyhow::bail!("{flag_name} key cannot be empty (got '{raw}')");
    }

    Ok((key.to_string(), raw_value.to_string()))
}

/// Parse a parameter value from JSON-like text.
///
/// If `raw` parses as JSON, that JSON value is returned. Otherwise the
/// original input is treated as a string.
pub fn parse_param_value(raw: &str) -> JsonValue {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return JsonValue::String(String::new());
    }

    serde_json::from_str::<JsonValue>(trimmed)
        .unwrap_or_else(|_| JsonValue::String(raw.to_string()))
}

/// Insert a dot-delimited path into a vendor map.
///
/// For example `gemini.model` inserts `{ "gemini": { "model": ... } }`.
pub fn insert_vendor_path(vendor: &mut BTreeMap<String, JsonValue>, key: &str, value: JsonValue) {
    let parts: Vec<&str> = key.split('.').filter(|p| !p.trim().is_empty()).collect();
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        vendor.insert(parts[0].to_string(), value);
        return;
    }

    let root = parts[0].to_string();
    let root_value = vendor
        .entry(root)
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if !root_value.is_object() {
        *root_value = JsonValue::Object(JsonMap::new());
    }

    let mut current = root_value;
    for part in &parts[1..parts.len() - 1] {
        let obj = current
            .as_object_mut()
            .expect("insert_vendor_path: current node must be an object");
        let entry = obj
            .entry((*part).to_string())
            .or_insert_with(|| JsonValue::Object(JsonMap::new()));
        if !entry.is_object() {
            *entry = JsonValue::Object(JsonMap::new());
        }
        current = entry;
    }

    if let Some(last) = parts.last() {
        let obj = current
            .as_object_mut()
            .expect("insert_vendor_path: final parent must be an object");
        obj.insert((*last).to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_param_value_parses_jsonish_values() {
        assert_eq!(parse_param_value("true"), json!(true));
        assert_eq!(parse_param_value("1.5"), json!(1.5));
        assert_eq!(parse_param_value("{\"a\":1}"), json!({"a": 1}));
        assert_eq!(
            parse_param_value("gemini-2.0-flash"),
            json!("gemini-2.0-flash")
        );
    }

    #[test]
    fn insert_vendor_path_writes_nested_values() {
        let mut vendor = BTreeMap::new();
        insert_vendor_path(&mut vendor, "gemini.model", json!("gemini-2.5-flash"));
        insert_vendor_path(&mut vendor, "gemini.vertex", json!(true));

        assert_eq!(
            vendor.get("gemini"),
            Some(&json!({
                "model": "gemini-2.5-flash",
                "vertex": true
            }))
        );
    }

    #[test]
    fn parse_key_value_requires_equals() {
        let err = parse_key_value_flag("foo", "--param").unwrap_err();
        assert!(
            err.to_string().contains("KEY=VALUE"),
            "unexpected error: {err}"
        );
    }
}
