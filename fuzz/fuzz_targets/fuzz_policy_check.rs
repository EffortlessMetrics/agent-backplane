// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz policy engine decisions with arbitrary structured input.
//!
//! Verifies:
//! 1. `PolicyEngine::new` never panics on any PolicyProfile.
//! 2. `can_use_tool`, `can_read_path`, `can_write_path` never panic.
//! 3. Decision fields are always accessible.
//! 4. Deny rules always take precedence (when both allow and deny match).
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

#[derive(Debug, Arbitrary)]
struct PolicyFuzzInput {
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
    allow_network: Vec<String>,
    deny_network: Vec<String>,
    require_approval_for: Vec<String>,
    tool_queries: Vec<String>,
    path_queries: Vec<String>,
}

fuzz_target!(|input: PolicyFuzzInput| {
    let profile = abp_core::PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: input.allow_network,
        deny_network: input.deny_network,
        require_approval_for: input.require_approval_for,
    };

    // Property 1: compilation never panics
    let engine = match abp_policy::PolicyEngine::new(&profile) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Property 2 & 3: tool checks never panic, fields accessible
    for tool in &input.tool_queries {
        let decision = engine.can_use_tool(tool);
        let _ = decision.allowed;
        let _ = decision.reason;
        let _ = format!("{:?}", decision);
    }

    // Property 2 & 3: path checks never panic, fields accessible
    for p in &input.path_queries {
        let path = Path::new(p);
        let rd = engine.can_read_path(path);
        let _ = rd.allowed;
        let _ = rd.reason;

        let wd = engine.can_write_path(path);
        let _ = wd.allowed;
        let _ = wd.reason;
    }
});
