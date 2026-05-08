// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rust mock sidecar for abp-host integration tests.

use serde_json::{Map, Value, json};
use std::env;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

fn make_hello(backend_id: &str, version: &str) -> Value {
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

fn read_run() -> String {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .expect("read run frame");
    let run: Value = serde_json::from_str(&line).expect("parse run frame");
    run["id"].as_str().expect("run id").to_owned()
}

fn make_event(
    ref_id: &str,
    event_type: &str,
    extra: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    let mut event = Map::new();
    event.insert("ts".to_owned(), json!("2024-01-01T00:00:00Z"));
    event.insert("type".to_owned(), json!(event_type));
    for (key, value) in extra {
        event.insert(key.to_owned(), value);
    }

    json!({"t": "event", "ref_id": ref_id, "event": event})
}

fn make_receipt(backend_id: &str, ref_id: &str) -> Value {
    let now = chrono::Utc::now().to_rfc3339();
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
        "usage": {"input_tokens": 0, "output_tokens": 0},
        "trace": [],
        "artifacts": [],
        "verification": {"harness_ok": true},
        "outcome": "complete",
        "receipt_sha256": null
    })
}

fn make_final(backend_id: &str, ref_id: &str) -> Value {
    json!({"t": "final", "ref_id": ref_id, "receipt": make_receipt(backend_id, ref_id)})
}

fn emit(value: Value) {
    println!(
        "{}",
        serde_json::to_string(&value).expect("serialize frame")
    );
    io::stdout().flush().expect("flush stdout");
}

fn empty_line() {
    println!();
    io::stdout().flush().expect("flush stdout");
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn main() {
    let mode = env::args().nth(1).unwrap_or_else(|| "default".to_owned());
    let backend_id = if matches!(
        mode.as_str(),
        "fatal_with_code" | "wrong_ref_final" | "slow_hello"
    ) {
        "mock-deep"
    } else {
        "mock-test"
    };

    match mode.as_str() {
        "invalid_json_first_line" => {
            println!("NOT JSON");
            io::stdout().flush().expect("flush stdout");
        }
        "default" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("mock test started"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "multi_events" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            for i in 0..5 {
                emit(make_event(
                    &ref_id,
                    "run_started",
                    [("message", json!(format!("event {i}")))],
                ));
            }
            emit(make_final(backend_id, &ref_id));
        }
        "multi_event_kinds" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("started"))],
            ));
            emit(make_event(
                &ref_id,
                "assistant_delta",
                [("text", json!("Hello "))],
            ));
            emit(make_event(
                &ref_id,
                "assistant_message",
                [("text", json!("Hello world"))],
            ));
            emit(make_event(
                &ref_id,
                "file_changed",
                [("path", json!("test.txt")), ("summary", json!("created"))],
            ));
            emit(make_event(
                &ref_id,
                "run_completed",
                [("message", json!("done"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "slow" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("starting slow"))],
            ));
            sleep_ms(300);
            emit(make_event(
                &ref_id,
                "assistant_message",
                [("text", json!("thinking..."))],
            ));
            sleep_ms(300);
            emit(make_event(
                &ref_id,
                "run_completed",
                [("message", json!("done slow"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "bad_json_midstream" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("about to break"))],
            ));
            println!("this is not valid json {{{{");
            io::stdout().flush().expect("flush stdout");
            emit(make_event(
                &ref_id,
                "run_completed",
                [("message", json!("unreachable"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "wrong_version" => {
            emit(make_hello(backend_id, "abp/v999.0"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("wrong version"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "no_hello" => emit(make_event(
            "fake",
            "run_started",
            [("message", json!("no hello"))],
        )),
        "fatal" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("about to fail"))],
            ));
            emit(json!({"t": "fatal", "ref_id": ref_id, "error": "something went wrong"}));
        }
        "hang" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("going to hang"))],
            ));
            sleep_ms(5_000);
        }
        "echo_env" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            let val = env::var("ABP_TEST_VAR").unwrap_or_else(|_| "<unset>".to_owned());
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!(format!("ABP_TEST_VAR={val}")))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "echo_cwd" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            let cwd = env::current_dir()
                .expect("current dir")
                .display()
                .to_string();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!(format!("cwd={cwd}")))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "exit_zero" => std::process::exit(0),
        "exit_nonzero" => std::process::exit(42),
        "no_final" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("no final coming"))],
            ));
            emit(make_event(
                &ref_id,
                "assistant_message",
                [("text", json!("still going"))],
            ));
        }
        "multi_final" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("multi final"))],
            ));
            emit(make_final(backend_id, &ref_id));
            emit(make_final(backend_id, &ref_id));
        }
        "drop_midstream" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("about to drop"))],
            ));
            std::process::exit(1);
        }
        "hello_extra_fields" => {
            let mut hello = make_hello(backend_id, "abp/v0.1");
            hello["extra_field"] = json!("should be ignored");
            hello["future_feature"] = json!({"nested": true});
            emit(hello);
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("extra fields ok"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "large_payload" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "assistant_message",
                [("text", json!("A".repeat(100_000)))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "unicode_content" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("Unicode: 你好世界 🌍 こんにちは мир"))],
            ));
            emit(make_event(
                &ref_id,
                "assistant_message",
                [("text", json!("Emoji: 🚀🎉💻 Math: ∑∫∂ñ"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "wrong_ref_id" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("correct ref"))],
            ));
            emit(make_event(
                "wrong-ref-id-12345",
                "assistant_message",
                [("text", json!("wrong ref"))],
            ));
            emit(make_event(
                &ref_id,
                "run_completed",
                [("message", json!("correct again"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "empty_lines" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            empty_line();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("around empty lines"))],
            ));
            empty_line();
            empty_line();
            emit(make_event(
                &ref_id,
                "assistant_message",
                [("text", json!("still going"))],
            ));
            empty_line();
            emit(make_final(backend_id, &ref_id));
        }
        "tool_call_events" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("tool test"))],
            ));
            emit(make_event(
                &ref_id,
                "tool_call",
                [
                    ("tool_name", json!("read_file")),
                    ("tool_use_id", json!("tc-1")),
                    ("input", json!({"path": "test.txt"})),
                ],
            ));
            emit(make_event(
                &ref_id,
                "tool_result",
                [
                    ("tool_name", json!("read_file")),
                    ("tool_use_id", json!("tc-1")),
                    ("output", json!({"content": "hello"})),
                    ("is_error", json!(false)),
                ],
            ));
            emit(make_event(
                &ref_id,
                "run_completed",
                [("message", json!("tools done"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "no_hello_hang" => sleep_ms(30_000),
        "graceful_exit" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("graceful"))],
            ));
            emit(make_event(
                &ref_id,
                "run_completed",
                [("message", json!("done gracefully"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        "fatal_with_code" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("about to fail with code"))],
            ));
            emit(json!({
                "t": "fatal",
                "ref_id": ref_id,
                "error": "rate limited",
                "error_code": "backend_rate_limited"
            }));
        }
        "wrong_ref_final" => {
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("will send wrong final"))],
            ));
            emit(json!({
                "t": "final",
                "ref_id": "totally-wrong-ref-id",
                "receipt": make_receipt(backend_id, "totally-wrong-ref-id")
            }));
        }
        "slow_hello" => {
            sleep_ms(1_000);
            emit(make_hello(backend_id, "abp/v0.1"));
            let ref_id = read_run();
            emit(make_event(
                &ref_id,
                "run_started",
                [("message", json!("slow hello done"))],
            ));
            emit(make_final(backend_id, &ref_id));
        }
        _ => {
            eprintln!("Unknown mode: {mode}");
            std::process::exit(1);
        }
    }
}
