// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz config parsing with arbitrary TOML-like strings.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Attempt to parse as TOML into RuntimeConfig — must never panic.
        let _ = toml::from_str::<abp_core::RuntimeConfig>(s);

        // Attempt to parse as TOML into PolicyProfile.
        let _ = toml::from_str::<abp_core::PolicyProfile>(s);

        // Attempt to parse as TOML into WorkspaceSpec.
        let _ = toml::from_str::<abp_core::WorkspaceSpec>(s);

        // Attempt to parse as generic TOML value then re-serialize as JSON.
        if let Ok(val) = toml::from_str::<toml::Value>(s) {
            let _ = serde_json::to_string(&val);
        }
    }
});
