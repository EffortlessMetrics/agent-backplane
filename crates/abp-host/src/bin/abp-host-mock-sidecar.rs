// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust mock sidecar for abp-host integration tests.
//!
//! This intentionally replaces the former Python test helper so protocol tests
//! do not depend on an external Python runtime.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::process;
use std::thread;
use std::time::Duration;

const CONTRACT_VERSION: &str = "abp/v0.1";
const BACKEND_ID: &str = "mock-test";

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let mode = mode_arg();

    match mode.as_str() {
        "python_no_response" => {
            thread::sleep(Duration::from_secs(5));
        }
        "python_exit_zero" => {}
        "python_invalid_json" => {
            writeln!(io::stdout(), "not json")?;
            io::stdout().flush()?;
        }
        "default" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "mock test started" }),
            )?;
            emit_final(&ref_id)?;
        }
        "multi_events" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            for i in 0..5 {
                emit_event(
                    &ref_id,
                    "run_started",
                    json!({ "message": format!("event {i}") }),
                )?;
            }
            emit_final(&ref_id)?;
        }
        "multi_event_kinds" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(&ref_id, "run_started", json!({ "message": "started" }))?;
            emit_event(&ref_id, "assistant_delta", json!({ "text": "Hello " }))?;
            emit_event(
                &ref_id,
                "assistant_message",
                json!({ "text": "Hello world" }),
            )?;
            emit_event(
                &ref_id,
                "file_changed",
                json!({ "path": "test.txt", "summary": "created" }),
            )?;
            emit_event(&ref_id, "run_completed", json!({ "message": "done" }))?;
            emit_final(&ref_id)?;
        }
        "slow" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "starting slow" }),
            )?;
            thread::sleep(Duration::from_millis(300));
            emit_event(
                &ref_id,
                "assistant_message",
                json!({ "text": "thinking..." }),
            )?;
            thread::sleep(Duration::from_millis(300));
            emit_event(&ref_id, "run_completed", json!({ "message": "done slow" }))?;
            emit_final(&ref_id)?;
        }
        "bad_json_midstream" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "about to break" }),
            )?;
            writeln!(io::stdout(), "this is not valid json {{{{")?;
            io::stdout().flush()?;
            emit_event(
                &ref_id,
                "run_completed",
                json!({ "message": "unreachable" }),
            )?;
            emit_final(&ref_id)?;
        }
        "wrong_version" => {
            emit_hello("abp/v999.0")?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "wrong version" }),
            )?;
            emit_final(&ref_id)?;
        }
        "no_hello" => {
            emit_event("fake", "run_started", json!({ "message": "no hello" }))?;
        }
        "fatal" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "about to fail" }),
            )?;
            emit(json!({ "t": "fatal", "ref_id": ref_id, "error": "something went wrong" }))?;
        }
        "hang" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "going to hang" }),
            )?;
            thread::sleep(Duration::from_secs(5));
        }
        "echo_env" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            let value = std::env::var("ABP_TEST_VAR").unwrap_or_else(|_| "<unset>".to_string());
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": format!("ABP_TEST_VAR={value}") }),
            )?;
            emit_final(&ref_id)?;
        }
        "echo_cwd" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            let cwd = std::env::current_dir().context("read cwd")?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": format!("cwd={}", cwd.display()) }),
            )?;
            emit_final(&ref_id)?;
        }
        "exit_nonzero" => process::exit(42),
        "no_final" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "no final coming" }),
            )?;
            emit_event(
                &ref_id,
                "assistant_message",
                json!({ "text": "still going" }),
            )?;
        }
        "multi_final" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(&ref_id, "run_started", json!({ "message": "multi final" }))?;
            emit_final(&ref_id)?;
            emit_final(&ref_id)?;
        }
        "drop_midstream" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "about to drop" }),
            )?;
            io::stdout().flush()?;
            process::exit(1);
        }
        "hello_extra_fields" => {
            emit(json!({
                "t": "hello",
                "contract_version": CONTRACT_VERSION,
                "backend": backend(),
                "capabilities": {},
                "mode": "mapped",
                "extra_field": "should be ignored",
                "future_feature": { "nested": true }
            }))?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "extra fields ok" }),
            )?;
            emit_final(&ref_id)?;
        }
        "large_payload" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "assistant_message",
                json!({ "text": "A".repeat(100_000) }),
            )?;
            emit_final(&ref_id)?;
        }
        "unicode_content" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "Unicode: 你好世界 🌍 こんにちは мир" }),
            )?;
            emit_event(
                &ref_id,
                "assistant_message",
                json!({ "text": "Emoji: 🚀🎉💻 Math: ∑∫∂ñ" }),
            )?;
            emit_final(&ref_id)?;
        }
        "wrong_ref_id" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(&ref_id, "run_started", json!({ "message": "correct ref" }))?;
            emit_event(
                "wrong-ref-id-12345",
                "assistant_message",
                json!({ "text": "wrong ref" }),
            )?;
            emit_event(
                &ref_id,
                "run_completed",
                json!({ "message": "correct again" }),
            )?;
            emit_final(&ref_id)?;
        }
        "empty_lines" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            println!();
            emit_event(
                &ref_id,
                "run_started",
                json!({ "message": "around empty lines" }),
            )?;
            println!("\n");
            emit_event(
                &ref_id,
                "assistant_message",
                json!({ "text": "still going" }),
            )?;
            println!();
            emit_final(&ref_id)?;
        }
        "tool_call_events" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(&ref_id, "run_started", json!({ "message": "tool test" }))?;
            emit_event(
                &ref_id,
                "tool_call",
                json!({ "tool_name": "read_file", "tool_use_id": "tc-1", "input": { "path": "test.txt" } }),
            )?;
            emit_event(
                &ref_id,
                "tool_result",
                json!({ "tool_name": "read_file", "tool_use_id": "tc-1", "output": { "content": "hello" }, "is_error": false }),
            )?;
            emit_event(&ref_id, "run_completed", json!({ "message": "tools done" }))?;
            emit_final(&ref_id)?;
        }
        "no_hello_hang" => thread::sleep(Duration::from_secs(30)),
        "graceful_exit" => {
            emit_hello(CONTRACT_VERSION)?;
            let ref_id = read_run()?;
            emit_event(&ref_id, "run_started", json!({ "message": "graceful" }))?;
            emit_event(
                &ref_id,
                "run_completed",
                json!({ "message": "done gracefully" }),
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

fn emit_hello(version: &str) -> Result<()> {
    emit(json!({
        "t": "hello",
        "contract_version": version,
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
