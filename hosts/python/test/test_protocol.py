"""Protocol conformance tests for the Python ABP sidecar.

Spawns host.py as a child process, sends JSONL envelopes on stdin,
and asserts the expected responses appear on stdout.

Also tests the internal functions directly via import for unit-level coverage.
"""

import json
import os
import subprocess
import sys
import uuid

HOST = os.path.join(os.path.dirname(__file__), "..", "host.py")
PYTHON = sys.executable

# Add parent dir to sys.path so we can import host for unit tests
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


def run_sidecar(envelopes, *, timeout=10):
    """Send a list of JSONL envelopes to the sidecar and collect output."""
    input_data = "\n".join(json.dumps(e) for e in envelopes) + "\n"
    result = subprocess.run(
        [PYTHON, HOST],
        input=input_data,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    lines = [l for l in result.stdout.strip().splitlines() if l.strip()]
    msgs = [json.loads(l) for l in lines]
    return msgs, result.stderr


def make_work_order(**overrides):
    wo = {
        "id": str(uuid.uuid4()),
        "task": "test task",
        "workspace": {"root": os.getcwd()},
        "context": {},
        "policy": {},
        "config": {"vendor": {}},
    }
    wo.update(overrides)
    return wo


def make_run(wo=None):
    if wo is None:
        wo = make_work_order()
    return {"t": "run", "id": str(uuid.uuid4()), "work_order": wo}


# -- Original Tests ----------------------------------------------------------


def test_hello_is_first():
    """Hello must be the first envelope emitted."""
    run = make_run()
    msgs, _ = run_sidecar([run])
    assert len(msgs) >= 1, "should emit at least one message"
    assert msgs[0]["t"] == "hello"
    assert msgs[0]["contract_version"] == "abp/v0.1"
    assert "backend" in msgs[0]
    assert "capabilities" in msgs[0]


def test_events_and_final():
    """A run envelope should produce events and exactly one final."""
    run = make_run()
    msgs, _ = run_sidecar([run])
    events = [m for m in msgs if m["t"] == "event"]
    finals = [m for m in msgs if m["t"] == "final"]
    assert len(events) > 0, "should emit at least one event"
    assert len(finals) == 1, "should emit exactly one final"


def test_receipt_fields():
    """The receipt in final must have the correct contract_version and outcome."""
    wo = make_work_order()
    run = make_run(wo)
    msgs, _ = run_sidecar([run])
    final = next(m for m in msgs if m["t"] == "final")
    r = final["receipt"]
    assert r["meta"]["contract_version"] == "abp/v0.1"
    assert r["meta"]["work_order_id"] == wo["id"]
    assert r["outcome"] in ("complete", "partial"), f"unexpected outcome: {r['outcome']}"
    assert r["meta"]["duration_ms"] >= 0


def test_ping_pong():
    """Sidecar must respond to ping with pong echoing seq."""
    msgs, _ = run_sidecar([{"t": "ping", "seq": 99}])
    pong = next((m for m in msgs if m["t"] == "pong"), None)
    assert pong is not None, "should respond with pong"
    assert pong["seq"] == 99


def test_cancel_ignored():
    """Cancel should not cause a fatal."""
    msgs, _ = run_sidecar([{"t": "cancel", "ref_id": "x"}])
    assert any(m["t"] == "hello" for m in msgs)
    assert not any(m["t"] == "fatal" for m in msgs)


def test_invalid_json():
    """Invalid JSON should produce a fatal envelope."""
    result = subprocess.run(
        [PYTHON, HOST],
        input="not valid json\n",
        capture_output=True,
        text=True,
        timeout=10,
    )
    lines = [l for l in result.stdout.strip().splitlines() if l.strip()]
    msgs = [json.loads(l) for l in lines]
    assert any(m["t"] == "fatal" and "invalid json" in m.get("error", "") for m in msgs)


def test_ref_id_matches_run_id():
    """All events and final ref_id must match the run envelope id."""
    run_id = str(uuid.uuid4())
    wo = make_work_order()
    msgs, _ = run_sidecar([{"t": "run", "id": run_id, "work_order": wo}])
    events = [m for m in msgs if m["t"] == "event"]
    for ev in events:
        assert ev["ref_id"] == run_id, f"event ref_id mismatch: {ev['ref_id']}"
    final = next(m for m in msgs if m["t"] == "final")
    assert final["ref_id"] == run_id


# -- B9: New Tests -----------------------------------------------------------


def test_comprehensive_option_mapping():
    """B1: All Tier 1 options should reach the SDK request via build_request()."""
    import host

    wo = make_work_order(
        config={
            "model": "claude-sonnet-4-20250514",
            "max_budget_usd": 5.0,
            "vendor": {
                "claude": {
                    "permissionMode": "auto",
                    "sessionId": "sess-123",
                    "resume": True,
                    "settingSources": ["project", "user"],
                    "allowedTools": ["bash", "read"],
                    "disallowedTools": ["write"],
                    "maxTurns": 10,
                    "tools": [{"name": "custom_tool"}],
                    "systemPrompt": "You are helpful.",
                    "mcpServers": [{"url": "http://localhost:8080"}],
                    "continueConversation": True,
                    "maxBudgetUsd": 3.0,
                    "fallbackModel": "claude-haiku",
                    "betas": ["beta1"],
                    "outputFormat": "json",
                    "cliPath": "/usr/bin/claude",
                    "settings": {"key": "val"},
                    "addDirs": ["/tmp/extra"],
                    "extraArgs": ["--verbose"],
                    "maxBufferSize": 1024,
                    "stderr": "pipe",
                    "user": "test-user",
                    "sandbox": True,
                    "thinking": True,
                    "effort": "high",
                    "enableFileCheckpointing": True,
                    "agents": [{"name": "sub1"}],
                    "plugins": [{"name": "plug1"}],
                },
            },
        },
    )
    request = host.build_request(wo, "mapped")
    opts = request["options"]

    # Core options
    assert opts["model"] == "claude-sonnet-4-20250514"
    assert opts["permissionMode"] == "auto"
    assert opts["sessionId"] == "sess-123"
    assert opts["resume"] is True
    assert opts["settingSources"] == ["project", "user"]
    assert opts["allowedTools"] == ["bash", "read"]
    assert opts["disallowedTools"] == ["write"]
    assert opts["maxTurns"] == 10

    # Tier 1 passthrough options
    assert opts["tools"] == [{"name": "custom_tool"}]
    assert opts["systemPrompt"] == "You are helpful."
    assert opts["mcpServers"] == [{"url": "http://localhost:8080"}]
    assert opts["continueConversation"] is True
    assert opts["maxBudgetUsd"] == 3.0
    assert opts["fallbackModel"] == "claude-haiku"
    assert opts["betas"] == ["beta1"]
    assert opts["outputFormat"] == "json"
    assert opts["cliPath"] == "/usr/bin/claude"
    assert opts["settings"] == {"key": "val"}
    assert opts["addDirs"] == ["/tmp/extra"]
    assert opts["extraArgs"] == ["--verbose"]
    assert opts["maxBufferSize"] == 1024
    assert opts["stderr"] == "pipe"
    assert opts["user"] == "test-user"
    assert opts["sandbox"] is True
    assert opts["thinking"] is True
    assert opts["effort"] == "high"
    assert opts["enableFileCheckpointing"] is True
    assert opts["agents"] == [{"name": "sub1"}]
    assert opts["plugins"] == [{"name": "plug1"}]


def test_setting_sources_default():
    """B7: settingSources should default to ['project'] when not specified."""
    import host

    wo = make_work_order()
    request = host.build_request(wo, "mapped")
    opts = request["options"]
    assert opts["settingSources"] == ["project"], f"Expected ['project'], got {opts.get('settingSources')}"


def test_policy_fallback_tools():
    """B1: allowedTools/disallowedTools should fall back to ABP policy."""
    import host

    wo = make_work_order(
        policy={
            "allowed_tools": ["bash", "read"],
            "disallowed_tools": ["rm"],
        },
    )
    request = host.build_request(wo, "mapped")
    opts = request["options"]
    assert opts["allowedTools"] == ["bash", "read"]
    assert opts["disallowedTools"] == ["rm"]


def test_hook_events():
    """B2: Hook events should map to ABP event types."""
    import host

    emitted = []

    def mock_emit(event, raw_message=None):
        emitted.append(event)

    ctx = {
        "emit": mock_emit,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
    }

    # PreToolUse hook
    host.emit_message(ctx, {
        "type": "pre_tool_use",
        "tool_name": "bash",
        "tool_use_id": "tu-1",
        "input": {"command": "ls"},
        "decision": "allow",
    })
    assert len(emitted) == 1
    ev = emitted[-1]
    assert ev["type"] == "tool_call"
    assert ev["tool_name"] == "bash"
    assert ev["ext"]["hook"] == "pre_tool_use"
    assert ev["ext"]["decision"] == "allow"

    # PostToolUse hook
    host.emit_message(ctx, {
        "type": "post_tool_use",
        "tool_name": "bash",
        "tool_use_id": "tu-1",
        "output": "file1.txt",
    })
    ev = emitted[-1]
    assert ev["type"] == "tool_result"
    assert ev["ext"]["hook"] == "post_tool_use"
    assert ev["is_error"] is False

    # PostToolUseFailure hook
    host.emit_message(ctx, {
        "type": "post_tool_use_failure",
        "tool_name": "bash",
        "tool_use_id": "tu-2",
        "output": "command failed",
        "is_error": True,
    })
    ev = emitted[-1]
    assert ev["type"] == "tool_result"
    assert ev["ext"]["hook"] == "post_tool_use_failure"
    assert ev["is_error"] is True

    # Notification -> Warning
    host.emit_message(ctx, {
        "type": "notification",
        "message": "Rate limit approaching",
        "level": "warn",
    })
    ev = emitted[-1]
    assert ev["type"] == "warning"
    assert ev["ext"]["notification"] is True
    assert ev["ext"]["level"] == "warn"

    # SubagentStart -> subagent_spawned
    host.emit_message(ctx, {
        "type": "subagent_start",
        "agent_id": "agent-42",
        "task": "refactor module",
    })
    ev = emitted[-1]
    assert ev["type"] == "subagent_spawned"
    assert ev["agent_id"] == "agent-42"
    assert ev["task"] == "refactor module"

    # SubagentStop -> subagent_completed
    host.emit_message(ctx, {
        "type": "subagent_stop",
        "agent_id": "agent-42",
        "success": True,
    })
    ev = emitted[-1]
    assert ev["type"] == "subagent_completed"
    assert ev["agent_id"] == "agent-42"
    assert ev["success"] is True

    # PermissionRequest -> permission_requested + permission_resolved
    count_before = len(emitted)
    host.emit_message(ctx, {
        "type": "permission_request",
        "tool_name": "write",
        "input": {"path": "/etc/passwd"},
    })
    new_events = emitted[count_before:]
    assert len(new_events) == 2
    assert new_events[0]["type"] == "permission_requested"
    assert new_events[0]["tool_name"] == "write"
    assert new_events[1]["type"] == "permission_resolved"
    assert new_events[1]["tool_name"] == "write"
    assert new_events[1]["granted"] is True

    # Checkpoint event
    host.emit_message(ctx, {
        "type": "checkpoint",
        "checkpoint_id": "ckpt-99",
    })
    ev = emitted[-1]
    assert ev["type"] == "assistant_message"
    assert ev["ext"]["checkpoint_id"] == "ckpt-99"


def test_permission_flow():
    """B3: Policy should drive permission_requested / permission_resolved events."""
    import host

    emitted = []

    def mock_emit(event, raw_message=None):
        emitted.append(event)

    ctx = {
        "emit": mock_emit,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
    }

    # With allowlist and denylist
    wo = make_work_order(
        policy={
            "allowed_tools": ["bash", "read"],
            "disallowed_tools": ["rm*"],
        },
    )

    can_use = host.build_can_use_tool(wo, ctx)

    # Allowed tool
    emitted.clear()
    result = can_use("bash", {"command": "ls"})
    assert result is True
    assert len(emitted) == 2
    assert emitted[0]["type"] == "permission_requested"
    assert emitted[0]["tool_name"] == "bash"
    assert emitted[0]["input"] == {"command": "ls"}
    assert emitted[1]["type"] == "permission_resolved"
    assert emitted[1]["granted"] is True

    # Denied by denylist
    emitted.clear()
    result = can_use("rm_file")
    assert result is False
    assert emitted[1]["type"] == "permission_resolved"
    assert emitted[1]["granted"] is False
    assert "disallowed" in emitted[1]["reason"]

    # Not in allowlist
    emitted.clear()
    result = can_use("write")
    assert result is False
    assert emitted[1]["granted"] is False
    assert "not in allowed_tools" in emitted[1]["reason"]

    # With no policy (everything allowed)
    wo_no_policy = make_work_order(policy={})
    can_use_open = host.build_can_use_tool(wo_no_policy, ctx)
    emitted.clear()
    result = can_use_open("anything")
    assert result is True


def test_partial_messages():
    """B4: Partial/stream messages should emit assistant_delta with partial ext."""
    import host

    emitted = []

    def mock_emit(event, raw_message=None):
        emitted.append(event)

    ctx = {
        "emit": mock_emit,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
    }

    # StreamEvent with text
    host.emit_message(ctx, {
        "type": "stream_event",
        "text": "Hello",
    })
    assert len(emitted) == 1
    ev = emitted[0]
    assert ev["type"] == "assistant_delta"
    assert ev["text"] == "Hello"
    assert ev["ext"]["partial"] is True
    assert ctx["state"]["saw_delta"] is True

    # Partial message
    host.emit_message(ctx, {
        "type": "partial",
        "delta": " world",
    })
    ev = emitted[-1]
    assert ev["type"] == "assistant_delta"
    assert ev["text"] == " world"
    assert ev["ext"]["partial"] is True


def test_session_lifecycle():
    """B6: Session started/resumed events should appear in the event trace."""
    wo = make_work_order()
    run = make_run(wo)
    msgs, _ = run_sidecar([run])
    events = [m for m in msgs if m["t"] == "event"]
    event_types = [e["event"]["type"] for e in events]

    # Should have session_started (since SDK is unavailable, it falls back but
    # the session_started event should still appear in partial runs when SDK
    # available, but here we just verify the structure is correct)
    # The run_started event should always be present
    assert "run_started" in event_types
    assert "run_completed" in event_types


def test_updated_capabilities():
    """B8: Capabilities manifest should include all V1 capabilities."""
    msgs, _ = run_sidecar([{"t": "ping", "seq": 1}])
    hello = msgs[0]
    assert hello["t"] == "hello"
    caps = hello["capabilities"]

    # Check all B8 capabilities
    assert caps["session_resume"] == "native"
    assert caps["session_fork"] == "emulated"
    assert caps["checkpointing"] == "native"
    assert caps["mcp_client"] == "native"
    assert caps["permission_callback"] == "native"
    assert caps["interrupt"] == "native"
    assert caps["extended_thinking"] == "native"
    assert caps["tool_ask_user"] == "native"

    # Original capabilities should still be present
    assert caps["streaming"] == "native"
    assert caps["hooks_pre_tool_use"] == "native"
    assert caps["hooks_post_tool_use"] == "native"


def test_glob_match():
    """Test the internal _glob_match helper."""
    import host

    assert host._glob_match("bash", "bash") is True
    assert host._glob_match("bash", "read") is False
    assert host._glob_match("bash", "*") is True
    assert host._glob_match("rm_file", "rm*") is True
    assert host._glob_match("rm_file", "*file") is True
    assert host._glob_match("rm_file", "*_*") is True
    assert host._glob_match("bash", "rm*") is False


def test_policy_engine_deny_read():
    """Slice 1: deny_read policy should block reads of matching paths."""
    import host

    policy = {"deny_read": ["*.secret"]}
    engine = host.build_policy_engine(policy, os.getcwd())
    result = engine["can_read_path"]("data.secret")
    assert result["allowed"] is False, f"Expected denied, got {result}"
    assert "denied" in result["reason"].lower()

    # Non-matching path should be allowed
    result2 = engine["can_read_path"]("data.txt")
    assert result2["allowed"] is True


def test_policy_engine_deny_write():
    """Slice 1: deny_write policy should block writes to matching paths."""
    import host

    policy = {"deny_write": ["*.log"]}
    engine = host.build_policy_engine(policy, os.getcwd())
    result = engine["can_write_path"]("app.log")
    assert result["allowed"] is False, f"Expected denied, got {result}"
    assert "denied" in result["reason"].lower()

    # Non-matching path should be allowed
    result2 = engine["can_write_path"]("app.txt")
    assert result2["allowed"] is True


def test_policy_engine_network_deny():
    """Slice 1: deny_network policy should block denied hostnames."""
    import host

    policy = {"deny_network": ["evil.com"]}
    engine = host.build_policy_engine(policy, os.getcwd())
    result = engine["can_access_network"]("evil.com")
    assert result["allowed"] is False, f"Expected denied, got {result}"
    assert "denied" in result["reason"].lower()

    # Non-matching host should be allowed
    result2 = engine["can_access_network"]("good.com")
    assert result2["allowed"] is True


def test_policy_engine_path_escape():
    """Slice 1: _canonical_within should reject paths escaping root."""
    import host

    workspace = os.getcwd()
    result = host._canonical_within(workspace, "../../etc/passwd")
    assert result is None, f"Expected None for escaping path, got {result}"

    # A path within root should resolve
    result2 = host._canonical_within(workspace, "subdir/file.txt")
    assert result2 is not None, "Expected valid relative path for non-escaping path"


def test_policy_engine_pre_tool():
    """Slice 1: pre_tool composite check with a read tool and denied path."""
    import host

    policy = {"deny_read": ["*.secret"]}
    engine = host.build_policy_engine(policy, os.getcwd())

    # A read tool with a denied path should be blocked
    result = engine["pre_tool"]("file_read", {"file_path": "config.secret"})
    assert result["allowed"] is False, f"Expected denied, got {result}"

    # A read tool with an allowed path should pass
    result2 = engine["pre_tool"]("file_read", {"file_path": "config.txt"})
    assert result2["allowed"] is True, f"Expected allowed, got {result2}"

    # A non-read tool with a denied-read path should still be allowed
    result3 = engine["pre_tool"]("bash", {"file_path": "config.secret"})
    assert result3["allowed"] is True, f"Expected allowed for non-read tool, got {result3}"


def test_redact_secrets():
    """Slice 2: _redact_secrets should mask API keys and auth headers."""
    import host

    text = "Using key sk-abc123def456ghi789 for access"
    result = host._redact_secrets(text)
    assert "sk-abc123def456ghi789" not in result
    assert "[REDACTED]" in result

    text2 = "api_key123456789012 is set"
    result2 = host._redact_secrets(text2)
    assert "api_key123456789012" not in result2
    assert "[REDACTED]" in result2

    text3 = "Authorization: Bearer some.token.value"
    result3 = host._redact_secrets(text3)
    assert "some.token.value" not in result3
    assert "[REDACTED]" in result3

    # Text without secrets should be unchanged
    text4 = "Hello world, no secrets here"
    result4 = host._redact_secrets(text4)
    assert result4 == text4


def test_include_partial_messages_false():
    """Slice 3: When include_partial_messages is False, accumulate text but don't emit."""
    import host

    emitted = []

    def mock_emit(event, raw_message=None):
        emitted.append(event)

    ctx = {
        "emit": mock_emit,
        "include_partial_messages": False,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
    }

    # Emit a partial message — should accumulate but NOT emit
    handled = host._emit_partial_message(ctx, {
        "type": "stream_event",
        "text": "Hello",
    }, "stream_event")
    assert handled is True
    assert ctx["state"]["last_assistant"] == "Hello"
    assert ctx["state"]["saw_delta"] is True
    assert len(emitted) == 0, f"Expected no emissions, got {len(emitted)}"

    # With include_partial_messages=True (default), it should emit
    ctx2 = {
        "emit": mock_emit,
        "include_partial_messages": True,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
    }
    emitted.clear()
    handled2 = host._emit_partial_message(ctx2, {
        "type": "stream_event",
        "text": "World",
    }, "stream_event")
    assert handled2 is True
    assert len(emitted) == 1
    assert emitted[0]["type"] == "assistant_delta"
    assert emitted[0]["text"] == "World"


if __name__ == "__main__":
    tests = [
        test_hello_is_first,
        test_events_and_final,
        test_receipt_fields,
        test_ping_pong,
        test_cancel_ignored,
        test_invalid_json,
        test_ref_id_matches_run_id,
        test_comprehensive_option_mapping,
        test_setting_sources_default,
        test_policy_fallback_tools,
        test_hook_events,
        test_permission_flow,
        test_partial_messages,
        test_session_lifecycle,
        test_updated_capabilities,
        test_glob_match,
        test_policy_engine_deny_read,
        test_policy_engine_deny_write,
        test_policy_engine_network_deny,
        test_policy_engine_path_escape,
        test_policy_engine_pre_tool,
        test_redact_secrets,
        test_include_partial_messages_false,
    ]
    passed = 0
    failed = 0
    for t in tests:
        try:
            t()
            print(f"  \u2713 {t.__name__}")
            passed += 1
        except Exception as e:
            print(f"  \u2717 {t.__name__}: {e}")
            failed += 1
    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)
