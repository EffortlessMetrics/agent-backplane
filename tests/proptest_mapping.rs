// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for the mapping and dialect system.

use proptest::prelude::*;

use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingMatrix, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};

// ── Strategies ──────────────────────────────────────────────────────────

/// All dialects covered by `known_rules()`.
const KNOWN_DIALECTS: [Dialect; 4] = [
    Dialect::OpenAi,
    Dialect::Claude,
    Dialect::Gemini,
    Dialect::Codex,
];

const KNOWN_FEATURES: [&str; 4] = [
    features::TOOL_USE,
    features::STREAMING,
    features::THINKING,
    features::IMAGE_INPUT,
];

fn arb_dialect() -> BoxedStrategy<Dialect> {
    prop_oneof![
        Just(Dialect::OpenAi),
        Just(Dialect::Claude),
        Just(Dialect::Gemini),
        Just(Dialect::Codex),
        Just(Dialect::Kimi),
        Just(Dialect::Copilot),
    ]
    .boxed()
}

fn arb_known_dialect() -> BoxedStrategy<Dialect> {
    prop_oneof![
        Just(Dialect::OpenAi),
        Just(Dialect::Claude),
        Just(Dialect::Gemini),
        Just(Dialect::Codex),
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

fn arb_feature_name() -> BoxedStrategy<String> {
    prop_oneof![arb_known_feature(), "[a-z][a-z0-9_]{0,19}".prop_map(|s| s),].boxed()
}

fn arb_fidelity() -> BoxedStrategy<Fidelity> {
    prop_oneof![
        Just(Fidelity::Lossless),
        "[a-zA-Z ]{1,40}".prop_map(|w| Fidelity::LossyLabeled { warning: w }),
        "[a-zA-Z ]{1,40}".prop_map(|r| Fidelity::Unsupported { reason: r }),
    ]
    .boxed()
}

fn arb_mapping_rule() -> BoxedStrategy<MappingRule> {
    (
        arb_dialect(),
        arb_dialect(),
        arb_feature_name(),
        arb_fidelity(),
    )
        .prop_map(|(src, tgt, feature, fidelity)| MappingRule {
            source_dialect: src,
            target_dialect: tgt,
            feature,
            fidelity,
        })
        .boxed()
}

/// Numeric rank for fidelity comparison: Lossless(2) > LossyLabeled(1) > Unsupported(0).
fn fidelity_rank(f: &Fidelity) -> u8 {
    match f {
        Fidelity::Lossless => 2,
        Fidelity::LossyLabeled { .. } => 1,
        Fidelity::Unsupported { .. } => 0,
    }
}

// ── 1. Mapping reflexivity ─────────────────────────────────────────────

proptest! {
    /// Same-dialect mapping is always Lossless for any known feature.
    #[test]
    fn reflexivity_same_dialect_lossless(
        d in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        let rule = reg.lookup(d, d, &f)
            .expect("same-dialect rule must exist for known features");
        prop_assert!(
            rule.fidelity.is_lossless(),
            "{d}->{d} {f} should be Lossless, got {:?}",
            rule.fidelity
        );
    }
}

// ── 2. Mapping symmetry ────────────────────────────────────────────────

proptest! {
    /// If A→B is lossy then B→A must NOT be lossless (symmetry of fidelity
    /// degradation). Unsupported pairs are symmetric by construction.
    #[test]
    fn symmetry_lossy_not_lossless_reverse(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        if let (Some(ab), Some(ba)) = (reg.lookup(a, b, &f), reg.lookup(b, a, &f)) {
            // If A→B is unsupported, B→A must also be unsupported.
            if ab.fidelity.is_unsupported() {
                prop_assert!(
                    ba.fidelity.is_unsupported(),
                    "if {a}->{b} {f} is unsupported, {b}->{a} must also be unsupported"
                );
            }
            // If A→B is lossy (not lossless), B→A should not be better.
            if !ab.fidelity.is_lossless() {
                prop_assert!(
                    fidelity_rank(&ba.fidelity) <= fidelity_rank(&ab.fidelity),
                    "{a}->{b} {f} is {:?} but {b}->{a} is {:?} (unexpectedly better)",
                    ab.fidelity,
                    ba.fidelity,
                );
            }
        }
    }
}

// ── 3. Mapping transitivity ────────────────────────────────────────────

proptest! {
    /// If A→B is lossless and B→C is lossless, then A→C should be lossless.
    #[test]
    fn transitivity_lossless_chain(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        c in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        if let (Some(ab), Some(bc), Some(ac)) = (
            reg.lookup(a, b, &f),
            reg.lookup(b, c, &f),
            reg.lookup(a, c, &f),
        )
            && ab.fidelity.is_lossless()
            && bc.fidelity.is_lossless()
        {
            prop_assert!(
                ac.fidelity.is_lossless(),
                "{a}->{b} lossless + {b}->{c} lossless => {a}->{c} should be lossless, got {:?}",
                ac.fidelity,
            );
        }
    }
}

// ── 4. Matrix consistency ──────────────────────────────────────────────

proptest! {
    /// Every known dialect pair queried against known_rules() returns a
    /// consistent result — if a rule exists, its source/target match the query.
    #[test]
    fn matrix_consistency_rule_fields_match_query(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        if let Some(rule) = reg.lookup(a, b, &f) {
            prop_assert_eq!(rule.source_dialect, a);
            prop_assert_eq!(rule.target_dialect, b);
            prop_assert_eq!(&rule.feature, &f);
        }
    }

    /// Same-dialect pairs always have rules for every known feature.
    #[test]
    fn matrix_consistency_self_pairs_always_defined(
        d in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        prop_assert!(
            reg.lookup(d, d, &f).is_some(),
            "known_rules() must define self-mapping for every known feature"
        );
    }
}

// ── 5. Known rules consistency ─────────────────────────────────────────

proptest! {
    /// Registered rules' fidelity fields match what is_lossless / is_unsupported
    /// report (i.e. helper methods are consistent with the enum variant).
    #[test]
    fn known_rules_helpers_consistent(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        if let Some(rule) = reg.lookup(a, b, &f) {
            match &rule.fidelity {
                Fidelity::Lossless => {
                    prop_assert!(rule.fidelity.is_lossless());
                    prop_assert!(!rule.fidelity.is_unsupported());
                }
                Fidelity::LossyLabeled { .. } => {
                    prop_assert!(!rule.fidelity.is_lossless());
                    prop_assert!(!rule.fidelity.is_unsupported());
                }
                Fidelity::Unsupported { .. } => {
                    prop_assert!(!rule.fidelity.is_lossless());
                    prop_assert!(rule.fidelity.is_unsupported());
                }
            }
        }
    }
}

// ── 6. Fidelity ordering ──────────────────────────────────────────────

proptest! {
    /// Rank ordering: Lossless(2) > LossyLabeled(1) > Unsupported(0).
    #[test]
    fn fidelity_ordering(f in arb_fidelity()) {
        let rank = fidelity_rank(&f);
        match &f {
            Fidelity::Lossless => prop_assert_eq!(rank, 2),
            Fidelity::LossyLabeled { .. } => prop_assert_eq!(rank, 1),
            Fidelity::Unsupported { .. } => prop_assert_eq!(rank, 0),
        }
    }

    /// Lossless always ranks higher than LossyLabeled and Unsupported.
    #[test]
    fn lossless_beats_others(f in arb_fidelity()) {
        if !f.is_lossless() {
            prop_assert!(fidelity_rank(&Fidelity::Lossless) > fidelity_rank(&f));
        }
    }

    /// Unsupported always ranks lower than everything else.
    #[test]
    fn unsupported_is_worst(f in arb_fidelity()) {
        if !f.is_unsupported() {
            let u = Fidelity::Unsupported { reason: "x".into() };
            prop_assert!(fidelity_rank(&f) > fidelity_rank(&u));
        }
    }
}

// ── 7. Round-trip property ─────────────────────────────────────────────

proptest! {
    /// If fidelity(A→B) is Lossless then validate_mapping produces zero errors.
    #[test]
    fn roundtrip_lossless_no_errors(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        f in arb_known_feature(),
    ) {
        let reg = known_rules();
        if let Some(rule) = reg.lookup(a, b, &f)
            && rule.fidelity.is_lossless()
        {
            let results = validate_mapping(&reg, a, b, std::slice::from_ref(&f));
            prop_assert_eq!(results.len(), 1);
            prop_assert!(
                results[0].errors.is_empty(),
                "lossless {a}->{b} {f} should produce no validation errors, got {:?}",
                results[0].errors,
            );
            prop_assert!(results[0].fidelity.is_lossless());
        }
    }

    /// Lossless fidelity roundtrips through serde unchanged.
    #[test]
    fn fidelity_serde_roundtrip(f in arb_fidelity()) {
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(f, f2);
    }
}

// ── 8. Matrix completeness ─────────────────────────────────────────────

#[test]
fn matrix_completeness_self_mappings_and_rule_count() {
    let reg = known_rules();
    let n = KNOWN_DIALECTS.len();
    let f = KNOWN_FEATURES.len();

    // Self-dialect pairs must have all features defined.
    for &d in &KNOWN_DIALECTS {
        for &feat in &KNOWN_FEATURES {
            assert!(
                reg.lookup(d, d, feat).is_some(),
                "missing self-mapping for {d} feature={feat}"
            );
        }
    }

    // The matrix built from the registry should mark same-dialect pairs as
    // supported (they have lossless rules).
    let matrix = MappingMatrix::from_registry(&reg);
    for &d in &KNOWN_DIALECTS {
        assert!(
            matrix.is_supported(d, d),
            "{d}->{d} should be supported in matrix"
        );
    }

    // At minimum N*F self-mapping rules (N dialects × F features).
    assert!(
        reg.len() >= n * f,
        "expected at least {} rules, got {}",
        n * f,
        reg.len()
    );
}

// ── 9. Registry operations ─────────────────────────────────────────────

proptest! {
    /// Inserting a rule and looking it up yields the same fidelity.
    #[test]
    fn registry_insert_lookup_consistent(rule in arb_mapping_rule()) {
        let mut reg = MappingRegistry::new();
        reg.insert(rule.clone());
        let found = reg.lookup(rule.source_dialect, rule.target_dialect, &rule.feature);
        prop_assert!(found.is_some());
        prop_assert_eq!(&found.unwrap().fidelity, &rule.fidelity);
    }

    /// Inserting a second rule for the same key replaces the first.
    #[test]
    fn registry_insert_replaces(
        src in arb_dialect(),
        tgt in arb_dialect(),
        feat in arb_feature_name(),
        f1 in arb_fidelity(),
        f2 in arb_fidelity(),
    ) {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: src,
            target_dialect: tgt,
            feature: feat.clone(),
            fidelity: f1,
        });
        reg.insert(MappingRule {
            source_dialect: src,
            target_dialect: tgt,
            feature: feat.clone(),
            fidelity: f2.clone(),
        });
        prop_assert_eq!(reg.len(), 1);
        let found = reg.lookup(src, tgt, &feat).unwrap();
        prop_assert_eq!(&found.fidelity, &f2);
    }

    /// Lookup on a key never inserted returns None.
    #[test]
    fn registry_lookup_miss(
        src in arb_dialect(),
        tgt in arb_dialect(),
        feat in arb_feature_name(),
    ) {
        let reg = MappingRegistry::new();
        prop_assert!(reg.lookup(src, tgt, &feat).is_none());
    }

    /// Registry length equals number of distinct keys inserted.
    #[test]
    fn registry_len_matches_distinct_keys(rules in prop::collection::vec(arb_mapping_rule(), 0..30)) {
        let mut reg = MappingRegistry::new();
        let mut unique_keys = std::collections::HashSet::new();
        for rule in &rules {
            unique_keys.insert((rule.source_dialect, rule.target_dialect, rule.feature.clone()));
            reg.insert(rule.clone());
        }
        prop_assert_eq!(reg.len(), unique_keys.len());
    }

    /// Iterator yields exactly len() items.
    #[test]
    fn registry_iter_count(rules in prop::collection::vec(arb_mapping_rule(), 0..20)) {
        let mut reg = MappingRegistry::new();
        for rule in &rules {
            reg.insert(rule.clone());
        }
        prop_assert_eq!(reg.iter().count(), reg.len());
    }
}

// ── 10. Feature coverage ───────────────────────────────────────────────

#[test]
fn feature_coverage_self_mappings_all_features() {
    let reg = known_rules();
    // Self-dialect pairs must cover every known feature.
    for &d in &KNOWN_DIALECTS {
        for &f in &KNOWN_FEATURES {
            let rule = reg.lookup(d, d, f);
            assert!(
                rule.is_some(),
                "no self-mapping rule for feature={f} dialect={d}"
            );
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "self-mapping for feature={f} dialect={d} should be lossless"
            );
        }
    }
}

#[test]
fn feature_coverage_lossless_pairs_are_bidirectional() {
    let reg = known_rules();
    // For every registered lossless cross-dialect rule, the reverse should also exist.
    for rule in reg.iter() {
        if rule.source_dialect != rule.target_dialect && rule.fidelity.is_lossless() {
            let reverse = reg.lookup(rule.target_dialect, rule.source_dialect, &rule.feature);
            assert!(
                reverse.is_some(),
                "lossless {}->{} feature={} has no reverse rule",
                rule.source_dialect,
                rule.target_dialect,
                rule.feature,
            );
            assert!(
                reverse.unwrap().fidelity.is_lossless(),
                "lossless {}->{} feature={} reverse should also be lossless",
                rule.source_dialect,
                rule.target_dialect,
                rule.feature,
            );
        }
    }
}

proptest! {
    /// For any known dialect pair, validate_mapping returns one result per
    /// feature requested.
    #[test]
    fn validate_returns_one_result_per_feature(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        feats in prop::collection::vec(arb_known_feature(), 1..8),
    ) {
        let reg = known_rules();
        let results = validate_mapping(&reg, a, b, &feats);
        prop_assert_eq!(results.len(), feats.len());
    }

    /// Validate with an unknown feature always yields Unsupported.
    #[test]
    fn validate_unknown_feature_unsupported(
        a in arb_known_dialect(),
        b in arb_known_dialect(),
        feat in "[a-z]{6,10}_unknown",
    ) {
        let reg = known_rules();
        let results = validate_mapping(&reg, a, b, &[feat]);
        prop_assert_eq!(results.len(), 1);
        prop_assert!(results[0].fidelity.is_unsupported());
    }
}

// ── Additional property: Matrix from_registry ──────────────────────────

proptest! {
    /// Matrix.from_registry marks a pair as supported iff at least one
    /// non-unsupported rule exists for that pair.
    #[test]
    fn matrix_from_registry_supported_iff_non_unsupported(
        rules in prop::collection::vec(arb_mapping_rule(), 1..20),
    ) {
        let mut reg = MappingRegistry::new();
        for rule in &rules {
            reg.insert(rule.clone());
        }
        let matrix = MappingMatrix::from_registry(&reg);

        // Collect which pairs have at least one non-unsupported rule.
        let mut expected = std::collections::HashSet::new();
        for rule in reg.iter() {
            if !rule.fidelity.is_unsupported() {
                expected.insert((rule.source_dialect, rule.target_dialect));
            }
        }

        for &a in Dialect::all() {
            for &b in Dialect::all() {
                let supported = matrix.is_supported(a, b);
                let should_be = expected.contains(&(a, b));
                prop_assert_eq!(
                    supported, should_be,
                    "matrix.is_supported mismatch"
                );
            }
        }
    }
}

// ── Additional property: MappingRule serde roundtrip ────────────────────

proptest! {
    #[test]
    fn mapping_rule_serde_roundtrip(rule in arb_mapping_rule()) {
        let json = serde_json::to_string(&rule).unwrap();
        let rule2: MappingRule = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(rule, rule2);
    }
}
