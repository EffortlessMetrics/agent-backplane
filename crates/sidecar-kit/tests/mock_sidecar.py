"""Mock sidecar for sidecar-kit lifecycle tests.

Speaks the JSONL Frame protocol (tag = "t").

Modes:
  default         - hello → run → event → final
  large_stream    - hello → run → 100 events → final
  error_midstream - hello → run → event → fatal
  empty_work_order- hello → run → final (no events)
  tool_call       - hello → run → tool_call event → tool_result event → final
  multi_run       - hello → (run → event → final) repeated for each run on stdin
  slow            - hello → run → event (0.5s delay each) → final
  crash           - hello → run → event → exit(1)
"""
import sys
import json
import time

mode = sys.argv[1] if len(sys.argv) > 1 else "default"


def make_hello():
    return {
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {"id": "mock-lifecycle", "version": "0.1"},
        "capabilities": {"streaming": True},
    }


def read_run():
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    run = json.loads(line)
    return run["id"], run.get("work_order", {})


def make_event(ref_id, payload):
    return {"t": "event", "ref_id": ref_id, "event": payload}


def make_final(ref_id):
    return {
        "t": "final",
        "ref_id": ref_id,
        "receipt": {"status": "complete", "ref_id": ref_id},
    }


def emit(obj):
    print(json.dumps(obj), flush=True)


# ---- modes ----------------------------------------------------------------

if mode == "default":
    emit(make_hello())
    ref_id, wo = read_run()
    emit(make_event(ref_id, {"type": "progress", "step": 1}))
    emit(make_event(ref_id, {"type": "progress", "step": 2}))
    emit(make_final(ref_id))

elif mode == "large_stream":
    emit(make_hello())
    ref_id, wo = read_run()
    for i in range(100):
        emit(make_event(ref_id, {"type": "progress", "index": i}))
    emit(make_final(ref_id))

elif mode == "error_midstream":
    emit(make_hello())
    ref_id, wo = read_run()
    emit(make_event(ref_id, {"type": "progress", "step": 1}))
    emit({"t": "fatal", "ref_id": ref_id, "error": "processing failed"})

elif mode == "empty_work_order":
    emit(make_hello())
    ref_id, wo = read_run()
    emit(make_final(ref_id))

elif mode == "tool_call":
    emit(make_hello())
    ref_id, wo = read_run()
    emit(make_event(ref_id, {"type": "tool_call", "tool": "read_file", "args": {"path": "test.txt"}}))
    emit(make_event(ref_id, {"type": "tool_result", "tool": "read_file", "result": "file contents"}))
    emit(make_final(ref_id))

elif mode == "multi_run":
    emit(make_hello())
    # Handle up to 3 sequential runs
    for _ in range(3):
        try:
            line = sys.stdin.readline()
            if not line:
                break
            run = json.loads(line)
            ref_id = run["id"]
            emit(make_event(ref_id, {"type": "progress", "step": 1}))
            emit(make_final(ref_id))
        except Exception:
            break

elif mode == "slow":
    emit(make_hello())
    ref_id, wo = read_run()
    emit(make_event(ref_id, {"type": "progress", "step": "start"}))
    time.sleep(0.3)
    emit(make_event(ref_id, {"type": "progress", "step": "middle"}))
    time.sleep(0.3)
    emit(make_event(ref_id, {"type": "progress", "step": "end"}))
    emit(make_final(ref_id))

elif mode == "crash":
    emit(make_hello())
    ref_id, wo = read_run()
    emit(make_event(ref_id, {"type": "progress", "step": 1}))
    sys.stdout.flush()
    import os
    os._exit(1)

else:
    print(f"Unknown mode: {mode}", file=sys.stderr)
    sys.exit(1)
