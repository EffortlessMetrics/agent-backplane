// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz PolicyEngine compilation and checking with arbitrary patterns and paths.
//!
//! Constructs a PolicyProfile from structured fuzzer input, compiles it into
//! a PolicyEngine, and exercises tool/read/write checks with arbitrary names.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

#[derive(Debug, Arbitrary)]
struct PolicyInput {
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
    tool_queries: Vec<String>,
    path_queries: Vec<String>,
}

fuzz_target!(|input: PolicyInput| {
    let profile = abp_core::PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };

    // Compilation may fail on invalid glob patterns — that's expected.
    let engine = match abp_policy::PolicyEngine::new(&profile) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Exercise tool checking — must never panic.
    for tool in &input.tool_queries {
        let d = engine.can_use_tool(tool);
        let _ = d.allowed;
        let _ = d.reason;
    }

    // Exercise path checking — must never panic.
    for p in &input.path_queries {
        let path = Path::new(p);
        let rd = engine.can_read_path(path);
        let _ = rd.allowed;
        let wd = engine.can_write_path(path);
        let _ = wd.allowed;
    }
});
