// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz BackplaneConfig TOML parsing and validation.
//!
//! Feeds arbitrary byte strings through `parse_toml` and `validate_config`,
//! verifying:
//! 1. `parse_toml` never panics on arbitrary input.
//! 2. Successfully parsed configs can be validated without panics.
//! 3. Round-trip: serialize back to TOML and re-parse produces the same config.
//! 4. `validate_config` warnings are well-formed (Display never panics).
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // --- Property 1: parse_toml never panics ---
    let config = match abp_config::parse_toml(s) {
        Ok(c) => c,
        Err(_) => return,
    };

    // --- Property 2: validate_config never panics ---
    match abp_config::validate_config(&config) {
        Ok(warnings) => {
            // Property 4: Display on warnings never panics.
            for w in &warnings {
                let _ = format!("{w}");
            }
        }
        Err(e) => {
            let _ = format!("{e}");
        }
    }

    // --- Property 3: round-trip through TOML serialization ---
    if let Ok(toml_str) = toml::to_string(&config) {
        if let Ok(rt) = abp_config::parse_toml(&toml_str) {
            assert_eq!(config, rt, "TOML round-trip must be lossless");
        }
    }

    // --- Property 5: serde_json round-trip ---
    if let Ok(json) = serde_json::to_string(&config) {
        let _ = serde_json::from_str::<abp_config::BackplaneConfig>(&json);
    }
});
