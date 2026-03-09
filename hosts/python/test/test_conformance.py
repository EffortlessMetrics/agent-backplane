"""Cross-surface conformance tests for Python sidecar.

Loads test fixtures from tests/fixtures/claude_agent_conformance.json and
validates behavior against host.py.
"""

import json
import os
import sys

FIXTURE_PATH = os.path.join(
    os.path.dirname(__file__), "..", "..", "..", "tests", "fixtures", "claude_agent_conformance.json"
)

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


def load_fixtures():
    with open(FIXTURE_PATH) as f:
        return json.load(f)


def make_ctx():
    emitted = []
    def emit(event, raw_message=None):
        payload = {"ts": "2025-01-01T00:00:00Z", **event}
        if raw_message is not None:
            payload["ext"] = {"raw_message": raw_message}
        emitted.append(payload)
    return {
        "emit": emit,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
        "include_partial_messages": True,
        "emitted": emitted,
    }


def test_hello_shape():
    """Fixture: hello_shape"""
    import host
    assert hasattr(host, "CONTRACT_VERSION")
    assert host.CONTRACT_VERSION == "abp/v0.1"
    assert isinstance(host.backend, dict)
    assert isinstance(host.capabilities, dict)
    assert "id" in host.backend
    assert "streaming" in host.capabilities


def test_hook_pre_tool_use():
    """Fixture: hook_pre_tool_use"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {
        "type": "pre_tool_use",
        "tool_name": "bash",
        "tool_use_id": "tu-conf-1",
        "input": {"command": "ls"},
        "decision": "allow",
    })
    emitted = ctx["emitted"]
    tool_calls = [e for e in emitted if e.get("type") == "tool_call"]
    assert len(tool_calls) >= 1, "Expected at least one tool_call event"
    assert tool_calls[0].get("ext", {}).get("hook") == "pre_tool_use"


def test_hook_post_tool_use():
    """Fixture: hook_post_tool_use"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {
        "type": "post_tool_use",
        "tool_name": "bash",
        "tool_use_id": "tu-conf-2",
        "output": "file1.txt",
    })
    emitted = ctx["emitted"]
    tool_results = [e for e in emitted if e.get("type") == "tool_result"]
    assert len(tool_results) >= 1, "Expected at least one tool_result event"
    assert tool_results[0].get("ext", {}).get("hook") == "post_tool_use"


def test_permission_flow():
    """Fixture: permission_flow"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {
        "type": "permission_request",
        "tool_name": "write",
        "input": {"path": "/tmp/test"},
    })
    emitted = ctx["emitted"]
    types = [e.get("type") for e in emitted]
    assert "permission_requested" in types
    assert "permission_resolved" in types
    resolved = [e for e in emitted if e.get("type") == "permission_resolved"]
    assert resolved[0]["granted"] is True


def test_partial_messages():
    """Fixture: partial_messages"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {"type": "stream_event", "text": "hello"})
    host.emit_message(ctx, {"type": "partial", "delta": " world"})
    emitted = ctx["emitted"]
    deltas = [e for e in emitted if e.get("type") == "assistant_delta"]
    assert len(deltas) >= 2, f"Expected at least 2 deltas, got {len(deltas)}"


def test_checkpoint_event():
    """Fixture: checkpoint_event"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {
        "type": "checkpoint",
        "checkpoint_id": "ckpt-conf-1",
    })
    emitted = ctx["emitted"]
    msgs = [e for e in emitted if e.get("type") == "assistant_message"]
    assert len(msgs) >= 1
    # Should have checkpoint ext
    has_ckpt = any(e.get("ext", {}).get("checkpoint_id") == "ckpt-conf-1" for e in msgs)
    assert has_ckpt, "Expected checkpoint_id in ext"


def test_notification_event():
    """Fixture: notification_event"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {
        "type": "notification",
        "message": "Rate limit approaching",
        "level": "warn",
    })
    emitted = ctx["emitted"]
    warnings = [e for e in emitted if e.get("type") == "warning"]
    assert len(warnings) >= 1


def test_subagent_lifecycle():
    """Fixture: subagent_lifecycle"""
    import host
    ctx = make_ctx()
    host.emit_message(ctx, {
        "type": "subagent_start",
        "agent_id": "agent-conf-1",
        "task": "refactor",
    })
    host.emit_message(ctx, {
        "type": "subagent_stop",
        "agent_id": "agent-conf-1",
        "success": True,
    })
    emitted = ctx["emitted"]
    types = [e.get("type") for e in emitted]
    assert "subagent_spawned" in types
    assert "subagent_completed" in types


def test_receipt_shape():
    """Fixture: receipt_shape - validated via sidecar run."""
    from test_protocol import run_sidecar, make_run, make_work_order
    wo = make_work_order()
    run = make_run(wo)
    msgs, _ = run_sidecar([run])
    final = next(m for m in msgs if m["t"] == "final")
    r = final["receipt"]
    for field in ("meta", "backend", "outcome", "usage", "usage_raw"):
        assert field in r, f"Receipt missing '{field}'"
    meta = r["meta"]
    for mf in ("run_id", "contract_version", "started_at", "finished_at", "duration_ms"):
        assert mf in meta, f"Receipt meta missing '{mf}'"


if __name__ == "__main__":
    tests = [
        test_hello_shape,
        test_hook_pre_tool_use,
        test_hook_post_tool_use,
        test_permission_flow,
        test_partial_messages,
        test_checkpoint_event,
        test_notification_event,
        test_subagent_lifecycle,
        test_receipt_shape,
    ]
    passed = 0
    failed = 0
    for t in tests:
        try:
            t()
            print(f"  PASS {t.__name__}")
            passed += 1
        except Exception as e:
            print(f"  FAIL {t.__name__}: {e}")
            failed += 1
    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)
