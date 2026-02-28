// SPDX-License-Identifier: MIT OR Apache-2.0
//! Edge-case tests for ProjectionMatrix and dialect translation.

use abp_core::{ContextPacket, ContextSnippet, WorkOrderBuilder};
use abp_integrations::projection::{Dialect, ProjectionMatrix, supported_translations, translate};

fn sample_wo() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

// ---------------------------------------------------------------------------
// 1. Default-constructed ProjectionMatrix behaves identically to ::new()
// ---------------------------------------------------------------------------

#[test]
fn default_and_new_equivalent() {
    let def = ProjectionMatrix;
    let new = ProjectionMatrix::new();
    assert_eq!(def.supported_translations(), new.supported_translations());
}

// ---------------------------------------------------------------------------
// 2. Empty matrix: supported_translations returns correct count
// ---------------------------------------------------------------------------

#[test]
fn supported_translations_count() {
    let pairs = supported_translations();
    // Identity: 5 dialects => 5 pairs + ABP-to-vendor for 4 non-ABP = 9 total
    let identity_count = Dialect::ALL.len();
    let abp_to_vendor_count = Dialect::ALL.iter().filter(|&&d| d != Dialect::Abp).count();
    assert_eq!(pairs.len(), identity_count + abp_to_vendor_count);
}

// ---------------------------------------------------------------------------
// 3. Identity mapping deserializes back to equivalent WorkOrder
// ---------------------------------------------------------------------------

#[test]
fn identity_roundtrip_deserializes_back() {
    let wo = sample_wo();
    let val = translate(Dialect::Abp, Dialect::Abp, &wo).unwrap();
    let deserialized: abp_core::WorkOrder = serde_json::from_value(val).unwrap();
    assert_eq!(deserialized.id, wo.id);
    assert_eq!(deserialized.task, wo.task);
}

// ---------------------------------------------------------------------------
// 4. All non-identity, non-ABP-source pairs error
// ---------------------------------------------------------------------------

#[test]
fn all_vendor_to_vendor_pairs_unsupported() {
    let wo = sample_wo();
    let vendors = [Dialect::Claude, Dialect::Codex, Dialect::Gemini, Dialect::Kimi];

    for &from in &vendors {
        for &to in &vendors {
            if from == to {
                continue; // identity is supported
            }
            let result = translate(from, to, &wo);
            assert!(
                result.is_err(),
                "{from:?} -> {to:?} should be unsupported"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Vendor-to-ABP pairs unsupported (reverse mapping)
// ---------------------------------------------------------------------------

#[test]
fn vendor_to_abp_unsupported() {
    let wo = sample_wo();
    for &d in &[Dialect::Claude, Dialect::Codex, Dialect::Gemini, Dialect::Kimi] {
        let result = translate(d, Dialect::Abp, &wo);
        assert!(
            result.is_err(),
            "{d:?} -> Abp should be unsupported"
        );
    }
}

// ---------------------------------------------------------------------------
// 6. Custom model propagates through ABP-to-vendor translations
// ---------------------------------------------------------------------------

#[test]
fn custom_model_propagates() {
    let wo = WorkOrderBuilder::new("task").model("my-custom-model").build();

    for &dialect in &[Dialect::Claude, Dialect::Codex, Dialect::Gemini, Dialect::Kimi] {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let model = val.get("model").and_then(|m| m.as_str()).unwrap();
        assert_eq!(
            model, "my-custom-model",
            "custom model not propagated for {dialect:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Context snippets embedded in all vendor translations
// ---------------------------------------------------------------------------

#[test]
fn snippets_in_all_translations() {
    let wo = WorkOrderBuilder::new("fix it")
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "stack_trace".into(),
                content: "thread 'main' panicked at src/main.rs:10".into(),
            }],
        })
        .build();

    for &dialect in &[Dialect::Claude, Dialect::Codex, Dialect::Gemini, Dialect::Kimi] {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let json_str = serde_json::to_string(&val).unwrap();
        assert!(
            json_str.contains("stack_trace"),
            "snippet name missing for {dialect:?}"
        );
        assert!(
            json_str.contains("panicked at src/main.rs:10"),
            "snippet content missing for {dialect:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Default model fallbacks are non-empty for all vendors
// ---------------------------------------------------------------------------

#[test]
fn default_model_fallbacks_nonempty() {
    let wo = sample_wo(); // no model override

    for &dialect in &[Dialect::Claude, Dialect::Codex, Dialect::Gemini, Dialect::Kimi] {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let model = val.get("model").and_then(|m| m.as_str()).unwrap();
        assert!(
            !model.is_empty(),
            "default model empty for {dialect:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Dialect::ALL has expected length and contents
// ---------------------------------------------------------------------------

#[test]
fn dialect_all_complete() {
    assert_eq!(Dialect::ALL.len(), 5);
    assert!(Dialect::ALL.contains(&Dialect::Abp));
    assert!(Dialect::ALL.contains(&Dialect::Claude));
    assert!(Dialect::ALL.contains(&Dialect::Codex));
    assert!(Dialect::ALL.contains(&Dialect::Gemini));
    assert!(Dialect::ALL.contains(&Dialect::Kimi));
}

// ---------------------------------------------------------------------------
// 10. Supported translations do not contain unsupported pairs
// ---------------------------------------------------------------------------

#[test]
fn supported_translations_no_cross_vendor() {
    let pairs = supported_translations();
    let vendors = [Dialect::Claude, Dialect::Codex, Dialect::Gemini, Dialect::Kimi];

    for &from in &vendors {
        for &to in &vendors {
            if from != to {
                assert!(
                    !pairs.contains(&(from, to)),
                    "cross-vendor pair ({from:?}, {to:?}) should not be supported"
                );
            }
        }
    }
    // Also no vendor-to-ABP
    for &v in &vendors {
        assert!(
            !pairs.contains(&(v, Dialect::Abp)),
            "vendor-to-ABP pair ({v:?}, Abp) should not be supported"
        );
    }
}

// ---------------------------------------------------------------------------
// 11. Translation is pure: same input, same output (deterministic)
// ---------------------------------------------------------------------------

#[test]
fn translation_deterministic_all_pairs() {
    let wo = sample_wo();
    let matrix = ProjectionMatrix::new();

    for (from, to) in matrix.supported_translations() {
        let a = translate(from, to, &wo).unwrap();
        let b = translate(from, to, &wo).unwrap();
        assert_eq!(a, b, "non-deterministic for ({from:?}, {to:?})");
    }
}

// ---------------------------------------------------------------------------
// 12. Dialect serialization round-trips through serde
// ---------------------------------------------------------------------------

#[test]
fn dialect_serde_roundtrip() {
    for &d in Dialect::ALL {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back, "serde roundtrip failed for {d:?}");
    }
}

// ---------------------------------------------------------------------------
// 13. Dialect serde uses snake_case
// ---------------------------------------------------------------------------

#[test]
fn dialect_serde_snake_case() {
    assert_eq!(serde_json::to_string(&Dialect::Abp).unwrap(), "\"abp\"");
    assert_eq!(serde_json::to_string(&Dialect::Claude).unwrap(), "\"claude\"");
    assert_eq!(serde_json::to_string(&Dialect::Codex).unwrap(), "\"codex\"");
    assert_eq!(serde_json::to_string(&Dialect::Gemini).unwrap(), "\"gemini\"");
    assert_eq!(serde_json::to_string(&Dialect::Kimi).unwrap(), "\"kimi\"");
}
