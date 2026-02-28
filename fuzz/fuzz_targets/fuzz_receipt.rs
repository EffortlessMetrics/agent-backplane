#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(receipt) = serde_json::from_str::<abp_core::Receipt>(s) {
            let _ = abp_core::receipt_hash(&receipt);
        }
    }
});
