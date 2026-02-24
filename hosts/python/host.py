#!/usr/bin/env python3

"""ABP Python sidecar with optional Claude SDK client mode."""

from __future__ import annotations

import asyncio
import importlib
import inspect
import json
import os
import sys
import uuid
from datetime import datetime, timezone
from typing import Any, Dict, Optional


CONTRACT_VERSION = "abp/v0.1"
ADAPTER_VERSION = "0.2.0"
DEFAULT_SDK_MODULES = ("claude_agent_sdk",)

backend = {
    "id": "python_sidecar",
    "backend_version": sys.version.split()[0],
    "adapter_version": ADAPTER_VERSION,
}

capabilities = {
    "streaming": "native",
    "tool_read": "emulated",
    "tool_write": "emulated",
    "tool_edit": "emulated",
    "structured_output_json_schema": "emulated",
    "hooks_pre_tool_use": "native",
    "hooks_post_tool_use": "native",
    "session_resume": "emulated",
}

cached_sdk: Optional[Dict[str, Any]] = None
cached_clients: Dict[str, Any] = {}


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def write(obj: Dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def safe_string(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    try:
        return json.dumps(value)
    except Exception:
        return str(value)


def as_object(value: Any) -> Dict[str, Any]:
    return value if isinstance(value, dict) else {}


def as_bool(value: Any, default: bool = False) -> bool:
    if isinstance(value, bool):
        return value
    return default


def get_vendor_namespace(work_order: Dict[str, Any], namespace: str) -> Dict[str, Any]:
    vendor = as_object(as_object(work_order.get("config")).get("vendor"))
    out = dict(as_object(vendor.get(namespace)))
    prefix = f"{namespace}."
    for key, value in vendor.items():
        if key.startswith(prefix):
            out[key[len(prefix):]] = value
    return out


def get_abp_vendor_value(work_order: Dict[str, Any], key: str) -> Any:
    vendor = as_object(as_object(work_order.get("config")).get("vendor"))
    abp = as_object(vendor.get("abp"))
    if key in abp:
        return abp[key]
    dotted = f"abp.{key}"
    if dotted in vendor:
        return vendor[dotted]
    return None


def get_execution_mode(work_order: Dict[str, Any]) -> str:
    return "passthrough" if get_abp_vendor_value(work_order, "mode") == "passthrough" else "mapped"


def get_passthrough_request(work_order: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    value = get_abp_vendor_value(work_order, "request")
    return value if isinstance(value, dict) else None


def build_prompt(work_order: Dict[str, Any]) -> str:
    prompt = str(work_order.get("task") or "").strip()
    context = as_object(work_order.get("context"))
    files = context.get("files") if isinstance(context.get("files"), list) else []
    snippets = context.get("snippets") if isinstance(context.get("snippets"), list) else []

    if files:
        prompt += "\n\nContext files:\n"
        for value in files:
            prompt += f"- {safe_string(value)}\n"
    if snippets:
        prompt += "\nContext snippets:\n"
        for raw in snippets:
            snippet = as_object(raw)
            prompt += f"\n[{safe_string(snippet.get('name') or 'snippet')}]\n{safe_string(snippet.get('content') or '')}\n"
    return prompt


def build_request(work_order: Dict[str, Any], mode: str) -> Dict[str, Any]:
    passthrough = get_passthrough_request(work_order)
    if mode == "passthrough" and passthrough is not None:
        return passthrough

    cfg = as_object(work_order.get("config"))
    claude_cfg = get_vendor_namespace(work_order, "claude")
    options = {
        "cwd": as_object(work_order.get("workspace")).get("root"),
        "model": cfg.get("model"),
        "permissionMode": claude_cfg.get("permissionMode") or claude_cfg.get("permission_mode"),
        "sessionId": claude_cfg.get("sessionId") or claude_cfg.get("session_id"),
        "resume": claude_cfg.get("resume") or claude_cfg.get("resume_session"),
        "settingSources": claude_cfg.get("settingSources") or claude_cfg.get("setting_sources"),
        "allowedTools": claude_cfg.get("allowedTools") or claude_cfg.get("allowed_tools"),
        "disallowedTools": claude_cfg.get("disallowedTools") or claude_cfg.get("disallowed_tools"),
        "maxTurns": claude_cfg.get("maxTurns") or claude_cfg.get("max_turns"),
    }
    options = {k: v for k, v in options.items() if v is not None}
    env_cfg = as_object(cfg.get("env"))
    if env_cfg:
        options["env"] = env_cfg

    return {"prompt": build_prompt(work_order), "options": options}


def normalize_usage(raw: Any) -> Dict[str, int]:
    usage = as_object(as_object(raw).get("usage")) or as_object(raw)
    out: Dict[str, int] = {}
    for target, keys in (
        ("input_tokens", ("input_tokens", "inputTokens", "prompt_tokens", "promptTokens")),
        ("output_tokens", ("output_tokens", "outputTokens", "completion_tokens", "completionTokens")),
        ("cache_read_tokens", ("cache_read_tokens", "cacheReadTokens")),
        ("cache_write_tokens", ("cache_write_tokens", "cacheWriteTokens")),
    ):
        for key in keys:
            value = usage.get(key)
            if isinstance(value, (int, float)):
                out[target] = int(value)
                break
    return out


def resolve_client_session_key(
    work_order: Dict[str, Any],
    request: Dict[str, Any],
    abp: Dict[str, Any],
    claude: Dict[str, Any],
) -> str:
    explicit = abp.get("client_session_key") or abp.get("clientSessionKey")
    explicit = explicit or claude.get("client_session_key") or claude.get("clientSessionKey")
    if isinstance(explicit, str) and explicit.strip():
        return explicit.strip()

    options = as_object(request.get("options"))
    session_id = options.get("sessionId") or options.get("session_id")
    if isinstance(session_id, str) and session_id.strip():
        return f"session:{session_id.strip()}"

    workspace_root = as_object(work_order.get("workspace")).get("root")
    if isinstance(workspace_root, str) and workspace_root.strip():
        return f"workspace:{workspace_root.strip()}"

    return "default"


def collect_usage(state: Dict[str, Any], message: Any) -> None:
    msg = as_object(message)
    usage = as_object(msg.get("usage"))
    nested = as_object(as_object(msg.get("message")).get("usage"))
    state["usage_raw"] = {**as_object(state["usage_raw"]), **usage, **nested}


def lower_type(message: Dict[str, Any]) -> str:
    return str(message.get("type") or message.get("kind") or message.get("event") or "").lower()


async def maybe_await(value: Any) -> Any:
    return await value if inspect.isawaitable(value) else value


async def to_async_iterable(value: Any):
    resolved = await maybe_await(value)
    if resolved is None:
        return
    if hasattr(resolved, "__aiter__"):
        async for item in resolved:
            yield item
        return
    if hasattr(resolved, "__iter__") and not isinstance(resolved, (str, bytes, bytearray, dict)):
        for item in resolved:
            yield item
        return
    yield resolved


def emit_message(ctx: Dict[str, Any], raw: Any, passthrough: bool = False) -> None:
    if passthrough:
        message = as_object(raw)
        text = message.get("text") or message.get("delta") or message.get("content") or ""
        kind = "assistant_delta"
        payload: Dict[str, Any] = {"text": str(text)}
        msg_type = lower_type(message)
        if "usage" in message:
            kind = "usage"
            payload = {"usage": message.get("usage")}
        elif "error" in msg_type:
            kind = "error"
            payload = {"message": safe_string(message.get("error") or message.get("message"))}
        elif "tool" in msg_type:
            tool_name = str(message.get("tool_name") or message.get("toolName") or message.get("name") or "unknown_tool")
            tool_use_id = message.get("tool_use_id") or message.get("toolUseId") or message.get("id")
            if "result" in msg_type or "output" in message or "result" in message:
                kind = "tool_result"
                payload = {
                    "tool_name": tool_name,
                    "tool_use_id": tool_use_id,
                    "output": message.get("output") if "output" in message else message.get("result"),
                    "is_error": as_bool(message.get("is_error") or message.get("isError")),
                }
            else:
                kind = "tool_call"
                payload = {
                    "tool_name": tool_name,
                    "tool_use_id": tool_use_id,
                    "parent_tool_use_id": None,
                    "input": message.get("input") or message.get("arguments") or message.get("args") or {},
                }
        elif "assistant" in msg_type or "message" in msg_type:
            kind = "assistant_message"
            payload = {"text": str(text)}
        ctx["emit"]({"type": kind, **payload}, raw_message=raw)
        return

    message = as_object(raw)
    if not message:
        return
    msg_type = lower_type(message)
    text = message.get("text") or message.get("delta") or (message.get("content") if isinstance(message.get("content"), str) else "")
    if text:
        if "delta" in msg_type or "stream" in msg_type:
            ctx["state"]["last_assistant"] += str(text)
            ctx["state"]["saw_delta"] = True
            ctx["emit"]({"type": "assistant_delta", "text": str(text)})
        else:
            ctx["state"]["last_assistant"] = str(text)
            ctx["state"]["saw_message"] = True
            ctx["emit"]({"type": "assistant_message", "text": str(text)})

    tool_name = message.get("tool_name") or message.get("toolName") or message.get("name")
    if tool_name:
        tool_use_id = message.get("tool_use_id") or message.get("toolUseId") or message.get("id")
        if "result" in msg_type or "output" in message or "result" in message:
            ctx["emit"](
                {
                    "type": "tool_result",
                    "tool_name": str(tool_name),
                    "tool_use_id": tool_use_id,
                    "output": message.get("output") if "output" in message else message.get("result"),
                    "is_error": as_bool(message.get("is_error") or message.get("isError")),
                }
            )
        else:
            ctx["emit"](
                {
                    "type": "tool_call",
                    "tool_name": str(tool_name),
                    "tool_use_id": tool_use_id,
                    "parent_tool_use_id": None,
                    "input": message.get("input") or message.get("arguments") or message.get("args") or {},
                }
            )
    if "error" in msg_type:
        ctx["emit"]({"type": "error", "message": safe_string(message.get("error") or message.get("message"))})


def resolve_sdk() -> Dict[str, Any]:
    global cached_sdk
    if cached_sdk is not None:
        return cached_sdk

    candidates = []
    env_module = os.environ.get("ABP_CLAUDE_SDK_MODULE")
    if env_module and env_module.strip():
        candidates.append(env_module.strip())
    candidates.extend(DEFAULT_SDK_MODULES)

    last_error: Optional[Exception] = None
    for candidate in candidates:
        try:
            module = importlib.import_module(candidate)
            query_fn = getattr(module, "query", None)
            client_ctor = getattr(module, "ClaudeSDKClient", None)
            create_client = getattr(module, "create_client", None) or getattr(module, "createClient", None)
            options_ctor = getattr(module, "ClaudeAgentOptions", None)
            if not callable(query_fn) and not callable(client_ctor) and not callable(create_client):
                continue
            cached_sdk = {
                "module_name": candidate,
                "query_fn": query_fn if callable(query_fn) else None,
                "client_ctor": client_ctor if callable(client_ctor) else None,
                "create_client": create_client if callable(create_client) else None,
                "options_ctor": options_ctor if callable(options_ctor) else None,
            }
            return cached_sdk
        except Exception as err:  # noqa: BLE001
            last_error = err
    raise RuntimeError(f"unable to load Claude SDK: {safe_string(last_error)}")


async def invoke_query(query_fn: Any, request: Dict[str, Any]) -> Any:
    if not callable(query_fn):
        raise RuntimeError("query() is unavailable")
    try:
        return await maybe_await(query_fn(request))
    except Exception:
        if "prompt" in request:
            try:
                return await maybe_await(query_fn(request.get("prompt"), request.get("options")))
            except Exception:
                return await maybe_await(query_fn(request.get("prompt")))
        raise


async def run_with_sdk(ctx: Dict[str, Any], work_order: Dict[str, Any], mode: str) -> Dict[str, Any]:
    request = build_request(work_order, mode)
    passthrough = mode == "passthrough" and get_passthrough_request(work_order) is not None
    try:
        sdk = resolve_sdk()
    except Exception as err:  # noqa: BLE001
        ctx["emit"]({"type": "warning", "message": safe_string(err)})
        ctx["emit"](
            {
                "type": "assistant_message",
                "text": "Claude SDK is unavailable. Install claude_agent_sdk to enable Python client/query execution.",
            }
        )
        return {
            "usage_raw": {
                "mode": "fallback",
                "reason": "sdk_unavailable",
                "error": safe_string(err),
            },
            "usage": {},
            "outcome": "partial",
        }

    abp = get_vendor_namespace(work_order, "abp")
    claude = get_vendor_namespace(work_order, "claude")
    client_mode = as_bool(abp.get("client_mode"), as_bool(claude.get("client_mode"), False))
    client_persist = as_bool(abp.get("client_persist"), as_bool(claude.get("client_persist"), False))
    timeout_ms = abp.get("client_timeout_ms") or abp.get("clientTimeoutMs") or 0
    timeout_s = float(timeout_ms) / 1000.0 if isinstance(timeout_ms, (int, float)) and timeout_ms > 0 else None
    client_session_key: Optional[str] = None

    if client_mode and not (callable(sdk.get("create_client")) or callable(sdk.get("client_ctor"))):
        ctx["emit"](
            {
                "type": "warning",
                "message": "abp.client_mode=true requested, but Python SDK does not expose ClaudeSDKClient; falling back to query().",
            }
        )
        client_mode = False
    if not client_mode and not callable(sdk.get("query_fn")):
        ctx["emit"](
            {
                "type": "warning",
                "message": "Python Claude SDK module does not expose query(); returning fallback outcome.",
            }
        )
        return {
            "usage_raw": {
                "sdk_module": sdk["module_name"],
                "mode": "fallback",
                "reason": "query_unavailable",
                "client_mode": client_mode,
            },
            "usage": {},
            "outcome": "partial",
        }

    state = ctx["state"]
    if client_mode:
        options = as_object(request.get("options"))
        options_ctor = sdk.get("options_ctor")
        built_options: Any = options
        if callable(options_ctor):
            try:
                built_options = options_ctor(**options)
            except Exception:
                try:
                    built_options = options_ctor(options)
                except Exception:
                    built_options = options

        client_session_key = resolve_client_session_key(work_order, request, abp, claude)
        use_cached_client = client_persist and client_session_key in cached_clients
        if use_cached_client:
            client = cached_clients[client_session_key]
        elif callable(sdk.get("create_client")):
            client = await maybe_await(sdk["create_client"](built_options))
        else:
            ctor = sdk["client_ctor"]
            try:
                client = await maybe_await(ctor(options=built_options))
            except Exception:
                client = await maybe_await(ctor(built_options))

        if not use_cached_client:
            connect = getattr(client, "connect", None)
            if callable(connect):
                await maybe_await(connect())
            if client_persist:
                cached_clients[client_session_key] = client

        try:
            query_call = invoke_query(getattr(client, "query", None), request)
            try:
                query_result = await asyncio.wait_for(query_call, timeout_s) if timeout_s else await query_call
            except asyncio.TimeoutError as timeout_err:
                interrupt = getattr(client, "interrupt", None) or getattr(client, "cancel", None)
                if callable(interrupt):
                    await maybe_await(interrupt())
                raise RuntimeError(
                    f"Claude SDK client query timed out after {int(timeout_s * 1000)}ms"
                ) from timeout_err
            receive = getattr(client, "receive_response", None) or getattr(client, "receiveResponse", None)
            source = await maybe_await(receive()) if callable(receive) else query_result
            async for item in to_async_iterable(source):
                collect_usage(state, item)
                emit_message(ctx, item, passthrough=passthrough)
        except Exception:
            if client_persist and client_session_key and cached_clients.get(client_session_key) is client:
                cached_clients.pop(client_session_key, None)
            disconnect = getattr(client, "disconnect", None) or getattr(client, "close", None)
            if callable(disconnect):
                await maybe_await(disconnect())
            raise
        finally:
            if not client_persist:
                disconnect = getattr(client, "disconnect", None) or getattr(client, "close", None)
                if callable(disconnect):
                    await maybe_await(disconnect())
    else:
        response = await invoke_query(sdk.get("query_fn"), request)
        async for item in to_async_iterable(response):
            collect_usage(state, item)
            emit_message(ctx, item, passthrough=passthrough)

    if not passthrough and state["saw_delta"] and not state["saw_message"] and state["last_assistant"]:
        ctx["emit"]({"type": "assistant_message", "text": state["last_assistant"]})

    return {
        "usage_raw": {
            "sdk_module": sdk["module_name"],
            "transport": "client" if client_mode else "query",
            "client_mode": client_mode,
            **(
                {
                    "client_persist": client_persist,
                    "client_session_key": client_session_key,
                }
                if client_mode
                else {}
            ),
            **as_object(state["usage_raw"]),
        },
        "usage": normalize_usage(state["usage_raw"]),
        "outcome": "complete",
        **({"stream_equivalent": True} if passthrough else {}),
    }


async def handle_run(msg: Dict[str, Any]) -> None:
    run_id = msg.get("id") or str(uuid.uuid4())
    work_order = as_object(msg.get("work_order"))
    mode = get_execution_mode(work_order)
    started_at = now_iso()
    trace = []

    def emit(event: Dict[str, Any], raw_message: Any = None) -> None:
        payload = {"ts": now_iso(), **event}
        if raw_message is not None:
            payload["ext"] = {"raw_message": raw_message}
        trace.append(payload)
        write({"t": "event", "ref_id": run_id, "event": payload})

    emit({"type": "run_started", "message": f"python sidecar starting: {safe_string(work_order.get('task'))}"})
    emit({"type": "assistant_message", "text": f"Execution mode: {mode}"})

    ctx = {
        "emit": emit,
        "state": {
            "usage_raw": {},
            "last_assistant": "",
            "saw_delta": False,
            "saw_message": False,
        },
    }

    outcome = "complete"
    usage_raw: Dict[str, Any] = {}
    usage: Dict[str, int] = {}
    stream_equivalent = False
    try:
        result = await run_with_sdk(ctx, work_order, mode)
        usage_raw = as_object(result.get("usage_raw"))
        usage = normalize_usage(usage_raw)
        outcome = str(result.get("outcome") or "complete")
        stream_equivalent = bool(result.get("stream_equivalent"))
    except Exception as err:  # noqa: BLE001
        outcome = "failed"
        emit({"type": "error", "message": f"adapter error: {safe_string(err)}"})

    emit({"type": "run_completed", "message": f"python sidecar run completed with outcome={outcome}"})
    finished_at = now_iso()
    duration_ms = max(
        0,
        int((datetime.fromisoformat(finished_at.replace("Z", "+00:00")) - datetime.fromisoformat(started_at.replace("Z", "+00:00"))).total_seconds() * 1000),
    )

    receipt: Dict[str, Any] = {
        "meta": {
            "run_id": run_id,
            "work_order_id": work_order.get("id"),
            "contract_version": CONTRACT_VERSION,
            "started_at": started_at,
            "finished_at": finished_at,
            "duration_ms": duration_ms,
        },
        "backend": backend,
        "capabilities": capabilities,
        "mode": mode,
        "usage_raw": usage_raw,
        "usage": usage,
        "trace": trace,
        "artifacts": [],
        "verification": {"git_diff": None, "git_status": None, "harness_ok": True},
        "outcome": outcome,
        "receipt_sha256": None,
    }
    if mode == "passthrough" and stream_equivalent:
        receipt["stream_equivalent"] = True
    write({"t": "final", "ref_id": run_id, "receipt": receipt})


async def close_cached_clients() -> None:
    for key, client in list(cached_clients.items()):
        cached_clients.pop(key, None)
        disconnect = getattr(client, "disconnect", None) or getattr(client, "close", None)
        if callable(disconnect):
            try:
                await maybe_await(disconnect())
            except Exception:
                pass


async def main() -> None:
    write(
        {
            "t": "hello",
            "contract_version": CONTRACT_VERSION,
            "backend": backend,
            "capabilities": capabilities,
            "mode": "mapped",
        }
    )

    while True:
        line = await asyncio.to_thread(sys.stdin.readline)
        if line == "":
            break
        raw = line.strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except Exception as err:  # noqa: BLE001
            write({"t": "fatal", "ref_id": None, "error": f"invalid json: {safe_string(err)}"})
            continue
        if msg.get("t") != "run":
            continue
        try:
            await handle_run(as_object(msg))
        except Exception as err:  # noqa: BLE001
            write({"t": "fatal", "ref_id": msg.get("id"), "error": f"run failed: {safe_string(err)}"})

    await close_cached_clients()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except Exception as err:  # noqa: BLE001
        write({"t": "fatal", "ref_id": None, "error": f"python host failed: {safe_string(err)}"})
        sys.exit(1)
