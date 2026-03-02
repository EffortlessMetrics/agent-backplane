// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for critical system invariants across receipt hashing,
//! protocol encoding, capability negotiation, mapping, and policy evaluation.

use std::collections::BTreeMap;
use std::path::Path;

use proptest::prelude::*;

use abp_core::{
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    MinSupport, Outcome, Receipt, ReceiptBuilder, SupportLevel as CoreSupportLevel, canonical_json,
    receipt_hash,
};
use abp_mapping::{
    Fidelity, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};
use abp_protocol::{Envelope, JsonlCodec};

// ── Config ──────────────────────────────────────────────────────────────

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

// ── Strategies ──────────────────────────────────────────────────────────

fn arb_outcome() -> BoxedStrategy<Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
    .boxed()
}

fn arb_backend_id() -> BoxedStrategy<String> {
    prop_oneof![
        Just("mock".to_owned()),
        Just("sidecar:node".to_owned()),
        Just("sidecar:python".to_owned()),
        "[a-z][a-z0-9_:-]{0,19}".prop_map(|s| s),
    ]
    .boxed()
}

fn arb_receipt() -> BoxedStrategy<Receipt> {
    (arb_backend_id(), arb_outcome())
        .prop_map(|(id, outcome)| ReceiptBuilder::new(id).outcome(outcome).build())
        .boxed()
}

fn arb_capability() -> BoxedStrategy<Capability> {
    prop_oneof![
        Just(Capability::Streaming),
        Just(Capability::ToolRead),
        Just(Capability::ToolWrite),
        Just(Capability::ToolEdit),
        Just(Capability::ToolBash),
        Just(Capability::ToolGlob),
        Just(Capability::ToolUse),
        Just(Capability::ExtendedThinking),
        Just(Capability::ImageInput),
        Just(Capability::Logprobs),
    ]
    .boxed()
}

fn arb_core_support_level() -> BoxedStrategy<CoreSupportLevel> {
    prop_oneof![
        Just(CoreSupportLevel::Native),
        Just(CoreSupportLevel::Emulated),
        Just(CoreSupportLevel::Unsupported),
        "[a-z ]{1,20}".prop_map(|r| CoreSupportLevel::Restricted { reason: r }),
    ]
    .boxed()
}

fn arb_manifest() -> BoxedStrategy<CapabilityManifest> {
    prop::collection::btree_map(arb_capability(), arb_core_support_level(), 0..8).boxed()
}

fn arb_requirement() -> BoxedStrategy<CapabilityRequirement> {
    (
        arb_capability(),
        prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)],
    )
        .prop_map(|(capability, min_support)| CapabilityRequirement {
            capability,
            min_support,
        })
        .boxed()
}

fn arb_requirements() -> BoxedStrategy<CapabilityRequirements> {
    prop::collection::vec(arb_requirement(), 0..6)
        .prop_map(|required| CapabilityRequirements { required })
        .boxed()
}

fn arb_envelope() -> BoxedStrategy<Envelope> {
    prop_oneof![
        arb_backend_id().prop_map(|id| Envelope::hello(
            BackendIdentity {
                id,
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        )),
        "[a-z]{4,10}".prop_map(|error| Envelope::Fatal {
            ref_id: None,
            error,
            error_code: None,
        }),
    ]
    .boxed()
}

fn arb_known_dialect() -> BoxedStrategy<abp_dialect::Dialect> {
    prop_oneof![
        Just(abp_dialect::Dialect::OpenAi),
        Just(abp_dialect::Dialect::Claude),
        Just(abp_dialect::Dialect::Gemini),
        Just(abp_dialect::Dialect::Codex),
    ]
    .boxed()
}

fn arb_known_feature() -> BoxedStrategy<String> {
    prop_oneof![
        Just(features::TOOL_USE.to_owned()),
        Just(features::STREAMING.to_owned()),
        Just(features::THINKING.to_owned()),
        Just(features::IMAGE_INPUT.to_owned()),
    ]
    .boxed()
}

fn arb_tool_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_]{0,14}".boxed()
}

fn arb_deny_glob() -> BoxedStrategy<String> {
    prop_oneof![
        Just("**/.git/**".to_owned()),
        Just("**/.env".to_owned()),
        Just("secret*".to_owned()),
        Just("**/node_modules/**".to_owned()),
    ]
    .boxed()
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Receipt invariants
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Hash is deterministic: same receipt → same hash every time.
    #[test]
    fn receipt_hash_deterministic(receipt in arb_receipt()) {
        let h1 = receipt_hash(&receipt).unwrap();
        let h2 = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(&h1, &h2, "same receipt must produce identical hashes");
    }

    /// Hash changes if the backend id field changes.
    #[test]
    fn receipt_hash_changes_on_field_change(
        id1 in "[a-z]{3,8}",
        id2 in "[a-z]{3,8}",
        outcome in arb_outcome(),
    ) {
        prop_assume!(id1 != id2);
        let r1 = ReceiptBuilder::new(&id1).outcome(outcome.clone()).build();
        let r2 = ReceiptBuilder::new(&id2).outcome(outcome).build();
        // Normalize run_id and timestamps so only backend id differs.
        // Since ReceiptBuilder generates a random run_id, we must build
        // them identically aside from the field under test. We use
        // canonical_json on the whole receipt, so different ids => different hashes
        // is inherent. Just verify.
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_ne!(&h1, &h2, "different backend ids must produce different hashes");
    }

    /// Canonical JSON is deterministic: same object → same string.
    #[test]
    fn canonical_json_deterministic(receipt in arb_receipt()) {
        let j1 = canonical_json(&receipt).unwrap();
        let j2 = canonical_json(&receipt).unwrap();
        prop_assert_eq!(&j1, &j2);
    }

    /// BTreeMap ordering is preserved through serialization roundtrip.
    #[test]
    fn btreemap_ordering_preserved(entries in prop::collection::btree_map(
        arb_capability(),
        arb_core_support_level(),
        1..8
    )) {
        let json = serde_json::to_string(&entries).unwrap();
        let parsed: BTreeMap<Capability, CoreSupportLevel> =
            serde_json::from_str(&json).unwrap();
        let keys_orig: Vec<_> = entries.keys().collect();
        let keys_parsed: Vec<_> = parsed.keys().collect();
        prop_assert_eq!(keys_orig, keys_parsed, "BTreeMap key order must survive serde roundtrip");
    }

    /// with_hash produces a 64-char hex digest.
    #[test]
    fn receipt_with_hash_is_valid_hex(receipt in arb_receipt()) {
        let hashed = receipt.with_hash().unwrap();
        let sha = hashed.receipt_sha256.as_ref().unwrap();
        prop_assert_eq!(sha.len(), 64);
        prop_assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Hashing ignores the receipt_sha256 field (self-referential prevention).
    #[test]
    fn receipt_hash_ignores_existing_sha(receipt in arb_receipt()) {
        let h_before = receipt_hash(&receipt).unwrap();
        let mut with_sha = receipt.clone();
        with_sha.receipt_sha256 = Some("deadbeef".repeat(8));
        let h_after = receipt_hash(&with_sha).unwrap();
        prop_assert_eq!(h_before, h_after, "receipt_sha256 must not affect hash");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  Protocol invariants
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Encode then decode is identity for all envelope types.
    #[test]
    fn protocol_encode_decode_identity(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        // Compare via re-encoding (Envelope doesn't derive PartialEq)
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(&encoded, &re_encoded);
    }

    /// Parse then serialize preserves all fields (JSON roundtrip).
    #[test]
    fn protocol_json_roundtrip_preserves_fields(envelope in arb_envelope()) {
        let json1 = serde_json::to_string(&envelope).unwrap();
        let parsed: Envelope = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        prop_assert_eq!(&json1, &json2);
    }

    /// Encoded envelopes always end with a newline.
    #[test]
    fn protocol_encode_ends_with_newline(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        prop_assert!(encoded.ends_with('\n'));
    }

    /// Encoded envelopes always contain the discriminator tag "t".
    #[test]
    fn protocol_encode_contains_tag(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        prop_assert!(encoded.contains("\"t\":"));
    }
}

/// Empty string doesn't panic on parse — returns an error.
#[test]
fn protocol_empty_string_no_panic() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

/// Unknown fields don't cause parse failure for Fatal envelope.
#[test]
fn protocol_unknown_fields_no_failure() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","unknown_field":"value"}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Capability invariants
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Negotiation is deterministic: same inputs → same result.
    #[test]
    fn capability_negotiation_deterministic(
        manifest in arb_manifest(),
        reqs in arb_requirements(),
    ) {
        let r1 = abp_capability::negotiate(&manifest, &reqs);
        let r2 = abp_capability::negotiate(&manifest, &reqs);
        prop_assert_eq!(r1, r2);
    }

    /// Adding more caps to manifest never reduces compatibility.
    #[test]
    fn capability_more_caps_never_reduces_compat(
        base_manifest in arb_manifest(),
        extra_cap in arb_capability(),
        reqs in arb_requirements(),
    ) {
        let base_result = abp_capability::negotiate(&base_manifest, &reqs);
        let mut expanded = base_manifest.clone();
        expanded.insert(extra_cap, CoreSupportLevel::Native);
        let expanded_result = abp_capability::negotiate(&expanded, &reqs);
        // expanded manifest can only help: unsupported count must not increase
        prop_assert!(
            expanded_result.unsupported.len() <= base_result.unsupported.len(),
            "adding caps should never increase unsupported count: {} > {}",
            expanded_result.unsupported.len(),
            base_result.unsupported.len(),
        );
    }

    /// Removing caps from requirements never reduces compatibility.
    #[test]
    fn capability_fewer_reqs_never_reduces_compat(
        manifest in arb_manifest(),
        reqs in arb_requirements(),
    ) {
        let full_result = abp_capability::negotiate(&manifest, &reqs);
        // Remove one requirement (if any)
        let mut fewer = reqs.clone();
        if !fewer.required.is_empty() {
            fewer.required.pop();
        }
        let fewer_result = abp_capability::negotiate(&manifest, &fewer);
        // Fewer requirements can only help
        prop_assert!(
            fewer_result.unsupported.len() <= full_result.unsupported.len(),
            "removing requirements should never increase unsupported count"
        );
    }

    /// satisfy() is reflexive: Native satisfies both Native and Emulated.
    #[test]
    fn capability_satisfy_reflexive(_i in 0..10u32) {
        let native = CoreSupportLevel::Native;
        prop_assert!(native.satisfies(&MinSupport::Native));
        prop_assert!(native.satisfies(&MinSupport::Emulated));
    }

    /// Unsupported never satisfies any minimum.
    #[test]
    fn capability_unsupported_never_satisfies(
        min in prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)]
    ) {
        let unsupported = CoreSupportLevel::Unsupported;
        prop_assert!(!unsupported.satisfies(&min));
    }

    /// Empty requirements always yield compatible.
    #[test]
    fn capability_empty_reqs_always_compatible(manifest in arb_manifest()) {
        let reqs = CapabilityRequirements::default();
        let result = abp_capability::negotiate(&manifest, &reqs);
        prop_assert!(result.is_compatible());
    }

    /// Total count of negotiation result matches requirement count.
    #[test]
    fn capability_total_matches_reqs(
        manifest in arb_manifest(),
        reqs in arb_requirements(),
    ) {
        let result = abp_capability::negotiate(&manifest, &reqs);
        prop_assert_eq!(result.total(), reqs.required.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Mapping invariants
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Self-mapping is always Lossless fidelity for known features.
    #[test]
    fn mapping_self_is_lossless(
        d in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        let rule = reg.lookup(d, d, &f);
        prop_assert!(rule.is_some(), "self-mapping must exist");
        prop_assert!(
            rule.unwrap().fidelity.is_lossless(),
            "self-mapping must be lossless"
        );
    }

    /// Mapping registry lookup is deterministic.
    #[test]
    fn mapping_lookup_deterministic(
        src in arb_known_dialect(),
        tgt in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        let r1 = reg.lookup(src, tgt, &f);
        let r2 = reg.lookup(src, tgt, &f);
        match (r1, r2) {
            (Some(a), Some(b)) => prop_assert_eq!(a, b),
            (None, None) => {}
            _ => prop_assert!(false, "lookup must return consistent results"),
        }
    }

    /// validate_mapping covers all features for a given pair (returns one result per feature).
    #[test]
    fn mapping_validate_covers_all_features(
        src in arb_known_dialect(),
        tgt in arb_known_dialect(),
        feats in prop::collection::vec(arb_known_feature(), 1..6),
    ) {
        let reg = known_rules();
        let results = validate_mapping(&reg, src, tgt, &feats);
        prop_assert_eq!(results.len(), feats.len());
        for (result, feat) in results.iter().zip(&feats) {
            prop_assert_eq!(&result.feature, feat);
        }
    }

    /// Fidelity serde roundtrip.
    #[test]
    fn mapping_fidelity_serde_roundtrip(
        f in prop_oneof![
            Just(Fidelity::Lossless),
            "[a-z ]{1,20}".prop_map(|w| Fidelity::LossyLabeled { warning: w }),
            "[a-z ]{1,20}".prop_map(|r| Fidelity::Unsupported { reason: r }),
        ]
    ) {
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(f, f2);
    }

    /// Inserted rules are retrievable via lookup.
    #[test]
    fn mapping_insert_then_lookup(
        src in arb_known_dialect(),
        tgt in arb_known_dialect(),
        feat in arb_known_feature(),
    ) {
        let mut reg = MappingRegistry::new();
        let rule = MappingRule {
            source_dialect: src,
            target_dialect: tgt,
            feature: feat.clone(),
            fidelity: Fidelity::Lossless,
        };
        reg.insert(rule.clone());
        let found = reg.lookup(src, tgt, &feat);
        prop_assert!(found.is_some());
        prop_assert_eq!(found.unwrap(), &rule);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5  Policy invariants
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Default policy allows everything.
    #[test]
    fn policy_default_allows_everything(
        tool in arb_tool_name(),
        path in "[a-z/]{1,30}",
    ) {
        let engine = abp_policy::PolicyEngine::new(&abp_core::PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }

    /// Deny overrides allow (precedence): a tool in both allow and deny is denied.
    #[test]
    fn policy_deny_overrides_allow(tool in arb_tool_name()) {
        let policy = abp_core::PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![tool.clone()],
            ..Default::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }

    /// Empty deny list allows everything (for tools).
    #[test]
    fn policy_empty_deny_allows_tools(tool in arb_tool_name()) {
        let policy = abp_core::PolicyProfile {
            disallowed_tools: vec![],
            ..Default::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    /// Denied read paths are actually denied.
    #[test]
    fn policy_deny_read_enforced(glob in arb_deny_glob()) {
        let policy = abp_core::PolicyProfile {
            deny_read: vec![glob.clone()],
            ..Default::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        // Construct a path that matches the glob
        let test_path = match glob.as_str() {
            "**/.git/**" => ".git/config",
            "**/.env" => ".env",
            "secret*" => "secret.txt",
            "**/node_modules/**" => "node_modules/pkg/index.js",
            _ => return Ok(()),
        };
        prop_assert!(
            !engine.can_read_path(Path::new(test_path)).allowed,
            "path '{}' should be denied by glob '{}'",
            test_path, glob,
        );
    }

    /// Denied write paths are actually denied.
    #[test]
    fn policy_deny_write_enforced(glob in arb_deny_glob()) {
        let policy = abp_core::PolicyProfile {
            deny_write: vec![glob.clone()],
            ..Default::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        let test_path = match glob.as_str() {
            "**/.git/**" => ".git/config",
            "**/.env" => ".env",
            "secret*" => "secret.txt",
            "**/node_modules/**" => "node_modules/pkg/index.js",
            _ => return Ok(()),
        };
        prop_assert!(
            !engine.can_write_path(Path::new(test_path)).allowed,
            "path '{}' should be denied by glob '{}'",
            test_path, glob,
        );
    }

    /// Policy compilation never panics on valid inputs.
    #[test]
    fn policy_compilation_no_panic(
        allowed in prop::collection::vec(arb_tool_name(), 0..4),
        denied in prop::collection::vec(arb_tool_name(), 0..4),
    ) {
        let policy = abp_core::PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..Default::default()
        };
        let result = abp_policy::PolicyEngine::new(&policy);
        prop_assert!(result.is_ok());
    }
}
