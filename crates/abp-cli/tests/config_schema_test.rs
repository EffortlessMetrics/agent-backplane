// SPDX-License-Identifier: MIT OR Apache-2.0

use schemars::schema_for;
use serde_json::json;

fn config_schema() -> serde_json::Value {
    let schema = schema_for!(abp_cli::config::BackplaneConfig);
    serde_json::to_value(schema).expect("schema to value")
}

#[test]
fn config_schema_is_generated() {
    let schema = config_schema();
    assert_eq!(
        schema.get("$schema").and_then(|v| v.as_str()),
        Some("https://json-schema.org/draft/2020-12/schema"),
    );
    assert!(schema.get("title").is_some() || schema.get("properties").is_some());
}

#[test]
fn example_config_validates_against_schema() {
    let schema = config_schema();
    let instance = json!({
        "backends": {
            "mock": { "type": "mock" },
            "openai": {
                "type": "sidecar",
                "command": "node",
                "args": ["sidecar.js"]
            }
        }
    });
    let validator = jsonschema::validator_for(&schema).expect("compile schema");
    assert!(validator.is_valid(&instance));
}

#[test]
fn invalid_config_fails_schema_validation() {
    let schema = config_schema();
    // backends should be an object, not a string
    let instance = json!({ "backends": "not-a-map" });
    let validator = jsonschema::validator_for(&schema).expect("compile schema");
    assert!(!validator.is_valid(&instance));
}

#[test]
fn schema_has_expected_properties() {
    let schema = config_schema();
    let props = schema
        .get("properties")
        .expect("schema should have properties");
    assert!(
        props.get("backends").is_some(),
        "schema should include 'backends' property"
    );
}
