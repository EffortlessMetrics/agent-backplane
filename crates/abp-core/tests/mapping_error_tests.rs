// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the MappingError taxonomy in abp_core::error.

use abp_core::error::{MappingError, MappingErrorKind, MappingResult};

// ---------------------------------------------------------------------------
// Helper constructors
// ---------------------------------------------------------------------------

fn fidelity_loss() -> MappingError {
    MappingError::FidelityLoss {
        field: "system_prompt".into(),
        source_dialect: "anthropic".into(),
        target_dialect: "openai".into(),
        detail: "system prompt caching not supported".into(),
    }
}

fn unsupported_cap() -> MappingError {
    MappingError::UnsupportedCapability {
        capability: "tool_use".into(),
        dialect: "gemini".into(),
    }
}

fn emulation_required() -> MappingError {
    MappingError::EmulationRequired {
        feature: "streaming".into(),
        detail: "polling loop emulates token stream".into(),
    }
}

fn incompatible_model() -> MappingError {
    MappingError::IncompatibleModel {
        requested: "claude-3-opus".into(),
        dialect: "openai".into(),
        suggestion: Some("gpt-4o".into()),
    }
}

fn incompatible_model_no_suggestion() -> MappingError {
    MappingError::IncompatibleModel {
        requested: "claude-3-opus".into(),
        dialect: "openai".into(),
        suggestion: None,
    }
}

fn param_not_mappable() -> MappingError {
    MappingError::ParameterNotMappable {
        parameter: "top_k".into(),
        value: "40".into(),
        dialect: "openai".into(),
    }
}

fn streaming_unsupported() -> MappingError {
    MappingError::StreamingUnsupported {
        dialect: "batch_only_backend".into(),
    }
}

// ---------------------------------------------------------------------------
// Display formatting
// ---------------------------------------------------------------------------

#[test]
fn display_fidelity_loss() {
    let msg = fidelity_loss().to_string();
    assert!(msg.contains("ABP_E_FIDELITY_LOSS"));
    assert!(msg.contains("system_prompt"));
    assert!(msg.contains("anthropic"));
    assert!(msg.contains("openai"));
}

#[test]
fn display_unsupported_capability() {
    let msg = unsupported_cap().to_string();
    assert!(msg.contains("ABP_E_UNSUPPORTED_CAP"));
    assert!(msg.contains("tool_use"));
    assert!(msg.contains("gemini"));
}

#[test]
fn display_emulation_required() {
    let msg = emulation_required().to_string();
    assert!(msg.contains("ABP_E_EMULATION_REQUIRED"));
    assert!(msg.contains("streaming"));
    assert!(msg.contains("polling loop"));
}

#[test]
fn display_incompatible_model_with_suggestion() {
    let msg = incompatible_model().to_string();
    assert!(msg.contains("ABP_E_INCOMPATIBLE_MODEL"));
    assert!(msg.contains("claude-3-opus"));
    assert!(msg.contains("try gpt-4o"));
}

#[test]
fn display_incompatible_model_no_suggestion() {
    let msg = incompatible_model_no_suggestion().to_string();
    assert!(msg.contains("ABP_E_INCOMPATIBLE_MODEL"));
    assert!(msg.contains("claude-3-opus"));
    assert!(!msg.contains("try"));
}

#[test]
fn display_param_not_mappable() {
    let msg = param_not_mappable().to_string();
    assert!(msg.contains("ABP_E_PARAM_NOT_MAPPABLE"));
    assert!(msg.contains("top_k"));
    assert!(msg.contains("40"));
}

#[test]
fn display_streaming_unsupported() {
    let msg = streaming_unsupported().to_string();
    assert!(msg.contains("ABP_E_STREAMING_UNSUPPORTED"));
    assert!(msg.contains("batch_only_backend"));
}

// ---------------------------------------------------------------------------
// Stable error codes
// ---------------------------------------------------------------------------

#[test]
fn error_code_stability() {
    assert_eq!(fidelity_loss().code(), "ABP_E_FIDELITY_LOSS");
    assert_eq!(unsupported_cap().code(), "ABP_E_UNSUPPORTED_CAP");
    assert_eq!(emulation_required().code(), "ABP_E_EMULATION_REQUIRED");
    assert_eq!(incompatible_model().code(), "ABP_E_INCOMPATIBLE_MODEL");
    assert_eq!(param_not_mappable().code(), "ABP_E_PARAM_NOT_MAPPABLE");
    assert_eq!(
        streaming_unsupported().code(),
        "ABP_E_STREAMING_UNSUPPORTED"
    );
}

#[test]
fn error_code_constants_match() {
    assert_eq!(MappingError::FIDELITY_LOSS_CODE, fidelity_loss().code());
    assert_eq!(MappingError::UNSUPPORTED_CAP_CODE, unsupported_cap().code());
    assert_eq!(
        MappingError::EMULATION_REQUIRED_CODE,
        emulation_required().code()
    );
    assert_eq!(
        MappingError::INCOMPATIBLE_MODEL_CODE,
        incompatible_model().code()
    );
    assert_eq!(
        MappingError::PARAM_NOT_MAPPABLE_CODE,
        param_not_mappable().code()
    );
    assert_eq!(
        MappingError::STREAMING_UNSUPPORTED_CODE,
        streaming_unsupported().code()
    );
}

// ---------------------------------------------------------------------------
// Error categorization (kind)
// ---------------------------------------------------------------------------

#[test]
fn fidelity_loss_is_degraded() {
    assert_eq!(fidelity_loss().kind(), MappingErrorKind::Degraded);
    assert!(fidelity_loss().is_degraded());
    assert!(!fidelity_loss().is_fatal());
    assert!(!fidelity_loss().is_emulated());
}

#[test]
fn unsupported_capability_is_fatal() {
    assert_eq!(unsupported_cap().kind(), MappingErrorKind::Fatal);
    assert!(unsupported_cap().is_fatal());
    assert!(!unsupported_cap().is_degraded());
}

#[test]
fn emulation_required_is_emulated() {
    assert_eq!(emulation_required().kind(), MappingErrorKind::Emulated);
    assert!(emulation_required().is_emulated());
    assert!(!emulation_required().is_fatal());
}

#[test]
fn incompatible_model_is_fatal() {
    assert_eq!(incompatible_model().kind(), MappingErrorKind::Fatal);
    assert!(incompatible_model().is_fatal());
}

#[test]
fn param_not_mappable_is_degraded() {
    assert_eq!(param_not_mappable().kind(), MappingErrorKind::Degraded);
    assert!(param_not_mappable().is_degraded());
}

#[test]
fn streaming_unsupported_is_fatal() {
    assert_eq!(streaming_unsupported().kind(), MappingErrorKind::Fatal);
    assert!(streaming_unsupported().is_fatal());
}

// ---------------------------------------------------------------------------
// Serde roundtrip
// ---------------------------------------------------------------------------

#[test]
fn serde_roundtrip_fidelity_loss() {
    let err = fidelity_loss();
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn serde_roundtrip_all_variants() {
    let errors: Vec<MappingError> = vec![
        fidelity_loss(),
        unsupported_cap(),
        emulation_required(),
        incompatible_model(),
        incompatible_model_no_suggestion(),
        param_not_mappable(),
        streaming_unsupported(),
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back, "roundtrip failed for {:?}", err);
    }
}

#[test]
fn serde_json_contains_type_tag() {
    let json = serde_json::to_string(&fidelity_loss()).unwrap();
    assert!(json.contains("\"type\":\"fidelity_loss\""));

    let json = serde_json::to_string(&streaming_unsupported()).unwrap();
    assert!(json.contains("\"type\":\"streaming_unsupported\""));
}

// ---------------------------------------------------------------------------
// MappingErrorKind serde roundtrip
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_kind_serde_roundtrip() {
    for kind in [
        MappingErrorKind::Fatal,
        MappingErrorKind::Degraded,
        MappingErrorKind::Emulated,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let back: MappingErrorKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }
}

#[test]
fn mapping_error_kind_display() {
    assert_eq!(MappingErrorKind::Fatal.to_string(), "fatal");
    assert_eq!(MappingErrorKind::Degraded.to_string(), "degraded");
    assert_eq!(MappingErrorKind::Emulated.to_string(), "emulated");
}

// ---------------------------------------------------------------------------
// MappingResult alias
// ---------------------------------------------------------------------------

#[test]
fn mapping_result_ok() {
    fn get_result() -> MappingResult<i32> {
        Ok(42)
    }
    let r = get_result();
    assert!(r.is_ok());
    assert_eq!(r.unwrap(), 42);
}

#[test]
fn mapping_result_err() {
    fn get_result() -> MappingResult<i32> {
        Err(streaming_unsupported())
    }
    let r = get_result();
    assert!(r.is_err());
    assert_eq!(r.unwrap_err().code(), "ABP_E_STREAMING_UNSUPPORTED");
}

// ---------------------------------------------------------------------------
// std::error::Error trait
// ---------------------------------------------------------------------------

#[test]
fn mapping_error_is_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(fidelity_loss());
    assert!(err.to_string().contains("ABP_E_FIDELITY_LOSS"));
}
