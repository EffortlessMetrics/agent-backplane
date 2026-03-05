// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz PolicyProfile compilation into PolicyEngine.
//!
//! Constructs a PolicyProfile from arbitrary strings and feeds it to the
//! policy compiler. Verifies that compilation never panics (it may return
//! errors on invalid globs) and that subsequent checks are crash-free.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

#[derive(Debug, Arbitrary)]
struct FuzzPolicy {
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    deny_read: Vec<String>,
    deny_write: Vec<String>,
    allow_network: Vec<String>,
    deny_network: Vec<String>,
    require_approval_for: Vec<String>,
    /// Paths and tool names to probe against the compiled engine.
    probe_tools: Vec<String>,
    probe_paths: Vec<String>,
}

fuzz_target!(|input: FuzzPolicy| {
    let profile = abp_core::PolicyProfile {
        allowed_tools: input.allowed_tools,
        disallowed_tools: input.disallowed_tools,
        deny_read: input.deny_read,
        deny_write: input.deny_write,
        allow_network: input.allow_network,
        deny_network: input.deny_network,
        require_approval_for: input.require_approval_for,
    };

    // Compilation may fail on malformed globs — that is expected.
    let engine = match abp_policy::PolicyEngine::new(&profile) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Tool checks must never panic.
    for tool in &input.probe_tools {
        let d = engine.can_use_tool(tool);
        let _ = d.allowed;
        let _ = d.reason;
    }

    // Path checks must never panic.
    for p in &input.probe_paths {
        let _ = engine.can_read_path(Path::new(p));
        let _ = engine.can_write_path(Path::new(p));
    }
});
