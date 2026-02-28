// SPDX-License-Identifier: MIT OR Apache-2.0
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::BufReader;

fuzz_target!(|data: &[u8]| {
    let reader = BufReader::new(data);
    for result in abp_protocol::JsonlCodec::decode_stream(reader) {
        let _ = result;
    }
});
