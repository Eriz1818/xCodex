from __future__ import annotations

"""
xCodex hooks kit: Python helper.

This file is meant to be vendored into `$CODEX_HOME/hooks/` via:

    xcodex hooks install python

It provides a single convenience function, `read_payload()`, that hides the
most error-prone part of writing external hooks: handling stdin vs the
`payload-path` envelope that Codex uses for large payloads.

Optional typed helpers:
- `xcodex_hooks_types.py` contains generated TypedDict event types.
- `xcodex_hooks_models.py` contains generated dataclass models + a tolerant parser.
- `xcodex_hooks_runtime.py` contains small runtime helpers (TypeGuard predicates).

Docs:
- Hooks overview: docs/xcodex/hooks.md
- Hook SDK installers: docs/xcodex/hooks-sdks.md
- Authoritative config reference: docs/config.md#hooks
"""

import json
import pathlib
import sys
from typing import TYPE_CHECKING, Any, Dict, Optional

if TYPE_CHECKING:
    from xcodex_hooks_models import HookEvent
    from xcodex_hooks_types import HookPayload


def read_payload(raw: Optional[str] = None) -> "HookPayload":
    """
    Read a hook payload as a dict.

    Input:
    - If `raw` is provided, it is treated as the full stdin string.
    - Otherwise, the function reads stdin (`sys.stdin.read()`).

    Output:
    - Returns the full payload dict for the event (e.g. `type=tool-call-finished`).

    Behavior:
    - For small payloads, stdin is the full JSON payload.
    - For large payloads, stdin is a small JSON envelope containing `payload-path`,
      which points to the full JSON payload written under CODEX_HOME.

    Typical usage in a hook script:

        payload = read_payload()
        if payload.get("type") != "tool-call-finished":
            return 0
        # ... your logic ...
    """
    if raw is None:
        raw = sys.stdin.read()

    raw = raw or "{}"
    payload: Dict[str, Any] = json.loads(raw)

    payload_path = payload.get("payload-path")
    if payload_path:
        payload = json.loads(pathlib.Path(payload_path).read_text(encoding="utf-8"))

    return payload


def read_payload_model(raw: Optional[str] = None) -> "HookEvent":
    """
    Read a hook payload and parse it into a dataclass model.

    This is optional sugar over:
    - `payload = read_payload()`
    - `event = xcodex_hooks_models.parse_hook_event(payload)`

    The parser is tolerant by design: unknown event types return `UnknownHookEvent`,
    and unknown fields are preserved in `.extras` / `.raw`.
    """
    payload = read_payload(raw)
    import xcodex_hooks_models

    return xcodex_hooks_models.parse_hook_event(payload)
