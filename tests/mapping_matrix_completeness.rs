// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests verifying the mapping matrix (dialect × dialect) is complete
//! for all SDK pairs, with correct fidelity annotations for every cell.

use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingMatrix, MappingRule, features, known_rules, validate_mapping};

// ── Helpers ─────────────────────────────────────────────────────────────

/// All well-known features that should be present in every dialect pair.
const ALL_FEATURES: &[&str] = &[
    features::TOOL_USE,
    features::STREAMING,
    features::THINKING,
    features::IMAGE_INPUT,
    features::CODE_EXEC,
];

// ── 1. Complete coverage ────────────────────────────────────────────────

#[test]
fn all_cross_dialect_pairs_have_rules_for_every_feature() {
    let reg = known_rules();
    let dialects = Dialect::all();
    let mut missing = Vec::new();

    for &src in dialects {
        for &tgt in dialects {
            if src == tgt {
                continue;
            }
            for &feat in ALL_FEATURES {
                if reg.lookup(src, tgt, feat).is_none() {
                    missing.push(format!("{src} -> {tgt} [{feat}]"));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Missing mapping rules for {} pairs:\n  {}",
        missing.len(),
        missing.join("\n  "),
    );
}

#[test]
fn matrix_has_no_undefined_cells() {
    let reg = known_rules();
    let dialects = Dialect::all();

    for &src in dialects {
        for &tgt in dialects {
            for &feat in ALL_FEATURES {
                assert!(
                    reg.lookup(src, tgt, feat).is_some(),
                    "Undefined cell: {src} -> {tgt} [{feat}]",
                );
            }
        }
    }
}

// ── 2. All rules have a Fidelity level ──────────────────────────────────

#[test]
fn all_rules_have_explicit_fidelity() {
    let reg = known_rules();
    for rule in reg.iter() {
        match &rule.fidelity {
            Fidelity::Lossless => {}
            Fidelity::LossyLabeled { warning } => {
                assert!(
                    !warning.is_empty(),
                    "LossyLabeled rule {}->{} [{}] has empty warning",
                    rule.source_dialect,
                    rule.target_dialect,
                    rule.feature,
                );
            }
            Fidelity::Unsupported { reason } => {
                assert!(
                    !reason.is_empty(),
                    "Unsupported rule {}->{} [{}] has empty reason",
                    rule.source_dialect,
                    rule.target_dialect,
                    rule.feature,
                );
            }
        }
    }
}

// ── 3. Self-mapping is always Lossless ──────────────────────────────────

#[test]
fn self_mapping_always_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        for &feat in ALL_FEATURES {
            let rule = reg
                .lookup(d, d, feat)
                .unwrap_or_else(|| panic!("Missing self-mapping for {d} [{feat}]"));
            assert!(
                rule.fidelity.is_lossless(),
                "Self-mapping {d} -> {d} [{feat}] should be Lossless, got {:?}",
                rule.fidelity,
            );
        }
    }
}

// ── 4. Mapping symmetry ────────────────────────────────────────────────

#[test]
fn mapping_symmetry_fidelity_defined_both_ways() {
    let reg = known_rules();
    let dialects = Dialect::all();

    for &a in dialects {
        for &b in dialects {
            if a == b {
                continue;
            }
            for &feat in ALL_FEATURES {
                let ab = reg.lookup(a, b, feat);
                let ba = reg.lookup(b, a, feat);
                assert!(
                    ab.is_some() && ba.is_some(),
                    "Asymmetric rule: {a}->{b} [{feat}] has rule={}, {b}->{a} has rule={}",
                    ab.is_some(),
                    ba.is_some(),
                );
            }
        }
    }
}

#[test]
fn lossy_mapping_reverse_also_defined() {
    let reg = known_rules();

    for rule in reg.iter() {
        if matches!(rule.fidelity, Fidelity::LossyLabeled { .. }) {
            let reverse = reg.lookup(rule.target_dialect, rule.source_dialect, &rule.feature);
            assert!(
                reverse.is_some(),
                "Lossy mapping {}->{} [{}] has no reverse rule",
                rule.source_dialect,
                rule.target_dialect,
                rule.feature,
            );
        }
    }
}

// ── 5. Known lossy mappings have documented reasons ─────────────────────

#[test]
fn lossy_mappings_have_nonempty_warnings() {
    let reg = known_rules();
    for rule in reg.iter() {
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(
                !warning.trim().is_empty(),
                "Lossy rule {}->{} [{}] has blank warning",
                rule.source_dialect,
                rule.target_dialect,
                rule.feature,
            );
        }
    }
}

// ── 6. Per-feature analysis ─────────────────────────────────────────────

#[test]
fn feature_streaming_all_cross_dialect_lossless() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let rule = reg.lookup(src, tgt, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {src} -> {tgt} should be Lossless",
            );
        }
    }
}

#[test]
fn feature_tool_use_fidelity_breakdown() {
    let reg = known_rules();
    let dialects = Dialect::all();
    let mut lossless_pairs = Vec::new();
    let mut lossy_pairs = Vec::new();
    let mut unsupported_pairs = Vec::new();

    for &src in dialects {
        for &tgt in dialects {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::TOOL_USE).unwrap();
            match &rule.fidelity {
                Fidelity::Lossless => lossless_pairs.push((src, tgt)),
                Fidelity::LossyLabeled { .. } => lossy_pairs.push((src, tgt)),
                Fidelity::Unsupported { .. } => unsupported_pairs.push((src, tgt)),
            }
        }
    }

    // All 6 dialects support tool_use, so no unsupported pairs.
    assert!(
        unsupported_pairs.is_empty(),
        "Unexpected unsupported tool_use pairs: {unsupported_pairs:?}",
    );
    // Codex pairs are lossy; others are lossless.
    assert!(
        !lossy_pairs.is_empty(),
        "Expected some lossy tool_use pairs involving Codex",
    );
    assert!(
        !lossless_pairs.is_empty(),
        "Expected some lossless tool_use pairs",
    );
}

#[test]
fn feature_thinking_cross_dialect_all_lossy() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::THINKING).unwrap();
            assert!(
                matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
                "thinking {src} -> {tgt} should be Lossy, got {:?}",
                rule.fidelity,
            );
        }
    }
}

#[test]
fn feature_image_input_fidelity_breakdown() {
    let reg = known_rules();
    let dialects = Dialect::all();
    let mut lossless_pairs = Vec::new();
    let mut unsupported_pairs = Vec::new();

    for &src in dialects {
        for &tgt in dialects {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::IMAGE_INPUT).unwrap();
            match &rule.fidelity {
                Fidelity::Lossless => lossless_pairs.push((src, tgt)),
                Fidelity::Unsupported { .. } => unsupported_pairs.push((src, tgt)),
                Fidelity::LossyLabeled { .. } => {}
            }
        }
    }

    // OpenAI, Claude, Gemini support images; Codex, Kimi, Copilot do not.
    assert!(
        !lossless_pairs.is_empty(),
        "Expected lossless image pairs among OpenAI/Claude/Gemini",
    );
    assert!(
        !unsupported_pairs.is_empty(),
        "Expected unsupported image pairs involving Codex/Kimi/Copilot",
    );
    // Verify the three image-capable dialects have lossless pairs with each other.
    for &(src, tgt) in &lossless_pairs {
        assert!(
            matches!(src, Dialect::OpenAi | Dialect::Claude | Dialect::Gemini),
            "Unexpected lossless image source: {src}",
        );
        assert!(
            matches!(tgt, Dialect::OpenAi | Dialect::Claude | Dialect::Gemini),
            "Unexpected lossless image target: {tgt}",
        );
    }
}

#[test]
fn feature_code_exec_fidelity_breakdown() {
    let reg = known_rules();
    let dialects = Dialect::all();
    let mut lossy_pairs = Vec::new();
    let mut unsupported_pairs = Vec::new();

    for &src in dialects {
        for &tgt in dialects {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::CODE_EXEC).unwrap();
            match &rule.fidelity {
                Fidelity::Lossless => {
                    panic!("Unexpected lossless code_exec: {src} -> {tgt}");
                }
                Fidelity::LossyLabeled { .. } => lossy_pairs.push((src, tgt)),
                Fidelity::Unsupported { .. } => unsupported_pairs.push((src, tgt)),
            }
        }
    }

    // Kimi is the only dialect that can't do code_exec.
    for &(src, tgt) in &unsupported_pairs {
        assert!(
            src == Dialect::Kimi || tgt == Dialect::Kimi,
            "Unexpected unsupported code_exec pair not involving Kimi: {src} -> {tgt}",
        );
    }
    assert!(
        !lossy_pairs.is_empty(),
        "Expected lossy code_exec pairs among capable dialects",
    );
}

// ── 7. Best backend selection ───────────────────────────────────────────

#[test]
fn rank_targets_returns_best_backend() {
    let reg = known_rules();
    let features = &[
        features::TOOL_USE,
        features::STREAMING,
        features::IMAGE_INPUT,
    ];

    // From Claude, rank targets by lossless feature count.
    let ranked = reg.rank_targets(Dialect::Claude, features);

    assert!(
        !ranked.is_empty(),
        "rank_targets should return at least one target",
    );

    // The best target should have the most lossless features.
    let (best, best_lossless) = ranked[0];
    assert!(
        best_lossless > 0,
        "Best target for Claude should have at least 1 lossless feature",
    );

    // OpenAI and Gemini should rank high (all 3 features lossless).
    let top_dialects: Vec<Dialect> = ranked
        .iter()
        .filter(|(_, count)| *count == best_lossless)
        .map(|(d, _)| *d)
        .collect();
    assert!(
        top_dialects.contains(&Dialect::OpenAi) || top_dialects.contains(&Dialect::Gemini),
        "Expected OpenAI or Gemini among top targets for Claude, got {top_dialects:?}",
    );

    // Codex should rank lower due to unsupported image_input.
    let codex_rank = ranked.iter().position(|(d, _)| *d == Dialect::Codex);
    if let (Some(best_pos), Some(codex_pos)) =
        (ranked.iter().position(|(d, _)| *d == best), codex_rank)
    {
        assert!(
            codex_pos >= best_pos,
            "Codex should not outrank {best} for Claude",
        );
    }
}

#[test]
fn rank_targets_excludes_self() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    let dialects: Vec<Dialect> = ranked.iter().map(|(d, _)| *d).collect();
    assert!(
        !dialects.contains(&Dialect::OpenAi),
        "rank_targets should not include source dialect",
    );
}

// ── 8. Matrix is serializable ───────────────────────────────────────────

#[test]
fn mapping_rule_serde_roundtrip() {
    let reg = known_rules();
    let rules: Vec<&MappingRule> = reg.iter().collect();
    assert!(!rules.is_empty());

    for rule in rules {
        let json = serde_json::to_string(rule).unwrap();
        let deserialized: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule.source_dialect, deserialized.source_dialect);
        assert_eq!(rule.target_dialect, deserialized.target_dialect);
        assert_eq!(rule.feature, deserialized.feature);
        assert_eq!(rule.fidelity, deserialized.fidelity);
    }
}

#[test]
fn fidelity_serde_roundtrip_all_variants() {
    let variants = [
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "test warning".into(),
        },
        Fidelity::Unsupported {
            reason: "test reason".into(),
        },
    ];
    for f in &variants {
        let json = serde_json::to_string(f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, &f2);
    }
}

#[test]
fn matrix_serializable_from_registry() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);

    // Matrix should mark supported pairs as true.
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(matrix.is_supported(Dialect::Claude, Dialect::Gemini));

    // Verify matrix can be built and queried for all pairs.
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            // get() should return Some for any pair that has at least one
            // non-unsupported rule.
            let _supported = matrix.is_supported(src, tgt);
        }
    }
}

// ── 9. Validate known rules match IR conversion behavior ────────────────

#[test]
fn validate_mapping_produces_results_for_all_features() {
    let reg = known_rules();
    let features_list: Vec<String> = ALL_FEATURES.iter().map(|s| s.to_string()).collect();

    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            let results = validate_mapping(&reg, src, tgt, &features_list);
            assert_eq!(
                results.len(),
                ALL_FEATURES.len(),
                "validate_mapping {src}->{tgt} should return {} results",
                ALL_FEATURES.len(),
            );
        }
    }
}

// ── 10. Specific known-case fidelity assertions ─────────────────────────

#[test]
fn known_fidelity_openai_claude_tool_use() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .expect("OpenAI->Claude tool_use should be defined");
    // OpenAI and Claude tool calling formats differ but the IR normalizes them.
    assert!(
        !rule.fidelity.is_unsupported(),
        "OpenAI->Claude tool_use should not be unsupported",
    );
}

#[test]
fn known_fidelity_claude_gemini_thinking_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
        .expect("Claude->Gemini thinking should be defined");
    assert!(
        matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
        "Claude->Gemini thinking should be Lossy, got {:?}",
        rule.fidelity,
    );
}

#[test]
fn known_fidelity_gemini_openai_image_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Gemini, Dialect::OpenAi, features::IMAGE_INPUT)
        .expect("Gemini->OpenAI image_input should be defined");
    assert!(
        rule.fidelity.is_lossless(),
        "Gemini->OpenAI image_input should be Lossless, got {:?}",
        rule.fidelity,
    );
}

#[test]
fn known_fidelity_claude_codex_image_unsupported() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Codex, features::IMAGE_INPUT)
        .expect("Claude->Codex image_input should be defined");
    assert!(
        rule.fidelity.is_unsupported(),
        "Claude->Codex image_input should be Unsupported, got {:?}",
        rule.fidelity,
    );
}

#[test]
fn known_fidelity_kimi_openai_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::OpenAi, features::STREAMING)
        .expect("Kimi->OpenAI streaming should be defined");
    assert!(
        rule.fidelity.is_lossless(),
        "Kimi->OpenAI streaming should be Lossless (both SSE-based)",
    );
}

#[test]
fn known_fidelity_copilot_kimi_code_exec_unsupported() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::Kimi, features::CODE_EXEC)
        .expect("Copilot->Kimi code_exec should be defined");
    assert!(
        rule.fidelity.is_unsupported(),
        "Copilot->Kimi code_exec should be Unsupported (Kimi has no code exec)",
    );
}

// ── 11. Total rule count sanity check ───────────────────────────────────

#[test]
fn total_rule_count_matches_expected() {
    let reg = known_rules();
    let n_dialects = Dialect::all().len(); // 6
    let n_features = ALL_FEATURES.len(); // 5
    // Total: n_dialects * n_dialects * n_features (self + cross)
    let expected = n_dialects * n_dialects * n_features;
    assert_eq!(
        reg.len(),
        expected,
        "Expected {expected} rules ({}×{}×{}), got {}",
        n_dialects,
        n_dialects,
        n_features,
        reg.len(),
    );
}
