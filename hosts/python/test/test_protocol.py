"""Protocol conformance tests for the Python ABP sidecar.

Spawns host.py as a child process, sends JSONL envelopes on stdin,
and asserts the expected responses appear on stdout.
"""

import json
import os
import subprocess
import sys
import uuid

HOST = os.path.join(os.path.dirname(__file__), "..", "host.py")
PYTHON = sys.executable


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


# -- Tests -------------------------------------------------------------------


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


if __name__ == "__main__":
    tests = [
        test_hello_is_first,
        test_events_and_final,
        test_receipt_fields,
        test_ping_pong,
        test_cancel_ignored,
        test_invalid_json,
        test_ref_id_matches_run_id,
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
