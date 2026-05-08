// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust deep mock sidecar for abp-host protocol conformance tests.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::process;
use std::thread;
use std::time::Duration;

const CONTRACT_VERSION: &str = "abp/v0.1";
const BACKEND_ID: &str = "mock-deep";

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let mode = mode_arg();

    match mode.as_str() {
        "python_exit_zero" => {}
        "python_invalid_json" => {
            writeln!(io::stdout(), "NOT JSON")?;
            io::stdout().flush()?;
        }
        "fatal_with_code" => {
            emit_hello()?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "about to fail with code" }),
            )?;
            emit(json!({
                "t": "fatal",
                "ref_id": ref_id,
                "error": "rate limited",
                "error_code": "backend_rate_limited"
            }))?;
        }
        "wrong_ref_final" => {
            emit_hello()?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "will send wrong final" }),
            )?;
            emit(json!({
                "t": "final",
                "ref_id": "totally-wrong-ref-id",
                "receipt": receipt("totally-wrong-ref-id")
            }))?;
        }
        "slow_hello" => {
            thread::sleep(Duration::from_secs(1));
            emit_hello()?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "slow hello done" }),
            )?;
            emit_final(&ref_id)?;
        }
        _ => bail!("Unknown mode: {mode}"),
    }

    Ok(())
}

fn mode_arg() -> String {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => "default".to_string(),
        [flag, snippet, ..] if flag == "-c" => {
            if snippet.contains("time.sleep") {
                "python_no_response".to_string()
            } else if snippet.contains("print") {
                "python_invalid_json".to_string()
            } else {
                "python_exit_zero".to_string()
            }
        }
        [script]
            if script.ends_with("mock_sidecar.py") || script.ends_with("mock_sidecar_deep.py") =>
        {
            "default".to_string()
        }
        [script, mode, ..]
            if script.ends_with("mock_sidecar.py") || script.ends_with("mock_sidecar_deep.py") =>
        {
            mode.clone()
        }
        [mode, ..] => mode.clone(),
    }
}

fn backend() -> Value {
    json!({
        "id": BACKEND_ID,
        "backend_version": "0.1",
        "adapter_version": "0.1"
    })
}

fn emit_hello() -> Result<()> {
    emit(json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": backend(),
        "capabilities": {},
        "mode": "mapped"
    }))
}

fn read_run() -> Result<String> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("read run envelope")?;
    let run: Value = serde_json::from_str(&line).context("parse run envelope")?;
    run.get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .context("run envelope missing string id")
}

fn emit_event(ref_id: &str, event_type: &str, fields: Value) -> Result<()> {
    let mut event = serde_json::Map::new();
    event.insert("ts".to_string(), json!("2024-01-01T00:00:00Z"));
    event.insert("type".to_string(), json!(event_type));
    if let Value::Object(fields) = fields {
        event.extend(fields);
    }
    emit(json!({ "t": "event", "ref_id": ref_id, "event": event }))
}

fn emit_final(ref_id: &str) -> Result<()> {
    emit(json!({ "t": "final", "ref_id": ref_id, "receipt": receipt(ref_id) }))
}

fn receipt(ref_id: &str) -> Value {
    let now = Utc::now().to_rfc3339();
    json!({
        "meta": {
            "run_id": ref_id,
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": CONTRACT_VERSION,
            "started_at": now,
            "finished_at": now,
            "duration_ms": 0
        },
        "backend": backend(),
        "capabilities": {},
        "mode": "mapped",
        "usage_raw": {},
        "usage": { "input_tokens": 0, "output_tokens": 0 },
        "trace": [],
        "artifacts": [],
        "verification": { "harness_ok": true },
        "outcome": "complete",
        "receipt_sha256": null
    })
}

fn emit(value: Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).context("write JSONL value")?;
    writeln!(stdout).context("write JSONL newline")?;
    stdout.flush().context("flush JSONL value")
}
