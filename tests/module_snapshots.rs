// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive insta snapshot tests for modules added in recent waves.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{TimeZone, Utc};

use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, WorkOrderBuilder};
use abp_host::SidecarSpec;
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_protocol::Envelope;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_runtime::retry::{RetryPolicy, TimeoutConfig};

// ── Helpers ──────────────────────────────────────────────────────────────

/// Fixed timestamp for deterministic snapshots.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 5).unwrap()
}

fn sample_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting run".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello, world!".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tool-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tool-1".into()),
                output: serde_json::json!("fn main() {}"),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Warning {
                message: "rate limit approaching".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: fixed_ts2(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 1. RetryPolicy snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_policy_default_snapshot() {
    let policy = RetryPolicy::default();
    insta::assert_json_snapshot!("retry_policy_default", policy);
}

#[test]
fn retry_policy_custom_builder_snapshot() {
    let policy = RetryPolicy::builder()
        .max_retries(5)
        .initial_backoff(Duration::from_millis(250))
        .max_backoff(Duration::from_secs(30))
        .backoff_multiplier(3.0)
        .build();
    insta::assert_json_snapshot!("retry_policy_custom", policy);
}

#[test]
fn retry_policy_zero_retries_snapshot() {
    let policy = RetryPolicy::builder().max_retries(0).build();
    insta::assert_json_snapshot!("retry_policy_zero_retries", policy);
}

#[test]
fn retry_policy_backoff_delays_snapshot() {
    let policy = RetryPolicy::default();
    let delays: Vec<u64> = (0..4)
        .map(|attempt| policy.compute_delay(attempt).as_millis() as u64)
        .collect();
    insta::assert_json_snapshot!("retry_policy_backoff_delays", delays);
}

#[test]
fn timeout_config_full_snapshot() {
    let cfg = TimeoutConfig {
        run_timeout: Some(Duration::from_secs(300)),
        event_timeout: Some(Duration::from_secs(60)),
    };
    insta::assert_json_snapshot!("timeout_config_full", cfg);
}

#[test]
fn timeout_config_defaults_snapshot() {
    let cfg = TimeoutConfig::default();
    insta::assert_json_snapshot!("timeout_config_defaults", cfg);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. ProtocolVersion snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_version_current_snapshot() {
    let ver = ProtocolVersion::current();
    insta::assert_json_snapshot!("protocol_version_current", ver);
}

#[test]
fn protocol_version_parsed_snapshot() {
    let ver = ProtocolVersion::parse("abp/v2.5").unwrap();
    insta::assert_json_snapshot!("protocol_version_parsed", ver);
}

#[test]
fn protocol_version_display_snapshot() {
    let ver = ProtocolVersion::parse("abp/v1.3").unwrap();
    insta::assert_snapshot!("protocol_version_display", ver.to_string());
}

#[test]
fn version_range_snapshot() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    insta::assert_json_snapshot!("version_range", range);
}

#[test]
fn negotiate_version_compatible_snapshot() {
    let local = ProtocolVersion { major: 0, minor: 3 };
    let remote = ProtocolVersion { major: 0, minor: 1 };
    let result = negotiate_version(&local, &remote).unwrap();
    insta::assert_json_snapshot!("negotiate_version_compatible", result);
}

#[test]
fn negotiate_version_incompatible_snapshot() {
    let local = ProtocolVersion { major: 0, minor: 1 };
    let remote = ProtocolVersion { major: 1, minor: 0 };
    let err = negotiate_version(&local, &remote).unwrap_err();
    insta::assert_snapshot!("negotiate_version_incompatible", err.to_string());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. StreamingCodec snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn streaming_codec_encode_batch_snapshot() {
    let envelopes = vec![
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "out of memory".into(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "unknown".into(),
        },
    ];
    let batch = StreamingCodec::encode_batch(&envelopes);
    insta::assert_snapshot!("streaming_codec_encode_batch", batch);
}

#[test]
fn streaming_codec_line_count_snapshot() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n";
    let count = StreamingCodec::line_count(input);
    insta::assert_snapshot!("streaming_codec_line_count", count.to_string());
}

#[test]
fn streaming_codec_decode_errors_snapshot() {
    let input = "not valid json\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\nalso bad\n";
    let errors = StreamingCodec::validate_jsonl(input);
    let display: Vec<String> = errors
        .iter()
        .map(|(line, e)| format!("line {line}: {e}"))
        .collect();
    insta::assert_json_snapshot!("streaming_codec_decode_errors", display);
}

#[test]
fn streaming_codec_roundtrip_snapshot() {
    let backend = BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    };
    let hello = Envelope::hello(backend, CapabilityManifest::new());
    let encoded = StreamingCodec::encode_batch(&[hello]);
    let decoded = StreamingCodec::decode_batch(&encoded);
    assert_eq!(decoded.len(), 1);
    assert!(decoded[0].is_ok());
    // Snapshot the encoded wire format
    insta::assert_snapshot!("streaming_codec_hello_roundtrip", encoded);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. OutputFormat snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn output_format_json_snapshot() {
    let fmt = abp_cli::format::OutputFormat::Json;
    insta::assert_json_snapshot!("output_format_json", fmt);
}

#[test]
fn output_format_json_pretty_snapshot() {
    let fmt = abp_cli::format::OutputFormat::JsonPretty;
    insta::assert_json_snapshot!("output_format_json_pretty", fmt);
}

#[test]
fn output_format_text_snapshot() {
    let fmt = abp_cli::format::OutputFormat::Text;
    insta::assert_json_snapshot!("output_format_text", fmt);
}

#[test]
fn output_format_table_snapshot() {
    let fmt = abp_cli::format::OutputFormat::Table;
    insta::assert_json_snapshot!("output_format_table", fmt);
}

#[test]
fn output_format_compact_snapshot() {
    let fmt = abp_cli::format::OutputFormat::Compact;
    insta::assert_json_snapshot!("output_format_compact", fmt);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. ProcessConfig snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_config_default_snapshot() {
    let cfg = ProcessConfig::default();
    insta::assert_json_snapshot!("process_config_default", cfg);
}

#[test]
fn process_config_with_env_snapshot() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".into(), "sk-test-123".into());
    env.insert("NODE_ENV".into(), "production".into());
    let cfg = ProcessConfig {
        working_dir: Some("/tmp/workspace".into()),
        env_vars: env,
        timeout: Some(Duration::from_secs(120)),
        inherit_env: false,
    };
    insta::assert_json_snapshot!("process_config_with_env", cfg);
}

#[test]
fn process_status_variants_snapshot() {
    let statuses = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 12345 },
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];
    insta::assert_json_snapshot!("process_status_variants", statuses);
}

#[test]
fn process_info_snapshot() {
    let spec = SidecarSpec::new("node");
    let config = ProcessConfig::default();
    let info = ProcessInfo::new(spec, config);
    insta::assert_json_snapshot!("process_info_not_started", info);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. EventStream snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_stream_count_by_kind_snapshot() {
    let stream = EventStream::new(sample_events());
    let counts = stream.count_by_kind();
    insta::assert_json_snapshot!("event_stream_count_by_kind", counts);
}

#[test]
fn event_stream_filter_include_snapshot() {
    let stream = EventStream::new(sample_events());
    let filter = EventFilter::include_kinds(&["tool_call", "tool_result"]);
    let filtered = stream.filter(&filter);
    let counts = filtered.count_by_kind();
    insta::assert_json_snapshot!("event_stream_filter_include", counts);
}

#[test]
fn event_stream_filter_exclude_snapshot() {
    let stream = EventStream::new(sample_events());
    let filter = EventFilter::exclude_kinds(&["warning"]);
    let filtered = stream.filter(&filter);
    let counts = filtered.count_by_kind();
    insta::assert_json_snapshot!("event_stream_filter_exclude", counts);
}

#[test]
fn event_stream_by_kind_snapshot() {
    let stream = EventStream::new(sample_events());
    let tools = stream.by_kind("tool_call");
    assert_eq!(tools.len(), 1);
    let event = tools.iter().next().unwrap();
    insta::assert_json_snapshot!("event_stream_by_kind_tool_call", event, {
        ".ts" => "[TIMESTAMP]",
    });
}

#[test]
fn event_stream_empty_snapshot() {
    let stream = EventStream::new(vec![]);
    let counts = stream.count_by_kind();
    insta::assert_json_snapshot!("event_stream_empty", counts);
}

#[test]
fn event_stream_len_snapshot() {
    let stream = EventStream::new(sample_events());
    insta::assert_snapshot!("event_stream_len", stream.len().to_string());
}

// ═══════════════════════════════════════════════════════════════════════
// Bonus: WorkOrder builder snapshot
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_snapshot() {
    let wo = WorkOrderBuilder::new("Fix the login bug")
        .root("/tmp/workspace")
        .model("gpt-4o")
        .max_turns(10)
        .build();
    insta::assert_json_snapshot!("work_order_builder", wo, {
        ".id" => "[UUID]",
    });
}
