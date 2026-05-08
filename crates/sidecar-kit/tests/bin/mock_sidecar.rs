// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust mock sidecar for sidecar-kit lifecycle tests.

use serde_json::{Value, json};
use std::env;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

fn make_hello() -> Value {
    json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {"id": "mock-lifecycle", "version": "0.1"},
        "capabilities": {"streaming": true}
    })
}

fn read_run() -> Option<(String, Value)> {
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).ok()? == 0 {
        return None;
    }
    let run: Value = serde_json::from_str(&line).ok()?;
    Some((
        run["id"].as_str()?.to_owned(),
        run.get("work_order").cloned().unwrap_or_else(|| json!({})),
    ))
}

fn make_event(ref_id: &str, payload: Value) -> Value {
    json!({"t": "event", "ref_id": ref_id, "event": payload})
}

fn make_final(ref_id: &str) -> Value {
    json!({"t": "final", "ref_id": ref_id, "receipt": {"status": "complete", "ref_id": ref_id}})
}

fn emit(value: Value) {
    println!(
        "{}",
        serde_json::to_string(&value).expect("serialize frame")
    );
    io::stdout().flush().expect("flush stdout");
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn main() {
    let mode = env::args().nth(1).unwrap_or_else(|| "default".to_owned());

    match mode.as_str() {
        "default" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            emit(make_event(&ref_id, json!({"type": "progress", "step": 1})));
            emit(make_event(&ref_id, json!({"type": "progress", "step": 2})));
            emit(make_final(&ref_id));
        }
        "large_stream" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            for i in 0..100 {
                emit(make_event(&ref_id, json!({"type": "progress", "index": i})));
            }
            emit(make_final(&ref_id));
        }
        "error_midstream" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            emit(make_event(&ref_id, json!({"type": "progress", "step": 1})));
            emit(json!({"t": "fatal", "ref_id": ref_id, "error": "processing failed"}));
        }
        "empty_work_order" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            emit(make_final(&ref_id));
        }
        "tool_call" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            emit(make_event(
                &ref_id,
                json!({"type": "tool_call", "tool": "read_file", "args": {"path": "test.txt"}}),
            ));
            emit(make_event(
                &ref_id,
                json!({"type": "tool_result", "tool": "read_file", "result": "file contents"}),
            ));
            emit(make_final(&ref_id));
        }
        "multi_run" => {
            emit(make_hello());
            for _ in 0..3 {
                let Some((ref_id, _)) = read_run() else {
                    break;
                };
                emit(make_event(&ref_id, json!({"type": "progress", "step": 1})));
                emit(make_final(&ref_id));
            }
        }
        "slow" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            emit(make_event(
                &ref_id,
                json!({"type": "progress", "step": "start"}),
            ));
            sleep_ms(300);
            emit(make_event(
                &ref_id,
                json!({"type": "progress", "step": "middle"}),
            ));
            sleep_ms(300);
            emit(make_event(
                &ref_id,
                json!({"type": "progress", "step": "end"}),
            ));
            emit(make_final(&ref_id));
        }
        "crash" => {
            emit(make_hello());
            let Some((ref_id, _)) = read_run() else {
                return;
            };
            emit(make_event(&ref_id, json!({"type": "progress", "step": 1})));
            std::process::exit(1);
        }
        _ => {
            eprintln!("Unknown mode: {mode}");
            std::process::exit(1);
        }
    }
}
