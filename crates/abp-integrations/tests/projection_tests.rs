// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::WorkOrderBuilder;
use abp_integrations::projection::{Dialect, ProjectionMatrix, supported_translations, translate};

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Refactor the auth module").build()
}

#[test]
fn identity_translation_preserves_data() {
    let wo = sample_work_order();
    let matrix = ProjectionMatrix::new();

    for &dialect in Dialect::ALL {
        let result = matrix.translate(dialect, dialect, &wo).unwrap();
        // Identity always serialises the original work order.
        let expected = serde_json::to_value(&wo).unwrap();
        assert_eq!(result, expected, "identity failed for {dialect:?}");
    }
}

#[test]
fn abp_to_claude_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Claude request must have model");
    assert!(obj.contains_key("messages"), "Claude request must have messages");
}

#[test]
fn abp_to_codex_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Codex request must have model");
    assert!(obj.contains_key("input"), "Codex request must have input");
}

#[test]
fn abp_to_gemini_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Gemini request must have model");
    assert!(obj.contains_key("contents"), "Gemini request must have contents");
}

#[test]
fn abp_to_kimi_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Kimi request must have model");
    assert!(obj.contains_key("messages"), "Kimi request must have messages");
}

#[test]
fn unsupported_translation_returns_error() {
    let wo = sample_work_order();
    // Vendor-to-vendor (non-identity, non-ABP source) is unsupported in v0.1.
    let result = translate(Dialect::Claude, Dialect::Codex, &wo);
    assert!(result.is_err(), "Claude->Codex should be unsupported");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("unsupported"), "error should mention 'unsupported': {msg}");
}

#[test]
fn supported_translations_includes_all_identity_pairs() {
    let pairs = supported_translations();
    for &dialect in Dialect::ALL {
        assert!(
            pairs.contains(&(dialect, dialect)),
            "missing identity pair for {dialect:?}"
        );
    }
}

#[test]
fn supported_translations_includes_abp_to_vendor_pairs() {
    let pairs = supported_translations();
    for &dialect in Dialect::ALL {
        if dialect != Dialect::Abp {
            assert!(
                pairs.contains(&(Dialect::Abp, dialect)),
                "missing ABP->{dialect:?} pair"
            );
        }
    }
}

#[test]
fn projection_matrix_struct_matches_free_functions() {
    let wo = sample_work_order();
    let matrix = ProjectionMatrix::new();

    // Method should give the same results as the free function.
    let method_result = matrix.translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    let free_result = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    assert_eq!(method_result, free_result);

    assert_eq!(matrix.supported_translations(), supported_translations());
}
