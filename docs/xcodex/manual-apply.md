# Manual Apply for Patch Approvals

## 1. Purpose

This document specifies the `xcodex` expansion required to support a new patch-approval outcome: `manual apply`.

The intent is to let the user decline automatic patch application while still opening the proposed edit set in an external editor flow that is optimized by a Neovim plugin.

This design only covers the `xcodex` side. The Neovim plugin is treated as an external integration target. The plugin may later automate more of the loop, but the initial design assumes the user can manually inform the agent after applying or adjusting the changes.

## 2. Goals

- Add a `manual apply` option to file-edit approval prompts.
- Preserve access to the exact proposed file changes at the moment the user chooses the option.
- Launch an external editor flow suitable for a Neovim plugin.
- Prevent the normal auto-apply path from continuing.
- Threat `manual apply` as the option `No, and tell xCodex what to do differently`.
- Keep the current approval pipeline stable for existing approve/deny flows.
- Leave a clean extension point for later plugin-to-`xcodex` callbacks.

## 3. Non-Goals

- Automatically informing the agent that the user finished manual application.
- Automatically reconciling the user-edited result back into the current turn.
- Supporting partial acceptance semantics in the first version.
- Replacing the existing `Approved`, `ApprovedForSession`, or `Abort` semantics.
- Defining Neovim plugin internals beyond the request payload it should receive.

## 4. User Experience

### 4.1 Prompt Expansion

When `xcodex` shows the patch approval modal:

- `Yes, proceed`
- `Yes, and don't ask again for these files`
- `Manual apply in editor`
- `No, and tell xCodex what to do differently`

The prompt text remains:

- `Would you like to make the following edits?`

Recommended v1 shortcuts:

- `y` => `Yes, proceed`
- `a` => `Yes, and don't ask again for these files`
- `m` => `Manual apply in editor`
- `n` and `Esc` => `No, and tell xCodex what to do differently`

### 4.2 Manual Apply Flow

1. Agent proposes file edits.
2. `xcodex` renders the diff summary in the existing approval modal.
3. User selects `Manual apply in editor`.
4. `xcodex` serializes the patch request into a stable payload.
5. `xcodex` launches the configured external editor command.
6. The editor opens with enough context for a plugin to present the requested edits cleanly.
7. `xcodex` resolves the pending patch approval as non-approved so automatic patch application does not run.
8. The user may manually apply some or all changes and later tell the agent what happened.

## 5. Current Baseline

Today the patch approval flow is:

1. `ApplyPatchRuntime::start_approval_async()` requests approval from the session.
2. `Session::request_patch_approval()` stores a pending `oneshot::Sender<ReviewDecision>`.
3. `EventMsg::ApplyPatchApprovalRequest` is emitted.
4. TUI converts that event into `ApprovalRequest::ApplyPatch`.
5. `ApprovalOverlay` renders the diff and choices.
6. A selection emits `AppEvent::CodexOp(Op::PatchApproval { ... })`.
7. Core resolves the pending approval.
8. The orchestrator either applies the patch or rejects it based on `ReviewDecision`.

This is a blocking approval flow implemented with async events plus a pending `oneshot`, not a direct synchronous terminal prompt.

## 6. Required xcodex Changes

### 6.1 Add a New Approval Outcome at the TUI Layer

The current patch options are defined in:

- `codex-rs/xcodex/tui2/src/bottom_pane/approval_overlay.rs`

Add a new TUI-only approval action:

- `ManualApply`

This should be added to the `ApprovalDecision` enum rather than overloading `ReviewDecision`.

Rationale:

- `ReviewDecision` is a core approval result with existing semantics.
- `manual apply` is a UI workflow choice, not a core approval grant.
- It must launch editor-side behavior before deciding how to close the pending approval.

### 6.2 Add a New App Event

Add an application-level event carrying only the data needed for manual apply:

- `AppEvent::OpenManualPatchApply(ManualPatchApplyRequest)`

The event must preserve:

- patch approval id
- current working directory
- full `HashMap<PathBuf, FileChange>`
- optional reason

Rationale:

- `Op::PatchApproval` only carries `id` and `ReviewDecision`.
- once reduced to `Op::PatchApproval`, the diff and file-path payload is lost
- manual apply needs the original patch data
- a dedicated request type avoids passing unrelated `ApprovalRequest` variants
- UI-only display flags such as diff layout should not leak into the serialization boundary

### 6.3 Add a Manual Apply Launch Path in the App Layer

The app layer already supports external editor launch for plan files.

Reuse that pattern by adding a new handler in:

- `codex-rs/xcodex/tui2/src/app.rs`

The handler should:

1. prepare a temporary payload file or payload directory
2. leave alternate screen and restore terminal state
3. launch the configured editor command
4. restore TUI after the editor exits
5. emit a visible transcript/history message explaining that auto-apply was skipped

The initial implementation should not require the editor process to return a structured success signal.

### 6.4 Resolve the Pending Approval as Non-Approved

Choosing `manual apply` must prevent auto-apply.

The safest initial behavior is:

- after launching the editor flow, submit `Op::PatchApproval { id, decision: ReviewDecision::Abort }`

Rationale:

- `Abort` already stops the active turn in `handlers::patch_approval()`
- it guarantees the pending approval is resolved
- it avoids a race where the agent continues acting as if the patch was rejected but the user is still editing
- it is the least ambiguous first-version behavior

`Denied` is not preferred for v1 because it allows the turn to continue and risks conflicting follow-up actions while the user is manually editing.

## 7. Proposed Manual Apply Payload

`xcodex` should generate a stable JSON payload for the editor/plugin.

Suggested shape:

```json
{
  "schema_version": 1,
  "kind": "manual_patch_apply_request",
  "approval_id": "call_123",
  "thread_id": "optional-thread-id-if-available",
  "turn_id": "optional-turn-id-if-available",
  "cwd": "/repo",
  "reason": "optional approval reason",
  "changes": [
    {
      "path": "/repo/src/app.rs",
      "kind": "update",
      "move_path": null,
      "diff": "@@ -10,2 +10,3 @@\n-foo\n+bar"
    },
    {
      "path": "/repo/src/new.rs",
      "kind": "add",
      "move_path": null,
      "diff": "full file content here"
    }
  ]
}
```

### 7.1 Payload Requirements

- `schema_version` for future compatibility
- `approval_id` so future plugin callbacks can identify the original request
- absolute `cwd`
- absolute paths in `changes`
- a normalized `kind`
- `move_path` for rename/move support
- raw `diff` or raw full-file content matching current internal semantics
- `thread_id` only when the app can retrieve the active conversation id without adding new core plumbing
- `turn_id` only when the patch approval context already exposes it at the app/TUI boundary

### 7.2 Data Source

This payload can be derived primarily from:

- `ApprovalRequest::ApplyPatch`

Optional enrichment may come from app-level context:

- active conversation id
- active turn id, if already available without widening the core approval protocol

No new core protocol is required for the first version.

## 8. Editor Launch Contract

### 8.1 Initial Contract

`xcodex` launches the editor with one argument:

- path to the generated manual-apply payload file

Example:

```sh
$EDITOR /tmp/xcodex-manual-apply-<id>.json
```

This keeps the launch path simple and editor-agnostic.

### 8.2 Neovim Plugin Expectation

The Neovim plugin can:

- detect the payload file by filename or JSON `kind`
- parse the request
- open target files
- display proposed hunks
- assist manual application inside the IDE

The plugin does not need to report back to `xcodex` for v1.

### 8.3 Optional Future Contract

Reserve support for a callback artifact written by the plugin, for example:

- sibling `result.json`
- a named pipe
- a socket
- an `xcodex` CLI callback command

This is explicitly deferred.

## 9. TUI and History Behavior

When the user selects `manual apply`, `xcodex` should append a transcript/history cell similar to:

- `Opened editor for manual patch application. Automatic apply was skipped.`

This serves two purposes:

- documents why the turn stopped
- gives the user a clear boundary between agent-owned changes and user-owned manual edits

## 10. State Model

### 10.1 New States

Conceptually, patch approval gains a new branch:

- pending
- approved
- approved_for_session
- manual_apply_selected
- denied
- aborted

The first version does not need a new core persisted status enum if `manual_apply_selected` is represented as:

- TUI-local action
- followed by `ReviewDecision::Abort`
- plus a history message

### 10.2 Why No New Core ReviewDecision Yet

Adding `ReviewDecision::ManualApply` would require deeper changes across:

- protocol
- app-server
- telemetry
- orchestrator
- history rendering

That is unnecessary for v1 because manual apply is operationally equivalent to:

- launch external workflow
- stop auto-apply
- stop the turn

## 11. Failure Modes

### 11.1 Editor Launch Fails

If editor launch fails:

- show an error in transcript/history
- keep the approval modal closed
- resolve the pending approval as `Abort`

Reason:

- the user chose not to auto-apply
- silently falling back to auto-apply would violate intent

### 11.2 Payload Write Fails

If the payload file cannot be created:

- show an error
- do not auto-apply
- resolve as `Abort`

### 11.3 Plugin Missing

If the plugin is not installed:

- opening the payload file should still work
- the JSON payload is the fallback UX

This is acceptable for v1.

## 12. Security and Safety

- Manual apply must never implicitly grant write approval to the agent.
- The payload file must contain only the patch request data already visible in the approval UI.
- Temporary payloads should be written under a controlled temp directory using collision-safe file creation.
- Payload files should be created with owner-only read/write permissions where the platform supports it.
- Payload lifecycle should be explicit:
  - keep the payload after editor exit during v1 so plugin failures are debuggable
  - name it predictably enough for discovery, but not by reusing untrusted input directly
  - consider automatic cleanup or retention caps later

## 13. Testing Requirements

### 13.1 TUI Tests

Add or update snapshot coverage for:

- patch approval modal with new `Manual apply in editor` option
- shortcut handling for the new option
- transcript/history output after selecting manual apply

### 13.2 Behavioral Tests

Add tests that verify:

- selecting manual apply does not emit `ReviewDecision::Approved`
- selecting manual apply launches the app event with patch payload intact
- selecting manual apply ultimately resolves the patch request as non-approved
- orchestrator does not continue into patch application

### 13.3 Payload Tests

Add unit coverage for:

- payload serialization for add/delete/update
- move-path handling
- stable ordering of files where relevant

## 14. Rollout Plan

### Phase 1

- Add manual apply option in TUI
- add app event and editor launch path
- emit payload JSON file
- resolve patch approval as `Abort`
- add tests and docs

### Phase 2

- improve editor invocation UX
- add helper command or dedicated payload filename conventions
- refine transcript/history language

### Phase 3

- optional plugin callback into `xcodex`
- optional agent-facing summary of what the user changed
- optional partial-apply semantics

## 15. Recommended Implementation Notes

- keep `manual apply` TUI-local in v1
- do not change `ReviewDecision` in the first pass
- reuse the existing external editor launch path instead of inventing a new process runner
- prefer a single payload file over multiple ad hoc temp files
- make the payload schema explicit and versioned from the start

## 16. Acceptance Criteria

This feature is complete for v1 when:

- patch approvals show a `Manual apply in editor` option
- the option has access to file paths and full patch content
- selecting it opens the editor flow with a stable payload file
- automatic patch application does not occur
- the active turn is safely stopped
- the user can manually tell the agent what happened afterward
- tests cover the new branch
