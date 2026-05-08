// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust mock sidecar for sidecar-kit lifecycle tests.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::process;
use std::thread;
use std::time::Duration;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mode = mode_arg();
    match mode.as_str() {
        "default" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            emit_event(&ref_id, json!({ "type": "progress", "step": 1 }))?;
            emit_event(&ref_id, json!({ "type": "progress", "step": 2 }))?;
            emit_final(&ref_id)?;
        }
        "large_stream" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            for i in 0..100 {
                emit_event(&ref_id, json!({ "type": "progress", "index": i }))?;
            }
            emit_final(&ref_id)?;
        }
        "error_midstream" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            emit_event(&ref_id, json!({ "type": "progress", "step": 1 }))?;
            emit(json!({ "t": "fatal", "ref_id": ref_id, "error": "processing failed" }))?;
        }
        "empty_work_order" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            emit_final(&ref_id)?;
        }
        "tool_call" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            emit_event(
                &ref_id,
                json!({ "type": "tool_call", "tool": "read_file", "args": { "path": "test.txt" } }),
            )?;
            emit_event(
                &ref_id,
                json!({ "type": "tool_result", "tool": "read_file", "result": "file contents" }),
            )?;
            emit_final(&ref_id)?;
        }
        "multi_run" => {
            emit_hello()?;
            for _ in 0..3 {
                match read_run() {
                    Ok((ref_id, _)) => {
                        emit_event(&ref_id, json!({ "type": "progress", "step": 1 }))?;
                        emit_final(&ref_id)?;
                    }
                    Err(_) => break,
                }
            }
        }
        "slow" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            emit_event(&ref_id, json!({ "type": "progress", "step": "start" }))?;
            thread::sleep(Duration::from_millis(300));
            emit_event(&ref_id, json!({ "type": "progress", "step": "middle" }))?;
            thread::sleep(Duration::from_millis(300));
            emit_event(&ref_id, json!({ "type": "progress", "step": "end" }))?;
            emit_final(&ref_id)?;
        }
        "crash" => {
            emit_hello()?;
            let (ref_id, _) = read_run()?;
            emit_event(&ref_id, json!({ "type": "progress", "step": 1 }))?;
            io::stdout().flush()?;
            process::exit(1);
        }
        _ => return Err(format!("Unknown mode: {mode}").into()),
    }
    Ok(())
}

fn mode_arg() -> String {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => "default".to_string(),
        [script] if script.ends_with("mock_sidecar.py") => "default".to_string(),
        [script, mode, ..] if script.ends_with("mock_sidecar.py") => mode.clone(),
        [mode, ..] => mode.clone(),
    }
}

fn emit_hello() -> Result<(), Box<dyn std::error::Error>> {
    emit(json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "mock-lifecycle", "version": "0.1" },
        "capabilities": { "streaming": true }
    }))
}

fn read_run() -> Result<(String, Value), Box<dyn std::error::Error>> {
    let mut line = String::new();
    let n = io::stdin().lock().read_line(&mut line)?;
    if n == 0 {
        process::exit(0);
    }
    let run: Value = serde_json::from_str(&line)?;
    let id = run
        .get("id")
        .and_then(Value::as_str)
        .ok_or("run envelope missing string id")?
        .to_string();
    Ok((
        id,
        run.get("work_order").cloned().unwrap_or_else(|| json!({})),
    ))
}

fn emit_event(ref_id: &str, payload: Value) -> Result<(), Box<dyn std::error::Error>> {
    emit(json!({ "t": "event", "ref_id": ref_id, "event": payload }))
}

fn emit_final(ref_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    emit(json!({
        "t": "final",
        "ref_id": ref_id,
        "receipt": { "status": "complete", "ref_id": ref_id }
    }))
}

fn emit(value: Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value)?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}
