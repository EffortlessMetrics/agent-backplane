"""Minimal mock sidecar for abp-host integration tests.

Speaks the JSONL protocol expected by SidecarClient.
Supports different test modes via an optional command-line argument.

Modes:
  default          - hello → run → event → final (original behaviour)
  multi_events     - hello → run → 5 events → final
  multi_event_kinds- hello → run → events of varied kinds → final
  slow             - hello → run → events with delays → final
  bad_json_midstream - hello → run → event → malformed line
  wrong_version    - hello with wrong contract version → run → final
  no_hello         - sends an event envelope as first line (no hello)
  fatal            - hello → run → event → fatal
  hang             - hello → run → event → sleep forever
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
            "id": "mock-test",
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


def make_final(ref_id):
    return {"t": "final", "ref_id": ref_id, "receipt": make_receipt(ref_id)}


def emit(obj):
    print(json.dumps(obj), flush=True)


# ---- modes ----------------------------------------------------------------

if mode == "default":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="mock test started"))
    emit(make_final(ref_id))

elif mode == "multi_events":
    emit(make_hello())
    ref_id = read_run()
    for i in range(5):
        emit(make_event(ref_id, "run_started", message=f"event {i}"))
    emit(make_final(ref_id))

elif mode == "multi_event_kinds":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="started"))
    emit(make_event(ref_id, "assistant_delta", text="Hello "))
    emit(make_event(ref_id, "assistant_message", text="Hello world"))
    emit(make_event(ref_id, "file_changed", path="test.txt", summary="created"))
    emit(make_event(ref_id, "run_completed", message="done"))
    emit(make_final(ref_id))

elif mode == "slow":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="starting slow"))
    time.sleep(0.3)
    emit(make_event(ref_id, "assistant_message", text="thinking..."))
    time.sleep(0.3)
    emit(make_event(ref_id, "run_completed", message="done slow"))
    emit(make_final(ref_id))

elif mode == "bad_json_midstream":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="about to break"))
    print("this is not valid json {{{", flush=True)
    # host terminates on bad JSON; lines below are unreachable
    emit(make_event(ref_id, "run_completed", message="unreachable"))
    emit(make_final(ref_id))

elif mode == "wrong_version":
    emit(make_hello(version="abp/v999.0"))
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="wrong version"))
    emit(make_final(ref_id))

elif mode == "no_hello":
    # Send a non-hello envelope as the very first line.
    emit(make_event("fake", "run_started", message="no hello"))

elif mode == "fatal":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="about to fail"))
    emit({"t": "fatal", "ref_id": ref_id, "error": "something went wrong"})

elif mode == "hang":
    emit(make_hello())
    ref_id = read_run()
    emit(make_event(ref_id, "run_started", message="going to hang"))
    # Sleep long enough that the test timeout fires first.
    time.sleep(5)

else:
    print(f"Unknown mode: {mode}", file=sys.stderr)
    sys.exit(1)
