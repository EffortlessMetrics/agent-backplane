#!/usr/bin/env python3

"""ABP Python sidecar with optional Claude SDK client mode."""

from __future__ import annotations

import asyncio
import importlib
import inspect
import json
import os
import re
import sys
import uuid
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional
from urllib.parse import urlparse


CONTRACT_VERSION = "abp/v0.1"
ADAPTER_VERSION = "0.3.0"
DEFAULT_SDK_MODULES = ("claude_agent_sdk",)
MAX_INLINE_OUTPUT_BYTES = 8192

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
    "session_resume": "native",
    "session_fork": "emulated",
    "checkpointing": "native",
    "mcp_client": "native",
    "permission_callback": "native",
    "interrupt": "native",
    "extended_thinking": "native",
    "tool_ask_user": "native",
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


def _pick(cfg: Dict[str, Any], camel: str, snake: str) -> Any:
    """Pick a value from config trying camelCase first, then snake_case."""
    return cfg.get(camel) or cfg.get(snake)


def build_request(work_order: Dict[str, Any], mode: str) -> Dict[str, Any]:
    passthrough = get_passthrough_request(work_order)
    if mode == "passthrough" and passthrough is not None:
        return passthrough

    cfg = as_object(work_order.get("config"))
    claude_cfg = get_vendor_namespace(work_order, "claude")
    policy = as_object(work_order.get("policy"))
    options: Dict[str, Any] = {
        "cwd": as_object(work_order.get("workspace")).get("root"),
        "model": cfg.get("model"),
        "permissionMode": _pick(claude_cfg, "permissionMode", "permission_mode"),
        "sessionId": _pick(claude_cfg, "sessionId", "session_id"),
        "resume": _pick(claude_cfg, "resume", "resume_session"),
        "allowedTools": _pick(claude_cfg, "allowedTools", "allowed_tools"),
        "disallowedTools": _pick(claude_cfg, "disallowedTools", "disallowed_tools"),
        "maxTurns": _pick(claude_cfg, "maxTurns", "max_turns"),
        # B1: Tier 1 direct passthrough options
        "tools": _pick(claude_cfg, "tools", "tools"),
        "systemPrompt": _pick(claude_cfg, "systemPrompt", "system_prompt"),
        "mcpServers": _pick(claude_cfg, "mcpServers", "mcp_servers"),
        "continueConversation": _pick(claude_cfg, "continueConversation", "continue_conversation"),
        "maxBudgetUsd": _pick(claude_cfg, "maxBudgetUsd", "max_budget_usd"),
        "fallbackModel": _pick(claude_cfg, "fallbackModel", "fallback_model"),
        "betas": _pick(claude_cfg, "betas", "betas"),
        "outputFormat": _pick(claude_cfg, "outputFormat", "output_format"),
        "cliPath": _pick(claude_cfg, "cliPath", "cli_path"),
        "settings": _pick(claude_cfg, "settings", "settings"),
        "addDirs": _pick(claude_cfg, "addDirs", "add_dirs"),
        "extraArgs": _pick(claude_cfg, "extraArgs", "extra_args"),
        "maxBufferSize": _pick(claude_cfg, "maxBufferSize", "max_buffer_size"),
        "stderr": _pick(claude_cfg, "stderr", "stderr"),
        "user": _pick(claude_cfg, "user", "user"),
        "sandbox": _pick(claude_cfg, "sandbox", "sandbox"),
        "thinking": _pick(claude_cfg, "thinking", "thinking"),
        "effort": _pick(claude_cfg, "effort", "effort"),
        "enableFileCheckpointing": _pick(claude_cfg, "enableFileCheckpointing", "enable_file_checkpointing"),
        "agents": _pick(claude_cfg, "agents", "agents"),
        "plugins": _pick(claude_cfg, "plugins", "plugins"),
        # Slice 3: Missing options parity with TS V1
        "includePartialMessages": _pick(claude_cfg, "includePartialMessages", "include_partial_messages"),
        "forkSession": _pick(claude_cfg, "forkSession", "fork_session"),
        "persistSession": _pick(claude_cfg, "persistSession", "persist_session"),
    }
    # B7: Default settingSources to ["project"] (parity with Node sidecar)
    setting_sources = _pick(claude_cfg, "settingSources", "setting_sources")
    options["settingSources"] = setting_sources if setting_sources is not None else ["project"]

    # Also pull allowed/disallowed tools from ABP policy if not set via vendor config
    if not options.get("allowedTools") and policy.get("allowed_tools"):
        options["allowedTools"] = policy["allowed_tools"]
    if not options.get("disallowedTools") and policy.get("disallowed_tools"):
        options["disallowedTools"] = policy["disallowed_tools"]

    # Also map top-level config budget if not set
    if not options.get("maxBudgetUsd") and cfg.get("max_budget_usd"):
        options["maxBudgetUsd"] = cfg["max_budget_usd"]

    options = {k: v for k, v in options.items() if v is not None}
    env_cfg = as_object(cfg.get("env"))
    if env_cfg:
        options["env"] = env_cfg

    return {"prompt": build_prompt(work_order), "options": options}


def build_can_use_tool(work_order: Dict[str, Any], ctx: Dict[str, Any]) -> Any:
    """B3: Synthesize a can_use_tool callback from ABP policy.

    Returns a callable that checks the tool name against allowed_tools and
    disallowed_tools, emitting permission_requested / permission_resolved events.
    Auto-approves based on policy when no interactive callback is available.
    Uses the full policy engine for path/network/approval checks.
    """
    policy = as_object(work_order.get("policy"))
    workspace_root = as_object(work_order.get("workspace")).get("root") or os.getcwd()
    engine = build_policy_engine(policy, workspace_root)

    def can_use_tool(tool_name: str, tool_input: Any = None) -> bool:
        input_value = tool_input if tool_input is not None else {}
        ctx["emit"]({
            "type": "permission_requested",
            "tool_name": str(tool_name),
            "input": input_value,
        })

        decision = engine["pre_tool"](tool_name, input_value)
        granted = decision["allowed"]
        reason = decision.get("reason") or None

        ctx["emit"]({
            "type": "permission_resolved",
            "tool_name": str(tool_name),
            "granted": granted,
            **({"reason": reason} if reason else {}),
        })
        return granted

    return can_use_tool


def _glob_match(name: str, pattern: str) -> bool:
    """Simple glob matching: supports '*' as wildcard."""
    if pattern == "*":
        return True
    if "*" not in pattern:
        return name == pattern
    # Convert simple glob to prefix/suffix match
    if pattern.startswith("*") and pattern.endswith("*"):
        return pattern[1:-1] in name
    if pattern.startswith("*"):
        return name.endswith(pattern[1:])
    if pattern.endswith("*"):
        return name.startswith(pattern[:-1])
    # Fallback: split on '*' and check containment
    parts = pattern.split("*")
    pos = 0
    for part in parts:
        idx = name.find(part, pos)
        if idx < 0:
            return False
        pos = idx + len(part)
    return True


def _to_posix(path_str: str) -> str:
    """Convert backslashes to forward slashes."""
    return str(path_str or "").replace("\\", "/")


def _canonical_within(root: str, path_str: str) -> Optional[str]:
    """Resolve path relative to root; return None if it escapes root."""
    try:
        root_real = os.path.realpath(root)
        candidate = os.path.join(root_real, path_str or ".")
        candidate_real = os.path.realpath(candidate)
        # Check if candidate is within root
        rel = os.path.relpath(candidate_real, root_real)
        rel_posix = _to_posix(rel)
        if rel_posix == ".." or rel_posix.startswith("../") or os.path.isabs(rel_posix):
            return None
        return rel_posix or "."
    except (ValueError, OSError):
        return None


def _collect_path_values(input_dict: Any) -> List[str]:
    """Extract path-like values from tool input dict."""
    if not isinstance(input_dict, dict):
        return []
    values: List[str] = []
    path_keys = ("path", "file_path", "file", "directory", "dir", "url", "uri", "endpoint")
    for key, val in input_dict.items():
        if key.lower() in path_keys or key in path_keys:
            if isinstance(val, str):
                values.append(val)
            elif isinstance(val, list):
                for item in val:
                    if isinstance(item, str):
                        values.append(item)
    return values


def _compile_glob_list(patterns: Any) -> List[str]:
    """Return the list of patterns as-is (uses _glob_match for matching)."""
    if isinstance(patterns, list):
        return [str(p) for p in patterns]
    return []


def _matches_any(patterns: List[str], value: str) -> bool:
    """Check if value matches any of the glob patterns."""
    return any(_glob_match(value, p) for p in patterns)


def _redact_secrets(text: str) -> str:
    """Mask API keys and authorization headers."""
    result = re.sub(
        r'\b(sk|api|token|secret)[_-]?[a-z0-9]{12,}\b',
        '[REDACTED]',
        text,
        flags=re.IGNORECASE,
    )
    result = re.sub(
        r'(authorization:\s*bearer\s+)[a-z0-9._-]+',
        r'\1[REDACTED]',
        result,
        flags=re.IGNORECASE,
    )
    return result


def _write_artifact(kind: str, name: str, content: str, artifact_root: str) -> str:
    """Write content to artifact_root/name and return the path."""
    os.makedirs(artifact_root, exist_ok=True)
    file_path = os.path.join(artifact_root, name)
    with open(file_path, "w", encoding="utf-8") as f:
        f.write(content)
    return file_path


def _trim_tool_output(tool_name: str, output: str, max_bytes: int, artifact_ctx: Dict[str, Any]) -> Any:
    """If output > max_bytes, write as artifact and return summary. Otherwise return unchanged."""
    if not isinstance(output, str):
        return output
    size = len(output.encode("utf-8"))
    if size <= max_bytes:
        return output
    run_id = artifact_ctx.get("run_id", "unknown")
    root = artifact_ctx.get("root", ".")
    artifact_root = os.path.join(root, ".agent-backplane", "artifacts", run_id)
    stamp = int(datetime.now(timezone.utc).timestamp() * 1000)
    base_name = f"{tool_name or 'tool'}-{stamp}.txt"
    artifact_path = _write_artifact("tool_output", base_name, output, artifact_root)
    preview = output[:2048]
    return {
        "output_preview": preview,
        "output_truncated": True,
        "bytes": size,
        "artifact_path": artifact_path,
    }


def build_policy_engine(policy: Dict[str, Any], workspace_root: str) -> Dict[str, Any]:
    """Build a policy engine dict with check functions, mirroring JS host.js."""
    allowed_tools = _compile_glob_list(policy.get("allowed_tools"))
    disallowed_tools = _compile_glob_list(policy.get("disallowed_tools"))
    deny_read = _compile_glob_list(policy.get("deny_read"))
    deny_write = _compile_glob_list(policy.get("deny_write"))
    deny_network = _compile_glob_list(policy.get("deny_network"))
    allow_network = _compile_glob_list(policy.get("allow_network"))
    require_approval_for = _compile_glob_list(policy.get("require_approval_for"))

    def can_use_tool(tool_name: str) -> Dict[str, Any]:
        if _matches_any(disallowed_tools, tool_name):
            return {"allowed": False, "reason": f"tool '{tool_name}' is disallowed"}
        if allowed_tools and not _matches_any(allowed_tools, tool_name):
            return {"allowed": False, "reason": f"tool '{tool_name}' is not in allowed_tools"}
        return {"allowed": True, "reason": ""}

    def can_read_path(rel: str) -> Dict[str, Any]:
        if _matches_any(deny_read, _to_posix(rel)):
            return {"allowed": False, "reason": f"read denied for '{rel}'"}
        return {"allowed": True, "reason": ""}

    def can_write_path(rel: str) -> Dict[str, Any]:
        if _matches_any(deny_write, _to_posix(rel)):
            return {"allowed": False, "reason": f"write denied for '{rel}'"}
        return {"allowed": True, "reason": ""}

    def can_access_network(hostname: str) -> Dict[str, Any]:
        if not hostname:
            return {"allowed": True, "reason": ""}
        if _matches_any(deny_network, hostname):
            return {"allowed": False, "reason": f"network denied for '{hostname}'"}
        if allow_network and not _matches_any(allow_network, hostname):
            return {"allowed": False, "reason": f"network host '{hostname}' is not in allow_network"}
        return {"allowed": True, "reason": ""}

    def requires_approval(tool_name: str) -> bool:
        return _matches_any(require_approval_for, tool_name)

    def pre_tool(tool_name: str, input_dict: Any = None) -> Dict[str, Any]:
        input_dict = input_dict if isinstance(input_dict, dict) else {}
        # a. Check can_use_tool first
        decision = can_use_tool(tool_name)
        if not decision["allowed"]:
            return decision

        # b. Check requires_approval
        if requires_approval(tool_name):
            return {
                "allowed": False,
                "reason": f"tool '{tool_name}' requires approval (approval callbacks are not configured in abp/v0.1)",
            }

        # c. Collect path values and validate within workspace root
        lower = tool_name.lower()
        paths = _collect_path_values(input_dict)
        for raw_path in paths:
            rel = _canonical_within(workspace_root, raw_path)
            if rel is None:
                return {"allowed": False, "reason": f"path escapes workspace root: '{raw_path}'"}

            # d. For read-like tools, check can_read_path
            if "read" in lower or "grep" in lower or "glob" in lower:
                read_decision = can_read_path(rel)
                if not read_decision["allowed"]:
                    return read_decision

            # e. For write-like tools, check can_write_path
            if "write" in lower or "edit" in lower or "patch" in lower:
                write_decision = can_write_path(rel)
                if not write_decision["allowed"]:
                    return write_decision

        # f. For web-like tools, check network access
        if "web" in lower or "fetch" in lower or "http" in lower:
            maybe_url = input_dict.get("url") or input_dict.get("uri") or input_dict.get("endpoint")
            if isinstance(maybe_url, str):
                try:
                    hostname = urlparse(maybe_url).hostname
                    net_decision = can_access_network(hostname or "")
                    if not net_decision["allowed"]:
                        return net_decision
                except Exception:
                    pass

        return {"allowed": True, "reason": ""}

    return {
        "can_use_tool": can_use_tool,
        "can_read_path": can_read_path,
        "can_write_path": can_write_path,
        "can_access_network": can_access_network,
        "requires_approval": requires_approval,
        "pre_tool": pre_tool,
    }


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


def _emit_hook_event(ctx: Dict[str, Any], message: Dict[str, Any], msg_type: str) -> bool:
    """B2: Detect and normalize SDK hook events to ABP event vocabulary.

    Returns True if the message was handled as a hook event.
    """
    # PreToolUse hook -> tool_call with hook ext
    if "pre_tool_use" in msg_type or "pretooluse" in msg_type:
        tool_name = str(message.get("tool_name") or message.get("toolName") or message.get("name") or "unknown_tool")
        tool_use_id = message.get("tool_use_id") or message.get("toolUseId") or message.get("id")
        decision = str(message.get("decision") or message.get("action") or "allow").lower()
        ctx["emit"]({
            "type": "tool_call",
            "tool_name": tool_name,
            "tool_use_id": tool_use_id,
            "parent_tool_use_id": None,
            "input": message.get("input") or message.get("arguments") or message.get("args") or {},
            "ext": {"hook": "pre_tool_use", "decision": decision},
        })
        return True

    # PostToolUse hook -> tool_result with hook ext
    if "post_tool_use" in msg_type or "posttooluse" in msg_type:
        is_failure = "failure" in msg_type or "error" in msg_type
        tool_name = str(message.get("tool_name") or message.get("toolName") or message.get("name") or "unknown_tool")
        tool_use_id = message.get("tool_use_id") or message.get("toolUseId") or message.get("id")
        hook_name = "post_tool_use_failure" if is_failure else "post_tool_use"
        ctx["emit"]({
            "type": "tool_result",
            "tool_name": tool_name,
            "tool_use_id": tool_use_id,
            "output": message.get("output") if "output" in message else message.get("result"),
            "is_error": is_failure or as_bool(message.get("is_error") or message.get("isError")),
            "ext": {"hook": hook_name},
        })
        return True

    # Notification -> warning with notification ext
    if "notification" in msg_type:
        level = str(message.get("level") or message.get("severity") or "info").lower()
        ctx["emit"]({
            "type": "warning",
            "message": safe_string(message.get("message") or message.get("text") or message.get("notification")),
            "ext": {"notification": True, "level": level},
        })
        return True

    # SubagentStart -> subagent_spawned
    if "subagent_start" in msg_type or "subagentstart" in msg_type:
        agent_id = str(message.get("agent_id") or message.get("agentId") or message.get("id") or str(uuid.uuid4()))
        task = str(message.get("task") or message.get("description") or "")
        ctx["emit"]({
            "type": "subagent_spawned",
            "agent_id": agent_id,
            "task": task,
        })
        return True

    # SubagentStop -> subagent_completed
    if "subagent_stop" in msg_type or "subagentstop" in msg_type or "subagent_complete" in msg_type:
        agent_id = str(message.get("agent_id") or message.get("agentId") or message.get("id") or "")
        success = as_bool(message.get("success") or message.get("ok"), True)
        ctx["emit"]({
            "type": "subagent_completed",
            "agent_id": agent_id,
            "success": success,
        })
        return True

    # PermissionRequest -> permission_requested + permission_resolved (auto)
    if "permission_request" in msg_type or "permissionrequest" in msg_type:
        tool_name = str(message.get("tool_name") or message.get("toolName") or message.get("name") or "unknown_tool")
        tool_input = message.get("input") or message.get("arguments") or message.get("args") or {}
        ctx["emit"]({
            "type": "permission_requested",
            "tool_name": tool_name,
            "input": tool_input,
        })
        # Auto-resolve since we cannot interactively prompt
        ctx["emit"]({
            "type": "permission_resolved",
            "tool_name": tool_name,
            "granted": True,
            "reason": "auto-approved (non-interactive sidecar)",
        })
        return True

    # B5: Checkpoint events
    if "checkpoint" in msg_type:
        checkpoint_id = str(message.get("checkpoint_id") or message.get("checkpointId") or message.get("id") or "")
        ctx["emit"]({
            "type": "assistant_message",
            "text": f"Checkpoint created: {checkpoint_id}" if checkpoint_id else "Checkpoint created",
            "ext": {"checkpoint_id": checkpoint_id} if checkpoint_id else {"checkpoint": True},
        })
        return True

    return False


def _emit_partial_message(ctx: Dict[str, Any], message: Dict[str, Any], msg_type: str) -> bool:
    """B4: Map partial/stream messages to assistant_delta with partial ext.

    Respects ctx["include_partial_messages"] — when False, accumulates text
    but does NOT emit the assistant_delta event.
    """
    if "stream_event" in msg_type or "partial" in msg_type:
        text = message.get("text") or message.get("delta") or message.get("content") or ""
        if text:
            ctx["state"]["last_assistant"] += str(text)
            ctx["state"]["saw_delta"] = True
            if ctx.get("include_partial_messages", True):
                ctx["emit"]({
                    "type": "assistant_delta",
                    "text": str(text),
                    "ext": {"partial": True},
                })
        return True
    return False


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

    # B2: Check for hook events first
    if _emit_hook_event(ctx, message, msg_type):
        return

    # B4: Check for partial/stream messages
    if _emit_partial_message(ctx, message, msg_type):
        return

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
    is_resume = False

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

    # Slice 3: Thread includePartialMessages into ctx
    options = as_object(request.get("options"))
    ctx["include_partial_messages"] = options.get("includePartialMessages", True)

    # B3: Build permission callback from policy
    can_use_tool = build_can_use_tool(work_order, ctx)

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
            is_resume = True
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

        # B3: Attach permission callback if client supports it
        if hasattr(client, "can_use_tool"):
            client.can_use_tool = can_use_tool
        elif hasattr(client, "set_permission_callback"):
            client.set_permission_callback(can_use_tool)

        # Check for resume from options
        options_obj = as_object(request.get("options"))
        if options_obj.get("resume") or options_obj.get("sessionId"):
            is_resume = True

        # B6: Emit session lifecycle events
        session_id = options_obj.get("sessionId") or client_session_key or str(uuid.uuid4())
        if is_resume:
            resumed_from = options_obj.get("sessionId") or client_session_key or ""
            ctx["emit"]({
                "type": "session_resumed",
                "session_id": session_id,
                "resumed_from": resumed_from,
            })
        else:
            ctx["emit"]({
                "type": "session_started",
                "session_id": session_id,
            })

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
        # B6: Emit session_started for query mode too
        session_id = str(uuid.uuid4())
        ctx["emit"]({
            "type": "session_started",
            "session_id": session_id,
        })

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
            "sdk_surface": "claude_agent_sdk",
            "client_mode": client_mode,
            "session_id": session_id,
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

    artifacts: List[Dict[str, Any]] = []

    ctx = {
        "emit": emit,
        "artifacts": artifacts,
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
        "artifacts": artifacts,
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
        t = msg.get("t")
        if t == "ping":
            write({"t": "pong", "seq": msg.get("seq")})
            continue
        if t == "cancel":
            continue
        if t != "run":
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
