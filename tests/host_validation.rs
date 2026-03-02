//! Lightweight validation tests for sidecar host examples in `hosts/`.
//!
//! These tests parse host scripts and verify they implement the ABP JSONL
//! protocol correctly without spawning any processes.

use std::path::{Path, PathBuf};

const CONTRACT_VERSION: &str = "abp/v0.1";

fn hosts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("hosts")
}

fn read_host(host: &str, file: &str) -> String {
    let path = hosts_dir().join(host).join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Host directory existence & required files
// ---------------------------------------------------------------------------

#[test]
fn all_host_directories_exist() {
    let hosts = [
        "node", "python", "claude", "gemini", "copilot", "codex", "kimi",
    ];
    for host in hosts {
        let dir = hosts_dir().join(host);
        assert!(dir.is_dir(), "missing host directory: {}", dir.display());
    }
}

#[test]
fn node_host_has_required_files() {
    let dir = hosts_dir().join("node");
    assert!(dir.join("host.js").is_file(), "node/host.js missing");
}

#[test]
fn python_host_has_required_files() {
    let dir = hosts_dir().join("python");
    assert!(dir.join("host.py").is_file(), "python/host.py missing");
}

#[test]
fn js_hosts_have_package_json() {
    let hosts = ["claude", "gemini", "copilot", "codex", "kimi"];
    for host in hosts {
        let pkg = hosts_dir().join(host).join("package.json");
        assert!(pkg.is_file(), "{host}/package.json missing");
    }
}

#[test]
fn advanced_hosts_have_capabilities_module() {
    let hosts = ["gemini", "copilot", "kimi", "codex"];
    for host in hosts {
        let caps = hosts_dir().join(host).join("capabilities.js");
        assert!(caps.is_file(), "{host}/capabilities.js missing");
    }
}

// ---------------------------------------------------------------------------
// Contract version declared correctly
// ---------------------------------------------------------------------------

#[test]
fn node_host_declares_contract_version() {
    let src = read_host("node", "host.js");
    assert!(
        src.contains(&format!("\"{}\"", CONTRACT_VERSION)),
        "node/host.js does not reference contract version {CONTRACT_VERSION}"
    );
}

#[test]
fn python_host_declares_contract_version() {
    let src = read_host("python", "host.py");
    assert!(
        src.contains(&format!("\"{}\"", CONTRACT_VERSION)),
        "python/host.py does not reference contract version {CONTRACT_VERSION}"
    );
}

#[test]
fn advanced_js_hosts_declare_contract_version() {
    let hosts = ["claude", "gemini", "copilot", "codex", "kimi"];
    for host in hosts {
        let src = read_host(host, "host.js");
        assert!(
            src.contains(&format!("\"{}\"", CONTRACT_VERSION)),
            "{host}/host.js does not reference contract version {CONTRACT_VERSION}"
        );
    }
}

// ---------------------------------------------------------------------------
// Envelope tag format: uses "t" not "type" for envelope discriminator
// ---------------------------------------------------------------------------

fn assert_envelope_tag_format(host: &str, src: &str) {
    // Must use t: "hello", t: "event", t: "final", t: "fatal"
    for tag in ["hello", "event", "final", "fatal"] {
        assert!(
            src.contains(&format!("t: \"{}\"", tag))
                || src.contains(&format!("t: '{}'", tag))
                || src.contains(&format!("\"t\": \"{}\"", tag))
                || src.contains(&format!("'t': '{}'", tag)),
            "{host} missing envelope tag t: \"{tag}\""
        );
    }
}

#[test]
fn node_host_uses_envelope_tag_t() {
    let src = read_host("node", "host.js");
    assert_envelope_tag_format("node/host.js", &src);
}

#[test]
fn python_host_uses_envelope_tag_t() {
    let src = read_host("python", "host.py");
    assert_envelope_tag_format("python/host.py", &src);
}

#[test]
fn advanced_js_hosts_use_envelope_tag_t() {
    let hosts = ["claude", "gemini", "copilot", "codex", "kimi"];
    for host in hosts {
        let src = read_host(host, "host.js");
        assert_envelope_tag_format(&format!("{host}/host.js"), &src);
    }
}

// ---------------------------------------------------------------------------
// JSONL protocol handling: hello/run/event/final/fatal
// ---------------------------------------------------------------------------

fn assert_protocol_envelope_handling(host_label: &str, src: &str) {
    // "hello" must be sent first
    assert!(
        src.contains("hello"),
        "{host_label}: no hello envelope handling"
    );
    // Must check for "run" envelope type
    assert!(
        src.contains("\"run\"") || src.contains("'run'"),
        "{host_label}: no run envelope handling"
    );
    // Must emit "event" envelopes
    assert!(
        src.contains("\"event\"") || src.contains("'event'"),
        "{host_label}: no event emission"
    );
    // Must emit "final" envelope
    assert!(
        src.contains("\"final\"") || src.contains("'final'"),
        "{host_label}: no final envelope emission"
    );
    // Must handle "fatal" error path
    assert!(
        src.contains("\"fatal\"") || src.contains("'fatal'"),
        "{host_label}: no fatal error handling"
    );
}

#[test]
fn all_hosts_handle_jsonl_protocol() {
    // JS hosts
    let js_hosts = ["node", "claude", "gemini", "copilot", "codex", "kimi"];
    for host in js_hosts {
        let src = read_host(host, "host.js");
        assert_protocol_envelope_handling(&format!("{host}/host.js"), &src);
    }
    // Python host
    let src = read_host("python", "host.py");
    assert_protocol_envelope_handling("python/host.py", &src);
}

// ---------------------------------------------------------------------------
// Error handling: hosts handle invalid JSON and unexpected envelopes
// ---------------------------------------------------------------------------

#[test]
fn node_host_handles_json_parse_errors() {
    let src = read_host("node", "host.js");
    assert!(
        src.contains("JSON.parse") && src.contains("catch"),
        "node/host.js should catch JSON parse errors"
    );
}

#[test]
fn python_host_handles_json_parse_errors() {
    let src = read_host("python", "host.py");
    assert!(
        src.contains("json.loads") && src.contains("except"),
        "python/host.py should catch JSON parse errors"
    );
}

#[test]
fn advanced_js_hosts_handle_json_parse_errors() {
    let hosts = ["claude", "gemini", "copilot", "codex", "kimi"];
    for host in hosts {
        let src = read_host(host, "host.js");
        assert!(
            src.contains("JSON.parse") && src.contains("catch"),
            "{host}/host.js should catch JSON parse errors"
        );
    }
}

// ---------------------------------------------------------------------------
// Hello envelope is sent before reading stdin
// ---------------------------------------------------------------------------

fn assert_hello_before_stdin(host_label: &str, src: &str) {
    // Find the write({t:"hello"...}) call — the actual hello emission
    let hello_write_pos = src
        .find("t: \"hello\"")
        .or_else(|| src.find("t: 'hello'"))
        .or_else(|| src.find("\"t\": \"hello\""));
    // Find the main stdin read loop (rl.on for node-style, while True for python)
    let read_loop_pos = src
        .find("rl.on(\"line\"")
        .or_else(|| src.find("rl.on('line'"))
        .or_else(|| src.find("for await (const line of rl)"))
        .or_else(|| src.find("while True"));
    if let (Some(hp), Some(rlp)) = (hello_write_pos, read_loop_pos) {
        assert!(
            hp < rlp,
            "{host_label}: hello envelope must be emitted before the main read loop (hello at {hp}, loop at {rlp})"
        );
    }
}

#[test]
fn hello_sent_before_read_loop() {
    let js_hosts = ["node", "claude", "gemini", "copilot", "codex", "kimi"];
    for host in js_hosts {
        let src = read_host(host, "host.js");
        assert_hello_before_stdin(&format!("{host}/host.js"), &src);
    }
    let src = read_host("python", "host.py");
    assert_hello_before_stdin("python/host.py", &src);
}

// ---------------------------------------------------------------------------
// Claude host: adapter module
// ---------------------------------------------------------------------------

#[test]
fn claude_host_has_adapter_module() {
    let dir = hosts_dir().join("claude");
    assert!(
        dir.join("adapter.js").is_file(),
        "claude/adapter.js missing"
    );
}

#[test]
fn claude_host_has_adapter_template() {
    let dir = hosts_dir().join("claude");
    assert!(
        dir.join("adapter.template.js").is_file(),
        "claude/adapter.template.js missing"
    );
}

fn assert_adapter_exports_run(host: &str, file: &str) {
    let src = read_host(host, file);
    assert!(
        src.contains("async run(")
            || src.contains("async function run(")
            || src.contains("run:")
            || src.contains("exports.run"),
        "{host}/{file} should export a run function"
    );
}

#[test]
fn claude_adapter_exports_run_function() {
    assert_adapter_exports_run("claude", "adapter.js");
}

#[test]
fn claude_adapter_template_exports_run_function() {
    assert_adapter_exports_run("claude", "adapter.template.js");
}

// ---------------------------------------------------------------------------
// Gemini host: adapter + mapper modules
// ---------------------------------------------------------------------------

#[test]
fn gemini_host_has_adapter_module() {
    let dir = hosts_dir().join("gemini");
    assert!(
        dir.join("adapter.js").is_file(),
        "gemini/adapter.js missing"
    );
}

#[test]
fn gemini_host_has_mapper_module() {
    let dir = hosts_dir().join("gemini");
    assert!(dir.join("mapper.js").is_file(), "gemini/mapper.js missing");
}

#[test]
fn gemini_adapter_exports_run_function() {
    assert_adapter_exports_run("gemini", "adapter.js");
}

// ---------------------------------------------------------------------------
// Copilot host: adapter contract structure
// ---------------------------------------------------------------------------

#[test]
fn copilot_host_has_adapter_module() {
    let dir = hosts_dir().join("copilot");
    assert!(
        dir.join("adapter.js").is_file(),
        "copilot/adapter.js missing"
    );
}

#[test]
fn copilot_host_has_adapter_template() {
    let dir = hosts_dir().join("copilot");
    assert!(
        dir.join("adapter.template.js").is_file(),
        "copilot/adapter.template.js missing"
    );
}

#[test]
fn copilot_adapter_exports_run_function() {
    assert_adapter_exports_run("copilot", "adapter.js");
}

#[test]
fn copilot_adapter_template_exports_run_function() {
    assert_adapter_exports_run("copilot", "adapter.template.js");
}

// ---------------------------------------------------------------------------
// Codex host: adapter module
// ---------------------------------------------------------------------------

#[test]
fn codex_host_has_adapter_module() {
    let dir = hosts_dir().join("codex");
    assert!(dir.join("adapter.js").is_file(), "codex/adapter.js missing");
}

#[test]
fn codex_adapter_exports_run_function() {
    assert_adapter_exports_run("codex", "adapter.js");
}

// ---------------------------------------------------------------------------
// Kimi host: adapter module
// ---------------------------------------------------------------------------

#[test]
fn kimi_host_has_adapter_module() {
    let dir = hosts_dir().join("kimi");
    assert!(dir.join("adapter.js").is_file(), "kimi/adapter.js missing");
}

#[test]
fn kimi_adapter_exports_run_function() {
    assert_adapter_exports_run("kimi", "adapter.js");
}

// ---------------------------------------------------------------------------
// Event structure: hosts use ABP event type names
// ---------------------------------------------------------------------------

const EXPECTED_EVENT_TYPES: &[&str] = &["run_started", "run_completed", "assistant_message"];

#[test]
fn all_hosts_emit_standard_event_types() {
    let js_hosts = ["node", "claude", "gemini", "copilot", "codex", "kimi"];
    for host in js_hosts {
        let src = read_host(host, "host.js");
        for event_type in EXPECTED_EVENT_TYPES {
            assert!(
                src.contains(event_type),
                "{host}/host.js missing event type: {event_type}"
            );
        }
    }
    let src = read_host("python", "host.py");
    for event_type in EXPECTED_EVENT_TYPES {
        assert!(
            src.contains(event_type),
            "python/host.py missing event type: {event_type}"
        );
    }
}

// ---------------------------------------------------------------------------
// Receipt structure: hosts produce receipts with required fields
// ---------------------------------------------------------------------------

const RECEIPT_FIELDS: &[&str] = &[
    "run_id",
    "work_order_id",
    "contract_version",
    "started_at",
    "finished_at",
    "duration_ms",
    "receipt_sha256",
    "outcome",
];

#[test]
fn all_hosts_produce_receipts_with_required_fields() {
    let js_hosts = ["node", "claude", "gemini", "copilot", "codex", "kimi"];
    for host in js_hosts {
        let src = read_host(host, "host.js");
        for field in RECEIPT_FIELDS {
            assert!(
                src.contains(field),
                "{host}/host.js missing receipt field: {field}"
            );
        }
    }
    let src = read_host("python", "host.py");
    for field in RECEIPT_FIELDS {
        assert!(
            src.contains(field),
            "python/host.py missing receipt field: {field}"
        );
    }
}

// ---------------------------------------------------------------------------
// Capabilities module structure for advanced hosts
// ---------------------------------------------------------------------------

#[test]
fn capabilities_modules_export_support_levels() {
    let hosts = ["gemini", "copilot", "kimi", "codex"];
    for host in hosts {
        let src = read_host(host, "capabilities.js");
        assert!(
            src.contains("native") && src.contains("emulated"),
            "{host}/capabilities.js should define native/emulated support levels"
        );
    }
}

// ---------------------------------------------------------------------------
// Codex host: mapper module
// ---------------------------------------------------------------------------

#[test]
fn codex_host_has_mapper_module() {
    let dir = hosts_dir().join("codex");
    assert!(dir.join("mapper.js").is_file(), "codex/mapper.js missing");
}

// ---------------------------------------------------------------------------
// Receipt hash handling: hosts null receipt_sha256 before hashing
// ---------------------------------------------------------------------------

#[test]
fn js_hosts_with_hashing_null_receipt_sha256() {
    // Advanced hosts compute receipt hashes and should null receipt_sha256 first
    let hosts = ["gemini", "copilot", "codex", "kimi"];
    for host in hosts {
        let src = read_host(host, "host.js");
        if src.contains("computeReceiptHash") || src.contains("receipt_sha256") {
            assert!(
                src.contains("receipt_sha256 = null") || src.contains("receipt_sha256: null"),
                "{host}/host.js: receipt_sha256 should be nulled before hashing"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test suites exist for advanced hosts
// ---------------------------------------------------------------------------

#[test]
fn advanced_hosts_have_test_directories() {
    let hosts = ["claude", "gemini", "copilot", "codex", "kimi"];
    for host in hosts {
        let test_dir = hosts_dir().join(host).join("test");
        assert!(test_dir.is_dir(), "{host}/test/ directory missing");
    }
}
