#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the JSON schema generation system (xtask schema).
//!
//! Categories:
//! 1. Schema structure validation (15 tests)
//! 2. Type coverage (15 tests)
//! 3. Enum representation (10 tests)
//! 4. Cross-reference integrity (10 tests)
//! 5. Schema determinism (10 tests)

use abp_cli::config::{BackendConfig, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, ExecutionLane, ExecutionMode, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_error::ErrorCode;
use chrono::Utc;
use schemars::schema_for;
use serde_json::Value;

// ── helpers ──────────────────────────────────────────────────────────────

fn schema_of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap()
}

fn assert_compiles(schema: &Value) {
    jsonschema::validator_for(schema).expect("schema must compile");
}

fn assert_valid(schema: &Value, instance: &Value) {
    let v = jsonschema::validator_for(schema).expect("schema compiles");
    if let Err(e) = v.validate(instance) {
        let msgs: Vec<String> = std::iter::once(format!("  - {e}"))
            .chain(v.iter_errors(instance).skip(1).map(|e| format!("  - {e}")))
            .collect();
        panic!("validation failed:\n{}", msgs.join("\n"));
    }
}

fn get_defs(schema: &Value) -> Vec<String> {
    schema["$defs"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn collect_refs(v: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs_inner(v, &mut refs);
    refs
}

fn collect_refs_inner(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                out.push(r.clone());
            }
            for val in map.values() {
                collect_refs_inner(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_refs_inner(val, out);
            }
        }
        _ => {}
    }
}

fn canonical_json(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

// ═════════════════════════════════════════════════════════════════════════
// 1. Schema structure validation (15 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn struct_work_order_schema_is_valid_json() {
    let s = schema_of::<WorkOrder>();
    assert!(s.is_object(), "WorkOrder schema must be a JSON object");
}

#[test]
fn struct_work_order_has_schema_field() {
    let s = schema_of::<WorkOrder>();
    assert_eq!(
        s["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "must declare Draft 2020-12"
    );
}

#[test]
fn struct_work_order_has_title() {
    let s = schema_of::<WorkOrder>();
    assert!(
        s.get("title").is_some(),
        "WorkOrder schema must have a title field"
    );
    assert_eq!(s["title"], "WorkOrder");
}

#[test]
fn struct_work_order_has_type_object() {
    let s = schema_of::<WorkOrder>();
    assert_eq!(s["type"], "object", "WorkOrder schema type must be object");
}

#[test]
fn struct_receipt_schema_is_valid_json() {
    let s = schema_of::<Receipt>();
    assert!(s.is_object());
}

#[test]
fn struct_receipt_has_schema_field() {
    let s = schema_of::<Receipt>();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn struct_receipt_has_title() {
    let s = schema_of::<Receipt>();
    assert_eq!(s["title"], "Receipt");
}

#[test]
fn struct_receipt_has_type_object() {
    let s = schema_of::<Receipt>();
    assert_eq!(s["type"], "object");
}

#[test]
fn struct_backplane_config_has_schema_field() {
    let s = schema_of::<BackplaneConfig>();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn struct_backplane_config_has_title() {
    let s = schema_of::<BackplaneConfig>();
    assert_eq!(s["title"], "BackplaneConfig");
}

#[test]
fn struct_agent_event_has_schema_draft() {
    let s = schema_of::<AgentEvent>();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn struct_agent_event_kind_has_schema_draft() {
    let s = schema_of::<AgentEventKind>();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn struct_policy_profile_has_type_object() {
    let s = schema_of::<PolicyProfile>();
    assert_eq!(s["type"], "object");
}

#[test]
fn struct_runtime_config_has_type_object() {
    let s = schema_of::<RuntimeConfig>();
    assert_eq!(s["type"], "object");
}

#[test]
fn struct_all_generated_schemas_compile_as_validators() {
    // Every schema the xtask generates must compile into a validator
    assert_compiles(&schema_of::<WorkOrder>());
    assert_compiles(&schema_of::<Receipt>());
    assert_compiles(&schema_of::<BackplaneConfig>());
}

// ═════════════════════════════════════════════════════════════════════════
// 2. Type coverage (15 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn type_work_order_schema_compiles() {
    assert_compiles(&schema_of::<WorkOrder>());
}

#[test]
fn type_receipt_schema_compiles() {
    assert_compiles(&schema_of::<Receipt>());
}

#[test]
fn type_agent_event_schema_compiles() {
    assert_compiles(&schema_of::<AgentEvent>());
}

#[test]
fn type_agent_event_kind_schema_compiles() {
    assert_compiles(&schema_of::<AgentEventKind>());
}

#[test]
fn type_capability_schema_compiles() {
    assert_compiles(&schema_of::<Capability>());
}

#[test]
fn type_policy_profile_schema_compiles() {
    assert_compiles(&schema_of::<PolicyProfile>());
}

#[test]
fn type_error_code_schema_compiles() {
    assert_compiles(&schema_of::<ErrorCode>());
}

#[test]
fn type_execution_mode_schema_compiles() {
    assert_compiles(&schema_of::<ExecutionMode>());
}

#[test]
fn type_execution_lane_schema_compiles() {
    assert_compiles(&schema_of::<ExecutionLane>());
}

#[test]
fn type_outcome_schema_compiles() {
    assert_compiles(&schema_of::<Outcome>());
}

#[test]
fn type_support_level_schema_compiles() {
    assert_compiles(&schema_of::<SupportLevel>());
}

#[test]
fn type_backend_identity_schema_compiles() {
    assert_compiles(&schema_of::<BackendIdentity>());
}

#[test]
fn type_workspace_spec_schema_compiles() {
    assert_compiles(&schema_of::<WorkspaceSpec>());
}

#[test]
fn type_backend_config_schema_compiles() {
    assert_compiles(&schema_of::<BackendConfig>());
}

#[test]
fn type_coverage_validates_real_instances() {
    let wo_schema = schema_of::<WorkOrder>();
    let wo = serde_json::to_value(WorkOrderBuilder::new("test").build()).unwrap();
    assert_valid(&wo_schema, &wo);

    let r_schema = schema_of::<Receipt>();
    let r = serde_json::to_value(
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build(),
    )
    .unwrap();
    assert_valid(&r_schema, &r);

    let e_schema = schema_of::<AgentEvent>();
    let e = serde_json::to_value(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    })
    .unwrap();
    assert_valid(&e_schema, &e);
}

// ═════════════════════════════════════════════════════════════════════════
// 3. Enum representation (10 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn enum_execution_mode_uses_snake_case() {
    let s = schema_of::<ExecutionMode>();
    let variants: Vec<String> = s["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["const"].as_str().map(String::from))
        .collect();
    assert!(variants.contains(&"passthrough".to_string()));
    assert!(variants.contains(&"mapped".to_string()));
}

#[test]
fn enum_outcome_uses_snake_case() {
    let s = schema_of::<Outcome>();
    let variants: Vec<String> = s["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["const"].as_str().map(String::from))
        .collect();
    assert!(variants.contains(&"complete".to_string()));
    assert!(variants.contains(&"partial".to_string()));
    assert!(variants.contains(&"failed".to_string()));
}

#[test]
fn enum_execution_lane_uses_snake_case() {
    let s = schema_of::<ExecutionLane>();
    let variants: Vec<String> = s["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["const"].as_str().map(String::from))
        .collect();
    assert!(variants.contains(&"patch_first".to_string()));
    assert!(variants.contains(&"workspace_first".to_string()));
}

#[test]
fn enum_workspace_mode_uses_snake_case() {
    let s = schema_of::<WorkspaceMode>();
    let variants: Vec<String> = s["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["const"].as_str().map(String::from))
        .collect();
    assert!(variants.contains(&"pass_through".to_string()));
    assert!(variants.contains(&"staged".to_string()));
}

#[test]
fn enum_agent_event_kind_uses_tag_type_discriminator() {
    // AgentEventKind uses #[serde(tag = "type", rename_all = "snake_case")]
    // Schema should encode this as oneOf with "type" discriminator
    let s = schema_of::<AgentEventKind>();
    let one_of = s["oneOf"]
        .as_array()
        .expect("AgentEventKind should use oneOf");
    // Each variant should be an object with a "type" property
    for variant in one_of {
        // Resolve $ref if present, or check inline
        if variant.get("$ref").is_none() {
            // Inline variant check
            if let Some(props) = variant.get("properties") {
                assert!(
                    props.get("type").is_some(),
                    "tagged enum variant must have 'type' property"
                );
            }
        }
    }
    assert!(!one_of.is_empty());
}

#[test]
fn enum_capability_has_many_variants() {
    let s = schema_of::<Capability>();
    let one_of = s["oneOf"].as_array().expect("Capability should use oneOf");
    // Capability has 40+ variants
    assert!(
        one_of.len() >= 30,
        "Capability should have at least 30 variants, got {}",
        one_of.len()
    );
}

#[test]
fn enum_capability_variant_names_are_snake_case() {
    let s = schema_of::<Capability>();
    let one_of = s["oneOf"].as_array().unwrap();
    for variant in one_of {
        if let Some(name) = variant["const"].as_str() {
            assert!(
                name.chars().all(|c| c.is_lowercase() || c == '_'),
                "Capability variant '{name}' should be snake_case"
            );
        }
    }
}

#[test]
fn enum_error_code_uses_snake_case_variants() {
    let s = schema_of::<ErrorCode>();
    let one_of = s["oneOf"].as_array().expect("ErrorCode should use oneOf");
    for variant in one_of {
        if let Some(name) = variant["const"].as_str() {
            assert!(
                name.chars().all(|c| c.is_lowercase() || c == '_'),
                "ErrorCode variant '{name}' should be snake_case"
            );
        }
    }
}

#[test]
fn enum_support_level_has_restricted_object_variant() {
    // SupportLevel has Restricted { reason: String } which is not a simple string enum
    let s = schema_of::<SupportLevel>();
    let one_of = s["oneOf"]
        .as_array()
        .expect("SupportLevel should use oneOf");
    // At least one variant should be an object (Restricted)
    let has_object = one_of.iter().any(|v| {
        v.get("type").and_then(|t| t.as_str()) == Some("object")
            || v.get("properties").is_some()
            || v.get("$ref").is_some()
    });
    let has_simple = one_of.iter().any(|v| v.get("const").is_some());
    assert!(
        has_object,
        "SupportLevel must have an object variant (Restricted)"
    );
    assert!(has_simple, "SupportLevel must have simple string variants");
}

#[test]
fn enum_backend_config_uses_tag_type_discriminator() {
    // BackendConfig uses #[serde(tag = "type")]
    let s = schema_of::<BackendConfig>();
    let one_of = s["oneOf"]
        .as_array()
        .expect("BackendConfig should use oneOf");
    assert!(
        one_of.len() >= 2,
        "BackendConfig should have at least Mock and Sidecar"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 4. Cross-reference integrity (10 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn refs_work_order_all_refs_resolve() {
    let s = schema_of::<WorkOrder>();
    let defs = get_defs(&s);
    let refs = collect_refs(&s);
    for r in &refs {
        if let Some(name) = r.strip_prefix("#/$defs/") {
            assert!(
                defs.contains(&name.to_string()),
                "$ref '{r}' not found in $defs. Available: {defs:?}"
            );
        }
    }
}

#[test]
fn refs_receipt_all_refs_resolve() {
    let s = schema_of::<Receipt>();
    let defs = get_defs(&s);
    let refs = collect_refs(&s);
    for r in &refs {
        if let Some(name) = r.strip_prefix("#/$defs/") {
            assert!(
                defs.contains(&name.to_string()),
                "$ref '{r}' not found in $defs. Available: {defs:?}"
            );
        }
    }
}

#[test]
fn refs_agent_event_all_refs_resolve() {
    let s = schema_of::<AgentEvent>();
    let defs = get_defs(&s);
    let refs = collect_refs(&s);
    for r in &refs {
        if let Some(name) = r.strip_prefix("#/$defs/") {
            assert!(
                defs.contains(&name.to_string()),
                "$ref '{r}' not found in $defs. Available: {defs:?}"
            );
        }
    }
}

#[test]
fn refs_backplane_config_all_refs_resolve() {
    let s = schema_of::<BackplaneConfig>();
    let defs = get_defs(&s);
    let refs = collect_refs(&s);
    for r in &refs {
        if let Some(name) = r.strip_prefix("#/$defs/") {
            assert!(
                defs.contains(&name.to_string()),
                "$ref '{r}' not found in $defs. Available: {defs:?}"
            );
        }
    }
}

#[test]
fn refs_work_order_defs_not_empty() {
    let s = schema_of::<WorkOrder>();
    let defs = get_defs(&s);
    // WorkOrder has nested types (ExecutionLane, WorkspaceSpec, etc.)
    assert!(
        !defs.is_empty(),
        "WorkOrder schema should have $defs for nested types"
    );
}

#[test]
fn refs_receipt_defs_not_empty() {
    let s = schema_of::<Receipt>();
    let defs = get_defs(&s);
    assert!(
        !defs.is_empty(),
        "Receipt schema should have $defs for nested types"
    );
}

#[test]
fn refs_no_dangling_refs_in_work_order() {
    let s = schema_of::<WorkOrder>();
    let schema_str = serde_json::to_string_pretty(&s).unwrap();
    let defs = get_defs(&s);
    let refs = collect_refs(&s);
    let dangling: Vec<_> = refs
        .iter()
        .filter(|r| r.starts_with("#/$defs/"))
        .filter(|r| {
            let name = r.strip_prefix("#/$defs/").unwrap();
            !defs.contains(&name.to_string())
        })
        .collect();
    assert!(
        dangling.is_empty(),
        "Dangling $refs found: {dangling:?}\nSchema: {schema_str}"
    );
}

#[test]
fn refs_no_dangling_refs_in_receipt() {
    let s = schema_of::<Receipt>();
    let defs = get_defs(&s);
    let refs = collect_refs(&s);
    let dangling: Vec<_> = refs
        .iter()
        .filter(|r| r.starts_with("#/$defs/"))
        .filter(|r| {
            let name = r.strip_prefix("#/$defs/").unwrap();
            !defs.contains(&name.to_string())
        })
        .collect();
    assert!(dangling.is_empty(), "Dangling $refs found: {dangling:?}");
}

#[test]
fn refs_work_order_properties_reference_known_types() {
    let s = schema_of::<WorkOrder>();
    let props = s["properties"]
        .as_object()
        .expect("WorkOrder should have properties");
    // These properties must exist in the schema
    for expected in [
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(
            props.contains_key(expected),
            "WorkOrder missing property '{expected}'"
        );
    }
}

#[test]
fn refs_receipt_properties_reference_known_types() {
    let s = schema_of::<Receipt>();
    let props = s["properties"]
        .as_object()
        .expect("Receipt should have properties");
    for expected in [
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
        "receipt_sha256",
    ] {
        assert!(
            props.contains_key(expected),
            "Receipt missing property '{expected}'"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 5. Schema determinism (10 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn determinism_work_order_same_output_twice() {
    let a = canonical_json(&schema_of::<WorkOrder>());
    let b = canonical_json(&schema_of::<WorkOrder>());
    assert_eq!(a, b, "WorkOrder schema generation must be deterministic");
}

#[test]
fn determinism_receipt_same_output_twice() {
    let a = canonical_json(&schema_of::<Receipt>());
    let b = canonical_json(&schema_of::<Receipt>());
    assert_eq!(a, b, "Receipt schema generation must be deterministic");
}

#[test]
fn determinism_agent_event_same_output_twice() {
    let a = canonical_json(&schema_of::<AgentEvent>());
    let b = canonical_json(&schema_of::<AgentEvent>());
    assert_eq!(a, b);
}

#[test]
fn determinism_capability_same_output_twice() {
    let a = canonical_json(&schema_of::<Capability>());
    let b = canonical_json(&schema_of::<Capability>());
    assert_eq!(a, b);
}

#[test]
fn determinism_error_code_same_output_twice() {
    let a = canonical_json(&schema_of::<ErrorCode>());
    let b = canonical_json(&schema_of::<ErrorCode>());
    assert_eq!(a, b);
}

#[test]
fn determinism_backplane_config_same_output_twice() {
    let a = canonical_json(&schema_of::<BackplaneConfig>());
    let b = canonical_json(&schema_of::<BackplaneConfig>());
    assert_eq!(a, b);
}

#[test]
fn determinism_btreemap_ordering_in_runtime_config() {
    // RuntimeConfig uses BTreeMap<String, Value> for vendor and env fields
    // Schema generation must produce the same output regardless of insertion order
    let a = schema_of::<RuntimeConfig>();
    let b = schema_of::<RuntimeConfig>();
    assert_eq!(
        serde_json::to_string_pretty(&a).unwrap(),
        serde_json::to_string_pretty(&b).unwrap()
    );
}

#[test]
fn determinism_btreemap_ordering_in_capability_manifest() {
    // CapabilityManifest = BTreeMap<Capability, SupportLevel>
    // The schema for Receipt.capabilities must be stable
    let a = schema_of::<Receipt>();
    let b = schema_of::<Receipt>();
    let a_caps = &a["properties"]["capabilities"];
    let b_caps = &b["properties"]["capabilities"];
    assert_eq!(
        canonical_json(a_caps),
        canonical_json(b_caps),
        "capability manifest schema must be deterministic"
    );
}

#[test]
fn determinism_pretty_and_compact_parse_to_same() {
    let s = schema_of::<WorkOrder>();
    let pretty = serde_json::to_string_pretty(&s).unwrap();
    let compact = serde_json::to_string(&s).unwrap();
    let reparsed_pretty: Value = serde_json::from_str(&pretty).unwrap();
    let reparsed_compact: Value = serde_json::from_str(&compact).unwrap();
    assert_eq!(reparsed_pretty, reparsed_compact);
}

#[test]
fn determinism_ten_iterations_produce_identical_schema() {
    let baseline = canonical_json(&schema_of::<WorkOrder>());
    for i in 0..10 {
        let current = canonical_json(&schema_of::<WorkOrder>());
        assert_eq!(
            baseline, current,
            "Iteration {i}: schema differs from baseline"
        );
    }
}
