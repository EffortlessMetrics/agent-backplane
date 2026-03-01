// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz policy evaluation with random tool names, paths, and policy profiles.
//!
//! Constructs PolicyProfile from fuzzer-derived data, compiles into both
//! PolicyEngine and ComposedEngine, then exercises tool/read/write checks.
//! Also tests PolicyAuditor wrapping and PolicyValidator. Ensures no panics
//! regardless of pattern or query content.
#![no_main]
use abp_policy::audit::PolicyAuditor;
use abp_policy::compose::{ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator};
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

#[derive(Debug, Arbitrary)]
struct PolicyFuzzInput {
    // First policy profile.
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
    allow_network: Vec<String>,
    deny_network: Vec<String>,
    require_approval_for: Vec<String>,
    // Second policy profile (for composition).
    allowed_tools_2: Vec<String>,
    disallowed_tools_2: Vec<String>,
    deny_read_2: Vec<String>,
    deny_write_2: Vec<String>,
    // Queries.
    tool_queries: Vec<String>,
    path_queries: Vec<String>,
    // Composition mode selector.
    precedence_idx: u8,
}

fuzz_target!(|input: PolicyFuzzInput| {
    let profile1 = abp_core::PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: input.allow_network,
        deny_network: input.deny_network,
        require_approval_for: input.require_approval_for,
    };

    // --- Validate the profile (never panics) ---
    let _ = PolicyValidator::validate(&profile1);

    // --- Single PolicyEngine ---
    if let Ok(engine) = abp_policy::PolicyEngine::new(&profile1) {
        for tool in &input.tool_queries {
            let d = engine.can_use_tool(tool);
            // Decision fields must be accessible.
            let _ = d.allowed;
            let _ = d.reason;
        }
        for p in &input.path_queries {
            let path = Path::new(p);
            let rd = engine.can_read_path(path);
            let wd = engine.can_write_path(path);
            let _ = (rd.allowed, wd.allowed);
        }

        // --- PolicyAuditor wrapping ---
        let mut auditor = PolicyAuditor::new(engine);
        for tool in &input.tool_queries {
            let _ = auditor.check_tool(tool);
        }
        for p in &input.path_queries {
            let _ = auditor.check_read(p);
            let _ = auditor.check_write(p);
        }
        let _ = auditor.denied_count();
        let _ = auditor.allowed_count();
        let _ = auditor.summary();
        let _ = auditor.entries();
    }

    // --- ComposedEngine with two profiles ---
    let profile2 = abp_core::PolicyProfile {
        allowed_tools: input.allowed_tools_2,
        disallowed_tools: input.disallowed_tools_2,
        deny_read: input.deny_read_2,
        deny_write: input.deny_write_2,
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };

    let precedence = match input.precedence_idx % 3 {
        0 => PolicyPrecedence::DenyOverrides,
        1 => PolicyPrecedence::AllowOverrides,
        _ => PolicyPrecedence::FirstApplicable,
    };

    if let Ok(composed) = ComposedEngine::new(
        vec![profile1.clone(), profile2],
        precedence,
    ) {
        for tool in &input.tool_queries {
            let _: abp_policy::compose::PolicyDecision = composed.check_tool(tool);
        }
        for p in &input.path_queries {
            let _: abp_policy::compose::PolicyDecision = composed.check_read(p);
            let _: abp_policy::compose::PolicyDecision = composed.check_write(p);
        }
    }

    // --- PolicySet merge ---
    let mut pset = PolicySet::new("fuzz-set");
    pset.add(profile1);
    let merged = pset.merge();
    let _ = abp_policy::PolicyEngine::new(&merged);
});
