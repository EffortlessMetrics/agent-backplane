"""Minimal mock sidecar for abp-host integration tests.

Speaks the JSONL protocol expected by SidecarClient:
  1. Emits a hello envelope
  2. Reads the run envelope from stdin
  3. Emits a run_started event
  4. Emits a final envelope with a receipt
"""
import sys
import json
import datetime

# 1. Hello
hello = {
    "t": "hello",
    "contract_version": "abp/v0.1",
    "backend": {
        "id": "mock-test",
        "backend_version": "0.1",
        "adapter_version": "0.1",
    },
    "capabilities": {},
    "mode": "mapped",
}
print(json.dumps(hello), flush=True)

# 2. Read the run envelope
line = sys.stdin.readline()
run = json.loads(line)
ref_id = run["id"]

# 3. Event
event = {
    "t": "event",
    "ref_id": ref_id,
    "event": {
        "ts": "2024-01-01T00:00:00Z",
        "type": "run_started",
        "message": "mock test started",
    },
}
print(json.dumps(event), flush=True)

# 4. Final receipt
now = datetime.datetime.now(datetime.timezone.utc).isoformat()
receipt = {
    "meta": {
        "run_id": ref_id,
        "work_order_id": "00000000-0000-0000-0000-000000000000",
        "contract_version": "abp/v0.1",
        "started_at": now,
        "finished_at": now,
        "duration_ms": 0,
    },
    "backend": {
        "id": "mock-test",
        "backend_version": "0.1",
        "adapter_version": "0.1",
    },
    "capabilities": {},
    "mode": "mapped",
    "usage_raw": {},
    "usage": {"input_tokens": 0, "output_tokens": 0},
    "trace": [],
    "artifacts": [],
    "verification": {"harness_ok": True},
    "outcome": "complete",
    "receipt_sha256": None,
}
final_env = {"t": "final", "ref_id": ref_id, "receipt": receipt}
print(json.dumps(final_env), flush=True)
