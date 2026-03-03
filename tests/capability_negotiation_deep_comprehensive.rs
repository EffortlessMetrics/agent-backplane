#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

use abp_capability::{
    CompatibilityReport, NegotiationResult, SupportLevel as CapSupportLevel, check_capability,
    generate_report, negotiate,
};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport, CapabilityReportEntry,
    DialectSupportLevel, NegotiationRequest,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use std::collections::BTreeMap;

// =========================================================================
// Helpers
// =========================================================================

fn manifest(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn reqs(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn reqs_native(caps: &[Capability]) -> CapabilityRequirements {
    reqs(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Native))
            .collect::<Vec<_>>(),
    )
}

fn reqs_emulated(caps: &[Capability]) -> CapabilityRequirements {
    reqs(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

/// All 26 Capability variants.
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

// =========================================================================
// Module: capability_variants — every Capability variant exists
// =========================================================================

mod capability_variants {
    use super::*;

    #[test]
    fn streaming_variant_exists() {
        let c = Capability::Streaming;
        assert_eq!(format!("{c:?}"), "Streaming");
    }

    #[test]
    fn tool_read_variant_exists() {
        let c = Capability::ToolRead;
        assert_eq!(format!("{c:?}"), "ToolRead");
    }

    #[test]
    fn tool_write_variant_exists() {
        let c = Capability::ToolWrite;
        assert_eq!(format!("{c:?}"), "ToolWrite");
    }

    #[test]
    fn tool_edit_variant_exists() {
        let c = Capability::ToolEdit;
        assert_eq!(format!("{c:?}"), "ToolEdit");
    }

    #[test]
    fn tool_bash_variant_exists() {
        let c = Capability::ToolBash;
        assert_eq!(format!("{c:?}"), "ToolBash");
    }

    #[test]
    fn tool_glob_variant_exists() {
        let c = Capability::ToolGlob;
        assert_eq!(format!("{c:?}"), "ToolGlob");
    }

    #[test]
    fn tool_grep_variant_exists() {
        let c = Capability::ToolGrep;
        assert_eq!(format!("{c:?}"), "ToolGrep");
    }

    #[test]
    fn tool_web_search_variant_exists() {
        let c = Capability::ToolWebSearch;
        assert_eq!(format!("{c:?}"), "ToolWebSearch");
    }

    #[test]
    fn tool_web_fetch_variant_exists() {
        let c = Capability::ToolWebFetch;
        assert_eq!(format!("{c:?}"), "ToolWebFetch");
    }

    #[test]
    fn tool_ask_user_variant_exists() {
        let c = Capability::ToolAskUser;
        assert_eq!(format!("{c:?}"), "ToolAskUser");
    }

    #[test]
    fn hooks_pre_tool_use_variant_exists() {
        let c = Capability::HooksPreToolUse;
        assert_eq!(format!("{c:?}"), "HooksPreToolUse");
    }

    #[test]
    fn hooks_post_tool_use_variant_exists() {
        let c = Capability::HooksPostToolUse;
        assert_eq!(format!("{c:?}"), "HooksPostToolUse");
    }

    #[test]
    fn session_resume_variant_exists() {
        let c = Capability::SessionResume;
        assert_eq!(format!("{c:?}"), "SessionResume");
    }

    #[test]
    fn session_fork_variant_exists() {
        let c = Capability::SessionFork;
        assert_eq!(format!("{c:?}"), "SessionFork");
    }

    #[test]
    fn checkpointing_variant_exists() {
        let c = Capability::Checkpointing;
        assert_eq!(format!("{c:?}"), "Checkpointing");
    }

    #[test]
    fn structured_output_json_schema_variant_exists() {
        let c = Capability::StructuredOutputJsonSchema;
        assert_eq!(format!("{c:?}"), "StructuredOutputJsonSchema");
    }

    #[test]
    fn mcp_client_variant_exists() {
        let c = Capability::McpClient;
        assert_eq!(format!("{c:?}"), "McpClient");
    }

    #[test]
    fn mcp_server_variant_exists() {
        let c = Capability::McpServer;
        assert_eq!(format!("{c:?}"), "McpServer");
    }

    #[test]
    fn tool_use_variant_exists() {
        let c = Capability::ToolUse;
        assert_eq!(format!("{c:?}"), "ToolUse");
    }

    #[test]
    fn extended_thinking_variant_exists() {
        let c = Capability::ExtendedThinking;
        assert_eq!(format!("{c:?}"), "ExtendedThinking");
    }

    #[test]
    fn image_input_variant_exists() {
        let c = Capability::ImageInput;
        assert_eq!(format!("{c:?}"), "ImageInput");
    }

    #[test]
    fn pdf_input_variant_exists() {
        let c = Capability::PdfInput;
        assert_eq!(format!("{c:?}"), "PdfInput");
    }

    #[test]
    fn code_execution_variant_exists() {
        let c = Capability::CodeExecution;
        assert_eq!(format!("{c:?}"), "CodeExecution");
    }

    #[test]
    fn logprobs_variant_exists() {
        let c = Capability::Logprobs;
        assert_eq!(format!("{c:?}"), "Logprobs");
    }

    #[test]
    fn seed_determinism_variant_exists() {
        let c = Capability::SeedDeterminism;
        assert_eq!(format!("{c:?}"), "SeedDeterminism");
    }

    #[test]
    fn stop_sequences_variant_exists() {
        let c = Capability::StopSequences;
        assert_eq!(format!("{c:?}"), "StopSequences");
    }

    #[test]
    fn all_variants_count_is_26() {
        assert_eq!(all_capabilities().len(), 26);
    }

    #[test]
    fn all_variants_are_unique() {
        let caps = all_capabilities();
        let set: BTreeMap<&Capability, ()> = caps.iter().map(|c| (c, ())).collect();
        assert_eq!(set.len(), caps.len());
    }
}

// =========================================================================
// Module: support_level_variants — SupportLevel and MinSupport
// =========================================================================

mod support_level_variants {
    use super::*;

    #[test]
    fn native_debug() {
        assert_eq!(format!("{:?}", SupportLevel::Native), "Native");
    }

    #[test]
    fn emulated_debug() {
        assert_eq!(format!("{:?}", SupportLevel::Emulated), "Emulated");
    }

    #[test]
    fn unsupported_debug() {
        assert!(matches!(format!("{:?}", SupportLevel::Unsupported { .. })), "Unsupported");
    }

    #[test]
    fn restricted_debug() {
        let r = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        let dbg = format!("{r:?}");
        assert!(dbg.contains("Restricted"));
        assert!(dbg.contains("policy"));
    }

    #[test]
    fn min_support_native_debug() {
        assert_eq!(format!("{:?}", MinSupport::Native), "Native");
    }

    #[test]
    fn min_support_emulated_debug() {
        assert_eq!(format!("{:?}", MinSupport::Emulated), "Emulated");
    }

    // -- SupportLevel::satisfies --

    #[test]
    fn native_satisfies_native() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_does_not_satisfy_native() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn emulated_satisfies_emulated() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn unsupported_does_not_satisfy_native() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn unsupported_does_not_satisfy_emulated() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_does_not_satisfy_native() {
        let r = SupportLevel::Restricted { reason: "x".into() };
        assert!(!r.satisfies(&MinSupport::Native));
    }

    #[test]
    fn restricted_satisfies_emulated() {
        let r = SupportLevel::Restricted { reason: "x".into() };
        assert!(r.satisfies(&MinSupport::Emulated));
    }
}

// =========================================================================
// Module: requirement_matching — CapabilityRequirement / CapabilityRequirements
// =========================================================================

mod requirement_matching {
    use super::*;

    #[test]
    fn single_native_requirement_met() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let r = reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
    }

    #[test]
    fn single_native_requirement_unmet() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
    }

    #[test]
    fn emulated_requirement_met_by_native() {
        let m = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
        let r = reqs_emulated(&[Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.native.len(), 1);
    }

    #[test]
    fn emulated_requirement_met_by_emulated() {
        let m = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
        let r = reqs_emulated(&[Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
    }

    #[test]
    fn emulated_requirement_unmet_by_unsupported() {
        let m = manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
        let r = reqs_emulated(&[Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
    }

    #[test]
    fn multiple_requirements_all_met() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Emulated),
        ]);
        let r = reqs_emulated(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
    }

    #[test]
    fn multiple_requirements_one_unmet() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported.len(), 1);
    }

    #[test]
    fn requirement_with_restricted_backend() {
        let m = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let r = reqs_emulated(&[Capability::ToolBash]);
        let res = negotiate(&m, &r);
        // Restricted is treated as emulatable
        assert!(res.is_compatible());
    }

    #[test]
    fn requirement_restricted_not_native() {
        let m = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        // check_capability maps Restricted to Emulated, so it's not native
        let level = check_capability(&m, &Capability::ToolBash);
        assert!(matches!(level, CapSupportLevel::Emulated { .. }));
    }

    #[test]
    fn empty_requirements_always_compatible() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = CapabilityRequirements::default();
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.total(), 0);
    }
}

// =========================================================================
// Module: negotiation_logic — native > emulated > unsupported ranking
// =========================================================================

mod negotiation_logic {
    use super::*;

    #[test]
    fn native_ranks_above_emulated() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]);
        let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native, vec![Capability::Streaming]);
        assert_eq!(res.emulated, vec![Capability::ToolRead]);
    }

    #[test]
    fn emulated_ranks_above_unsupported() {
        let m = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
        let r = reqs_native(&[Capability::ToolRead, Capability::Logprobs]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulated, vec![Capability::ToolRead]);
        assert_eq!(res.unsupported, vec![Capability::Logprobs]);
    }

    #[test]
    fn all_three_tiers_in_one_negotiation() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            // Logprobs missing → unsupported
        ]);
        let r = reqs_native(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::Logprobs,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native.len(), 1);
        assert_eq!(res.emulated.len(), 1);
        assert_eq!(res.unsupported.len(), 1);
    }

    #[test]
    fn all_native_is_best_outcome() {
        let all = all_capabilities();
        let m: CapabilityManifest = all
            .iter()
            .map(|c| (c.clone(), SupportLevel::Native))
            .collect();
        let r = reqs_native(&all);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.native.len(), 26);
        assert!(res.emulated.is_empty());
        assert!(res.unsupported.is_empty());
    }

    #[test]
    fn all_emulated_still_compatible() {
        let all = all_capabilities();
        let m: CapabilityManifest = all
            .iter()
            .map(|c| (c.clone(), SupportLevel::Emulated))
            .collect();
        let r = reqs_native(&all);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert!(res.native.is_empty());
        assert_eq!(res.emulated.len(), 26);
    }

    #[test]
    fn all_unsupported_is_worst_outcome() {
        let all = all_capabilities();
        let m: CapabilityManifest = all
            .iter()
            .map(|c| (c.clone(), SupportLevel::Unsupported))
            .collect();
        let r = reqs_native(&all);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported.len(), 26);
    }

    #[test]
    fn empty_manifest_fails_all() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = reqs_native(&[Capability::Streaming, Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported.len(), 2);
    }

    #[test]
    fn superset_manifest_satisfies_subset_requirements() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
        ]);
        let r = reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.total(), 1);
    }
}

// =========================================================================
// Module: core_negotiator — CapabilityNegotiator from abp_core::negotiate
// =========================================================================

mod core_negotiator {
    use super::*;

    fn core_request(
        required: Vec<Capability>,
        preferred: Vec<Capability>,
        min: SupportLevel,
    ) -> NegotiationRequest {
        NegotiationRequest {
            required,
            preferred,
            minimum_support: min,
        }
    }

    #[test]
    fn negotiate_basic_satisfied() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(res.is_compatible);
        assert_eq!(res.satisfied, vec![Capability::Streaming]);
    }

    #[test]
    fn negotiate_unsatisfied_missing() {
        let m: CapabilityManifest = BTreeMap::new();
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(!res.is_compatible);
        assert_eq!(res.unsatisfied, vec![Capability::Streaming]);
    }

    #[test]
    fn negotiate_bonus_preferred() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]);
        let req = core_request(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            SupportLevel::Emulated,
        );
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(res.is_compatible);
        assert_eq!(res.bonus, vec![Capability::ToolRead]);
    }

    #[test]
    fn negotiate_preferred_missing_still_compatible() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let req = core_request(
            vec![Capability::Streaming],
            vec![Capability::Logprobs],
            SupportLevel::Emulated,
        );
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(res.is_compatible);
        assert!(res.bonus.is_empty());
    }

    #[test]
    fn negotiate_min_native_rejects_emulated() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Native);
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(!res.is_compatible);
    }

    #[test]
    fn negotiate_min_native_accepts_native() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Native);
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(res.is_compatible);
    }

    #[test]
    fn negotiate_min_emulated_accepts_emulated() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(res.is_compatible);
    }

    #[test]
    fn negotiate_min_emulated_accepts_restricted() {
        let m = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let req = core_request(vec![Capability::ToolBash], vec![], SupportLevel::Emulated);
        let res = CapabilityNegotiator::negotiate(&req, &m);
        // Restricted rank=1, Emulated rank=2, so Restricted < Emulated → not satisfied
        // Actually: min_rank for Emulated is 2, Restricted rank is 1. 1 >= 2 is false.
        assert!(!res.is_compatible);
    }

    #[test]
    fn negotiate_restricted_satisfies_restricted_min() {
        let m = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let req = core_request(
            vec![Capability::ToolBash],
            vec![],
            SupportLevel::Restricted {
                reason: String::new(),
            },
        );
        let res = CapabilityNegotiator::negotiate(&req, &m);
        assert!(res.is_compatible);
    }

    #[test]
    fn best_match_returns_none_when_no_compatible() {
        let m1 = manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
        let req = core_request(vec![Capability::ToolRead], vec![], SupportLevel::Emulated);
        let result = CapabilityNegotiator::best_match(&req, &[("backend_a", m1)]);
        assert!(result.is_none());
    }

    #[test]
    fn best_match_picks_only_compatible() {
        let m1: CapabilityManifest = BTreeMap::new();
        let m2 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
        let (name, res) =
            CapabilityNegotiator::best_match(&req, &[("bad", m1), ("good", m2)]).unwrap();
        assert_eq!(name, "good");
        assert!(res.is_compatible);
    }

    #[test]
    fn best_match_prefers_higher_score() {
        let m1 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let m2 = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]);
        let req = core_request(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            SupportLevel::Emulated,
        );
        let (name, _) = CapabilityNegotiator::best_match(&req, &[("a", m1), ("b", m2)]).unwrap();
        assert_eq!(name, "b");
    }

    #[test]
    fn best_match_tie_breaks_by_name() {
        let m1 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let m2 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
        let (name, _) =
            CapabilityNegotiator::best_match(&req, &[("alpha", m1), ("beta", m2)]).unwrap();
        // Tie-break: name_b.cmp(name_a), so "beta" > "alpha" → "alpha" wins
        assert_eq!(name, "alpha");
    }

    #[test]
    fn best_match_empty_manifests_list() {
        let req = core_request(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
        let result = CapabilityNegotiator::best_match(&req, &[]);
        assert!(result.is_none());
    }
}

// =========================================================================
// Module: capability_diff — CapabilityDiff tests
// =========================================================================

mod capability_diff {
    use super::*;

    #[test]
    fn diff_no_changes() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let d = CapabilityDiff::diff(&m, &m);
        assert!(d.added.is_empty());
        assert!(d.removed.is_empty());
        assert!(d.upgraded.is_empty());
        assert!(d.downgraded.is_empty());
    }

    #[test]
    fn diff_added() {
        let old: CapabilityManifest = BTreeMap::new();
        let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.added, vec![Capability::Streaming]);
        assert!(d.removed.is_empty());
    }

    #[test]
    fn diff_removed() {
        let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let new: CapabilityManifest = BTreeMap::new();
        let d = CapabilityDiff::diff(&old, &new);
        assert!(d.added.is_empty());
        assert_eq!(d.removed, vec![Capability::Streaming]);
    }

    #[test]
    fn diff_upgraded_emulated_to_native() {
        let old = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
        let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.upgraded.len(), 1);
        assert_eq!(d.upgraded[0].0, Capability::Streaming);
    }

    #[test]
    fn diff_downgraded_native_to_emulated() {
        let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let new = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.downgraded.len(), 1);
        assert_eq!(d.downgraded[0].0, Capability::Streaming);
    }

    #[test]
    fn diff_downgraded_native_to_unsupported() {
        let old = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
        let new = manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.downgraded.len(), 1);
    }

    #[test]
    fn diff_upgraded_unsupported_to_native() {
        let old = manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
        let new = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.upgraded.len(), 1);
    }

    #[test]
    fn diff_mixed_changes() {
        let old = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]);
        let new = manifest(&[
            (Capability::Streaming, SupportLevel::Emulated), // downgraded
            (Capability::ToolWrite, SupportLevel::Native),   // added
                                                             // ToolRead removed
        ]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.added, vec![Capability::ToolWrite]);
        assert_eq!(d.removed, vec![Capability::ToolRead]);
        assert_eq!(d.downgraded.len(), 1);
    }

    #[test]
    fn diff_empty_manifests() {
        let d = CapabilityDiff::diff(&BTreeMap::new(), &BTreeMap::new());
        assert!(d.added.is_empty());
        assert!(d.removed.is_empty());
        assert!(d.upgraded.is_empty());
        assert!(d.downgraded.is_empty());
    }

    #[test]
    fn diff_restricted_to_native_is_upgrade() {
        let old = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted { reason: "x".into() },
        )]);
        let new = manifest(&[(Capability::ToolBash, SupportLevel::Native)]);
        let d = CapabilityDiff::diff(&old, &new);
        assert_eq!(d.upgraded.len(), 1);
    }
}

// =========================================================================
// Module: backend_declaration — declaring backend capabilities
// =========================================================================

mod backend_declaration {
    use super::*;
    use abp_core::Outcome;

    #[test]
    fn receipt_builder_with_empty_capabilities() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        assert!(r.capabilities.is_empty());
    }

    #[test]
    fn receipt_builder_with_capabilities() {
        let caps = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]);
        let r = ReceiptBuilder::new("mock")
            .capabilities(caps.clone())
            .build();
        assert_eq!(r.capabilities.len(), 2);
        assert!(r.capabilities.contains_key(&Capability::Streaming));
    }

    #[test]
    fn receipt_preserves_capability_manifest() {
        let caps = manifest(&[
            (Capability::ToolBash, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Emulated),
        ]);
        let r = ReceiptBuilder::new("backend-x").capabilities(caps).build();
        assert_eq!(r.capabilities.len(), 3);
        assert!(matches!(
            r.capabilities.get(&Capability::Streaming),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn manifest_btreemap_ordering() {
        let m = manifest(&[
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]);
        let keys: Vec<_> = m.keys().collect();
        // BTreeMap sorts by Ord on Capability
        for i in 1..keys.len() {
            assert!(keys[i - 1] < keys[i]);
        }
    }

    #[test]
    fn manifest_insert_overwrites() {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Emulated);
        m.insert(Capability::Streaming, SupportLevel::Native);
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn full_backend_manifest() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
            (Capability::ToolBash, SupportLevel::Native),
            (Capability::ToolGlob, SupportLevel::Native),
            (Capability::ToolGrep, SupportLevel::Native),
            (Capability::ToolUse, SupportLevel::Native),
            (Capability::ExtendedThinking, SupportLevel::Emulated),
            (Capability::ImageInput, SupportLevel::Native),
        ]);
        assert_eq!(m.len(), 10);
    }
}

// =========================================================================
// Module: serde_roundtrips — serialization / deserialization
// =========================================================================

mod serde_roundtrips {
    use super::*;

    #[test]
    fn capability_serde_streaming() {
        let c = Capability::Streaming;
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(j, "\"streaming\"");
        let back: Capability = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn capability_serde_tool_read() {
        let j = serde_json::to_string(&Capability::ToolRead).unwrap();
        assert_eq!(j, "\"tool_read\"");
    }

    #[test]
    fn capability_serde_tool_web_search() {
        let j = serde_json::to_string(&Capability::ToolWebSearch).unwrap();
        assert_eq!(j, "\"tool_web_search\"");
    }

    #[test]
    fn capability_serde_mcp_client() {
        let j = serde_json::to_string(&Capability::McpClient).unwrap();
        assert_eq!(j, "\"mcp_client\"");
    }

    #[test]
    fn capability_serde_extended_thinking() {
        let j = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
        assert_eq!(j, "\"extended_thinking\"");
    }

    #[test]
    fn capability_serde_structured_output() {
        let j = serde_json::to_string(&Capability::StructuredOutputJsonSchema).unwrap();
        assert_eq!(j, "\"structured_output_json_schema\"");
    }

    #[test]
    fn support_level_serde_native() {
        let j = serde_json::to_string(&SupportLevel::Native).unwrap();
        let back: SupportLevel = serde_json::from_str(&j).unwrap();
        assert!(matches!(back, SupportLevel::Native));
    }

    #[test]
    fn support_level_serde_emulated() {
        let j = serde_json::to_string(&SupportLevel::Emulated).unwrap();
        let back: SupportLevel = serde_json::from_str(&j).unwrap();
        assert!(matches!(back, SupportLevel::Emulated));
    }

    #[test]
    fn support_level_serde_unsupported() {
        let j = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
        let back: SupportLevel = serde_json::from_str(&j).unwrap();
        assert!(matches!(back, SupportLevel::Unsupported));
    }

    #[test]
    fn support_level_serde_restricted() {
        let r = SupportLevel::Restricted {
            reason: "policy violation".into(),
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: SupportLevel = serde_json::from_str(&j).unwrap();
        if let SupportLevel::Restricted { reason } = back {
            assert_eq!(reason, "policy violation");
        } else {
            panic!("Expected Restricted variant");
        }
    }

    #[test]
    fn min_support_serde_native() {
        let j = serde_json::to_string(&MinSupport::Native).unwrap();
        let back: MinSupport = serde_json::from_str(&j).unwrap();
        assert!(matches!(back, MinSupport::Native));
    }

    #[test]
    fn min_support_serde_emulated() {
        let j = serde_json::to_string(&MinSupport::Emulated).unwrap();
        let back: MinSupport = serde_json::from_str(&j).unwrap();
        assert!(matches!(back, MinSupport::Emulated));
    }

    #[test]
    fn capability_requirement_serde_roundtrip() {
        let req = CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        };
        let j = serde_json::to_string(&req).unwrap();
        let back: CapabilityRequirement = serde_json::from_str(&j).unwrap();
        assert_eq!(back.capability, Capability::Streaming);
        assert!(matches!(back.min_support, MinSupport::Native));
    }

    #[test]
    fn capability_requirements_serde_roundtrip() {
        let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let j = serde_json::to_string(&r).unwrap();
        let back: CapabilityRequirements = serde_json::from_str(&j).unwrap();
        assert_eq!(back.required.len(), 2);
    }

    #[test]
    fn manifest_serde_roundtrip() {
        let m = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::Logprobs, SupportLevel::Unsupported),
        ]);
        let j = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&j).unwrap();
        assert_eq!(back.len(), 3);
    }

    #[test]
    fn all_capability_variants_roundtrip() {
        for cap in all_capabilities() {
            let j = serde_json::to_string(&cap).unwrap();
            let back: Capability = serde_json::from_str(&j).unwrap();
            assert_eq!(back, cap);
        }
    }

    #[test]
    fn dialect_support_level_serde_native() {
        let d = DialectSupportLevel::Native;
        let j = serde_json::to_string(&d).unwrap();
        let back: DialectSupportLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(back, DialectSupportLevel::Native);
    }

    #[test]
    fn dialect_support_level_serde_emulated() {
        let d = DialectSupportLevel::Emulated {
            detail: "polyfill".into(),
        };
        let j = serde_json::to_string(&d).unwrap();
        let back: DialectSupportLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn dialect_support_level_serde_unsupported() {
        let d = DialectSupportLevel::Unsupported {
            reason: "not available".into(),
        };
        let j = serde_json::to_string(&d).unwrap();
        let back: DialectSupportLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn capability_report_entry_serde() {
        let e = CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        };
        let j = serde_json::to_string(&e).unwrap();
        let back: CapabilityReportEntry = serde_json::from_str(&j).unwrap();
        assert_eq!(back.capability, Capability::Streaming);
    }

    #[test]
    fn capability_report_serde_roundtrip() {
        let report = CapabilityReport {
            source_dialect: "claude".into(),
            target_dialect: "openai".into(),
            entries: vec![CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            }],
        };
        let j = serde_json::to_string(&report).unwrap();
        let back: CapabilityReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.source_dialect, "claude");
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn cap_support_level_serde_native() {
        let l = CapSupportLevel::Native;
        let j = serde_json::to_string(&l).unwrap();
        let back: CapSupportLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(back, CapSupportLevel::Native);
    }

    #[test]
    fn cap_support_level_serde_emulated_with_strategy() {
        let l = CapSupportLevel::Emulated {
            strategy: "wrapper".into(),
        };
        let j = serde_json::to_string(&l).unwrap();
        let back: CapSupportLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn negotiation_result_serde_roundtrip() {
        let res = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        let j = serde_json::to_string(&res).unwrap();
        let back: NegotiationResult = serde_json::from_str(&j).unwrap();
        assert_eq!(back, res);
    }

    #[test]
    fn compatibility_report_serde_roundtrip() {
        let res = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&res);
        let j = serde_json::to_string(&report).unwrap();
        let back: CompatibilityReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back, report);
    }
}

// =========================================================================
// Module: edge_cases — empty, duplicate, corner cases
// =========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_manifest_empty_requirements() {
        let res = negotiate(&BTreeMap::new(), &CapabilityRequirements::default());
        assert!(res.is_compatible());
        assert_eq!(res.total(), 0);
    }

    #[test]
    fn duplicate_requirements_are_preserved() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let r = reqs_native(&[Capability::Streaming, Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native.len(), 2);
    }

    #[test]
    fn duplicate_unsupported_requirements() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = reqs_native(&[
            Capability::Logprobs,
            Capability::Logprobs,
            Capability::Logprobs,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported.len(), 3);
    }

    #[test]
    fn manifest_with_all_unsupported_values() {
        let m: CapabilityManifest = all_capabilities()
            .into_iter()
            .map(|c| (c, SupportLevel::Unsupported))
            .collect();
        let r = reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
    }

    #[test]
    fn single_capability_manifest() {
        let m = manifest(&[(Capability::CodeExecution, SupportLevel::Native)]);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn large_manifest_26_capabilities() {
        let m: CapabilityManifest = all_capabilities()
            .into_iter()
            .map(|c| (c, SupportLevel::Native))
            .collect();
        assert_eq!(m.len(), 26);
    }

    #[test]
    fn requirement_for_missing_capability() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
        let r = reqs_native(&[Capability::Logprobs]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported, vec![Capability::Logprobs]);
    }

    #[test]
    fn explicit_unsupported_same_as_missing() {
        let m_explicit = manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
        let m_missing: CapabilityManifest = BTreeMap::new();
        let r = reqs_native(&[Capability::Logprobs]);

        let res1 = negotiate(&m_explicit, &r);
        let res2 = negotiate(&m_missing, &r);
        assert_eq!(res1.is_compatible(), res2.is_compatible());
        assert_eq!(res1.unsupported.len(), res2.unsupported.len());
    }

    #[test]
    fn order_of_requirements_preserved_in_result() {
        let m = manifest(&[
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Native),
        ]);
        let r = reqs_native(&[
            Capability::ToolWrite,
            Capability::Streaming,
            Capability::ToolRead,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native[0], Capability::ToolWrite);
        assert_eq!(res.native[1], Capability::Streaming);
        assert_eq!(res.native[2], Capability::ToolRead);
    }

    #[test]
    fn manifest_btreemap_is_deterministic() {
        let m1 = manifest(&[
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Native),
        ]);
        let m2 = manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]);
        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        assert_eq!(j1, j2);
    }
}

// =========================================================================
// Module: cross_sdk_comparison — dialect manifest comparisons
// =========================================================================

mod cross_sdk_comparison {
    use super::*;
    use abp_core::negotiate::dialect_manifest;

    #[test]
    fn claude_manifest_has_streaming() {
        let m = dialect_manifest("claude");
        assert!(m.contains_key(&Capability::Streaming));
    }

    #[test]
    fn openai_manifest_has_streaming() {
        let m = dialect_manifest("openai");
        assert!(m.contains_key(&Capability::Streaming));
    }

    #[test]
    fn gemini_manifest_has_streaming() {
        let m = dialect_manifest("gemini");
        assert!(m.contains_key(&Capability::Streaming));
    }

    #[test]
    fn unknown_dialect_returns_empty() {
        let m = dialect_manifest("unknown_backend");
        assert!(m.is_empty());
    }

    #[test]
    fn claude_has_extended_thinking_native() {
        let m = dialect_manifest("claude");
        assert_eq!(
            m.get(&Capability::ExtendedThinking),
            Some(&DialectSupportLevel::Native)
        );
    }

    #[test]
    fn openai_lacks_extended_thinking() {
        let m = dialect_manifest("openai");
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn gemini_emulates_extended_thinking() {
        let m = dialect_manifest("gemini");
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(DialectSupportLevel::Emulated { .. })
        ));
    }

    #[test]
    fn openai_has_logprobs_native() {
        let m = dialect_manifest("openai");
        assert_eq!(
            m.get(&Capability::Logprobs),
            Some(&DialectSupportLevel::Native)
        );
    }

    #[test]
    fn claude_lacks_logprobs() {
        let m = dialect_manifest("claude");
        assert!(matches!(
            m.get(&Capability::Logprobs),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn gemini_lacks_logprobs() {
        let m = dialect_manifest("gemini");
        assert!(matches!(
            m.get(&Capability::Logprobs),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn openai_has_seed_determinism() {
        let m = dialect_manifest("openai");
        assert_eq!(
            m.get(&Capability::SeedDeterminism),
            Some(&DialectSupportLevel::Native)
        );
    }

    #[test]
    fn claude_lacks_seed_determinism() {
        let m = dialect_manifest("claude");
        assert!(matches!(
            m.get(&Capability::SeedDeterminism),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn gemini_has_pdf_input_native() {
        let m = dialect_manifest("gemini");
        assert_eq!(
            m.get(&Capability::PdfInput),
            Some(&DialectSupportLevel::Native)
        );
    }

    #[test]
    fn openai_lacks_pdf_input() {
        let m = dialect_manifest("openai");
        assert!(matches!(
            m.get(&Capability::PdfInput),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn claude_emulates_pdf_input() {
        let m = dialect_manifest("claude");
        assert!(matches!(
            m.get(&Capability::PdfInput),
            Some(DialectSupportLevel::Emulated { .. })
        ));
    }

    #[test]
    fn all_dialects_have_tool_use() {
        for dialect in &["claude", "openai", "gemini"] {
            let m = dialect_manifest(dialect);
            assert!(
                m.contains_key(&Capability::ToolUse),
                "{dialect} should have ToolUse"
            );
        }
    }

    #[test]
    fn openai_structured_output_native() {
        let m = dialect_manifest("openai");
        assert_eq!(
            m.get(&Capability::StructuredOutputJsonSchema),
            Some(&DialectSupportLevel::Native)
        );
    }

    #[test]
    fn claude_structured_output_emulated() {
        let m = dialect_manifest("claude");
        assert!(matches!(
            m.get(&Capability::StructuredOutputJsonSchema),
            Some(DialectSupportLevel::Emulated { .. })
        ));
    }

    #[test]
    fn all_dialects_have_stop_sequences() {
        for dialect in &["claude", "openai", "gemini"] {
            let m = dialect_manifest(dialect);
            assert_eq!(
                m.get(&Capability::StopSequences),
                Some(&DialectSupportLevel::Native),
                "{dialect} should natively support StopSequences"
            );
        }
    }
}

// =========================================================================
// Module: emulation_labeling — how emulation is described
// =========================================================================

mod emulation_labeling {
    use super::*;
    use abp_core::negotiate::dialect_manifest;

    #[test]
    fn claude_code_execution_emulated_via_bash() {
        let m = dialect_manifest("claude");
        if let Some(DialectSupportLevel::Emulated { detail }) = m.get(&Capability::CodeExecution) {
            assert!(detail.contains("tool_bash"));
        } else {
            panic!("Expected Emulated for CodeExecution on Claude");
        }
    }

    #[test]
    fn claude_pdf_emulated_conversion() {
        let m = dialect_manifest("claude");
        if let Some(DialectSupportLevel::Emulated { detail }) = m.get(&Capability::PdfInput) {
            assert!(detail.contains("text"));
        } else {
            panic!("Expected Emulated for PdfInput on Claude");
        }
    }

    #[test]
    fn gemini_code_execution_emulated() {
        let m = dialect_manifest("gemini");
        if let Some(DialectSupportLevel::Emulated { detail }) = m.get(&Capability::CodeExecution) {
            assert!(detail.contains("code_execution"));
        } else {
            panic!("Expected Emulated for CodeExecution on Gemini");
        }
    }

    #[test]
    fn gemini_extended_thinking_emulated() {
        let m = dialect_manifest("gemini");
        if let Some(DialectSupportLevel::Emulated { detail }) = m.get(&Capability::ExtendedThinking)
        {
            assert!(detail.contains("thinking"));
        } else {
            panic!("Expected Emulated for ExtendedThinking on Gemini");
        }
    }

    #[test]
    fn unsupported_has_reason_string() {
        let m = dialect_manifest("claude");
        if let Some(DialectSupportLevel::Unsupported { reason }) = m.get(&Capability::Logprobs) {
            assert!(!reason.is_empty());
        } else {
            panic!("Expected Unsupported for Logprobs on Claude");
        }
    }

    #[test]
    fn check_capability_restricted_includes_reason() {
        let m = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let level = check_capability(&m, &Capability::ToolBash);
        if let CapSupportLevel::Emulated { strategy } = level {
            assert!(strategy.contains("sandbox"));
        } else {
            panic!("Expected Emulated from Restricted");
        }
    }

    #[test]
    fn emulated_strategy_default_is_adapter() {
        let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
        let level = check_capability(&m, &Capability::Streaming);
        assert_eq!(
            level,
            CapSupportLevel::Emulated {
                strategy: "adapter".into()
            }
        );
    }
}

// =========================================================================
// Module: workorder_config — capabilities in WorkOrder
// =========================================================================

mod workorder_config {
    use super::*;

    #[test]
    fn workorder_default_requirements_empty() {
        let wo = WorkOrderBuilder::new("test task").build();
        assert!(wo.requirements.required.is_empty());
    }

    #[test]
    fn workorder_with_single_requirement() {
        let r = reqs_native(&[Capability::Streaming]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        assert_eq!(wo.requirements.required.len(), 1);
        assert_eq!(
            wo.requirements.required[0].capability,
            Capability::Streaming
        );
    }

    #[test]
    fn workorder_with_multiple_requirements() {
        let r = reqs(&[
            (Capability::Streaming, MinSupport::Native),
            (Capability::ToolRead, MinSupport::Emulated),
            (Capability::ToolWrite, MinSupport::Native),
        ]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        assert_eq!(wo.requirements.required.len(), 3);
    }

    #[test]
    fn workorder_requirements_serde_roundtrip() {
        let r = reqs(&[
            (Capability::Streaming, MinSupport::Native),
            (Capability::ToolRead, MinSupport::Emulated),
        ]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        let j = serde_json::to_string(&wo).unwrap();
        let back: abp_core::WorkOrder = serde_json::from_str(&j).unwrap();
        assert_eq!(back.requirements.required.len(), 2);
    }

    #[test]
    fn workorder_check_capabilities_against_claude() {
        let r = reqs_native(&[Capability::Streaming, Capability::ToolUse]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        let report = abp_core::negotiate::check_capabilities(&wo, "claude", "claude");
        assert!(report.all_satisfiable());
    }

    #[test]
    fn workorder_check_capabilities_against_openai() {
        let r = reqs_native(&[Capability::Logprobs, Capability::SeedDeterminism]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        let report = abp_core::negotiate::check_capabilities(&wo, "claude", "openai");
        assert!(report.all_satisfiable());
    }

    #[test]
    fn workorder_check_logprobs_fails_on_claude() {
        let r = reqs_native(&[Capability::Logprobs]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        let report = abp_core::negotiate::check_capabilities(&wo, "openai", "claude");
        assert!(!report.all_satisfiable());
    }

    #[test]
    fn workorder_with_model_and_requirements() {
        let r = reqs_native(&[Capability::Streaming]);
        let wo = WorkOrderBuilder::new("task")
            .model("gpt-4")
            .requirements(r)
            .build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(wo.requirements.required.len(), 1);
    }

    #[test]
    fn workorder_check_empty_requirements() {
        let wo = WorkOrderBuilder::new("task").build();
        let report = abp_core::negotiate::check_capabilities(&wo, "claude", "openai");
        assert!(report.all_satisfiable());
        assert!(report.entries.is_empty());
    }

    #[test]
    fn workorder_check_unknown_dialect() {
        let r = reqs_native(&[Capability::Streaming]);
        let wo = WorkOrderBuilder::new("task").requirements(r).build();
        let report = abp_core::negotiate::check_capabilities(&wo, "claude", "unknown");
        assert!(!report.all_satisfiable());
    }
}

// =========================================================================
// Module: capability_report — CapabilityReport methods
// =========================================================================

mod capability_report {
    use super::*;

    fn make_report(entries: Vec<(Capability, DialectSupportLevel)>) -> CapabilityReport {
        CapabilityReport {
            source_dialect: "test_src".into(),
            target_dialect: "test_tgt".into(),
            entries: entries
                .into_iter()
                .map(|(cap, support)| CapabilityReportEntry {
                    capability: cap,
                    support,
                })
                .collect(),
        }
    }

    #[test]
    fn native_capabilities_filter() {
        let report = make_report(vec![
            (Capability::Streaming, DialectSupportLevel::Native),
            (
                Capability::ToolRead,
                DialectSupportLevel::Emulated { detail: "x".into() },
            ),
        ]);
        assert_eq!(report.native_capabilities().len(), 1);
    }

    #[test]
    fn emulated_capabilities_filter() {
        let report = make_report(vec![
            (Capability::Streaming, DialectSupportLevel::Native),
            (
                Capability::ToolRead,
                DialectSupportLevel::Emulated { detail: "x".into() },
            ),
            (
                Capability::ToolWrite,
                DialectSupportLevel::Emulated { detail: "y".into() },
            ),
        ]);
        assert_eq!(report.emulated_capabilities().len(), 2);
    }

    #[test]
    fn unsupported_capabilities_filter() {
        let report = make_report(vec![
            (Capability::Streaming, DialectSupportLevel::Native),
            (
                Capability::Logprobs,
                DialectSupportLevel::Unsupported {
                    reason: "n/a".into(),
                },
            ),
        ]);
        assert_eq!(report.unsupported_capabilities().len(), 1);
    }

    #[test]
    fn all_satisfiable_true() {
        let report = make_report(vec![
            (Capability::Streaming, DialectSupportLevel::Native),
            (
                Capability::ToolRead,
                DialectSupportLevel::Emulated { detail: "x".into() },
            ),
        ]);
        assert!(report.all_satisfiable());
    }

    #[test]
    fn all_satisfiable_false() {
        let report = make_report(vec![(
            Capability::Logprobs,
            DialectSupportLevel::Unsupported {
                reason: "n/a".into(),
            },
        )]);
        assert!(!report.all_satisfiable());
    }

    #[test]
    fn all_satisfiable_empty() {
        let report = make_report(vec![]);
        assert!(report.all_satisfiable());
    }

    #[test]
    fn to_receipt_metadata_is_json() {
        let report = make_report(vec![(Capability::Streaming, DialectSupportLevel::Native)]);
        let val = report.to_receipt_metadata();
        assert!(val.is_object());
    }

    #[test]
    fn to_receipt_metadata_contains_dialects() {
        let report = make_report(vec![(Capability::Streaming, DialectSupportLevel::Native)]);
        let val = report.to_receipt_metadata();
        let obj = val.as_object().unwrap();
        assert_eq!(obj["source_dialect"], "test_src");
        assert_eq!(obj["target_dialect"], "test_tgt");
    }

    #[test]
    fn report_with_mixed_support() {
        let report = make_report(vec![
            (Capability::Streaming, DialectSupportLevel::Native),
            (
                Capability::ToolRead,
                DialectSupportLevel::Emulated {
                    detail: "adapter".into(),
                },
            ),
            (
                Capability::Logprobs,
                DialectSupportLevel::Unsupported {
                    reason: "not available".into(),
                },
            ),
        ]);
        assert_eq!(report.native_capabilities().len(), 1);
        assert_eq!(report.emulated_capabilities().len(), 1);
        assert_eq!(report.unsupported_capabilities().len(), 1);
        assert!(!report.all_satisfiable());
    }
}

// =========================================================================
// Module: compatibility_report — generate_report from abp-capability
// =========================================================================

mod compatibility_report_tests {
    use super::*;

    #[test]
    fn report_compatible_summary() {
        let res = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&res);
        assert!(report.compatible);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn report_incompatible_summary() {
        let res = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&res);
        assert!(!report.compatible);
        assert!(report.summary.contains("incompatible"));
    }

    #[test]
    fn report_counts_correct() {
        let res = NegotiationResult {
            native: vec![Capability::Streaming, Capability::ToolUse],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs, Capability::SeedDeterminism],
        };
        let report = generate_report(&res);
        assert_eq!(report.native_count, 2);
        assert_eq!(report.emulated_count, 1);
        assert_eq!(report.unsupported_count, 2);
    }

    #[test]
    fn report_details_length_matches() {
        let res = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&res);
        assert_eq!(report.details.len(), 3);
    }

    #[test]
    fn report_empty_result() {
        let res = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&res);
        assert!(report.compatible);
        assert_eq!(report.native_count, 0);
    }

    #[test]
    fn report_all_emulated_compatible() {
        let res = NegotiationResult {
            native: vec![],
            emulated: vec![Capability::Streaming, Capability::ToolRead],
            unsupported: vec![],
        };
        let report = generate_report(&res);
        assert!(report.compatible);
        assert_eq!(report.emulated_count, 2);
    }

    #[test]
    fn report_summary_contains_counts() {
        let res = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&res);
        assert!(report.summary.contains("1 native"));
        assert!(report.summary.contains("1 emulatable"));
        assert!(report.summary.contains("1 unsupported"));
    }
}

// =========================================================================
// Module: check_capability_coverage — check_capability for every variant
// =========================================================================

mod check_capability_coverage {
    use super::*;

    #[test]
    fn check_native_returns_native() {
        let m = manifest(&[(Capability::ToolUse, SupportLevel::Native)]);
        assert_eq!(
            check_capability(&m, &Capability::ToolUse),
            CapSupportLevel::Native
        );
    }

    #[test]
    fn check_emulated_returns_emulated() {
        let m = manifest(&[(Capability::ToolUse, SupportLevel::Emulated)]);
        assert!(matches!(
            check_capability(&m, &Capability::ToolUse),
            CapSupportLevel::Emulated { .. }
        ));
    }

    #[test]
    fn check_unsupported_returns_unsupported() {
        let m = manifest(&[(Capability::ToolUse, SupportLevel::Unsupported)]);
        assert_eq!(
            check_capability(&m, &Capability::ToolUse),
            CapSupportLevel::Unsupported
        );
    }

    #[test]
    fn check_missing_returns_unsupported() {
        let m: CapabilityManifest = BTreeMap::new();
        assert_eq!(
            check_capability(&m, &Capability::ToolUse),
            CapSupportLevel::Unsupported
        );
    }

    #[test]
    fn check_restricted_returns_emulated() {
        let m = manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "policy".into(),
            },
        )]);
        let level = check_capability(&m, &Capability::ToolBash);
        assert!(matches!(level, CapSupportLevel::Emulated { .. }));
    }

    #[test]
    fn check_each_capability_against_native_manifest() {
        for cap in all_capabilities() {
            let m = manifest(&[(cap.clone(), SupportLevel::Native)]);
            assert_eq!(
                check_capability(&m, &cap),
                CapSupportLevel::Native,
                "Failed for {cap:?}"
            );
        }
    }

    #[test]
    fn check_each_capability_against_empty_manifest() {
        for cap in all_capabilities() {
            assert_eq!(
                check_capability(&BTreeMap::new(), &cap),
                CapSupportLevel::Unsupported,
                "Failed for {cap:?}"
            );
        }
    }
}
