// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the capability system: Capability enum, SupportLevel, MinSupport,
//! CapabilityManifest, and CapabilityRequirements.

use abp_core::*;
use serde_json::json;
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────

fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ]
}

/// Expected snake_case wire names for every Capability variant.
fn expected_wire_names() -> Vec<(&'static str, Capability)> {
    vec![
        ("streaming", Capability::Streaming),
        ("tool_read", Capability::ToolRead),
        ("tool_write", Capability::ToolWrite),
        ("tool_edit", Capability::ToolEdit),
        ("tool_bash", Capability::ToolBash),
        ("tool_glob", Capability::ToolGlob),
        ("tool_grep", Capability::ToolGrep),
        ("tool_web_search", Capability::ToolWebSearch),
        ("tool_web_fetch", Capability::ToolWebFetch),
        ("tool_ask_user", Capability::ToolAskUser),
        ("hooks_pre_tool_use", Capability::HooksPreToolUse),
        ("hooks_post_tool_use", Capability::HooksPostToolUse),
        ("session_resume", Capability::SessionResume),
        ("session_fork", Capability::SessionFork),
        ("checkpointing", Capability::Checkpointing),
        (
            "structured_output_json_schema",
            Capability::StructuredOutputJsonSchema,
        ),
        ("mcp_client", Capability::McpClient),
        ("mcp_server", Capability::McpServer),
        ("tool_use", Capability::ToolUse),
        ("extended_thinking", Capability::ExtendedThinking),
        ("image_input", Capability::ImageInput),
        ("pdf_input", Capability::PdfInput),
        ("code_execution", Capability::CodeExecution),
        ("logprobs", Capability::Logprobs),
        ("seed_determinism", Capability::SeedDeterminism),
        ("stop_sequences", Capability::StopSequences),
    ]
}

fn satisfies(level: &SupportLevel, min: &MinSupport) -> bool {
    level.satisfies(min)
}

// ── 1. Capability serde ─────────────────────────────────────────────

mod capability_serde {
    use super::*;

    #[test]
    fn all_variants_serialize_to_snake_case() {
        for (expected_name, cap) in expected_wire_names() {
            let value = serde_json::to_value(&cap).unwrap();
            assert_eq!(
                value,
                json!(expected_name),
                "Capability::{cap:?} should serialize to \"{expected_name}\""
            );
        }
    }

    #[test]
    fn all_variants_round_trip() {
        for cap in all_capabilities() {
            let json = serde_json::to_string(&cap).unwrap();
            let back: Capability = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_value(&cap).unwrap(),
                serde_json::to_value(&back).unwrap(),
                "round-trip failed for {cap:?}"
            );
        }
    }

    #[test]
    fn deserialize_from_string() {
        let cap: Capability = serde_json::from_value(json!("mcp_server")).unwrap();
        assert_eq!(serde_json::to_value(&cap).unwrap(), json!("mcp_server"));
    }

    #[test]
    fn unknown_variant_is_rejected() {
        let result = serde_json::from_value::<Capability>(json!("teleport"));
        assert!(result.is_err());
    }
}

// ── 2. MinSupport ordering / SupportLevel::satisfies ────────────────

mod min_support {
    use super::*;

    #[test]
    fn native_satisfies_native() {
        assert!(satisfies(&SupportLevel::Native, &MinSupport::Native));
    }

    #[test]
    fn emulated_does_not_satisfy_native() {
        assert!(!satisfies(&SupportLevel::Emulated, &MinSupport::Native));
    }

    #[test]
    fn restricted_does_not_satisfy_native() {
        let r = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(!satisfies(&r, &MinSupport::Native));
    }

    #[test]
    fn unsupported_does_not_satisfy_native() {
        assert!(!satisfies(&SupportLevel::Unsupported, &MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated() {
        assert!(satisfies(&SupportLevel::Native, &MinSupport::Emulated));
    }

    #[test]
    fn emulated_satisfies_emulated() {
        assert!(satisfies(&SupportLevel::Emulated, &MinSupport::Emulated));
    }

    #[test]
    fn restricted_satisfies_emulated() {
        let r = SupportLevel::Restricted {
            reason: "sandbox".into(),
        };
        assert!(satisfies(&r, &MinSupport::Emulated));
    }

    #[test]
    fn unsupported_does_not_satisfy_emulated() {
        assert!(!satisfies(
            &SupportLevel::Unsupported,
            &MinSupport::Emulated
        ));
    }

    #[test]
    fn min_support_serde_round_trip() {
        for (expected, ms) in [
            ("native", MinSupport::Native),
            ("emulated", MinSupport::Emulated),
        ] {
            let value = serde_json::to_value(&ms).unwrap();
            assert_eq!(value, json!(expected));
            let back: MinSupport = serde_json::from_value(value).unwrap();
            assert_eq!(serde_json::to_value(&back).unwrap(), json!(expected));
        }
    }
}

// ── 3. CapabilityManifest lookup ────────────────────────────────────

mod manifest_lookup {
    use super::*;

    fn sample_manifest() -> CapabilityManifest {
        BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (
                Capability::ToolBash,
                SupportLevel::Restricted {
                    reason: "sandbox only".into(),
                },
            ),
            (Capability::McpClient, SupportLevel::Unsupported),
        ])
    }

    #[test]
    fn lookup_existing_native() {
        let m = sample_manifest();
        let level = m.get(&Capability::Streaming).unwrap();
        assert!(level.satisfies(&MinSupport::Native));
    }

    #[test]
    fn lookup_existing_emulated() {
        let m = sample_manifest();
        let level = m.get(&Capability::ToolRead).unwrap();
        assert!(level.satisfies(&MinSupport::Emulated));
        assert!(!level.satisfies(&MinSupport::Native));
    }

    #[test]
    fn lookup_existing_restricted() {
        let m = sample_manifest();
        let level = m.get(&Capability::ToolBash).unwrap();
        assert!(level.satisfies(&MinSupport::Emulated));
        assert!(!level.satisfies(&MinSupport::Native));
    }

    #[test]
    fn lookup_existing_unsupported() {
        let m = sample_manifest();
        let level = m.get(&Capability::McpClient).unwrap();
        assert!(!level.satisfies(&MinSupport::Emulated));
        assert!(!level.satisfies(&MinSupport::Native));
    }

    #[test]
    fn lookup_missing_capability() {
        let m = sample_manifest();
        assert!(!m.contains_key(&Capability::SessionFork));
    }

    #[test]
    fn manifest_serde_round_trip() {
        let m = sample_manifest();
        let json = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m.len(), back.len());
        // Verify key presence
        assert!(back.contains_key(&Capability::Streaming));
        assert!(back.contains_key(&Capability::ToolRead));
        assert!(back.contains_key(&Capability::ToolBash));
        assert!(back.contains_key(&Capability::McpClient));
    }
}

// ── 4. Empty manifest ───────────────────────────────────────────────

mod empty_manifest {
    use super::*;

    #[test]
    fn empty_manifest_has_no_entries() {
        let m = CapabilityManifest::new();
        assert!(m.is_empty());
    }

    #[test]
    fn empty_manifest_returns_none_for_any_capability() {
        let m = CapabilityManifest::new();
        for cap in all_capabilities() {
            assert!(
                !m.contains_key(&cap),
                "empty manifest should not contain {cap:?}"
            );
        }
    }

    #[test]
    fn empty_manifest_serializes_to_empty_object() {
        let m = CapabilityManifest::new();
        let value = serde_json::to_value(&m).unwrap();
        assert_eq!(value, json!({}));
    }

    #[test]
    fn empty_requirements_default() {
        let reqs = CapabilityRequirements::default();
        assert!(reqs.required.is_empty());
    }
}

// ── 5. Requirement matching against manifest ────────────────────────

mod requirement_matching {
    use super::*;

    fn check_requirements(manifest: &CapabilityManifest, reqs: &[CapabilityRequirement]) -> bool {
        reqs.iter().all(|req| {
            manifest
                .get(&req.capability)
                .is_some_and(|level| level.satisfies(&req.min_support))
        })
    }

    #[test]
    fn single_requirement_satisfied() {
        let manifest = BTreeMap::from([(Capability::Streaming, SupportLevel::Native)]);
        let reqs = vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }];
        assert!(check_requirements(&manifest, &reqs));
    }

    #[test]
    fn single_requirement_not_satisfied_level_too_low() {
        let manifest = BTreeMap::from([(Capability::Streaming, SupportLevel::Emulated)]);
        let reqs = vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }];
        assert!(!check_requirements(&manifest, &reqs));
    }

    #[test]
    fn single_requirement_not_satisfied_missing() {
        let manifest = BTreeMap::from([(Capability::Streaming, SupportLevel::Native)]);
        let reqs = vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }];
        assert!(!check_requirements(&manifest, &reqs));
    }

    #[test]
    fn multiple_requirements_all_satisfied() {
        let manifest = BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::McpClient, SupportLevel::Emulated),
        ]);
        let reqs = vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Emulated,
            },
        ];
        assert!(check_requirements(&manifest, &reqs));
    }

    #[test]
    fn multiple_requirements_partial_satisfaction() {
        let manifest = BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]);
        let reqs = vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            // This one fails — ToolRead is Emulated, but Native is required.
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
        ];
        assert!(!check_requirements(&manifest, &reqs));
    }

    #[test]
    fn empty_requirements_always_satisfied() {
        let manifest = CapabilityManifest::new();
        assert!(check_requirements(&manifest, &[]));
    }

    #[test]
    fn restricted_satisfies_emulated_requirement() {
        let manifest = BTreeMap::from([(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let reqs = vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }];
        assert!(check_requirements(&manifest, &reqs));
    }

    #[test]
    fn restricted_does_not_satisfy_native_requirement() {
        let manifest = BTreeMap::from([(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let reqs = vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Native,
        }];
        assert!(!check_requirements(&manifest, &reqs));
    }
}

// ── 6. CapabilityRequirements serde ─────────────────────────────────

mod requirements_serde {
    use super::*;

    #[test]
    fn round_trip() {
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
            ],
        };
        let json = serde_json::to_string_pretty(&reqs).unwrap();
        let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
        assert_eq!(back.required.len(), 2);
    }

    #[test]
    fn empty_requirements_round_trip() {
        let reqs = CapabilityRequirements::default();
        let json = serde_json::to_string(&reqs).unwrap();
        let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
        assert!(back.required.is_empty());
    }

    #[test]
    fn json_shape_matches_contract() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpServer,
                min_support: MinSupport::Emulated,
            }],
        };
        let value = serde_json::to_value(&reqs).unwrap();
        assert_eq!(value["required"][0]["capability"], json!("mcp_server"));
        assert_eq!(value["required"][0]["min_support"], json!("emulated"));
    }
}

// ── 7. Manifest in Receipt ──────────────────────────────────────────

mod manifest_in_receipt {
    use super::*;

    #[test]
    fn manifest_survives_receipt_serialization() {
        let manifest: CapabilityManifest = BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (
                Capability::ToolBash,
                SupportLevel::Restricted {
                    reason: "sandbox only".into(),
                },
            ),
        ]);
        let json = serde_json::to_value(&manifest).unwrap();
        let back: CapabilityManifest = serde_json::from_value(json).unwrap();
        assert_eq!(back.len(), 3);
        assert!(back.contains_key(&Capability::Streaming));
        assert!(back.contains_key(&Capability::ToolRead));
        assert!(back.contains_key(&Capability::ToolBash));
    }
}

// ── 8. Deterministic manifest ordering ──────────────────────────────

mod manifest_ordering {
    use super::*;

    #[test]
    fn insertion_order_does_not_affect_serialization() {
        let mut m1 = CapabilityManifest::new();
        m1.insert(Capability::McpServer, SupportLevel::Emulated);
        m1.insert(Capability::Streaming, SupportLevel::Native);
        m1.insert(Capability::ToolRead, SupportLevel::Native);

        let mut m2 = CapabilityManifest::new();
        m2.insert(Capability::ToolRead, SupportLevel::Native);
        m2.insert(Capability::Streaming, SupportLevel::Native);
        m2.insert(Capability::McpServer, SupportLevel::Emulated);

        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        assert_eq!(
            j1, j2,
            "BTreeMap must produce identical JSON regardless of insertion order"
        );
    }
}
