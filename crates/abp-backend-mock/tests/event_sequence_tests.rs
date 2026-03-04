use abp_backend_core::Backend;
use abp_backend_mock::scenarios::{
    EventSequenceBuilder, EventStep, MockBackendRecorder, MockScenario, ScenarioMockBackend,
};
use abp_core::{AgentEvent, AgentEventKind, Outcome, UsageNormalized, WorkOrderBuilder};
use tokio::sync::mpsc;
use uuid::Uuid;

fn test_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_backend(
    backend: &impl Backend,
    task: &str,
) -> anyhow::Result<(abp_core::Receipt, Vec<AgentEvent>)> {
    let wo = test_work_order(task);
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await?;
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    Ok((receipt, events))
}

fn event_kinds(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match &e.kind {
            AgentEventKind::RunStarted { .. } => "RunStarted".into(),
            AgentEventKind::RunCompleted { .. } => "RunCompleted".into(),
            AgentEventKind::AssistantMessage { text } => format!("Message({text})"),
            AgentEventKind::AssistantDelta { text } => format!("Delta({text})"),
            AgentEventKind::ToolCall { tool_name, .. } => format!("ToolCall({tool_name})"),
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                ..
            } => format!("ToolResult({tool_name},err={is_error})"),
            AgentEventKind::FileChanged { path, .. } => format!("FileChanged({path})"),
            AgentEventKind::CommandExecuted { command, .. } => {
                format!("CommandExecuted({command})")
            }
            AgentEventKind::Warning { message } => format!("Warning({message})"),
            AgentEventKind::Error { message, .. } => format!("Error({message})"),
        })
        .collect()
}

// ===== EventSequenceBuilder basics =====

#[tokio::test]
async fn builder_message_emits_assistant_message() {
    let scenario = EventSequenceBuilder::new().message("hello world").build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "msg").await.unwrap();
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"Message(hello world)".to_string()));
}

#[tokio::test]
async fn builder_delta_emits_assistant_delta() {
    let scenario = EventSequenceBuilder::new()
        .delta("chunk1")
        .delta("chunk2")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "delta").await.unwrap();
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"Delta(chunk1)".to_string()));
    assert!(kinds.contains(&"Delta(chunk2)".to_string()));
}

#[tokio::test]
async fn builder_tool_call_and_result() {
    let scenario = EventSequenceBuilder::new()
        .tool_call("read_file", serde_json::json!({"path": "foo.txt"}))
        .tool_result("read_file", serde_json::json!({"content": "bar"}))
        .message("Done reading")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "tool").await.unwrap();
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"ToolCall(read_file)".to_string()));
    assert!(kinds.contains(&"ToolResult(read_file,err=false)".to_string()));
    assert!(kinds.contains(&"Message(Done reading)".to_string()));
}

#[tokio::test]
async fn builder_tool_error_sets_is_error() {
    let scenario = EventSequenceBuilder::new()
        .tool_call("write_file", serde_json::json!({"path": "x"}))
        .tool_error(
            "write_file",
            serde_json::json!({"error": "permission denied"}),
        )
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "tool-err").await.unwrap();
    let tool_results: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolResult { is_error, .. } => Some(*is_error),
            _ => None,
        })
        .collect();
    assert_eq!(tool_results, vec![true]);
}

#[tokio::test]
async fn builder_tool_call_full_with_ids() {
    let scenario = EventSequenceBuilder::new()
        .tool_call_full(
            "bash",
            Some("tc-1".into()),
            Some("parent-1".into()),
            serde_json::json!({"command": "ls"}),
        )
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "tool-full").await.unwrap();
    let tc = events.iter().find_map(|e| match &e.kind {
        AgentEventKind::ToolCall {
            tool_use_id,
            parent_tool_use_id,
            ..
        } => Some((tool_use_id.clone(), parent_tool_use_id.clone())),
        _ => None,
    });
    let (id, parent) = tc.unwrap();
    assert_eq!(id.unwrap(), "tc-1");
    assert_eq!(parent.unwrap(), "parent-1");
}

#[tokio::test]
async fn builder_file_changed_event() {
    let scenario = EventSequenceBuilder::new()
        .file_changed("src/main.rs", "Added error handling")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "file").await.unwrap();
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"FileChanged(src/main.rs)".to_string()));
}

#[tokio::test]
async fn builder_command_executed_event() {
    let scenario = EventSequenceBuilder::new()
        .command_executed("cargo test", 0, Some("all passed".into()))
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "cmd").await.unwrap();
    let cmd_events: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => Some((command.clone(), *exit_code, output_preview.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(cmd_events.len(), 1);
    assert_eq!(cmd_events[0].0, "cargo test");
    assert_eq!(cmd_events[0].1, Some(0));
    assert_eq!(cmd_events[0].2, Some("all passed".into()));
}

#[tokio::test]
async fn builder_warning_event() {
    let scenario = EventSequenceBuilder::new()
        .warning("deprecated API")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (_, events) = run_backend(&b, "warn").await.unwrap();
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"Warning(deprecated API)".to_string()));
}

#[tokio::test]
async fn builder_error_event_does_not_fail_run() {
    let scenario = EventSequenceBuilder::new()
        .error_event("non-fatal error")
        .message("still going")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, events) = run_backend(&b, "err-event").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"Error(non-fatal error)".to_string()));
    assert!(kinds.contains(&"Message(still going)".to_string()));
}

// ===== Per-event latency =====

#[tokio::test]
async fn builder_delay_before_event() {
    let scenario = EventSequenceBuilder::new()
        .delay_ms(50)
        .message("delayed msg")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let start = std::time::Instant::now();
    let (receipt, _) = run_backend(&b, "delay").await.unwrap();
    assert!(start.elapsed().as_millis() >= 40);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn builder_multiple_delays() {
    let scenario = EventSequenceBuilder::new()
        .delay_ms(30)
        .delta("a")
        .delay_ms(30)
        .delta("b")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let start = std::time::Instant::now();
    let _ = run_backend(&b, "multi-delay").await.unwrap();
    assert!(start.elapsed().as_millis() >= 50);
}

#[tokio::test]
async fn builder_delay_only_applies_to_next_event() {
    let scenario = EventSequenceBuilder::new()
        .delay_ms(50)
        .message("first")
        .message("second") // no delay
        .build();
    let b = ScenarioMockBackend::new(scenario);
    // Should complete; second event has no delay
    let (receipt, _) = run_backend(&b, "delay-reset").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Verify the second step has delay_before_ms == 0
    if let MockScenario::Custom { steps, .. } = EventSequenceBuilder::new()
        .delay_ms(50)
        .message("first")
        .message("second")
        .build()
    {
        assert_eq!(steps[0].delay_before_ms, 50);
        assert_eq!(steps[1].delay_before_ms, 0);
    } else {
        panic!("expected Custom variant");
    }
}

// ===== Token usage simulation =====

#[tokio::test]
async fn builder_usage_tokens_in_receipt() {
    let scenario = EventSequenceBuilder::new()
        .message("hi")
        .usage_tokens(150, 75)
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "usage").await.unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(150));
    assert_eq!(receipt.usage.output_tokens, Some(75));
}

#[tokio::test]
async fn builder_full_usage_in_receipt() {
    let usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.05),
    };
    let scenario = EventSequenceBuilder::new()
        .message("hi")
        .usage(usage)
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "full-usage").await.unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(1000));
    assert_eq!(receipt.usage.output_tokens, Some(500));
    assert_eq!(receipt.usage.cache_read_tokens, Some(200));
    assert_eq!(receipt.usage.cache_write_tokens, Some(100));
    assert_eq!(receipt.usage.request_units, Some(3));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.05));
}

#[tokio::test]
async fn default_usage_is_zero_when_not_set() {
    let scenario = EventSequenceBuilder::new().message("no usage").build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "no-usage").await.unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
}

// ===== Outcome control =====

#[tokio::test]
async fn builder_partial_outcome() {
    let scenario = EventSequenceBuilder::new()
        .message("partial work")
        .outcome(Outcome::Partial)
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "partial").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[tokio::test]
async fn builder_failed_outcome() {
    let scenario = EventSequenceBuilder::new()
        .message("failed work")
        .outcome(Outcome::Failed)
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "failed").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[tokio::test]
async fn builder_default_outcome_is_complete() {
    let scenario = EventSequenceBuilder::new().message("ok").build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "default-outcome").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===== Mid-stream error injection =====

#[tokio::test]
async fn builder_fail_after_emits_events_then_errors() {
    let scenario = EventSequenceBuilder::new()
        .message("step 1")
        .message("step 2")
        .fail_after("simulated crash")
        .build();
    let b = ScenarioMockBackend::new(scenario);

    let wo = test_work_order("crash-test");
    let (tx, mut rx) = mpsc::channel(64);
    let result = b.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("simulated crash"));

    // Events were still emitted before the error
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"RunStarted".to_string()));
    assert!(kinds.contains(&"Message(step 1)".to_string()));
    assert!(kinds.contains(&"Message(step 2)".to_string()));
    // No RunCompleted since we failed
    assert!(!kinds.contains(&"RunCompleted".to_string()));
}

#[tokio::test]
async fn builder_fail_after_with_tool_events() {
    let scenario = EventSequenceBuilder::new()
        .tool_call("read_file", serde_json::json!({"path": "x"}))
        .fail_after("network error during tool execution")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let wo = test_work_order("tool-crash");
    let (tx, mut rx) = mpsc::channel(64);
    let result = b.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"ToolCall(read_file)".to_string()));
}

// ===== Complex scenarios =====

#[tokio::test]
async fn builder_full_tool_use_cycle() {
    let scenario = EventSequenceBuilder::new()
        .message("I'll read the file for you")
        .tool_call("read_file", serde_json::json!({"path": "src/lib.rs"}))
        .tool_result("read_file", serde_json::json!({"content": "fn main() {}"}))
        .message("The file contains a main function")
        .tool_call("edit_file", serde_json::json!({"path": "src/lib.rs", "content": "fn main() { println!(\"hello\"); }"}))
        .tool_result("edit_file", serde_json::json!({"success": true}))
        .file_changed("src/lib.rs", "Added println")
        .message("I've updated the file")
        .usage_tokens(500, 200)
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, events) = run_backend(&b, "full-cycle").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.usage.input_tokens, Some(500));
    assert_eq!(receipt.usage.output_tokens, Some(200));

    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"ToolCall(read_file)".to_string()));
    assert!(kinds.contains(&"ToolResult(read_file,err=false)".to_string()));
    assert!(kinds.contains(&"ToolCall(edit_file)".to_string()));
    assert!(kinds.contains(&"FileChanged(src/lib.rs)".to_string()));
}

#[tokio::test]
async fn builder_mixed_streaming_and_tools() {
    let scenario = EventSequenceBuilder::new()
        .delta("thinking...")
        .delta("let me check...")
        .tool_call("bash", serde_json::json!({"command": "ls"}))
        .tool_result("bash", serde_json::json!({"output": "file1.rs\nfile2.rs"}))
        .command_executed("ls", 0, Some("file1.rs\nfile2.rs".into()))
        .message("Found 2 files")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, events) = run_backend(&b, "mixed").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"Delta(thinking...)".to_string()));
    assert!(kinds.contains(&"ToolCall(bash)".to_string()));
    assert!(kinds.contains(&"CommandExecuted(ls)".to_string()));
    assert!(kinds.contains(&"Message(Found 2 files)".to_string()));
}

#[tokio::test]
async fn builder_empty_sequence_still_completes() {
    let scenario = EventSequenceBuilder::new().build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, events) = run_backend(&b, "empty").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    // Should still have RunStarted + RunCompleted
    let kinds = event_kinds(&events);
    assert!(kinds.contains(&"RunStarted".to_string()));
    assert!(kinds.contains(&"RunCompleted".to_string()));
}

// ===== EventStep serialization =====

#[tokio::test]
async fn custom_scenario_serializes_to_json() {
    let scenario = EventSequenceBuilder::new()
        .message("hello")
        .tool_call("read", serde_json::json!({}))
        .usage_tokens(100, 50)
        .build();
    let json = serde_json::to_string(&scenario).unwrap();
    assert!(json.contains("\"type\":\"custom\""));
    let deser: MockScenario = serde_json::from_str(&json).unwrap();
    match deser {
        MockScenario::Custom {
            steps,
            usage,
            outcome,
            fail_after,
        } => {
            assert_eq!(steps.len(), 2);
            assert!(usage.is_some());
            assert_eq!(usage.unwrap().input_tokens, Some(100));
            assert_eq!(outcome, Outcome::Complete);
            assert!(fail_after.is_none());
        }
        _ => panic!("expected Custom variant"),
    }
}

#[tokio::test]
async fn event_step_serializes_and_deserializes() {
    let step = EventStep {
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        delay_before_ms: 42,
    };
    let json = serde_json::to_string(&step).unwrap();
    let deser: EventStep = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.delay_before_ms, 42);
    match deser.kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "hi"),
        _ => panic!("wrong variant"),
    }
}

// ===== Recorder assertion helpers =====

#[tokio::test]
async fn recorder_assert_call_count_passes() {
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(
        EventSequenceBuilder::new().message("ok").build(),
    ));
    let _ = run_backend(&recorder, "a").await.unwrap();
    let _ = run_backend(&recorder, "b").await.unwrap();
    recorder.assert_call_count(2).await;
}

#[tokio::test]
#[should_panic(expected = "expected 5 recorded calls, got 2")]
async fn recorder_assert_call_count_panics_on_mismatch() {
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(
        EventSequenceBuilder::new().message("ok").build(),
    ));
    let _ = run_backend(&recorder, "a").await.unwrap();
    let _ = run_backend(&recorder, "b").await.unwrap();
    recorder.assert_call_count(5).await;
}

#[tokio::test]
async fn recorder_assert_all_succeeded_passes() {
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(
        EventSequenceBuilder::new().message("ok").build(),
    ));
    let _ = run_backend(&recorder, "a").await.unwrap();
    let _ = run_backend(&recorder, "b").await.unwrap();
    recorder.assert_all_succeeded().await;
}

#[tokio::test]
#[should_panic(expected = "call 0 failed")]
async fn recorder_assert_all_succeeded_panics_on_failure() {
    let recorder =
        MockBackendRecorder::new(ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "E".into(),
            message: "fail".into(),
        }));
    let _ = run_backend(&recorder, "bad").await;
    recorder.assert_all_succeeded().await;
}

#[tokio::test]
async fn recorder_assert_all_failed_passes() {
    let recorder =
        MockBackendRecorder::new(ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "E".into(),
            message: "fail".into(),
        }));
    let _ = run_backend(&recorder, "a").await;
    let _ = run_backend(&recorder, "b").await;
    recorder.assert_all_failed().await;
}

#[tokio::test]
#[should_panic(expected = "call 0 unexpectedly succeeded")]
async fn recorder_assert_all_failed_panics_on_success() {
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(
        EventSequenceBuilder::new().message("ok").build(),
    ));
    let _ = run_backend(&recorder, "good").await.unwrap();
    recorder.assert_all_failed().await;
}

#[tokio::test]
async fn recorder_calls_matching_filters_by_task() {
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(
        EventSequenceBuilder::new().message("ok").build(),
    ));
    let _ = run_backend(&recorder, "alpha-task").await.unwrap();
    let _ = run_backend(&recorder, "beta-task").await.unwrap();
    let _ = run_backend(&recorder, "alpha-other").await.unwrap();
    let alpha_calls = recorder.calls_matching("alpha").await;
    assert_eq!(alpha_calls.len(), 2);
    let beta_calls = recorder.calls_matching("beta").await;
    assert_eq!(beta_calls.len(), 1);
    let none_calls = recorder.calls_matching("gamma").await;
    assert_eq!(none_calls.len(), 0);
}

// ===== Custom scenario in transient error =====

#[tokio::test]
async fn transient_error_then_custom_scenario() {
    let custom = EventSequenceBuilder::new()
        .message("recovered")
        .usage_tokens(50, 25)
        .build();
    let scenario = MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(custom),
    };
    let b = ScenarioMockBackend::new(scenario);
    let r1 = run_backend(&b, "attempt-1").await;
    assert!(r1.is_err());
    let (receipt, _) = run_backend(&b, "attempt-2").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.usage.input_tokens, Some(50));
}

// ===== Receipt integrity for custom scenarios =====

#[tokio::test]
async fn custom_receipt_has_valid_hash() {
    let scenario = EventSequenceBuilder::new()
        .message("hash test")
        .usage_tokens(10, 5)
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "hash").await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
}

#[tokio::test]
async fn custom_receipt_trace_matches_events() {
    let scenario = EventSequenceBuilder::new()
        .message("a")
        .tool_call("x", serde_json::json!({}))
        .tool_result("x", serde_json::json!({}))
        .message("b")
        .build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, events) = run_backend(&b, "trace").await.unwrap();
    assert_eq!(receipt.trace.len(), events.len());
}

// ===== Default trait on builder =====

#[tokio::test]
async fn builder_default_works() {
    let builder = EventSequenceBuilder::default();
    let scenario = builder.message("via default").build();
    let b = ScenarioMockBackend::new(scenario);
    let (receipt, _) = run_backend(&b, "default").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===== Recorder with custom scenario =====

#[tokio::test]
async fn recorder_records_custom_scenario_calls() {
    let scenario = EventSequenceBuilder::new()
        .message("recorded")
        .usage_tokens(100, 50)
        .build();
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(scenario));
    let _ = run_backend(&recorder, "rec-custom").await.unwrap();
    recorder.assert_call_count(1).await;
    let call = recorder.last_call().await.unwrap();
    assert_eq!(call.work_order.task, "rec-custom");
    assert!(call.result.is_ok());
}

#[tokio::test]
async fn recorder_records_fail_after_as_error() {
    let scenario = EventSequenceBuilder::new()
        .message("before crash")
        .fail_after("boom")
        .build();
    let recorder = MockBackendRecorder::new(ScenarioMockBackend::new(scenario));
    let _ = run_backend(&recorder, "crash").await;
    recorder.assert_call_count(1).await;
    let call = recorder.last_call().await.unwrap();
    assert!(call.result.is_err());
    assert!(call.result.unwrap_err().contains("boom"));
}
