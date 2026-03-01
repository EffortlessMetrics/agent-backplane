// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::error::{ErrorCatalog, ErrorCode, ErrorInfo};
use std::collections::{BTreeMap, HashSet};
use std::io;

/// Validate an error code matches `ABP-[CPLRS]\d{3}`.
fn matches_code_convention(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 8
        && b[..4] == *b"ABP-"
        && matches!(b[4], b'C' | b'P' | b'L' | b'R' | b'S')
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7].is_ascii_digit()
}

// ---------------------------------------------------------------------------
// Uniqueness & naming convention
// ---------------------------------------------------------------------------

#[test]
fn all_error_codes_are_unique() {
    let all = ErrorCatalog::all();
    let mut seen = HashSet::new();
    for code in &all {
        assert!(seen.insert(code.code()), "duplicate code: {}", code.code());
    }
}

#[test]
fn all_codes_follow_naming_convention() {
    for code in ErrorCatalog::all() {
        assert!(
            matches_code_convention(code.code()),
            "{:?} has non-conforming code: {}",
            code,
            code.code()
        );
    }
}

#[test]
fn at_least_fifty_codes() {
    assert!(
        ErrorCatalog::all().len() >= 50,
        "expected ≥50 codes, got {}",
        ErrorCatalog::all().len()
    );
}

// ---------------------------------------------------------------------------
// Category filtering
// ---------------------------------------------------------------------------

#[test]
fn category_contract_non_empty() {
    let codes = ErrorCatalog::by_category("contract");
    assert!(!codes.is_empty());
    for c in &codes {
        assert_eq!(c.category(), "contract");
    }
}

#[test]
fn category_protocol_non_empty() {
    let codes = ErrorCatalog::by_category("protocol");
    assert!(!codes.is_empty());
    for c in &codes {
        assert_eq!(c.category(), "protocol");
    }
}

#[test]
fn category_policy_non_empty() {
    let codes = ErrorCatalog::by_category("policy");
    assert!(!codes.is_empty());
    for c in &codes {
        assert_eq!(c.category(), "policy");
    }
}

#[test]
fn category_runtime_non_empty() {
    let codes = ErrorCatalog::by_category("runtime");
    assert!(!codes.is_empty());
    for c in &codes {
        assert_eq!(c.category(), "runtime");
    }
}

#[test]
fn category_system_non_empty() {
    let codes = ErrorCatalog::by_category("system");
    assert!(!codes.is_empty());
    for c in &codes {
        assert_eq!(c.category(), "system");
    }
}

#[test]
fn all_categories_covered() {
    let categories: HashSet<&str> = ErrorCatalog::all().iter().map(|c| c.category()).collect();
    for expected in &["contract", "protocol", "policy", "runtime", "system"] {
        assert!(
            categories.contains(expected),
            "missing category: {expected}"
        );
    }
}

#[test]
fn unknown_category_returns_empty() {
    assert!(ErrorCatalog::by_category("nonexistent").is_empty());
}

// ---------------------------------------------------------------------------
// Lookup by code string
// ---------------------------------------------------------------------------

#[test]
fn lookup_known_code() {
    let found = ErrorCatalog::lookup("ABP-C001");
    assert_eq!(found, Some(ErrorCode::InvalidContractVersion));
}

#[test]
fn lookup_unknown_code_returns_none() {
    assert_eq!(ErrorCatalog::lookup("ABP-Z999"), None);
}

#[test]
fn lookup_roundtrip_all_codes() {
    for code in ErrorCatalog::all() {
        let found = ErrorCatalog::lookup(code.code());
        assert_eq!(found, Some(code), "roundtrip failed for {}", code.code());
    }
}

// ---------------------------------------------------------------------------
// Descriptions
// ---------------------------------------------------------------------------

#[test]
fn all_descriptions_non_empty() {
    for code in ErrorCatalog::all() {
        assert!(
            !code.description().is_empty(),
            "{:?} has empty description",
            code
        );
    }
}

// ---------------------------------------------------------------------------
// ErrorCode Display
// ---------------------------------------------------------------------------

#[test]
fn error_code_display_shows_code_string() {
    assert_eq!(format!("{}", ErrorCode::IoError), "ABP-S001");
    assert_eq!(format!("{}", ErrorCode::ToolDenied), "ABP-L001");
}

// ---------------------------------------------------------------------------
// ErrorInfo display formatting
// ---------------------------------------------------------------------------

#[test]
fn error_info_display_basic() {
    let info = ErrorInfo::new(ErrorCode::IoError, "disk full");
    let s = info.to_string();
    assert!(s.contains("ABP-S001"));
    assert!(s.contains("disk full"));
}

#[test]
fn error_info_display_with_context() {
    let info = ErrorInfo::new(ErrorCode::ReadDenied, "blocked").with_context("path", "/etc/shadow");
    let s = info.to_string();
    assert!(s.contains("ABP-L002"));
    assert!(s.contains("path=/etc/shadow"));
}

// ---------------------------------------------------------------------------
// ErrorInfo builder pattern
// ---------------------------------------------------------------------------

#[test]
fn error_info_builder_chaining() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "sidecar:node")
        .with_context("timeout_ms", "5000");

    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert_eq!(info.context.len(), 2);
    assert_eq!(info.context["backend"], "sidecar:node");
    assert_eq!(info.context["timeout_ms"], "5000");
}

#[test]
fn error_info_empty_context_by_default() {
    let info = ErrorInfo::new(ErrorCode::InternalError, "oops");
    assert!(info.context.is_empty());
    assert!(info.source.is_none());
}

// ---------------------------------------------------------------------------
// ErrorInfo with source error
// ---------------------------------------------------------------------------

#[test]
fn error_info_with_source() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "gone");
    let info = ErrorInfo::new(ErrorCode::IoError, "file not found").with_source(io_err);
    assert!(info.source.is_some());

    // std::error::Error::source() works
    let src = std::error::Error::source(&info);
    assert!(src.is_some());
    assert!(src.unwrap().to_string().contains("gone"));
}

#[test]
fn error_info_debug_includes_source() {
    let io_err = io::Error::other("bad");
    let info = ErrorInfo::new(ErrorCode::IoError, "fail").with_source(io_err);
    let dbg = format!("{:?}", info);
    assert!(dbg.contains("bad"));
}

// ---------------------------------------------------------------------------
// Serde roundtrip for ErrorCode
// ---------------------------------------------------------------------------

#[test]
fn serde_roundtrip_error_code() {
    for code in ErrorCatalog::all() {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "serde roundtrip failed for {:?}", code);
    }
}

#[test]
fn serde_error_code_uses_snake_case() {
    let json = serde_json::to_string(&ErrorCode::InvalidContractVersion).unwrap();
    assert_eq!(json, "\"invalid_contract_version\"");
}

// ---------------------------------------------------------------------------
// Category letter in code matches category name
// ---------------------------------------------------------------------------

#[test]
fn code_letter_matches_category() {
    let letter_map: BTreeMap<&str, char> = [
        ("contract", 'C'),
        ("protocol", 'P'),
        ("policy", 'L'),
        ("runtime", 'R'),
        ("system", 'S'),
    ]
    .into_iter()
    .collect();

    for code in ErrorCatalog::all() {
        let expected_letter = letter_map[code.category()];
        let actual_letter = code.code().chars().nth(4).unwrap();
        assert_eq!(
            actual_letter,
            expected_letter,
            "{:?}: code {} has letter '{}' but category is '{}'",
            code,
            code.code(),
            actual_letter,
            code.category()
        );
    }
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn error_info_display_no_context_no_parens() {
    let info = ErrorInfo::new(ErrorCode::InternalError, "msg");
    let s = info.to_string();
    assert!(!s.contains('('));
}

#[test]
fn error_info_multiple_context_entries_separated() {
    let info = ErrorInfo::new(ErrorCode::ToolDenied, "denied")
        .with_context("tool", "bash")
        .with_context("user", "alice");
    let s = info.to_string();
    assert!(s.contains("tool=bash"));
    assert!(s.contains("user=alice"));
    assert!(s.contains(", "));
}
