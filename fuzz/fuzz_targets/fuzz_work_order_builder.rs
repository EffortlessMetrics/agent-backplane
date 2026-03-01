// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz WorkOrderBuilder with random inputs.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Use the raw string as the task.
        let builder = abp_core::WorkOrderBuilder::new(s);

        // Split data to derive additional builder inputs.
        let parts: Vec<&str> = s.splitn(5, '\n').collect();

        let mut b = builder;
        if let Some(root) = parts.get(1) {
            b = b.root(*root);
        }
        if let Some(model) = parts.get(2) {
            b = b.model(*model);
        }
        if let Some(turns_str) = parts.get(3) {
            if let Ok(turns) = turns_str.parse::<u32>() {
                b = b.max_turns(turns);
            }
        }
        if let Some(budget_str) = parts.get(4) {
            if let Ok(budget) = budget_str.parse::<f64>() {
                if budget.is_finite() {
                    b = b.max_budget_usd(budget);
                }
            }
        }

        // Build must never panic.
        let wo = b.build();

        // Round-trip through JSON must not panic.
        if let Ok(json) = serde_json::to_string(&wo) {
            let _ = serde_json::from_str::<abp_core::WorkOrder>(&json);
        }
    }
});
