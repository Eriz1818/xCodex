from __future__ import annotations

"""
xCodex hooks kit: Python typed helpers for external hooks.

This file is generated from the Rust hook payload schema (source-of-truth).
It is installed into `$CODEX_HOME/hooks/` by:
- `xcodex hooks install python`

Re-generate from the repo:
cd codex-rs
cargo run -p codex-core --bin hooks_python_types --features hooks-schema --quiet \
> common/src/hooks_sdk_assets/python/xcodex_hooks_types.py

Docs:
- Hooks overview: docs/xcodex/hooks.md
- Machine-readable schema: docs/xcodex/hooks.schema.json
- Hook SDK installers: docs/xcodex/hooks-sdks.md
"""


from typing import Any, Dict, List, Literal, Optional, TypedDict, Union
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import NotRequired, Required
else:
    class _Req:
        def __class_getitem__(cls, item):  # noqa: D401
            return item

    Required = NotRequired = _Req  # type: ignore

ApprovalKind = Union[Literal["apply-patch"], Literal["elicitation"], Literal["exec"]]

ToolCallStatus = Union[Literal["aborted"], Literal["completed"]]

HookPayloadBase = TypedDict(
"HookPayloadBase",
{
"schema-version": Required[int],
"event-id": Required[str],
"timestamp": Required[str],
},
total=False,
)

AgentTurnCompletePayload = TypedDict(
    "AgentTurnCompletePayload",
    {
        "type": Required[Literal["agent-turn-complete"]],
        "cwd": Required[str],
        "input-messages": Required[List[str]],
        "last-assistant-message": NotRequired[Union[None, str]],
        "thread-id": Required[str],
        "turn-id": Required[str],
    },
    total=False,
)

ApprovalRequestedPayload = TypedDict(
    "ApprovalRequestedPayload",
    {
        "type": Required[Literal["approval-requested"]],
        "approval-policy": NotRequired[Union[None, Union[Literal["never"], Literal["on-failure"], Literal["on-request"], Literal["untrusted"]]]],
        "call-id": NotRequired[Union[None, str]],
        "command": NotRequired[Union[List[str], None]],
        "cwd": NotRequired[Union[None, str]],
        "grant-root": NotRequired[Union[None, str]],
        "kind": Required[ApprovalKind],
        "message": NotRequired[Union[None, str]],
        "paths": NotRequired[Union[List[str], None]],
        "proposed-execpolicy-amendment": NotRequired[Union[List[str], None]],
        "reason": NotRequired[Union[None, str]],
        "request-id": NotRequired[Union[None, str]],
        "sandbox-policy": NotRequired[Union[Any, None]],
        "server-name": NotRequired[Union[None, str]],
        "thread-id": Required[str],
        "turn-id": NotRequired[Union[None, str]],
    },
    total=False,
)

ModelRequestStartedPayload = TypedDict(
    "ModelRequestStartedPayload",
    {
        "type": Required[Literal["model-request-started"]],
        "attempt": Required[int],
        "cwd": Required[str],
        "has-output-schema": Required[bool],
        "model": Required[str],
        "model-request-id": Required[str],
        "parallel-tool-calls": Required[bool],
        "prompt-input-item-count": Required[int],
        "provider": Required[str],
        "thread-id": Required[str],
        "tool-count": Required[int],
        "turn-id": Required[str],
    },
    total=False,
)

ModelResponseCompletedPayload = TypedDict(
    "ModelResponseCompletedPayload",
    {
        "type": Required[Literal["model-response-completed"]],
        "attempt": Required[int],
        "cwd": Required[str],
        "model-request-id": Required[str],
        "needs-follow-up": Required[bool],
        "response-id": Required[str],
        "thread-id": Required[str],
        "token-usage": NotRequired[Union[Any, None]],
        "turn-id": Required[str],
    },
    total=False,
)

SessionEndPayload = TypedDict(
    "SessionEndPayload",
    {
        "type": Required[Literal["session-end"]],
        "cwd": Required[str],
        "session-source": Required[str],
        "thread-id": Required[str],
    },
    total=False,
)

SessionStartPayload = TypedDict(
    "SessionStartPayload",
    {
        "type": Required[Literal["session-start"]],
        "cwd": Required[str],
        "session-source": Required[str],
        "thread-id": Required[str],
    },
    total=False,
)

ToolCallFinishedPayload = TypedDict(
    "ToolCallFinishedPayload",
    {
        "type": Required[Literal["tool-call-finished"]],
        "attempt": Required[int],
        "call-id": Required[str],
        "cwd": Required[str],
        "duration-ms": Required[int],
        "model-request-id": Required[str],
        "output-bytes": Required[int],
        "output-preview": NotRequired[Union[None, str]],
        "status": Required[ToolCallStatus],
        "success": Required[bool],
        "thread-id": Required[str],
        "tool-name": Required[str],
        "turn-id": Required[str],
    },
    total=False,
)

ToolCallStartedPayload = TypedDict(
    "ToolCallStartedPayload",
    {
        "type": Required[Literal["tool-call-started"]],
        "attempt": Required[int],
        "call-id": Required[str],
        "cwd": Required[str],
        "model-request-id": Required[str],
        "thread-id": Required[str],
        "tool-name": Required[str],
        "turn-id": Required[str],
    },
    total=False,
)

HookPayload = Union[
    HookPayloadBase,
    AgentTurnCompletePayload,
    ApprovalRequestedPayload,
    ModelRequestStartedPayload,
    ModelResponseCompletedPayload,
    SessionEndPayload,
    SessionStartPayload,
    ToolCallFinishedPayload,
    ToolCallStartedPayload,
    Dict[str, Any],
]

