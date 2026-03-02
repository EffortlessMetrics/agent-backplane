// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz ErrorCode deserialization, display, and AbpError construction.
//!
//! Verifies:
//! 1. Deserializing arbitrary strings as ErrorCode never panics.
//! 2. All ErrorCode variants have consistent Display/as_str/category.
//! 3. AbpError construction with arbitrary context never panics.
//! 4. AbpErrorDto round-trips through JSON.
#![no_main]
use abp_error::{AbpError, AbpErrorDto, ErrorCode};
use libfuzzer_sys::fuzz_target;

/// All known ErrorCode variants for exercising Display/category.
const ALL_CODES: &[ErrorCode] = &[
    ErrorCode::ProtocolInvalidEnvelope,
    ErrorCode::ProtocolUnexpectedMessage,
    ErrorCode::ProtocolVersionMismatch,
    ErrorCode::BackendNotFound,
    ErrorCode::BackendTimeout,
    ErrorCode::BackendCrashed,
    ErrorCode::CapabilityUnsupported,
    ErrorCode::CapabilityEmulationFailed,
    ErrorCode::PolicyDenied,
    ErrorCode::PolicyInvalid,
    ErrorCode::WorkspaceInitFailed,
    ErrorCode::WorkspaceStagingFailed,
    ErrorCode::IrLoweringFailed,
    ErrorCode::IrInvalid,
    ErrorCode::ReceiptHashMismatch,
    ErrorCode::ReceiptChainBroken,
    ErrorCode::DialectUnknown,
    ErrorCode::DialectMappingFailed,
    ErrorCode::ConfigInvalid,
    ErrorCode::Internal,
];

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // --- Property 1: JSON deserialization never panics ---
    if let Ok(code) = serde_json::from_str::<ErrorCode>(s) {
        // Display and as_str must not panic.
        let display = format!("{code}");
        let as_str = code.as_str();
        assert_eq!(display, as_str, "Display and as_str must agree");

        // Category must not panic.
        let cat = code.category();
        let _ = format!("{cat}");

        // Serde round-trip.
        let json = serde_json::to_string(&code).expect("ErrorCode must serialize");
        let rt: ErrorCode =
            serde_json::from_str(&json).expect("ErrorCode round-trip must succeed");
        assert_eq!(code, rt);
    }

    // --- Property 2: exercise all variants ---
    for &code in ALL_CODES {
        let _ = format!("{code}");
        let _ = code.as_str();
        let _ = code.category();
        let _ = format!("{:?}", code.category());
    }

    // --- Property 3: AbpError with arbitrary message/context never panics ---
    let code_idx = data.first().copied().unwrap_or(0) as usize % ALL_CODES.len();
    let code = ALL_CODES[code_idx];
    let err = AbpError::new(code, s).with_context("fuzz_key", s);
    let display = format!("{err}");
    assert!(!display.is_empty());
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());

    // --- Property 4: AbpErrorDto JSON round-trip ---
    let dto = AbpErrorDto::from(&err);
    if let Ok(json) = serde_json::to_string(&dto) {
        let rt: AbpErrorDto =
            serde_json::from_str(&json).expect("AbpErrorDto round-trip must succeed");
        assert_eq!(dto.code, rt.code);
        assert_eq!(dto.message, rt.message);
    }

    // --- Property 5: deserialize as ErrorCategory ---
    let _ = serde_json::from_str::<abp_error::ErrorCategory>(s);
});
