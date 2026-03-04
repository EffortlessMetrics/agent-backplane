#![allow(clippy::all)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Mutation-critical-path tests.
//!
//! Each test in this module is designed to **catch** cargo-mutants mutations in
//! the most security- and correctness-sensitive code paths. Tests assert exact
//! values (not just truthiness) so that flipping a boolean, swapping a variant,
//! or changing a score will cause a deterministic failure.

// ─── Receipt hash computation ────────────────────────────────────────────────

mod receipt_hash {
    use abp_receipt::{ReceiptBuilder, canonicalize, compute_hash, verify_hash};

    fn minimal_receipt() -> abp_core::Receipt {
        ReceiptBuilder::new("test-backend").build()
    }

    #[test]
    fn canonical_json_sets_hash_field_to_null() {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("should_be_nulled".into());
        let json = canonicalize(&r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            v["receipt_sha256"].is_null(),
            "receipt_sha256 must be null in canonical form"
        );
    }

    #[test]
    fn hash_is_64_hex_chars() {
        let hash = compute_hash(&minimal_receipt()).unwrap();
        assert_eq!(hash.len(), 64, "SHA-256 hex must be 64 chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "must be hex");
    }

    #[test]
    fn hash_is_deterministic() {
        let r = minimal_receipt();
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2, "same receipt must produce identical hash");
    }

    #[test]
    fn different_outcome_yields_different_hash() {
        let r1 = ReceiptBuilder::new("test")
            .outcome(abp_core::Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("test")
            .outcome(abp_core::Outcome::Failed)
            .build();
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2, "different outcomes must yield different hashes");
    }

    #[test]
    fn verify_hash_passes_for_correctly_hashed_receipt() {
        let r = ReceiptBuilder::new("test").with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
        assert!(verify_hash(&r), "valid hash must verify");
    }

    #[test]
    fn verify_hash_fails_for_tampered_receipt() {
        let mut r = ReceiptBuilder::new("test").with_hash().unwrap();
        r.outcome = abp_core::Outcome::Failed; // tamper
        assert!(!verify_hash(&r), "tampered receipt must not verify");
    }

    #[test]
    fn verify_hash_returns_true_when_no_hash_set() {
        let r = minimal_receipt();
        assert!(r.receipt_sha256.is_none());
        assert!(verify_hash(&r), "no hash means nothing to verify");
    }

    #[test]
    fn with_hash_builder_populates_sha256() {
        let r = ReceiptBuilder::new("test").with_hash().unwrap();
        let hash = r.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
        let recomputed = compute_hash(&r).unwrap();
        assert_eq!(hash, &recomputed);
    }

    #[test]
    fn hash_changes_when_backend_id_changes() {
        let r1 = ReceiptBuilder::new("a").build();
        let r2 = ReceiptBuilder::new("b").build();
        assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
    }
}

// ─── Policy engine allow/deny decisions ──────────────────────────────────────

mod policy_engine {
    use abp_core::PolicyProfile;
    use abp_policy::{Decision, PolicyEngine};
    use std::path::Path;

    fn empty_policy() -> PolicyProfile {
        PolicyProfile::default()
    }

    #[test]
    fn empty_profile_allows_all_tools() {
        let engine = PolicyEngine::new(&empty_policy()).unwrap();
        let d = engine.can_use_tool("BashExec");
        assert!(d.allowed, "empty profile must allow all tools");
    }

    #[test]
    fn empty_profile_allows_all_reads() {
        let engine = PolicyEngine::new(&empty_policy()).unwrap();
        let d = engine.can_read_path(Path::new("src/main.rs"));
        assert!(d.allowed);
    }

    #[test]
    fn empty_profile_allows_all_writes() {
        let engine = PolicyEngine::new(&empty_policy()).unwrap();
        let d = engine.can_write_path(Path::new("output.txt"));
        assert!(d.allowed);
    }

    #[test]
    fn denied_tool_returns_false() {
        let p = PolicyProfile {
            disallowed_tools: vec!["BashExec".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        let d = engine.can_use_tool("BashExec");
        assert!(!d.allowed, "denied tool must return allowed=false");
    }

    #[test]
    fn allowed_tool_returns_true() {
        let p = PolicyProfile {
            allowed_tools: vec!["ReadFile".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        let d = engine.can_use_tool("ReadFile");
        assert!(d.allowed, "allowed tool must return allowed=true");
    }

    #[test]
    fn deny_overrides_allow_for_tools() {
        let p = PolicyProfile {
            allowed_tools: vec!["Bash*".into()],
            disallowed_tools: vec!["BashExec".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        assert!(
            !engine.can_use_tool("BashExec").allowed,
            "deny must override allow"
        );
    }

    #[test]
    fn deny_read_blocks_path() {
        let p = PolicyProfile {
            deny_read: vec!["**/.env".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(!engine.can_read_path(Path::new("src/.env")).allowed);
    }

    #[test]
    fn deny_read_does_not_block_unmatched_path() {
        let p = PolicyProfile {
            deny_read: vec!["**/.env".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_blocks_pattern() {
        let p = PolicyProfile {
            deny_write: vec!["secret/**".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        assert!(!engine.can_write_path(Path::new("secret/key.pem")).allowed);
    }

    #[test]
    fn deny_write_allows_unmatched() {
        let p = PolicyProfile {
            deny_write: vec!["secret/**".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        assert!(
            engine
                .can_write_path(Path::new("public/index.html"))
                .allowed
        );
    }

    #[test]
    fn decision_allow_constructor() {
        let d = Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn decision_deny_constructor() {
        let d = Decision::deny("reason");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("reason"));
    }

    #[test]
    fn wildcard_deny_blocks_all() {
        let p = PolicyProfile {
            disallowed_tools: vec!["*".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&p).unwrap();
        assert!(!engine.can_use_tool("anything").allowed);
    }
}

// ─── Composed policy engine ──────────────────────────────────────────────────

mod composed_policy {
    use abp_core::PolicyProfile;
    use abp_policy::compose::{ComposedEngine, PolicyDecision, PolicyPrecedence};

    #[test]
    fn deny_overrides_strategy_denies_when_any_denies() {
        let allow_all = PolicyProfile::default();
        let deny_bash = PolicyProfile {
            disallowed_tools: vec!["BashExec".into()],
            ..Default::default()
        };

        let engine =
            ComposedEngine::new(vec![allow_all, deny_bash], PolicyPrecedence::DenyOverrides)
                .unwrap();
        match engine.check_tool("BashExec") {
            PolicyDecision::Deny { .. } => {}
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn allow_overrides_strategy_allows_when_any_allows() {
        let allow_all = PolicyProfile::default();
        let deny_bash = PolicyProfile {
            disallowed_tools: vec!["BashExec".into()],
            ..Default::default()
        };

        let engine =
            ComposedEngine::new(vec![allow_all, deny_bash], PolicyPrecedence::AllowOverrides)
                .unwrap();
        match engine.check_tool("BashExec") {
            PolicyDecision::Allow { .. } => {}
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn composed_check_read_denies_sensitive_path() {
        let deny_env = PolicyProfile {
            deny_read: vec!["**/.env".into()],
            ..Default::default()
        };
        let engine = ComposedEngine::new(vec![deny_env], PolicyPrecedence::DenyOverrides).unwrap();
        match engine.check_read(".env") {
            PolicyDecision::Deny { .. } => {}
            other => panic!("expected Deny for .env, got {other:?}"),
        }
    }

    #[test]
    fn composed_check_write_denies_pattern() {
        let deny_secrets = PolicyProfile {
            deny_write: vec!["secret/**".into()],
            ..Default::default()
        };
        let engine =
            ComposedEngine::new(vec![deny_secrets], PolicyPrecedence::DenyOverrides).unwrap();
        match engine.check_write("secret/key.pem") {
            PolicyDecision::Deny { .. } => {}
            other => panic!("expected Deny, got {other:?}"),
        }
    }
}

// ─── Capability negotiation ──────────────────────────────────────────────────

mod capability_negotiation {
    use abp_capability::{
        check_capability, generate_report, negotiate::NegotiationPolicy, negotiate::apply_policy,
        negotiate_capabilities,
    };
    use abp_core::{Capability, CapabilityManifest, SupportLevel};

    fn manifest_with(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
        caps.iter().cloned().collect()
    }

    #[test]
    fn native_capability_accepted() {
        let m = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.native[0], Capability::Streaming);
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn missing_capability_unsupported() {
        let m = CapabilityManifest::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert_eq!(result.unsupported.len(), 1);
        assert_eq!(result.unsupported[0].0, Capability::Streaming);
        assert!(result.native.is_empty());
    }

    #[test]
    fn emulated_capability_classified_correctly() {
        let m = manifest_with(&[(Capability::ToolBash, SupportLevel::Emulated)]);
        let result = negotiate_capabilities(&[Capability::ToolBash], &m);
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.emulated[0].0, Capability::ToolBash);
    }

    #[test]
    fn is_viable_true_when_all_native() {
        let m = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(result.is_viable());
    }

    #[test]
    fn is_viable_false_when_unsupported() {
        let m = CapabilityManifest::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(!result.is_viable());
    }

    #[test]
    fn is_viable_true_when_emulated() {
        let m = manifest_with(&[(Capability::ToolBash, SupportLevel::Emulated)]);
        let result = negotiate_capabilities(&[Capability::ToolBash], &m);
        assert!(result.is_viable());
    }

    #[test]
    fn strict_policy_rejects_unsupported() {
        let m = CapabilityManifest::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(apply_policy(&result, NegotiationPolicy::Strict).is_err());
    }

    #[test]
    fn permissive_policy_accepts_unsupported() {
        let m = CapabilityManifest::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn report_compatible_when_viable() {
        let m = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 1);
        assert_eq!(report.unsupported_count, 0);
    }

    #[test]
    fn report_incompatible_when_unsupported() {
        let m = CapabilityManifest::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert_eq!(report.unsupported_count, 1);
    }

    #[test]
    fn check_capability_returns_native_level() {
        let m = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
        let level = check_capability(&m, &Capability::Streaming);
        assert!(matches!(level, abp_capability::SupportLevel::Native));
    }

    #[test]
    fn check_capability_returns_unsupported_for_missing() {
        let m = CapabilityManifest::new();
        let level = check_capability(&m, &Capability::Streaming);
        assert!(matches!(
            level,
            abp_capability::SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn total_counts_all_categories() {
        let m = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
        let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolBash], &m);
        assert_eq!(result.total(), 2);
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn empty_requirements_always_viable() {
        let m = CapabilityManifest::new();
        let result = negotiate_capabilities(&[], &m);
        assert!(result.is_viable());
        assert_eq!(result.total(), 0);
    }
}

// ─── Contract version checks ─────────────────────────────────────────────────

mod contract_version {
    use abp_core::CONTRACT_VERSION;

    #[test]
    fn version_string_exact_value() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn version_starts_with_abp_prefix() {
        assert!(
            CONTRACT_VERSION.starts_with("abp/"),
            "must start with 'abp/'"
        );
    }

    #[test]
    fn version_has_semver_like_suffix() {
        let suffix = CONTRACT_VERSION
            .strip_prefix("abp/v")
            .expect("must have 'abp/v' prefix");
        assert!(
            suffix.contains('.'),
            "version suffix must have a dot separator"
        );
    }

    #[test]
    fn hello_envelope_roundtrips_version() {
        use abp_core::{BackendIdentity, CapabilityManifest, ExecutionMode};
        use abp_protocol::Envelope;

        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::Mapped,
        };
        let json = serde_json::to_string(&env).unwrap();
        let parsed: Envelope = serde_json::from_str(&json).unwrap();
        match parsed {
            Envelope::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, CONTRACT_VERSION),
            _ => panic!("wrong variant"),
        }
    }
}

// ─── Error code mapping ──────────────────────────────────────────────────────

mod error_codes {
    use abp_error::{ErrorCategory, ErrorCode};

    #[test]
    fn protocol_codes_map_to_protocol_category() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolHandshakeFailed.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::ProtocolMissingRefId.category(),
            ErrorCategory::Protocol
        );
    }

    #[test]
    fn backend_codes_map_to_backend_category() {
        assert_eq!(
            ErrorCode::BackendNotFound.category(),
            ErrorCategory::Backend
        );
        assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
        assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    }

    #[test]
    fn contract_codes_map_to_contract_category() {
        assert_eq!(
            ErrorCode::ContractVersionMismatch.category(),
            ErrorCategory::Contract
        );
        assert_eq!(
            ErrorCode::ContractInvalidReceipt.category(),
            ErrorCategory::Contract
        );
    }

    #[test]
    fn retryable_codes_exact_set() {
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendCrashed.is_retryable());
    }

    #[test]
    fn non_retryable_codes() {
        assert!(!ErrorCode::BackendNotFound.is_retryable());
        assert!(!ErrorCode::BackendAuthFailed.is_retryable());
        assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
        assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
    }

    #[test]
    fn as_str_returns_snake_case_exact() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.as_str(),
            "protocol_invalid_envelope"
        );
        assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
        assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
        assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
        assert_eq!(
            ErrorCode::ContractVersionMismatch.as_str(),
            "contract_version_mismatch"
        );
    }

    #[test]
    fn as_str_roundtrips_via_serde() {
        let code = ErrorCode::BackendRateLimited;
        let json = serde_json::to_string(&code).unwrap();
        let parsed: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, code);
    }

    #[test]
    fn mapping_codes_map_to_mapping_category() {
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
    }

    #[test]
    fn capability_codes_map_to_capability_category() {
        assert_eq!(
            ErrorCode::CapabilityUnsupported.category(),
            ErrorCategory::Capability
        );
        assert_eq!(
            ErrorCode::CapabilityEmulationFailed.category(),
            ErrorCategory::Capability
        );
    }

    #[test]
    fn receipt_codes_map_to_receipt_category() {
        assert_eq!(
            ErrorCode::ReceiptHashMismatch.category(),
            ErrorCategory::Receipt
        );
        assert_eq!(
            ErrorCode::ReceiptChainBroken.category(),
            ErrorCategory::Receipt
        );
    }

    #[test]
    fn policy_codes_map_to_policy_category() {
        assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
        assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
    }

    #[test]
    fn error_info_carries_code() {
        let info = abp_error::ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
        assert_eq!(info.code, ErrorCode::BackendTimeout);
    }
}

// ─── Dialect detection scoring ───────────────────────────────────────────────

mod dialect_detection {
    use abp_dialect::{Dialect, DialectDetector, detect::detect_dialect};
    use serde_json::json;

    #[test]
    fn openai_model_prefix_detected() {
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let result = detect_dialect(&req).expect("should detect OpenAI");
        assert_eq!(result.dialect, Dialect::OpenAi);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn claude_model_prefix_detected() {
        let req = json!({"model": "claude-3-opus-20240229", "messages": [{"role": "user", "content": "hi"}]});
        let result = detect_dialect(&req).expect("should detect Claude");
        assert_eq!(result.dialect, Dialect::Claude);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn gemini_model_prefix_detected() {
        let req = json!({"model": "gemini-1.5-pro", "contents": [{"role": "user", "parts": [{"text": "hi"}]}]});
        let result = detect_dialect(&req).expect("should detect Gemini");
        assert_eq!(result.dialect, Dialect::Gemini);
    }

    #[test]
    fn empty_object_returns_none() {
        let result = detect_dialect(&json!({}));
        assert!(
            result.is_none(),
            "empty object should not match any dialect"
        );
    }

    #[test]
    fn non_object_returns_none() {
        assert!(detect_dialect(&json!("hello")).is_none());
        assert!(detect_dialect(&json!(42)).is_none());
        assert!(detect_dialect(&json!(null)).is_none());
    }

    #[test]
    fn confidence_capped_at_one() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.7,
            "max_tokens": 100,
            "stream": true,
        });
        let result = detect_dialect(&req).unwrap();
        assert!(result.confidence <= 1.0, "confidence must not exceed 1.0");
    }

    #[test]
    fn dialect_labels_are_exact() {
        assert_eq!(Dialect::OpenAi.label(), "OpenAI");
        assert_eq!(Dialect::Claude.label(), "Claude");
        assert_eq!(Dialect::Gemini.label(), "Gemini");
        assert_eq!(Dialect::Codex.label(), "Codex");
        assert_eq!(Dialect::Kimi.label(), "Kimi");
        assert_eq!(Dialect::Copilot.label(), "Copilot");
    }

    #[test]
    fn detector_detect_all_returns_sorted() {
        let detector = DialectDetector::new();
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let results = detector.detect_all(&req);
        if results.len() >= 2 {
            assert!(results[0].confidence >= results[1].confidence);
        }
    }

    #[test]
    fn all_dialects_enumerated() {
        let all = Dialect::all();
        assert!(all.len() >= 6, "must have at least 6 dialects");
        assert!(all.contains(&Dialect::OpenAi));
        assert!(all.contains(&Dialect::Claude));
        assert!(all.contains(&Dialect::Gemini));
    }
}

// ─── Glob matching (workspace file exclusion) ────────────────────────────────

mod glob_matching {
    use abp_glob::{IncludeExcludeGlobs, MatchDecision};
    use std::path::Path;

    #[test]
    fn empty_globs_allow_everything() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        assert!(matches!(
            g.decide_path(Path::new("anything.txt")),
            MatchDecision::Allowed
        ));
    }

    #[test]
    fn exclude_denies_matching_path() {
        let g = IncludeExcludeGlobs::new(&[], &["*.secret".into()]).unwrap();
        let d = g.decide_path(Path::new("key.secret"));
        assert!(matches!(d, MatchDecision::DeniedByExclude));
        assert!(!d.is_allowed());
    }

    #[test]
    fn exclude_allows_non_matching_path() {
        let g = IncludeExcludeGlobs::new(&[], &["*.secret".into()]).unwrap();
        let d = g.decide_path(Path::new("readme.md"));
        assert!(matches!(d, MatchDecision::Allowed));
        assert!(d.is_allowed());
    }

    #[test]
    fn include_only_gates_membership() {
        let g = IncludeExcludeGlobs::new(&["*.rs".into()], &[]).unwrap();
        assert!(
            g.decide_path(Path::new("main.rs")).is_allowed(),
            "included extension must be allowed"
        );
        assert!(
            !g.decide_path(Path::new("main.py")).is_allowed(),
            "non-included extension must be denied"
        );
    }

    #[test]
    fn include_miss_returns_denied_by_missing_include() {
        let g = IncludeExcludeGlobs::new(&["*.rs".into()], &[]).unwrap();
        assert!(matches!(
            g.decide_path(Path::new("test.py")),
            MatchDecision::DeniedByMissingInclude
        ));
    }

    #[test]
    fn exclude_overrides_include() {
        let g = IncludeExcludeGlobs::new(&["*.rs".into()], &["test_*.rs".into()]).unwrap();
        assert!(g.decide_path(Path::new("lib.rs")).is_allowed());
        assert!(
            !g.decide_path(Path::new("test_lib.rs")).is_allowed(),
            "exclude must override include"
        );
    }

    #[test]
    fn double_star_matches_nested() {
        let g = IncludeExcludeGlobs::new(&[], &["**/.git/**".into()]).unwrap();
        assert!(!g.decide_path(Path::new(".git/config")).is_allowed());
        assert!(g.decide_path(Path::new("src/main.rs")).is_allowed());
    }

    #[test]
    fn decide_str_works_like_decide_path() {
        let g = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
        assert!(!g.decide_str("app.log").is_allowed());
        assert!(g.decide_str("app.txt").is_allowed());
    }

    #[test]
    fn match_decision_is_allowed_exact() {
        assert!(MatchDecision::Allowed.is_allowed());
        assert!(!MatchDecision::DeniedByExclude.is_allowed());
        assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
    }
}

// ─── IR normalization ────────────────────────────────────────────────────────

mod ir_normalization {
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
    use abp_ir::normalize::{
        dedup_system, extract_system, merge_adjacent_text, normalize_role, strip_empty, trim_text,
    };

    fn msg(role: IrRole, text: &str) -> IrMessage {
        IrMessage::text(role, text.to_string())
    }

    fn conv(msgs: Vec<IrMessage>) -> IrConversation {
        IrConversation::from_messages(msgs)
    }

    // -- normalize_role --

    #[test]
    fn normalize_role_standard_mappings() {
        assert_eq!(normalize_role("system"), Some(IrRole::System));
        assert_eq!(normalize_role("user"), Some(IrRole::User));
        assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_vendor_aliases() {
        assert_eq!(normalize_role("developer"), Some(IrRole::System));
        assert_eq!(normalize_role("human"), Some(IrRole::User));
        assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("function"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_unknown_returns_none() {
        assert_eq!(normalize_role(""), None);
        assert_eq!(normalize_role("admin"), None);
        assert_eq!(normalize_role("System"), None, "case-sensitive");
    }

    // -- dedup_system --

    #[test]
    fn dedup_system_merges_multiple_system_messages() {
        let c = conv(vec![
            msg(IrRole::System, "first"),
            msg(IrRole::User, "hello"),
            msg(IrRole::System, "second"),
        ]);
        let result = dedup_system(&c);
        let system_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .collect();
        assert_eq!(system_msgs.len(), 1, "must merge into exactly one");
        assert_eq!(system_msgs[0].text_content(), "first\nsecond");
    }

    #[test]
    fn dedup_system_preserves_non_system_order() {
        let c = conv(vec![
            msg(IrRole::System, "sys"),
            msg(IrRole::User, "u1"),
            msg(IrRole::Assistant, "a1"),
        ]);
        let result = dedup_system(&c);
        assert_eq!(result.messages.len(), 3);
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(result.messages[1].role, IrRole::User);
        assert_eq!(result.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn dedup_system_no_system_messages_unchanged() {
        let c = conv(vec![msg(IrRole::User, "hi")]);
        let result = dedup_system(&c);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    // -- trim_text --

    #[test]
    fn trim_text_strips_whitespace() {
        let c = conv(vec![msg(IrRole::User, "  hello  ")]);
        let result = trim_text(&c);
        assert_eq!(result.messages[0].text_content(), "hello");
    }

    #[test]
    fn trim_text_preserves_non_text_blocks() {
        let mut m = msg(IrRole::Assistant, "text");
        m.content.push(IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "bash".into(),
            input: serde_json::json!({}),
        });
        let c = conv(vec![m]);
        let result = trim_text(&c);
        assert_eq!(result.messages[0].content.len(), 2);
    }

    // -- strip_empty --

    #[test]
    fn strip_empty_removes_empty_messages() {
        let empty = IrMessage {
            role: IrRole::User,
            content: vec![],
            metadata: Default::default(),
        };
        let c = conv(vec![empty, msg(IrRole::User, "hi")]);
        let result = strip_empty(&c);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].text_content(), "hi");
    }

    #[test]
    fn strip_empty_keeps_non_empty() {
        let c = conv(vec![
            msg(IrRole::User, "hi"),
            msg(IrRole::Assistant, "hello"),
        ]);
        let result = strip_empty(&c);
        assert_eq!(result.messages.len(), 2);
    }

    // -- extract_system --

    #[test]
    fn extract_system_returns_system_text_and_remainder() {
        let c = conv(vec![
            msg(IrRole::System, "instructions"),
            msg(IrRole::User, "hello"),
        ]);
        let (sys, remainder) = extract_system(&c);
        assert_eq!(sys.as_deref(), Some("instructions"));
        assert_eq!(remainder.messages.len(), 1);
        assert_eq!(remainder.messages[0].role, IrRole::User);
    }

    #[test]
    fn extract_system_returns_none_when_no_system() {
        let c = conv(vec![msg(IrRole::User, "hello")]);
        let (sys, remainder) = extract_system(&c);
        assert!(sys.is_none());
        assert_eq!(remainder.messages.len(), 1);
    }

    // -- merge_adjacent_text --

    #[test]
    fn merge_adjacent_text_combines_consecutive_text_blocks() {
        let m = IrMessage {
            role: IrRole::User,
            content: vec![
                IrContentBlock::Text {
                    text: "part1".into(),
                },
                IrContentBlock::Text {
                    text: "part2".into(),
                },
            ],
            metadata: Default::default(),
        };
        let c = conv(vec![m]);
        let result = merge_adjacent_text(&c);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].content.len(), 1);
        let text = result.messages[0].text_content();
        assert!(text.contains("part1"));
        assert!(text.contains("part2"));
    }

    #[test]
    fn merge_adjacent_text_does_not_merge_different_roles() {
        let c = conv(vec![msg(IrRole::User, "u"), msg(IrRole::Assistant, "a")]);
        let result = merge_adjacent_text(&c);
        assert_eq!(result.messages.len(), 2);
    }
}

// ─── Protocol envelope serde ─────────────────────────────────────────────────

mod protocol_envelope {
    use abp_core::{BackendIdentity, CapabilityManifest, ExecutionMode};
    use abp_protocol::Envelope;

    fn test_backend() -> BackendIdentity {
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        }
    }

    #[test]
    fn envelope_tag_field_is_t() {
        let env = Envelope::Hello {
            contract_version: "abp/v0.1".into(),
            backend: test_backend(),
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::Mapped,
        };
        let json: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(
            json["t"].as_str(),
            Some("hello"),
            "discriminator must be 't'"
        );
    }

    #[test]
    fn fatal_envelope_has_error_code() {
        let env = Envelope::Fatal {
            ref_id: Some("r1".into()),
            error: "boom".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        };
        let json = serde_json::to_string(&env).unwrap();
        let parsed: Envelope = serde_json::from_str(&json).unwrap();
        match parsed {
            Envelope::Fatal { error_code, .. } => {
                assert_eq!(error_code, Some(abp_error::ErrorCode::BackendCrashed));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn run_envelope_carries_id() {
        let wo = abp_core::WorkOrderBuilder::new("test task").build();
        let env = Envelope::Run {
            id: "run-42".into(),
            work_order: wo,
        };
        let json = serde_json::to_string(&env).unwrap();
        let parsed: Envelope = serde_json::from_str(&json).unwrap();
        match parsed {
            Envelope::Run { id, .. } => assert_eq!(id, "run-42"),
            _ => panic!("wrong variant"),
        }
    }
}

// ─── Execution mode ──────────────────────────────────────────────────────────

mod execution_mode {
    use abp_core::ExecutionMode;

    #[test]
    fn default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn serde_roundtrip_passthrough() {
        let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
        assert_eq!(json, "\"passthrough\"");
        let parsed: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ExecutionMode::Passthrough);
    }

    #[test]
    fn serde_roundtrip_mapped() {
        let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
        assert_eq!(json, "\"mapped\"");
    }
}

// ─── Outcome enum ────────────────────────────────────────────────────────────

mod outcome {
    use abp_core::Outcome;

    #[test]
    fn outcome_serde_exact_values() {
        assert_eq!(
            serde_json::to_string(&Outcome::Complete).unwrap(),
            "\"complete\""
        );
        assert_eq!(
            serde_json::to_string(&Outcome::Partial).unwrap(),
            "\"partial\""
        );
        assert_eq!(
            serde_json::to_string(&Outcome::Failed).unwrap(),
            "\"failed\""
        );
    }

    #[test]
    fn outcome_roundtrips() {
        for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
            let json = serde_json::to_string(&outcome).unwrap();
            let parsed: Outcome = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, outcome);
        }
    }
}
