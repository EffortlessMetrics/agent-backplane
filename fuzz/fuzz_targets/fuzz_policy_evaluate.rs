// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz policy evaluation with arbitrary paths and tool names.
//!
//! Constructs PolicyProfile from fuzzer-derived data, compiles into
//! PolicyEngine, and exercises tool/read/write checks with arbitrary
//! query strings. Also tests PolicyAuditor and ComposedEngine.
//!
//! Verifies:
//! 1. PolicyEngine::new never panics on any pattern set.
//! 2. can_use_tool / can_read_path / can_write_path never panic.
//! 3. PolicyAuditor wrapping never panics and counters are consistent.
//! 4. ComposedEngine with multiple profiles never panics.
//! 5. PolicyValidator never panics on any profile.
#![no_main]
use abp_policy::audit::PolicyAuditor;
use abp_policy::compose::{ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator};
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

#[derive(Debug, Arbitrary)]
struct PolicyEvalInput {
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
    allow_network: Vec<String>,
    deny_network: Vec<String>,
    require_approval_for: Vec<String>,
    // Second profile for composition.
    allowed_tools_2: Vec<String>,
    disallowed_tools_2: Vec<String>,
    deny_read_2: Vec<String>,
    deny_write_2: Vec<String>,
    // Query strings.
    tool_queries: Vec<String>,
    path_queries: Vec<String>,
    // Composition mode.
    precedence_idx: u8,
}

fuzz_target!(|input: PolicyEvalInput| {
    let profile = abp_core::PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: input.allow_network,
        deny_network: input.deny_network,
        require_approval_for: input.require_approval_for,
    };

    // --- Property 5: PolicyValidator never panics ---
    let _ = PolicyValidator::validate(&profile);

    // --- Property 1: PolicyEngine::new never panics ---
    let engine = match abp_policy::PolicyEngine::new(&profile) {
        Ok(e) => e,
        Err(_) => return,
    };

    // --- Property 2: evaluation queries never panic ---
    for tool in &input.tool_queries {
        let d = engine.can_use_tool(tool);
        let _ = d.allowed;
        let _ = d.reason;
    }
    for p in &input.path_queries {
        let path = Path::new(p);
        let rd = engine.can_read_path(path);
        let wd = engine.can_write_path(path);
        let _ = (rd.allowed, wd.allowed);
    }

    // --- Property 3: PolicyAuditor wrapping ---
    let mut auditor = PolicyAuditor::new(engine);
    for tool in &input.tool_queries {
        let _ = auditor.check_tool(tool);
    }
    for p in &input.path_queries {
        let _ = auditor.check_read(p);
        let _ = auditor.check_write(p);
    }
    // Counters must be consistent with total queries.
    let total = auditor.denied_count() + auditor.allowed_count();
    let expected = input.tool_queries.len() + input.path_queries.len() * 2;
    assert_eq!(
        total, expected,
        "auditor counters must sum to total queries"
    );
    let _ = auditor.summary();
    let _ = auditor.entries();

    // --- Property 4: ComposedEngine with two profiles ---
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

    if let Ok(composed) = ComposedEngine::new(vec![profile.clone(), profile2], precedence) {
        for tool in &input.tool_queries {
            let _ = composed.check_tool(tool);
        }
        for p in &input.path_queries {
            let _ = composed.check_read(p);
            let _ = composed.check_write(p);
        }
    }

    // --- PolicySet merge ---
    let mut pset = PolicySet::new("fuzz-eval-set");
    pset.add(profile);
    let merged = pset.merge();
    let _ = abp_policy::PolicyEngine::new(&merged);
});
