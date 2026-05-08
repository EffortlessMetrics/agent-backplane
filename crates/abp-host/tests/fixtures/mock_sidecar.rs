// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust mock sidecar for abp-host integration tests.

use chrono::Utc;
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.last().map(String::as_str).unwrap_or("default");

    match mode {
        "default" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "mock test started" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "multi_events" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            for i in 0..5 {
                emit(&event(
                    &ref_id,
                    "run_started",
                    json!({ "message": format!("event {i}") }),
                ))?;
            }
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "multi_event_kinds" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "started" }),
            ))?;
            emit(&event(
                &ref_id,
                "assistant_delta",
                json!({ "text": "Hello " }),
            ))?;
            emit(&event(
                &ref_id,
                "assistant_message",
                json!({ "text": "Hello world" }),
            ))?;
            emit(&event(
                &ref_id,
                "file_changed",
                json!({ "path": "test.txt", "summary": "created" }),
            ))?;
            emit(&event(
                &ref_id,
                "run_completed",
                json!({ "message": "done" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "slow" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "starting slow" }),
            ))?;
            std::thread::sleep(Duration::from_millis(300));
            emit(&event(
                &ref_id,
                "assistant_message",
                json!({ "text": "thinking..." }),
            ))?;
            std::thread::sleep(Duration::from_millis(300));
            emit(&event(
                &ref_id,
                "run_completed",
                json!({ "message": "done slow" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "bad_json_midstream" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "about to break" }),
            ))?;
            println!("this is not valid json {{{{");
            io::stdout().flush()?;
            emit(&event(
                &ref_id,
                "run_completed",
                json!({ "message": "unreachable" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "wrong_version" => {
            emit(&hello("abp/v999.0", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "wrong version" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "no_hello" => emit(&event(
            "fake",
            "run_started",
            json!({ "message": "no hello" }),
        ))?,
        "fatal" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "about to fail" }),
            ))?;
            emit(&json!({ "t": "fatal", "ref_id": ref_id, "error": "something went wrong" }))?;
        }
        "hang" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "going to hang" }),
            ))?;
            std::thread::sleep(Duration::from_secs(5));
        }
        "echo_env" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            let val = std::env::var("ABP_TEST_VAR").unwrap_or_else(|_| "<unset>".into());
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": format!("ABP_TEST_VAR={val}") }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "echo_cwd" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            let cwd = std::env::current_dir()?.display().to_string();
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": format!("cwd={cwd}") }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "exit_nonzero" => std::process::exit(42),
        "no_final" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "no final coming" }),
            ))?;
            emit(&event(
                &ref_id,
                "assistant_message",
                json!({ "text": "still going" }),
            ))?;
        }
        "multi_final" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "multi final" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "drop_midstream" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "about to drop" }),
            ))?;
            std::process::exit(1);
        }
        "hello_extra_fields" => {
            let mut hello = hello("abp/v0.1", "mock-test");
            hello["extra_field"] = json!("should be ignored");
            hello["future_feature"] = json!({ "nested": true });
            emit(&hello)?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "extra fields ok" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "large_payload" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "assistant_message",
                json!({ "text": "A".repeat(100_000) }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "unicode_content" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "Unicode: 你好世界 🌍 こんにちは мир" }),
            ))?;
            emit(&event(
                &ref_id,
                "assistant_message",
                json!({ "text": "Emoji: 🚀🎉💻 Math: ∑∫∂ñ" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "wrong_ref_id" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "correct ref" }),
            ))?;
            emit(&event(
                "wrong-ref-id-12345",
                "assistant_message",
                json!({ "text": "wrong ref" }),
            ))?;
            emit(&event(
                &ref_id,
                "run_completed",
                json!({ "message": "correct again" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "empty_lines" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            println!();
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "around empty lines" }),
            ))?;
            println!();
            println!();
            emit(&event(
                &ref_id,
                "assistant_message",
                json!({ "text": "still going" }),
            ))?;
            println!();
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "tool_call_events" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "tool test" }),
            ))?;
            emit(&event(
                &ref_id,
                "tool_call",
                json!({ "tool_name": "read_file", "tool_use_id": "tc-1", "input": { "path": "test.txt" } }),
            ))?;
            emit(&event(
                &ref_id,
                "tool_result",
                json!({ "tool_name": "read_file", "tool_use_id": "tc-1", "output": { "content": "hello" }, "is_error": false }),
            ))?;
            emit(&event(
                &ref_id,
                "run_completed",
                json!({ "message": "tools done" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "no_hello_hang" => std::thread::sleep(Duration::from_secs(30)),
        "graceful_exit" => {
            emit(&hello("abp/v0.1", "mock-test"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "graceful" }),
            ))?;
            emit(&event(
                &ref_id,
                "run_completed",
                json!({ "message": "done gracefully" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-test"))?;
        }
        "fatal_with_code" => {
            emit(&hello("abp/v0.1", "mock-deep"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "about to fail with code" }),
            ))?;
            emit(
                &json!({ "t": "fatal", "ref_id": ref_id, "error": "rate limited", "error_code": "backend_rate_limited" }),
            )?;
        }
        "wrong_ref_final" => {
            emit(&hello("abp/v0.1", "mock-deep"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "will send wrong final" }),
            ))?;
            emit(&final_envelope("totally-wrong-ref-id", "mock-deep"))?;
        }
        "slow_hello" => {
            std::thread::sleep(Duration::from_secs(1));
            emit(&hello("abp/v0.1", "mock-deep"))?;
            let ref_id = read_run()?;
            emit(&event(
                &ref_id,
                "run_started",
                json!({ "message": "slow hello done" }),
            ))?;
            emit(&final_envelope(&ref_id, "mock-deep"))?;
        }
        other => return Err(format!("Unknown mode: {other}").into()),
    }
    Ok(())
}

fn hello(version: &str, backend_id: &str) -> Value {
    json!({
        "t": "hello",
        "contract_version": version,
        "backend": {
            "id": backend_id,
            "backend_version": "0.1",
            "adapter_version": "0.1"
        },
        "capabilities": {},
        "mode": "mapped"
    })
}

fn read_run() -> Result<String, Box<dyn std::error::Error>> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let run: Value = serde_json::from_str(&line)?;
    Ok(run["id"].as_str().unwrap_or_default().to_string())
}

fn event(ref_id: &str, event_type: &str, fields: Value) -> Value {
    let mut event = json!({ "ts": "2024-01-01T00:00:00Z", "type": event_type });
    if let (Some(dst), Some(src)) = (event.as_object_mut(), fields.as_object()) {
        dst.extend(src.clone());
    }
    json!({ "t": "event", "ref_id": ref_id, "event": event })
}

fn receipt(ref_id: &str, backend_id: &str) -> Value {
    let now = Utc::now().to_rfc3339();
    json!({
        "meta": {
            "run_id": ref_id,
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": now,
            "finished_at": now,
            "duration_ms": 0
        },
        "backend": {
            "id": backend_id,
            "backend_version": "0.1",
            "adapter_version": "0.1"
        },
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

fn final_envelope(ref_id: &str, backend_id: &str) -> Value {
    json!({ "t": "final", "ref_id": ref_id, "receipt": receipt(ref_id, backend_id) })
}

fn emit(value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(value)?);
    io::stdout().flush()?;
    Ok(())
}
