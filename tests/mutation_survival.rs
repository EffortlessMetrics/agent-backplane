// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests that target common mutation-surviving patterns.
//!
//! These tests exercise off-by-one boundaries, boolean logic, default values,
//! and error paths across the core ABP crates so that cargo-mutants produces
//! fewer surviving mutants.

// ============================================================================
// abp-core: receipt hashing, validation, builders, SupportLevel
// ============================================================================
mod core_mutations {
    use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
    use abp_core::validate::{ValidationError, validate_receipt};
    use abp_core::*;
    use chrono::{TimeDelta, Utc};
    use std::collections::BTreeMap;

    // -- receipt_hash edge cases -----------------------------------------

    #[test]
    fn receipt_hash_deterministic() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2, "same receipt must produce identical hashes");
    }

    #[test]
    fn receipt_hash_ignores_stored_hash() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let mut r2 = r1.clone();
        r2.receipt_sha256 = Some("bogus".into());
        // Hash must be the same regardless of what receipt_sha256 contains.
        assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn receipt_hash_length_is_64() {
        let r = ReceiptBuilder::new("x").build();
        assert_eq!(receipt_hash(&r).unwrap().len(), 64);
    }

    #[test]
    fn receipt_with_hash_populates_field() {
        let r = ReceiptBuilder::new("mock").build().with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
        let hash = r.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn receipt_with_hash_is_self_consistent() {
        let r = ReceiptBuilder::new("mock").build().with_hash().unwrap();
        let recomputed = receipt_hash(&r).unwrap();
        assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
    }

    #[test]
    fn different_outcomes_produce_different_hashes() {
        let base = || ReceiptBuilder::new("mock");
        let h_complete = receipt_hash(&base().outcome(Outcome::Complete).build()).unwrap();
        let h_failed = receipt_hash(&base().outcome(Outcome::Failed).build()).unwrap();
        let h_partial = receipt_hash(&base().outcome(Outcome::Partial).build()).unwrap();
        assert_ne!(h_complete, h_failed);
        assert_ne!(h_complete, h_partial);
        assert_ne!(h_failed, h_partial);
    }

    #[test]
    fn different_backend_ids_produce_different_hashes() {
        let h1 = receipt_hash(&ReceiptBuilder::new("a").build()).unwrap();
        let h2 = receipt_hash(&ReceiptBuilder::new("b").build()).unwrap();
        assert_ne!(h1, h2);
    }

    // -- sha256_hex ------------------------------------------------------

    #[test]
    fn sha256_hex_empty_input() {
        let h = sha256_hex(b"");
        assert_eq!(h.len(), 64);
        // Known SHA-256 of empty string.
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_known_value() {
        let h = sha256_hex(b"hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // -- canonical_json --------------------------------------------------

    #[test]
    fn canonical_json_deterministic() {
        let mut map = BTreeMap::new();
        map.insert("z", 1);
        map.insert("a", 2);
        let j1 = canonical_json(&map).unwrap();
        let j2 = canonical_json(&map).unwrap();
        assert_eq!(j1, j2);
        // BTreeMap keys are sorted.
        assert!(j1.find("\"a\"").unwrap() < j1.find("\"z\"").unwrap());
    }

    // -- SupportLevel::satisfies boundaries ------------------------------

    #[test]
    fn support_native_satisfies_native() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_emulated_does_not_satisfy_native() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_unsupported_does_not_satisfy_native() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_restricted_does_not_satisfy_native() {
        let r = SupportLevel::Restricted { reason: "x".into() };
        assert!(!r.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_native_satisfies_emulated() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_emulated_satisfies_emulated() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_restricted_satisfies_emulated() {
        let r = SupportLevel::Restricted { reason: "x".into() };
        assert!(r.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_unsupported_does_not_satisfy_emulated() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    // -- ReceiptBuilder duration_ms boundary (.max(0)) -------------------

    #[test]
    fn receipt_builder_duration_zero_when_same_timestamps() {
        let now = Utc::now();
        let r = ReceiptBuilder::new("mock")
            .started_at(now)
            .finished_at(now)
            .build();
        assert_eq!(r.meta.duration_ms, 0);
    }

    #[test]
    fn receipt_builder_duration_clamps_negative_to_zero() {
        let now = Utc::now();
        let earlier = now - TimeDelta::seconds(10);
        // started_at > finished_at → negative delta → clamped to 0
        let r = ReceiptBuilder::new("mock")
            .started_at(now)
            .finished_at(earlier)
            .build();
        assert_eq!(r.meta.duration_ms, 0);
    }

    #[test]
    fn receipt_builder_positive_duration() {
        let start = Utc::now();
        let end = start + TimeDelta::milliseconds(42);
        let r = ReceiptBuilder::new("mock")
            .started_at(start)
            .finished_at(end)
            .build();
        assert_eq!(r.meta.duration_ms, 42);
    }

    // -- validate_receipt boundaries -------------------------------------

    #[test]
    fn validate_receipt_ok_with_valid_hash() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(validate_receipt(&r).is_ok());
    }

    #[test]
    fn validate_receipt_catches_empty_backend_id() {
        let r = ReceiptBuilder::new("").build();
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::EmptyBackendId))
        );
    }

    #[test]
    fn validate_receipt_catches_bad_contract_version() {
        let mut r = ReceiptBuilder::new("mock").build();
        r.meta.contract_version = "wrong".into();
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::InvalidOutcome { .. }))
        );
    }

    #[test]
    fn validate_receipt_catches_started_after_finished() {
        let now = Utc::now();
        let earlier = now - TimeDelta::seconds(10);
        let mut r = ReceiptBuilder::new("mock").build();
        r.meta.started_at = now;
        r.meta.finished_at = earlier;
        let errs = validate_receipt(&r).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidOutcome { reason } if reason.contains("started_at"))));
    }

    #[test]
    fn validate_receipt_equal_timestamps_is_ok() {
        let now = Utc::now();
        let mut r = ReceiptBuilder::new("mock").build();
        r.meta.started_at = now;
        r.meta.finished_at = now;
        assert!(validate_receipt(&r).is_ok());
    }

    #[test]
    fn validate_receipt_catches_wrong_hash() {
        let mut r = ReceiptBuilder::new("mock").build().with_hash().unwrap();
        r.receipt_sha256 =
            Some("0000000000000000000000000000000000000000000000000000000000000000".into());
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ValidationError::InvalidHash { .. }))
        );
    }

    #[test]
    fn validate_receipt_no_hash_is_ok() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(r.receipt_sha256.is_none());
        assert!(validate_receipt(&r).is_ok());
    }

    // -- ConfigValidator boundary tests ----------------------------------

    #[test]
    fn config_validator_empty_task_is_error() {
        let wo = WorkOrderBuilder::new("").build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(
            warnings
                .iter()
                .any(|w| w.field == "task" && w.severity == WarningSeverity::Error)
        );
    }

    #[test]
    fn config_validator_whitespace_task_is_error() {
        let wo = WorkOrderBuilder::new("   ").build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(warnings.iter().any(|w| w.field == "task"));
    }

    #[test]
    fn config_validator_zero_max_turns_is_error() {
        let wo = WorkOrderBuilder::new("hi").max_turns(0).build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(
            warnings
                .iter()
                .any(|w| w.field == "config.max_turns" && w.severity == WarningSeverity::Error)
        );
    }

    #[test]
    fn config_validator_one_max_turn_is_ok() {
        let wo = WorkOrderBuilder::new("hi").max_turns(1).build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(!warnings.iter().any(|w| w.field == "config.max_turns"));
    }

    #[test]
    fn config_validator_zero_budget_is_error() {
        let wo = WorkOrderBuilder::new("hi").max_budget_usd(0.0).build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(warnings
            .iter()
            .any(|w| w.field == "config.max_budget_usd" && w.severity == WarningSeverity::Error));
    }

    #[test]
    fn config_validator_negative_budget_is_error() {
        let wo = WorkOrderBuilder::new("hi").max_budget_usd(-1.0).build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(warnings.iter().any(|w| w.field == "config.max_budget_usd"));
    }

    #[test]
    fn config_validator_small_positive_budget_is_ok() {
        let wo = WorkOrderBuilder::new("hi").max_budget_usd(0.01).build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(!warnings.iter().any(|w| w.field == "config.max_budget_usd"));
    }

    #[test]
    fn config_validator_empty_model_name_is_error() {
        let wo = WorkOrderBuilder::new("hi").model("  ").build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        assert!(warnings.iter().any(|w| w.field == "config.model"));
    }

    #[test]
    fn config_validator_valid_work_order_no_errors() {
        let wo = WorkOrderBuilder::new("fix bug")
            .max_turns(10)
            .max_budget_usd(5.0)
            .model("gpt-4")
            .build();
        let warnings = ConfigValidator::new().validate_work_order(&wo);
        let errors: Vec<_> = warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    // -- ConfigDefaults --------------------------------------------------

    #[test]
    fn config_defaults_are_positive() {
        assert!(ConfigDefaults::default_max_turns() > 0);
        assert!(ConfigDefaults::default_max_budget() > 0.0);
        assert!(!ConfigDefaults::default_model().is_empty());
    }

    #[test]
    fn apply_defaults_fills_none_fields() {
        let mut wo = WorkOrderBuilder::new("task").build();
        assert!(wo.config.max_turns.is_none());
        assert!(wo.config.max_budget_usd.is_none());
        assert!(wo.config.model.is_none());

        ConfigDefaults::apply_defaults(&mut wo);

        assert_eq!(
            wo.config.max_turns,
            Some(ConfigDefaults::default_max_turns())
        );
        assert_eq!(
            wo.config.max_budget_usd,
            Some(ConfigDefaults::default_max_budget())
        );
        assert_eq!(
            wo.config.model.as_deref(),
            Some(ConfigDefaults::default_model())
        );
    }

    #[test]
    fn apply_defaults_does_not_overwrite_existing() {
        let mut wo = WorkOrderBuilder::new("task")
            .max_turns(99)
            .max_budget_usd(42.0)
            .model("custom")
            .build();

        ConfigDefaults::apply_defaults(&mut wo);

        assert_eq!(wo.config.max_turns, Some(99));
        assert_eq!(wo.config.max_budget_usd, Some(42.0));
        assert_eq!(wo.config.model.as_deref(), Some("custom"));
    }

    // -- WorkOrderBuilder defaults ---------------------------------------

    #[test]
    fn work_order_builder_defaults() {
        let wo = WorkOrderBuilder::new("t").build();
        assert_eq!(wo.task, "t");
        assert!(wo.config.model.is_none());
        assert!(wo.config.max_turns.is_none());
        assert!(wo.config.max_budget_usd.is_none());
        assert!(wo.workspace.include.is_empty());
        assert!(wo.workspace.exclude.is_empty());
        assert_eq!(wo.workspace.root, ".");
    }

    // -- ExecutionMode default -------------------------------------------

    #[test]
    fn execution_mode_default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    // -- CONTRACT_VERSION ------------------------------------------------

    #[test]
    fn contract_version_format() {
        assert!(CONTRACT_VERSION.starts_with("abp/v"));
        assert!(CONTRACT_VERSION.contains('.'));
    }

    // -- MatchDecision::is_allowed (via IncludeExcludeGlobs) tested
    //    through abp-glob section below.
}

// ============================================================================
// abp-glob: pattern matching edge cases
// ============================================================================
mod glob_mutations {
    use abp_glob::{IncludeExcludeGlobs, MatchDecision, build_globset};

    fn p(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    // -- empty patterns --------------------------------------------------

    #[test]
    fn no_patterns_allows_everything() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
        assert_eq!(g.decide_str(""), MatchDecision::Allowed);
    }

    #[test]
    fn build_globset_empty_returns_none() {
        assert!(build_globset(&[]).unwrap().is_none());
    }

    #[test]
    fn build_globset_nonempty_returns_some() {
        assert!(build_globset(&p(&["*.rs"])).unwrap().is_some());
    }

    // -- include-only patterns -------------------------------------------

    #[test]
    fn include_only_allows_match() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn include_only_denies_non_match() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(
            g.decide_str("README.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn include_empty_string_denied_when_includes_set() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
    }

    // -- exclude-only patterns -------------------------------------------

    #[test]
    fn exclude_only_denies_match() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["*.log"])).unwrap();
        assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    }

    #[test]
    fn exclude_only_allows_non_match() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["*.log"])).unwrap();
        assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    }

    // -- exclude takes precedence over include ---------------------------

    #[test]
    fn exclude_overrides_include() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/gen/**"])).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("src/gen/out.rs"),
            MatchDecision::DeniedByExclude
        );
    }

    // -- MatchDecision::is_allowed boolean correctness -------------------

    #[test]
    fn is_allowed_true_only_for_allowed() {
        assert!(MatchDecision::Allowed.is_allowed());
        assert!(!MatchDecision::DeniedByExclude.is_allowed());
        assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
    }

    // -- invalid glob returns error, not panic ---------------------------

    #[test]
    fn invalid_glob_returns_error() {
        assert!(IncludeExcludeGlobs::new(&p(&["["]), &[]).is_err());
    }

    // -- single-char and boundary patterns -------------------------------

    #[test]
    fn single_char_pattern() {
        let g = IncludeExcludeGlobs::new(&p(&["?"]), &[]).unwrap();
        assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
    }

    #[test]
    fn exact_filename_pattern() {
        let g = IncludeExcludeGlobs::new(&p(&["Cargo.toml"]), &[]).unwrap();
        assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("Cargo.lock"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn decide_path_and_decide_str_agree() {
        use std::path::Path;
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/secret/**"])).unwrap();
        for c in &["src/lib.rs", "src/secret/x.pem", "README.md"] {
            assert_eq!(
                g.decide_str(c),
                g.decide_path(Path::new(c)),
                "mismatch for {c}"
            );
        }
    }
}

// ============================================================================
// abp-policy: allow/deny decision boundaries, empty policy behavior
// ============================================================================
mod policy_mutations {
    use abp_core::PolicyProfile;
    use abp_policy::PolicyEngine;
    use std::path::Path;

    // -- empty policy permits everything ---------------------------------

    #[test]
    fn empty_policy_allows_all_tools() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert!(e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("anything").allowed);
    }

    #[test]
    fn empty_policy_allows_all_reads() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert!(e.can_read_path(Path::new("any/path.txt")).allowed);
        assert!(e.can_read_path(Path::new("")).allowed);
    }

    #[test]
    fn empty_policy_allows_all_writes() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert!(e.can_write_path(Path::new("any/path.txt")).allowed);
    }

    // -- deny list takes precedence over allow list ----------------------

    #[test]
    fn denylist_overrides_wildcard_allow() {
        let p = PolicyProfile {
            allowed_tools: vec!["*".into()],
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn denylist_reason_contains_tool_name() {
        let p = PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        let d = e.can_use_tool("Bash");
        assert!(!d.allowed);
        assert!(d.reason.as_ref().unwrap().contains("Bash"));
    }

    // -- allowlist blocks unlisted tools ---------------------------------

    #[test]
    fn allowlist_blocks_unlisted() {
        let p = PolicyProfile {
            allowed_tools: vec!["Read".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        assert!(e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn allowlist_missing_reason_contains_not_in() {
        let p = PolicyProfile {
            allowed_tools: vec!["Read".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        let d = e.can_use_tool("Bash");
        assert!(d.reason.as_ref().unwrap().contains("not in allowlist"));
    }

    // -- deny_read / deny_write path checks ------------------------------

    #[test]
    fn deny_read_blocks_matching_path() {
        let p = PolicyProfile {
            deny_read: vec!["**/.env".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new("config/.env")).allowed);
        assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_blocks_matching_path() {
        let p = PolicyProfile {
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_read_reason_contains_denied() {
        let p = PolicyProfile {
            deny_read: vec!["secret*".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        let d = e.can_read_path(Path::new("secret.txt"));
        assert!(d.reason.as_ref().unwrap().contains("denied"));
    }

    #[test]
    fn deny_write_reason_contains_denied() {
        let p = PolicyProfile {
            deny_write: vec!["locked*".into()],
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&p).unwrap();
        let d = e.can_write_path(Path::new("locked.md"));
        assert!(d.reason.as_ref().unwrap().contains("denied"));
    }

    // -- Decision constructors -------------------------------------------

    #[test]
    fn decision_allow_fields() {
        let d = abp_policy::Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn decision_deny_fields() {
        let d = abp_policy::Decision::deny("nope");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("nope"));
    }
}

// ============================================================================
// abp-protocol: envelope parsing, version negotiation, validation
// ============================================================================
mod protocol_mutations {
    use abp_core::{BackendIdentity, CapabilityManifest, ExecutionMode};
    use abp_protocol::validate::EnvelopeValidator;
    use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
    use abp_protocol::*;

    // -- JsonlCodec round-trip -------------------------------------------

    #[test]
    fn encode_ends_with_newline() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn decode_round_trips_hello() {
        let hello = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let line = JsonlCodec::encode(&hello).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn decode_round_trips_fatal() {
        let fatal = Envelope::Fatal {
            ref_id: Some("r1".into()),
            error: "oom".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&fatal).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id.as_deref(), Some("r1"));
                assert_eq!(error, "oom");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn decode_invalid_json_returns_error() {
        assert!(JsonlCodec::decode("not json").is_err());
    }

    #[test]
    fn decode_empty_string_returns_error() {
        assert!(JsonlCodec::decode("").is_err());
    }

    #[test]
    fn decode_missing_discriminator_returns_error() {
        assert!(JsonlCodec::decode(r#"{"hello": true}"#).is_err());
    }

    // -- decode_stream ---------------------------------------------------

    #[test]
    fn decode_stream_skips_blank_lines() {
        use std::io::BufReader;
        let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n";
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 1);
    }

    #[test]
    fn decode_stream_empty_input() {
        use std::io::BufReader;
        let reader = BufReader::new("".as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(envelopes.is_empty());
    }

    // -- Envelope::hello default mode ------------------------------------

    #[test]
    fn hello_default_mode_is_mapped() {
        let h = Envelope::hello(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        match h {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_with_mode_passthrough() {
        let h = Envelope::hello_with_mode(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        match h {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
            _ => panic!("expected Hello"),
        }
    }

    // -- parse_version boundary cases ------------------------------------

    #[test]
    fn parse_version_valid() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
        assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
    }

    #[test]
    fn parse_version_invalid_formats() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("abp/v"), None);
        assert_eq!(parse_version("abp/v1"), None);
        assert_eq!(parse_version("abp/v.1"), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version("abp/vx.y"), None);
    }

    // -- is_compatible_version -------------------------------------------

    #[test]
    fn compatible_same_major() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn incompatible_different_major() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    }

    #[test]
    fn incompatible_invalid_input() {
        assert!(!is_compatible_version("garbage", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "garbage"));
    }

    // -- ProtocolVersion::is_compatible ----------------------------------

    #[test]
    fn protocol_version_compatible_same_major_higher_minor() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        assert!(v01.is_compatible(&v02));
    }

    #[test]
    fn protocol_version_compatible_same() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        assert!(v.is_compatible(&v));
    }

    #[test]
    fn protocol_version_incompatible_lower_minor() {
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        // v02.is_compatible(v01) → false because 1 < 2
        assert!(!v02.is_compatible(&v01));
    }

    #[test]
    fn protocol_version_incompatible_different_major() {
        let a = ProtocolVersion { major: 0, minor: 1 };
        let b = ProtocolVersion { major: 1, minor: 1 };
        assert!(!a.is_compatible(&b));
    }

    // -- VersionRange::contains ------------------------------------------

    #[test]
    fn version_range_contains_boundaries() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 })); // min
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 })); // mid
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 })); // max
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 })); // below
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 })); // above
    }

    // -- VersionRange::is_compatible -------------------------------------

    #[test]
    fn version_range_compatible_same_major() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        };
        assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
        assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
    }

    // -- negotiate_version -----------------------------------------------

    #[test]
    fn negotiate_same_versions() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&v, &v).unwrap();
        assert_eq!(result, v);
    }

    #[test]
    fn negotiate_picks_minimum() {
        let local = ProtocolVersion { major: 0, minor: 2 };
        let remote = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&local, &remote).unwrap();
        assert_eq!(result.minor, 1);
    }

    #[test]
    fn negotiate_incompatible_errors() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 1, minor: 0 };
        assert!(negotiate_version(&local, &remote).is_err());
    }

    // -- EnvelopeValidator -----------------------------------------------

    #[test]
    fn validate_hello_empty_backend_id_is_error() {
        let v = EnvelopeValidator::new();
        let hello = Envelope::Hello {
            contract_version: "abp/v0.1".into(),
            backend: BackendIdentity {
                id: "".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = v.validate(&hello);
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn validate_hello_empty_contract_version_is_error() {
        let v = EnvelopeValidator::new();
        let hello = Envelope::Hello {
            contract_version: "".into(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = v.validate(&hello);
        assert!(!result.valid);
    }

    #[test]
    fn validate_hello_invalid_contract_version_is_error() {
        let v = EnvelopeValidator::new();
        let hello = Envelope::Hello {
            contract_version: "invalid".into(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let result = v.validate(&hello);
        assert!(!result.valid);
    }

    #[test]
    fn validate_valid_hello_is_ok() {
        let v = EnvelopeValidator::new();
        let hello = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: Some("1.0".into()),
                adapter_version: Some("1.0".into()),
            },
            CapabilityManifest::new(),
        );
        let result = v.validate(&hello);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn validate_fatal_empty_error_is_invalid() {
        let v = EnvelopeValidator::new();
        let fatal = Envelope::Fatal {
            ref_id: Some("r1".into()),
            error: "".into(),
            error_code: None,
        };
        let result = v.validate(&fatal);
        assert!(!result.valid);
    }

    #[test]
    fn validate_fatal_missing_ref_id_warns() {
        let v = EnvelopeValidator::new();
        let fatal = Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        };
        let result = v.validate(&fatal);
        assert!(result.valid); // warning, not error
        assert!(!result.warnings.is_empty());
    }

    // -- Sequence validation ---------------------------------------------

    #[test]
    fn validate_empty_sequence() {
        let v = EnvelopeValidator::new();
        let errors = v.validate_sequence(&[]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, validate::SequenceError::MissingHello))
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, validate::SequenceError::MissingTerminal))
        );
    }

    #[test]
    fn validate_sequence_hello_not_first() {
        let v = EnvelopeValidator::new();
        let fatal = Envelope::Fatal {
            ref_id: None,
            error: "x".into(),
            error_code: None,
        };
        let hello = Envelope::hello(
            BackendIdentity {
                id: "t".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let errors = v.validate_sequence(&[fatal, hello]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, validate::SequenceError::HelloNotFirst { .. }))
        );
    }
}
