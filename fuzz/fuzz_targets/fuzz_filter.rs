// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz EventFilter with arbitrary events and filter configurations.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Try to parse as a JSON object with "filter_kinds" and "event" fields.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            // Extract filter kind names.
            let kinds: Vec<String> = val
                .get("kinds")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let kind_refs: Vec<&str> = kinds.iter().map(|s| s.as_str()).collect();

            // Try parsing an event from the "event" field.
            if let Some(event_val) = val.get("event") {
                if let Ok(event) = serde_json::from_value::<abp_core::AgentEvent>(event_val.clone())
                {
                    // Exercise both filter modes — must never panic.
                    let include = abp_core::filter::EventFilter::include_kinds(&kind_refs);
                    let exclude = abp_core::filter::EventFilter::exclude_kinds(&kind_refs);
                    let _ = include.matches(&event);
                    let _ = exclude.matches(&event);
                }
            }
        }
    }
});
