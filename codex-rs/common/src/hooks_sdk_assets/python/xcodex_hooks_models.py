from __future__ import annotations

"""
xCodex hooks kit: Python runtime models for external hooks.

This file is generated from the Rust hook payload schema (source-of-truth).
It is installed into `$CODEX_HOME/hooks/` by:
  - `xcodex hooks install python`

Re-generate from the repo:
  cd codex-rs
  cargo run -p codex-core --bin hooks_python_models --features hooks-schema --quiet \
    > common/src/hooks_sdk_assets/python/xcodex_hooks_models.py

This module is intentionally dependency-free (no pydantic). It aims to provide:
- ergonomic attribute access (dataclasses)
- forward compatibility (unknown fields are preserved in `.extras` / `.raw`)

Docs:
- Hooks overview: docs/xcodex/hooks.md
- Machine-readable schema: docs/xcodex/hooks.schema.json
- Compatibility policy: docs/xcodex/hooks.md (Compatibility policy)
"""

from dataclasses import dataclass
from typing import Any, Dict, List, Mapping, Optional, Union, Literal


@dataclass
class HookEventBase:
    schema_version: int
    event_id: str
    timestamp: str
    event_type: str

    raw: Dict[str, Any]
    extras: Dict[str, Any]


@dataclass
class UnknownHookEvent(HookEventBase):
    pass


HookEvent = Union[
    UnknownHookEvent,
    "AgentTurnCompleteHookEvent",
    "ApprovalRequestedHookEvent",
    "ModelRequestStartedHookEvent",
    "ModelResponseCompletedHookEvent",
    "SessionEndHookEvent",
    "SessionStartHookEvent",
    "ToolCallFinishedHookEvent",
    "ToolCallStartedHookEvent",
]


def _as_str(value: Any) -> Optional[str]:
    if value is None:
        return None
    if isinstance(value, str):
        return value
    try:
        return str(value)
    except Exception:
        return None


def _as_int(value: Any) -> Optional[int]:
    if value is None:
        return None
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    if isinstance(value, str):
        try:
            return int(value, 10)
        except Exception:
            return None
    return None


def _as_bool(value: Any) -> Optional[bool]:
    if value is None:
        return None
    if isinstance(value, bool):
        return value
    if isinstance(value, int):
        return value != 0
    if isinstance(value, str):
        v = value.strip().lower()
        if v in ("true", "1", "yes", "y", "on"):
            return True
        if v in ("false", "0", "no", "n", "off"):
            return False
    return None


def _as_str_list(value: Any) -> Optional[List[str]]:
    if value is None:
        return None
    if isinstance(value, list):
        out: List[str] = []
        for item in value:
            s = _as_str(item)
            if s is not None:
                out.append(s)
        return out
    return None


def _as_any_list(value: Any) -> Optional[List[Any]]:
    if value is None:
        return None
    if isinstance(value, list):
        return value
    return None

@dataclass
class AgentTurnCompleteHookEvent(HookEventBase):
    event_type: Literal["agent-turn-complete"]
    cwd: Optional[str] = None
    input_messages: Optional[List[str]] = None
    last_assistant_message: Optional[str] = None
    thread_id: Optional[str] = None
    turn_id: Optional[str] = None

@dataclass
class ApprovalRequestedHookEvent(HookEventBase):
    event_type: Literal["approval-requested"]
    approval_policy: Optional[Any] = None
    call_id: Optional[str] = None
    command: Optional[List[str]] = None
    cwd: Optional[str] = None
    grant_root: Optional[str] = None
    kind: Optional[str] = None
    message: Optional[str] = None
    paths: Optional[List[str]] = None
    proposed_execpolicy_amendment: Optional[List[str]] = None
    reason: Optional[str] = None
    request_id: Optional[str] = None
    sandbox_policy: Optional[Any] = None
    server_name: Optional[str] = None
    thread_id: Optional[str] = None
    turn_id: Optional[str] = None

@dataclass
class ModelRequestStartedHookEvent(HookEventBase):
    event_type: Literal["model-request-started"]
    attempt: Optional[int] = None
    cwd: Optional[str] = None
    has_output_schema: Optional[bool] = None
    model: Optional[str] = None
    model_request_id: Optional[str] = None
    parallel_tool_calls: Optional[bool] = None
    prompt_input_item_count: Optional[int] = None
    provider: Optional[str] = None
    thread_id: Optional[str] = None
    tool_count: Optional[int] = None
    turn_id: Optional[str] = None

@dataclass
class ModelResponseCompletedHookEvent(HookEventBase):
    event_type: Literal["model-response-completed"]
    attempt: Optional[int] = None
    cwd: Optional[str] = None
    model_request_id: Optional[str] = None
    needs_follow_up: Optional[bool] = None
    response_id: Optional[str] = None
    thread_id: Optional[str] = None
    token_usage: Optional[Any] = None
    turn_id: Optional[str] = None

@dataclass
class SessionEndHookEvent(HookEventBase):
    event_type: Literal["session-end"]
    cwd: Optional[str] = None
    session_source: Optional[str] = None
    thread_id: Optional[str] = None

@dataclass
class SessionStartHookEvent(HookEventBase):
    event_type: Literal["session-start"]
    cwd: Optional[str] = None
    session_source: Optional[str] = None
    thread_id: Optional[str] = None

@dataclass
class ToolCallFinishedHookEvent(HookEventBase):
    event_type: Literal["tool-call-finished"]
    attempt: Optional[int] = None
    call_id: Optional[str] = None
    cwd: Optional[str] = None
    duration_ms: Optional[int] = None
    model_request_id: Optional[str] = None
    output_bytes: Optional[int] = None
    output_preview: Optional[str] = None
    status: Optional[str] = None
    success: Optional[bool] = None
    thread_id: Optional[str] = None
    tool_name: Optional[str] = None
    turn_id: Optional[str] = None

@dataclass
class ToolCallStartedHookEvent(HookEventBase):
    event_type: Literal["tool-call-started"]
    attempt: Optional[int] = None
    call_id: Optional[str] = None
    cwd: Optional[str] = None
    model_request_id: Optional[str] = None
    thread_id: Optional[str] = None
    tool_name: Optional[str] = None
    turn_id: Optional[str] = None

def parse_hook_event(payload: Mapping[str, Any]) -> HookEvent:
    """
    Parse a raw hook payload dict into a dataclass model.

    This is tolerant by design:
    - unknown event types return UnknownHookEvent
    - unknown fields are preserved under `.extras` and `.raw`
    """
    schema_version = int(payload.get("schema-version") or 0)
    event_id = str(payload.get("event-id") or "")
    timestamp = str(payload.get("timestamp") or "")
    event_type = str(payload.get("type") or "")

    raw = dict(payload)
    base_known = {"schema-version", "event-id", "timestamp", "type"}

    def extras_for(known: set[str]) -> Dict[str, Any]:
        return {k: v for k, v in raw.items() if k not in known}

    if event_type == "":
        return UnknownHookEvent(schema_version, event_id, timestamp, event_type, raw, extras_for(base_known))

    if event_type == "agent-turn-complete":
        return AgentTurnCompleteHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="agent-turn-complete",
            raw=raw,
            extras=extras_for({"cwd", "event-id", "input-messages", "last-assistant-message", "schema-version", "thread-id", "timestamp", "turn-id", "type"}),
            cwd=_as_str(payload.get("cwd")),
            input_messages=_as_str_list(payload.get("input-messages")),
            last_assistant_message=_as_str(payload.get("last-assistant-message")),
            thread_id=_as_str(payload.get("thread-id")),
            turn_id=_as_str(payload.get("turn-id")),
        )

    if event_type == "approval-requested":
        return ApprovalRequestedHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="approval-requested",
            raw=raw,
            extras=extras_for({"approval-policy", "call-id", "command", "cwd", "event-id", "grant-root", "kind", "message", "paths", "proposed-execpolicy-amendment", "reason", "request-id", "sandbox-policy", "schema-version", "server-name", "thread-id", "timestamp", "turn-id", "type"}),
            approval_policy=payload.get("approval-policy"),
            call_id=_as_str(payload.get("call-id")),
            command=_as_str_list(payload.get("command")),
            cwd=_as_str(payload.get("cwd")),
            grant_root=_as_str(payload.get("grant-root")),
            kind=_as_str(payload.get("kind")),
            message=_as_str(payload.get("message")),
            paths=_as_str_list(payload.get("paths")),
            proposed_execpolicy_amendment=_as_str_list(payload.get("proposed-execpolicy-amendment")),
            reason=_as_str(payload.get("reason")),
            request_id=_as_str(payload.get("request-id")),
            sandbox_policy=payload.get("sandbox-policy"),
            server_name=_as_str(payload.get("server-name")),
            thread_id=_as_str(payload.get("thread-id")),
            turn_id=_as_str(payload.get("turn-id")),
        )

    if event_type == "model-request-started":
        return ModelRequestStartedHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="model-request-started",
            raw=raw,
            extras=extras_for({"attempt", "cwd", "event-id", "has-output-schema", "model", "model-request-id", "parallel-tool-calls", "prompt-input-item-count", "provider", "schema-version", "thread-id", "timestamp", "tool-count", "turn-id", "type"}),
            attempt=_as_int(payload.get("attempt")),
            cwd=_as_str(payload.get("cwd")),
            has_output_schema=_as_bool(payload.get("has-output-schema")),
            model=_as_str(payload.get("model")),
            model_request_id=_as_str(payload.get("model-request-id")),
            parallel_tool_calls=_as_bool(payload.get("parallel-tool-calls")),
            prompt_input_item_count=_as_int(payload.get("prompt-input-item-count")),
            provider=_as_str(payload.get("provider")),
            thread_id=_as_str(payload.get("thread-id")),
            tool_count=_as_int(payload.get("tool-count")),
            turn_id=_as_str(payload.get("turn-id")),
        )

    if event_type == "model-response-completed":
        return ModelResponseCompletedHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="model-response-completed",
            raw=raw,
            extras=extras_for({"attempt", "cwd", "event-id", "model-request-id", "needs-follow-up", "response-id", "schema-version", "thread-id", "timestamp", "token-usage", "turn-id", "type"}),
            attempt=_as_int(payload.get("attempt")),
            cwd=_as_str(payload.get("cwd")),
            model_request_id=_as_str(payload.get("model-request-id")),
            needs_follow_up=_as_bool(payload.get("needs-follow-up")),
            response_id=_as_str(payload.get("response-id")),
            thread_id=_as_str(payload.get("thread-id")),
            token_usage=payload.get("token-usage"),
            turn_id=_as_str(payload.get("turn-id")),
        )

    if event_type == "session-end":
        return SessionEndHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="session-end",
            raw=raw,
            extras=extras_for({"cwd", "event-id", "schema-version", "session-source", "thread-id", "timestamp", "type"}),
            cwd=_as_str(payload.get("cwd")),
            session_source=_as_str(payload.get("session-source")),
            thread_id=_as_str(payload.get("thread-id")),
        )

    if event_type == "session-start":
        return SessionStartHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="session-start",
            raw=raw,
            extras=extras_for({"cwd", "event-id", "schema-version", "session-source", "thread-id", "timestamp", "type"}),
            cwd=_as_str(payload.get("cwd")),
            session_source=_as_str(payload.get("session-source")),
            thread_id=_as_str(payload.get("thread-id")),
        )

    if event_type == "tool-call-finished":
        return ToolCallFinishedHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="tool-call-finished",
            raw=raw,
            extras=extras_for({"attempt", "call-id", "cwd", "duration-ms", "event-id", "model-request-id", "output-bytes", "output-preview", "schema-version", "status", "success", "thread-id", "timestamp", "tool-name", "turn-id", "type"}),
            attempt=_as_int(payload.get("attempt")),
            call_id=_as_str(payload.get("call-id")),
            cwd=_as_str(payload.get("cwd")),
            duration_ms=_as_int(payload.get("duration-ms")),
            model_request_id=_as_str(payload.get("model-request-id")),
            output_bytes=_as_int(payload.get("output-bytes")),
            output_preview=_as_str(payload.get("output-preview")),
            status=_as_str(payload.get("status")),
            success=_as_bool(payload.get("success")),
            thread_id=_as_str(payload.get("thread-id")),
            tool_name=_as_str(payload.get("tool-name")),
            turn_id=_as_str(payload.get("turn-id")),
        )

    if event_type == "tool-call-started":
        return ToolCallStartedHookEvent(
            schema_version=schema_version,
            event_id=event_id,
            timestamp=timestamp,
            event_type="tool-call-started",
            raw=raw,
            extras=extras_for({"attempt", "call-id", "cwd", "event-id", "model-request-id", "schema-version", "thread-id", "timestamp", "tool-name", "turn-id", "type"}),
            attempt=_as_int(payload.get("attempt")),
            call_id=_as_str(payload.get("call-id")),
            cwd=_as_str(payload.get("cwd")),
            model_request_id=_as_str(payload.get("model-request-id")),
            thread_id=_as_str(payload.get("thread-id")),
            tool_name=_as_str(payload.get("tool-name")),
            turn_id=_as_str(payload.get("turn-id")),
        )

    return UnknownHookEvent(schema_version, event_id, timestamp, event_type, raw, extras_for(base_known | {"type"}))
