//! Exhaustive tests for Display, Debug, and error chain implementations
//! across all error types in `abp-error`.

use std::error::Error;

use abp_error::{
    mapping_errors::MappingError,
    protocol_errors::ProtocolError,
    recovery::RecoveryStrategy,
    vendor_errors::{VendorApiError, VendorErrorDetail},
    AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo, ErrorLocation,
};

// =========================================================================
// Helpers
// =========================================================================

fn all_mapping_errors() -> Vec<MappingError> {
    vec![
        MappingError::FeatureUnsupported {
            feature: "vision".into(),
            source_dialect: "openai".into(),
            target_dialect: "gemini".into(),
        },
        MappingError::EmulationFailed {
            feature: "tool_use".into(),
            reason: "no adapter available".into(),
        },
        MappingError::FidelityLoss {
            field: "temperature".into(),
            original: "0.73".into(),
            approximation: "0.7".into(),
        },
        MappingError::AmbiguousMapping {
            field: "stop_sequence".into(),
            candidates: vec!["stop".into(), "end_turn".into()],
        },
        MappingError::NegotiationFailed {
            reason: "no compatible capability set".into(),
        },
    ]
}

fn all_protocol_errors() -> Vec<ProtocolError> {
    vec![
        ProtocolError::HandshakeFailed {
            reason: "no hello received".into(),
        },
        ProtocolError::VersionMismatch {
            expected: "abp/v0.1".into(),
            actual: "abp/v0.2".into(),
        },
        ProtocolError::EnvelopeMalformed {
            raw_line: "{bad json".into(),
            parse_error: "expected value at line 1".into(),
        },
        ProtocolError::StreamInterrupted {
            events_received: 42,
            reason: "EOF".into(),
        },
        ProtocolError::TimeoutExpired {
            operation: "hello".into(),
            timeout_ms: 5000,
        },
        ProtocolError::SidecarCrashed {
            exit_code: Some(1),
            stderr_tail: "segfault".into(),
        },
    ]
}

fn all_vendor_errors() -> Vec<VendorApiError> {
    vec![
        VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited")),
        VendorApiError::Claude(VendorErrorDetail::new(401, "invalid api key")),
        VendorApiError::Gemini(VendorErrorDetail::new(503, "overloaded").with_retry_after(30)),
        VendorApiError::Codex(
            VendorErrorDetail::new(500, "internal error").with_request_id("req-abc"),
        ),
        VendorApiError::Copilot(VendorErrorDetail::new(404, "model not found")),
        VendorApiError::Kimi(VendorErrorDetail::new(408, "gateway timeout")),
    ]
}

fn all_recovery_strategies() -> Vec<RecoveryStrategy> {
    vec![
        RecoveryStrategy::Retry {
            delay_ms: 1000,
            max_retries: 3,
        },
        RecoveryStrategy::Fallback {
            suggestion: "try another backend".into(),
        },
        RecoveryStrategy::Degrade {
            degradation: "lossy temperature mapping".into(),
        },
        RecoveryStrategy::Abort {
            reason: "unrecoverable version mismatch".into(),
        },
    ]
}

// =========================================================================
// 1. Error Display tests (15)
// =========================================================================

#[test]
fn display_mapping_error_all_variants_non_empty() {
    for err in all_mapping_errors() {
        let msg = err.to_string();
        assert!(!msg.is_empty(), "MappingError Display was empty: {:?}", err);
        assert!(msg.len() > 10, "MappingError Display too short: {msg}");
    }
}

#[test]
fn display_mapping_error_contains_code() {
    for err in all_mapping_errors() {
        let msg = err.to_string();
        let code = err.code();
        assert!(
            msg.contains(code),
            "MappingError Display should contain code {code}: got {msg}"
        );
    }
}

#[test]
fn display_mapping_error_is_informative() {
    let err = MappingError::FeatureUnsupported {
        feature: "vision".into(),
        source_dialect: "openai".into(),
        target_dialect: "gemini".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("vision"), "Display should mention the feature");
    assert!(
        msg.contains("openai"),
        "Display should mention source dialect"
    );
    assert!(
        msg.contains("gemini"),
        "Display should mention target dialect"
    );
}

#[test]
fn display_protocol_error_all_variants_non_empty() {
    for err in all_protocol_errors() {
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "ProtocolError Display was empty: {:?}",
            err
        );
        assert!(msg.len() > 10, "ProtocolError Display too short: {msg}");
    }
}

#[test]
fn display_protocol_error_contains_code() {
    for err in all_protocol_errors() {
        let msg = err.to_string();
        let code = err.code();
        assert!(
            msg.contains(code),
            "ProtocolError Display should contain code {code}: got {msg}"
        );
    }
}

#[test]
fn display_protocol_error_version_mismatch_shows_versions() {
    let err = ProtocolError::VersionMismatch {
        expected: "abp/v0.1".into(),
        actual: "abp/v0.2".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("abp/v0.1"), "should show expected version");
    assert!(msg.contains("abp/v0.2"), "should show actual version");
}

#[test]
fn display_vendor_error_all_variants_non_empty() {
    for err in all_vendor_errors() {
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "VendorApiError Display was empty: {:?}",
            err
        );
        assert!(msg.len() > 10, "VendorApiError Display too short: {msg}");
    }
}

#[test]
fn display_vendor_error_contains_code_and_vendor() {
    for err in all_vendor_errors() {
        let msg = err.to_string();
        let code = err.code();
        let vendor = err.vendor_name();
        assert!(
            msg.contains(code),
            "VendorApiError Display should contain code {code}: got {msg}"
        );
        assert!(
            msg.contains(vendor),
            "VendorApiError Display should contain vendor name {vendor}: got {msg}"
        );
    }
}

#[test]
fn display_vendor_error_shows_http_status() {
    let err = VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited"));
    let msg = err.to_string();
    assert!(msg.contains("429"), "should include HTTP status code");
}

#[test]
fn display_recovery_strategy_all_variants_non_empty() {
    for strat in all_recovery_strategies() {
        let msg = strat.to_string();
        assert!(
            !msg.is_empty(),
            "RecoveryStrategy Display was empty: {:?}",
            strat
        );
        assert!(msg.len() > 10, "RecoveryStrategy Display too short: {msg}");
    }
}

#[test]
fn display_recovery_strategy_contains_code() {
    for strat in all_recovery_strategies() {
        let msg = strat.to_string();
        let code = strat.code();
        assert!(
            msg.contains(code),
            "RecoveryStrategy Display should contain code {code}: got {msg}"
        );
    }
}

#[test]
fn display_abp_error_contains_error_code_str() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    let msg = err.to_string();
    assert!(
        msg.contains("backend_timeout"),
        "AbpError Display should contain error code as_str: got {msg}"
    );
}

#[test]
fn display_abp_error_shows_context() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let msg = err.to_string();
    assert!(
        msg.contains("openai"),
        "Display should include context values"
    );
}

#[test]
fn display_error_info_contains_code_str() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "write denied by policy");
    let msg = info.to_string();
    assert!(
        msg.contains("policy_denied"),
        "ErrorInfo Display should contain code as_str: got {msg}"
    );
    assert!(
        msg.contains("write denied"),
        "ErrorInfo Display should contain message"
    );
}

#[test]
fn display_error_location_format() {
    let loc = ErrorLocation::new("src/main.rs", 42, 5);
    let msg = loc.to_string();
    assert_eq!(msg, "src/main.rs:42:5");
}

// =========================================================================
// 2. Error Debug tests (10)
// =========================================================================

#[test]
fn debug_mapping_error_contains_type_name() {
    for err in all_mapping_errors() {
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("FeatureUnsupported")
                || dbg.contains("EmulationFailed")
                || dbg.contains("FidelityLoss")
                || dbg.contains("AmbiguousMapping")
                || dbg.contains("NegotiationFailed"),
            "MappingError Debug should contain variant name: got {dbg}"
        );
    }
}

#[test]
fn debug_protocol_error_contains_type_name() {
    for err in all_protocol_errors() {
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("HandshakeFailed")
                || dbg.contains("VersionMismatch")
                || dbg.contains("EnvelopeMalformed")
                || dbg.contains("StreamInterrupted")
                || dbg.contains("TimeoutExpired")
                || dbg.contains("SidecarCrashed"),
            "ProtocolError Debug should contain variant name: got {dbg}"
        );
    }
}

#[test]
fn debug_vendor_error_contains_type_name() {
    for err in all_vendor_errors() {
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("OpenAi")
                || dbg.contains("Claude")
                || dbg.contains("Gemini")
                || dbg.contains("Codex")
                || dbg.contains("Copilot")
                || dbg.contains("Kimi"),
            "VendorApiError Debug should contain variant name: got {dbg}"
        );
    }
}

#[test]
fn debug_recovery_strategy_contains_type_name() {
    for strat in all_recovery_strategies() {
        let dbg = format!("{:?}", strat);
        assert!(
            dbg.contains("Retry")
                || dbg.contains("Fallback")
                || dbg.contains("Degrade")
                || dbg.contains("Abort"),
            "RecoveryStrategy Debug should contain variant name: got {dbg}"
        );
    }
}

#[test]
fn debug_abp_error_contains_struct_name() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dbg = format!("{:?}", err);
    assert!(
        dbg.contains("AbpError"),
        "AbpError Debug should contain 'AbpError': got {dbg}"
    );
}

#[test]
fn debug_abp_error_shows_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "access denied");
    let dbg = format!("{:?}", err);
    assert!(
        dbg.contains("PolicyDenied"),
        "Debug should show code variant"
    );
    assert!(dbg.contains("access denied"), "Debug should show message");
}

#[test]
fn debug_abp_error_with_source_shows_cause() {
    let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(cause);
    let dbg = format!("{:?}", err);
    assert!(
        dbg.contains("file missing"),
        "Debug should show source error: got {dbg}"
    );
}

#[test]
fn debug_error_info_contains_struct_name() {
    let info = ErrorInfo::new(ErrorCode::Internal, "test");
    let dbg = format!("{:?}", info);
    assert!(
        dbg.contains("ErrorInfo"),
        "ErrorInfo Debug should contain 'ErrorInfo': got {dbg}"
    );
}

#[test]
fn debug_error_category_all_variants() {
    let categories = [
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ];
    for cat in categories {
        let dbg = format!("{:?}", cat);
        assert!(!dbg.is_empty(), "ErrorCategory Debug was empty");
        // Debug should use the variant name (PascalCase)
        assert!(
            dbg.chars().next().unwrap().is_uppercase(),
            "ErrorCategory Debug should start with uppercase: got {dbg}"
        );
    }
}

#[test]
fn debug_abp_error_dto_contains_struct_name() {
    let err = AbpError::new(ErrorCode::Internal, "snap");
    let dto: AbpErrorDto = (&err).into();
    let dbg = format!("{:?}", dto);
    assert!(
        dbg.contains("AbpErrorDto"),
        "AbpErrorDto Debug should contain struct name: got {dbg}"
    );
}

// =========================================================================
// 3. Error conversion tests (10)
// =========================================================================

#[test]
fn convert_mapping_feature_unsupported_to_abp_error() {
    let me = MappingError::FeatureUnsupported {
        feature: "vision".into(),
        source_dialect: "openai".into(),
        target_dialect: "gemini".into(),
    };
    let abp = me.into_abp_error();
    assert_eq!(abp.code, ErrorCode::MappingUnsupportedCapability);
    assert!(abp.has_context_key("mapping_code"));
}

#[test]
fn convert_mapping_emulation_failed_to_abp_error() {
    let me = MappingError::EmulationFailed {
        feature: "tool_use".into(),
        reason: "no adapter".into(),
    };
    let abp = me.into_abp_error();
    assert_eq!(abp.code, ErrorCode::CapabilityEmulationFailed);
}

#[test]
fn convert_mapping_fidelity_and_ambiguous_to_abp_error() {
    let fl = MappingError::FidelityLoss {
        field: "temp".into(),
        original: "0.73".into(),
        approximation: "0.7".into(),
    };
    assert_eq!(fl.into_abp_error().code, ErrorCode::MappingLossyConversion);

    let am = MappingError::AmbiguousMapping {
        field: "stop".into(),
        candidates: vec!["a".into(), "b".into()],
    };
    assert_eq!(am.into_abp_error().code, ErrorCode::MappingDialectMismatch);
}

#[test]
fn convert_protocol_handshake_to_abp_error() {
    let pe = ProtocolError::HandshakeFailed {
        reason: "no hello".into(),
    };
    let abp = pe.into_abp_error();
    assert_eq!(abp.code, ErrorCode::ProtocolHandshakeFailed);
    assert!(abp.has_context_key("protocol_code"));
}

#[test]
fn convert_protocol_version_mismatch_to_abp_error() {
    let pe = ProtocolError::VersionMismatch {
        expected: "abp/v0.1".into(),
        actual: "abp/v0.2".into(),
    };
    let abp = pe.into_abp_error();
    assert_eq!(abp.code, ErrorCode::ProtocolVersionMismatch);
}

#[test]
fn convert_protocol_remaining_variants_to_abp_error() {
    let envelope = ProtocolError::EnvelopeMalformed {
        raw_line: "bad".into(),
        parse_error: "syntax".into(),
    };
    assert_eq!(
        envelope.into_abp_error().code,
        ErrorCode::ProtocolInvalidEnvelope
    );

    let stream = ProtocolError::StreamInterrupted {
        events_received: 5,
        reason: "eof".into(),
    };
    assert_eq!(
        stream.into_abp_error().code,
        ErrorCode::ProtocolUnexpectedMessage
    );

    let timeout = ProtocolError::TimeoutExpired {
        operation: "run".into(),
        timeout_ms: 3000,
    };
    assert_eq!(timeout.into_abp_error().code, ErrorCode::BackendTimeout);

    let crash = ProtocolError::SidecarCrashed {
        exit_code: Some(137),
        stderr_tail: "killed".into(),
    };
    assert_eq!(crash.into_abp_error().code, ErrorCode::BackendCrashed);
}

#[test]
fn convert_vendor_api_error_to_abp_error() {
    let ve = VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited"));
    let abp = ve.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendRateLimited);
    assert!(abp.has_context_key("vendor_code"));
    assert!(abp.has_context_key("vendor_name"));
    assert!(abp.has_context_key("vendor_status"));
}

#[test]
fn convert_vendor_all_status_codes_to_abp_error() {
    let cases: Vec<(u16, ErrorCode)> = vec![
        (401, ErrorCode::BackendAuthFailed),
        (403, ErrorCode::PolicyDenied),
        (404, ErrorCode::BackendModelNotFound),
        (429, ErrorCode::BackendRateLimited),
        (500, ErrorCode::BackendUnavailable),
        (504, ErrorCode::BackendTimeout),
    ];
    for (status, expected_code) in cases {
        let ve = VendorApiError::OpenAi(VendorErrorDetail::new(status, "test"));
        let abp = ve.into_abp_error();
        assert_eq!(
            abp.code, expected_code,
            "HTTP {status} should map to {expected_code:?}"
        );
    }
}

#[test]
fn convert_io_error_to_abp_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.message.contains("access denied"));
    // The io::Error should be attached as a source
    assert!(
        abp.source().is_some(),
        "io::Error should be chained as source"
    );
}

#[test]
fn convert_serde_error_to_abp_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let msg = serde_err.to_string();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(
        abp.message.contains(&msg[..20]),
        "serde error message should be preserved"
    );
    assert!(abp.source().is_some(), "serde::Error should be chained");
}

// =========================================================================
// 4. Error source chain tests (5)
// =========================================================================

#[test]
fn source_chain_single_cause() {
    let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err =
        AbpError::new(ErrorCode::WorkspaceInitFailed, "workspace init failed").with_source(cause);

    assert!(err.source().is_some());
    let src = err.source().unwrap();
    assert!(
        src.to_string().contains("file not found"),
        "source should be the io error"
    );
    // The io error itself has no further source
    assert!(src.source().is_none());
}

#[test]
fn source_chain_depth_is_correct() {
    let inner = std::io::Error::other("disk full");
    let mid = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging failed").with_source(inner);
    // chain_depth counts the source chain from the error's source onward
    assert_eq!(mid.chain_depth(), 1);

    let outer =
        AbpError::new(ErrorCode::ExecutionWorkspaceError, "execution error").with_source(mid);
    // outer → mid (AbpError with io source) → io::Error = depth 2
    assert_eq!(outer.chain_depth(), 2);
}

#[test]
fn source_chain_traversable_via_std_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
    let err = AbpError::new(ErrorCode::Internal, "something failed").with_source(io_err);

    // Traverse using std::error::Error::source()
    let mut current: &dyn Error = &err;
    let mut depth = 0;
    while let Some(src) = current.source() {
        depth += 1;
        current = src;
    }
    assert_eq!(depth, 1, "should have exactly one level of cause");
}

#[test]
fn error_chain_iterator_yields_all_causes() {
    let cause = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
    let err = AbpError::new(ErrorCode::BackendTimeout, "backend timed out").with_source(cause);

    let chain: Vec<_> = err.error_chain().collect();
    assert_eq!(chain.len(), 1);
    assert!(chain[0].to_string().contains("timed out"));
}

#[test]
fn display_chain_multi_line_format() {
    let cause = std::io::Error::other("underlying I/O failure");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "workspace broke").with_source(cause);

    let chain_str = err.display_chain();
    let lines: Vec<&str> = chain_str.lines().collect();
    assert!(
        lines.len() >= 2,
        "display_chain should have at least 2 lines"
    );
    assert!(
        lines[0].contains("workspace_init_failed"),
        "first line should contain the error code"
    );
    assert!(
        lines[1].contains("caused by"),
        "second line should contain 'caused by'"
    );
    assert!(
        lines[1].contains("underlying I/O failure"),
        "second line should contain the cause message"
    );
}

// =========================================================================
// Bonus: additional coverage to reach 40+ tests
// =========================================================================

#[test]
fn display_error_code_all_variants_non_empty() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
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
    for code in codes {
        let msg = code.to_string();
        assert!(
            !msg.is_empty(),
            "ErrorCode::Display was empty for {:?}",
            code
        );
        // Must be informative, not just "error"
        assert_ne!(
            msg, "error",
            "ErrorCode Display should be descriptive: got {msg}"
        );
    }
}

#[test]
fn display_error_category_all_variants_non_empty() {
    let categories = [
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ];
    for cat in categories {
        let msg = cat.to_string();
        assert!(
            !msg.is_empty(),
            "ErrorCategory Display was empty for {:?}",
            cat
        );
    }
}

#[test]
fn convert_string_to_abp_error() {
    let abp: AbpError = String::from("something went wrong").into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert_eq!(abp.message, "something went wrong");
}

#[test]
fn convert_str_to_abp_error() {
    let abp: AbpError = "bad thing".into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert_eq!(abp.message, "bad thing");
}

#[test]
fn abp_error_no_source_returns_none() {
    let err = AbpError::new(ErrorCode::Internal, "no cause");
    assert!(err.source().is_none());
    assert_eq!(err.chain_depth(), 0);
}

#[test]
fn abp_error_with_location_debug() {
    let err = AbpError::new(ErrorCode::Internal, "located error")
        .with_location(ErrorLocation::new("lib.rs", 10, 1));
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("lib.rs"), "Debug should show file location");
    assert!(dbg.contains("10"), "Debug should show line number");
}
