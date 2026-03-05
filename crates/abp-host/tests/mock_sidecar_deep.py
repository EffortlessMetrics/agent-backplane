"""Deep mock sidecar for abp-host protocol_conformance_deep tests.

Provides modes not covered by mock_sidecar.py.

Modes:
  fatal_with_code   - hello -> run -> fatal with error_code
  wrong_ref_final   - hello -> run -> event -> final with wrong ref_id -> exit
  slow_hello        - sleep then hello -> run -> event -> final
"""
import sys
import json
import datetime
import time

mode = sys.argv[1] if len(sys.argv) > 1 else "default"


def make_hello(version="abp/v0.1"):
    return {
        "t": "hello",
        "contract_version": version,
        "backend": {
            "id": "mock-deep",
            "backend_version": "0.1",
            "adapter_version": "0.1",
        },
        "capabilities": {},
        "mode": "mapped",
    }


def read_run():
    line = sys.stdin.readline()
    run = json.loads(line)
    return run["id"]


def make_event(ref_id, event_type, **kwargs):
    event = {"ts": "2024-01-01T00:00:00Z", "type": event_type}
    event.update(kwargs)
    return {"t": "event", "ref_id": ref_id, "event": event}


def make_receipt(ref_id):
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    return {
        "meta": {
            "run_id": ref_id,
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": now,
            "finished_at": now,
            "duration_ms": 0,
        },
        "backend": {
            "id": "mock-deep",
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


def make_final(ref_id):
    return {"t": "final", "ref_id": ref_id, "receipt": make_receipt(ref_id)}


def emit(obj):
    print(json.dumps(obj), flush=True)


# ---- modes ----------------------------------------------------------------

if mode == "fatal_with_code":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="about to fail with code"))
    emit({
        "t": "fatal",
        "ref_id": ref_id,
        "error": "rate limited",
        "error_code": "backend_rate_limited",
    })

elif mode == "wrong_ref_final":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="will send wrong final"))
    emit({"t": "final", "ref_id": "totally-wrong-ref-id", "receipt": make_receipt("totally-wrong-ref-id")})
    # Exit after sending wrong final so the test doesn't hang.
    sys.exit(0)

elif mode == "slow_hello":
    time.sleep(1)
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="slow hello done"))
    emit(make_final(ref_id))

else:
    print(f"Unknown mode: {mode}", file=sys.stderr)
    sys.exit(1)
