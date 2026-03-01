// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validates that generated JSON schema files are valid JSON.

use std::path::Path;

const SCHEMA_DIR: &str = "contracts/schemas";

const EXPECTED_SCHEMAS: &[&str] = &["work_order.schema.json", "receipt.schema.json"];

#[test]
fn generated_schemas_are_valid_json() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should be in repo root")
        .join(SCHEMA_DIR);

    assert!(dir.exists(), "schema directory missing: {}", dir.display());

    for name in EXPECTED_SCHEMAS {
        let path = dir.join(name);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        let value: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

        // Basic JSON Schema structure checks
        let obj = value.as_object().expect("schema should be a JSON object");
        assert!(
            obj.contains_key("$schema") || obj.contains_key("type") || obj.contains_key("$ref"),
            "{name} missing top-level schema key"
        );
    }
}
