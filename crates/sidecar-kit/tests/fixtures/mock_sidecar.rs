// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust mock sidecar for sidecar-kit lifecycle tests.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::time::Duration;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args()
        .skip(1)
        .last()
        .unwrap_or_else(|| "default".into());
    match mode.as_str() {
        "default" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            emit(&event(&ref_id, json!({ "type": "progress", "step": 1 })))?;
            emit(&event(&ref_id, json!({ "type": "progress", "step": 2 })))?;
            emit(&final_envelope(&ref_id))?;
        }
        "large_stream" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            for i in 0..100 {
                emit(&event(&ref_id, json!({ "type": "progress", "index": i })))?;
            }
            emit(&final_envelope(&ref_id))?;
        }
        "error_midstream" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            emit(&event(&ref_id, json!({ "type": "progress", "step": 1 })))?;
            emit(&json!({ "t": "fatal", "ref_id": ref_id, "error": "processing failed" }))?;
        }
        "empty_work_order" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            emit(&final_envelope(&ref_id))?;
        }
        "tool_call" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            emit(&event(
                &ref_id,
                json!({ "type": "tool_call", "tool": "read_file", "args": { "path": "test.txt" } }),
            ))?;
            emit(&event(
                &ref_id,
                json!({ "type": "tool_result", "tool": "read_file", "result": "file contents" }),
            ))?;
            emit(&final_envelope(&ref_id))?;
        }
        "multi_run" => {
            emit(&hello())?;
            for _ in 0..3 {
                let Some((ref_id, _)) = try_read_run()? else {
                    break;
                };
                emit(&event(&ref_id, json!({ "type": "progress", "step": 1 })))?;
                emit(&final_envelope(&ref_id))?;
            }
        }
        "slow" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            emit(&event(
                &ref_id,
                json!({ "type": "progress", "step": "start" }),
            ))?;
            std::thread::sleep(Duration::from_millis(300));
            emit(&event(
                &ref_id,
                json!({ "type": "progress", "step": "middle" }),
            ))?;
            std::thread::sleep(Duration::from_millis(300));
            emit(&event(
                &ref_id,
                json!({ "type": "progress", "step": "end" }),
            ))?;
            emit(&final_envelope(&ref_id))?;
        }
        "crash" => {
            emit(&hello())?;
            let (ref_id, _) = read_run()?;
            emit(&event(&ref_id, json!({ "type": "progress", "step": 1 })))?;
            std::process::exit(1);
        }
        other => return Err(format!("Unknown mode: {other}").into()),
    }
    Ok(())
}

fn hello() -> Value {
    json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "mock-lifecycle", "version": "0.1" },
        "capabilities": { "streaming": true }
    })
}

fn read_run() -> Result<(String, Value), Box<dyn std::error::Error>> {
    try_read_run()?.ok_or_else(|| "missing run envelope".into())
}

fn try_read_run() -> Result<Option<(String, Value)>, Box<dyn std::error::Error>> {
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let run: Value = serde_json::from_str(&line)?;
    Ok(Some((
        run["id"].as_str().unwrap_or_default().to_string(),
        run.get("work_order").cloned().unwrap_or_else(|| json!({})),
    )))
}

fn event(ref_id: &str, payload: Value) -> Value {
    json!({ "t": "event", "ref_id": ref_id, "event": payload })
}

fn final_envelope(ref_id: &str) -> Value {
    json!({ "t": "final", "ref_id": ref_id, "receipt": { "status": "complete", "ref_id": ref_id } })
}

fn emit(value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(value)?);
    io::stdout().flush()?;
    Ok(())
}
