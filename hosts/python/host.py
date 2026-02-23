#!/usr/bin/env python3

"""Simple ABP sidecar example.

Transport: JSONL over stdin/stdout.
"""

import json
import sys
import uuid
from datetime import datetime, timezone


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def write(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


backend = {
    "id": "example_python_sidecar",
    "backend_version": sys.version.split()[0],
    "adapter_version": "0.1",
}

capabilities = {
    "streaming": "native",
    "tool_read": "emulated",
    "tool_write": "emulated",
    "tool_edit": "emulated",
    "structured_output_json_schema": "emulated",
}

# Hello first.
write(
    {
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": backend,
        "capabilities": capabilities,
    }
)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue

    try:
        msg = json.loads(line)
    except Exception as e:
        write({"t": "fatal", "ref_id": None, "error": f"invalid json: {e}"})
        continue

    if msg.get("t") == "run":
        run_id = msg.get("id") or str(uuid.uuid4())
        work_order = msg.get("work_order")
        started = now_iso()

        trace = []

        def emit(kind):
            ev = {"ts": now_iso(), **kind}
            trace.append(ev)
            write({"t": "event", "ref_id": run_id, "event": ev})

        emit({"type": "run_started", "message": f"python sidecar starting: {work_order.get('task')}"})
        emit({"type": "assistant_message", "text": "Hello from the Python sidecar. Replace me with a real SDK adapter."})
        emit({"type": "assistant_message", "text": f"workspace root: {work_order.get('workspace', {}).get('root')}"})
        emit({"type": "run_completed", "message": "python sidecar complete"})

        finished = now_iso()

        receipt = {
            "meta": {
                "run_id": run_id,
                "work_order_id": work_order.get("id"),
                "contract_version": "abp/v0.1",
                "started_at": started,
                "finished_at": finished,
                "duration_ms": 0,
            },
            "backend": backend,
            "capabilities": capabilities,
            "usage_raw": {"note": "example_python_sidecar"},
            "usage": {},
            "trace": trace,
            "artifacts": [],
            "verification": {"git_diff": None, "git_status": None, "harness_ok": True},
            "outcome": "complete",
            "receipt_sha256": None,
        }

        write({"t": "final", "ref_id": run_id, "receipt": receipt})
