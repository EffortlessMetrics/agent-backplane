// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz policy profile deserialization with arbitrary input formats.
//!
//! Exercises PolicyProfile deserialization from JSON, TOML, and arbitrary
//! strings. For successfully parsed profiles, compiles into PolicyEngine
//! and exercises tool/path checks.
//!
//! Verifies:
//! 1. Deserialization never panics on any input.
//! 2. Compilation errors are handled gracefully.
//! 3. All query methods are safe on compiled policies.
//! 4. PolicyAuditor wrapping never panics.
//! 5. PolicyValidator never panics on any profile.
#![no_main]
use abp_policy::audit::PolicyAuditor;
use abp_policy::compose::PolicyValidator;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // --- Property 1: try JSON deserialization ---
    let profile_json = serde_json::from_str::<abp_core::PolicyProfile>(s);

    // --- Property 1b: try TOML deserialization ---
    let profile_toml = toml::from_str::<abp_core::PolicyProfile>(s);

    // Process whichever succeeded.
    let profiles: Vec<abp_core::PolicyProfile> = [profile_json.ok(), profile_toml.ok()]
        .into_iter()
        .flatten()
        .collect();

    for profile in profiles {
        // --- Property 5: validator never panics ---
        let _ = PolicyValidator::validate(&profile);

        // --- Property 2: compilation handles errors gracefully ---
        let engine = match abp_policy::PolicyEngine::new(&profile) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // --- Property 3: query methods are safe ---
        // Use parts of the input as query strings.
        for line in s.lines().take(20) {
            let tool = line.trim();
            if !tool.is_empty() {
                let d = engine.can_use_tool(tool);
                let _ = d.allowed;
                let _ = d.reason;
            }
            let path = Path::new(tool);
            let _ = engine.can_read_path(path);
            let _ = engine.can_write_path(path);
        }

        // --- Property 4: auditor wrapping ---
        let mut auditor = PolicyAuditor::new(engine);
        for line in s.lines().take(10) {
            let _ = auditor.check_tool(line.trim());
            let _ = auditor.check_read(line.trim());
            let _ = auditor.check_write(line.trim());
        }
        let _ = auditor.denied_count();
        let _ = auditor.allowed_count();
        let _ = auditor.summary();
    }
});
