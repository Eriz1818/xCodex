from __future__ import annotations

"""
xCodex hooks kit: Python runtime helpers for external hooks.

This file is installed into `$CODEX_HOME/hooks/` by:

  xcodex hooks install python

It provides small "runtime parsing" helpers that keep your hook scripts:
- tolerant / forward-compatible (unknown fields/types do not break)
- readable (you can branch on event type with type narrowing)

Typing:
- TypedDict event types live in `xcodex_hooks_types.py`.
"""

from typing import Any, Mapping, Optional

try:
    from typing import TypeGuard
except ImportError:  # pragma: no cover
    class TypeGuard:  # type: ignore
        def __class_getitem__(cls, item):
            return bool

from xcodex_hooks_types import (
    AgentTurnCompletePayload,
    ApprovalRequestedPayload,
    HookPayload,
    ModelRequestStartedPayload,
    ModelResponseCompletedPayload,
    SessionEndPayload,
    SessionStartPayload,
    ToolCallFinishedPayload,
    ToolCallStartedPayload,
)


def _has_keys(payload: Mapping[str, Any], keys: tuple[str, ...]) -> bool:
    return all(k in payload for k in keys)


def is_agent_turn_complete(payload: Mapping[str, Any]) -> TypeGuard[AgentTurnCompletePayload]:
    return payload.get("type") == "agent-turn-complete" and _has_keys(
        payload, ("thread-id", "turn-id", "cwd", "input-messages")
    )


def is_approval_requested(payload: Mapping[str, Any]) -> TypeGuard[ApprovalRequestedPayload]:
    return payload.get("type") == "approval-requested" and _has_keys(payload, ("thread-id", "kind"))


def is_session_start(payload: Mapping[str, Any]) -> TypeGuard[SessionStartPayload]:
    return payload.get("type") == "session-start" and _has_keys(
        payload, ("thread-id", "cwd", "session-source")
    )


def is_session_end(payload: Mapping[str, Any]) -> TypeGuard[SessionEndPayload]:
    return payload.get("type") == "session-end" and _has_keys(
        payload, ("thread-id", "cwd", "session-source")
    )


def is_model_request_started(payload: Mapping[str, Any]) -> TypeGuard[ModelRequestStartedPayload]:
    return payload.get("type") == "model-request-started" and _has_keys(
        payload,
        (
            "thread-id",
            "turn-id",
            "cwd",
            "model-request-id",
            "attempt",
            "model",
            "provider",
            "prompt-input-item-count",
            "tool-count",
            "parallel-tool-calls",
            "has-output-schema",
        ),
    )


def is_model_response_completed(payload: Mapping[str, Any]) -> TypeGuard[ModelResponseCompletedPayload]:
    return payload.get("type") == "model-response-completed" and _has_keys(
        payload,
        (
            "thread-id",
            "turn-id",
            "cwd",
            "model-request-id",
            "attempt",
            "response-id",
            "needs-follow-up",
        ),
    )


def is_tool_call_started(payload: Mapping[str, Any]) -> TypeGuard[ToolCallStartedPayload]:
    return payload.get("type") == "tool-call-started" and _has_keys(
        payload,
        (
            "thread-id",
            "turn-id",
            "cwd",
            "model-request-id",
            "attempt",
            "tool-name",
            "call-id",
        ),
    )


def is_tool_call_finished(payload: Mapping[str, Any]) -> TypeGuard[ToolCallFinishedPayload]:
    return payload.get("type") == "tool-call-finished" and _has_keys(
        payload,
        (
            "thread-id",
            "turn-id",
            "cwd",
            "model-request-id",
            "attempt",
            "tool-name",
            "call-id",
            "status",
            "duration-ms",
            "success",
            "output-bytes",
        ),
    )


def as_hook_payload(payload: Mapping[str, Any]) -> Optional[HookPayload]:
    # Common required envelope fields.
    if not _has_keys(payload, ("schema-version", "event-id", "timestamp", "type")):
        return None
    return payload  # type: ignore[return-value]
