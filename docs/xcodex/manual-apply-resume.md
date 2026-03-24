# Resume After Manual Apply

## 1. Purpose

This document specifies the follow-on `xcodex` design needed to support real manual file edits in Neovim while preserving the normal deep-edit agent workflow.

The target outcome is:

1. agent proposes a patch
2. user chooses `Manual apply in editor`
3. Neovim opens the real target file and assists the manual edit
4. user completes the suggested edit in Neovim
5. `xcodex` resumes the same turn
6. the agent continues with review, more edits, commands, and follow-up fixes

This document supersedes the turn-aborting behavior described in [manual-apply.md](/home/neepo/Software/OpenCLaw/xCodex/docs/xcodex/manual-apply.md) for the resumed workflow.

## 2. Goals

- Support real manual edits performed outside `xcodex`.
- Keep the current turn alive across manual patch application.
- Avoid auto-applying the same patch a second time.
- Minimize churn in the main repo by changing only the patch approval path.
- Minimize churn in the Neovim side by reusing its existing completion signal.
- Preserve the current behavior of exec approvals, MCP elicitation, and normal patch approvals.

## 3. Non-Goals

- Building a generic "external tool satisfied" framework for all tools.
- Supporting partial acceptance semantics in the first resumed version.
- Verifying exact file contents before resuming in the first version.
- Replacing the existing `Approved` semantics for normal patch approvals.
- Redesigning the current Neovim overlay UX.

## 4. Problem Statement

The current `manual apply` flow is operationally equivalent to:

1. open editor
2. abort patch approval
3. interrupt the turn

That works for isolated edits, but it breaks deep edit chains because the agent cannot continue from the same reasoning context after the user makes the manual change.

The resumed workflow needs a new outcome with this meaning:

- the patch request was satisfied externally
- the turn may continue
- the `apply_patch` runtime must not execute the proposed patch

This is not the same as `ReviewDecision::Approved`.

## 5. Current Neovim Behavior

The current Neovim implementation already provides a concrete notion of "manual apply completed".

### 5.1 Payload Handling

On `BufReadPost`, if the opened file path matches:

- `/tmp/xcodex-manual-apply-*.json`

the Lua handler:

- reads the JSON payload
- requires `kind = "manual_patch_apply_request"`
- uses `changes[1].path` and `changes[1].diff`

### 5.2 File and Overlay Behavior

The handler:

- opens the real target file from `changes[1].path`
- parses the first diff hunk
- inserts blank lines at the target location
- renders the proposed added lines as extmark-based ghost text overlays

As the user types over the blank lines:

- correct characters are highlighted as match
- incorrect characters are highlighted as mismatch
- mistyped spaces are rendered as `_`
- remaining expected text stays ghosted

### 5.3 Completion Behavior

When the typed content fully matches the suggested lines:

- the overlay clears automatically

The current implementation therefore already has an implicit completion signal:

- `all suggested lines now match the typed content`

### 5.4 Manual Overlay Controls

The current Neovim behavior also includes:

- `<leader>m` clears the overlay manually
- `u` restores the last cleared overlay if it is hidden, otherwise performs normal undo

These are overlay controls only. Today they do not communicate patch state back to `xcodex`.

## 6. Design Principle

The resumed workflow should adapt to the existing Neovim completion model rather than replace it.

That means:

- `xcodex` should not invent a second definition of completion
- Neovim should reuse its existing "overlay fully matched" event as the completion trigger
- only a minimal callback artifact should be added so `xcodex` can observe that completion

## 7. Core Design

### 7.1 New Semantic

Introduce a patch-only outcome with this meaning:

- `ExternallyApplied`

Semantics:

- the user accepted responsibility for the patch outside `xcodex`
- the requested patch should be considered satisfied
- the turn should continue
- `apply_patch` should not run

This outcome should be scoped narrowly to patch approvals.

### 7.2 Why Not Reuse `Approved`

Reusing `ReviewDecision::Approved` is incorrect because it currently means:

- the orchestrator may continue into the normal `apply_patch` execution path

If the user has already edited the file manually, plain `Approved` would attempt to apply the same patch again and will likely fail or produce an inconsistent result.

### 7.3 Why Not Keep `ManualApply` TUI-Only

Keeping the new behavior entirely in the TUI is insufficient because the orchestrator currently only understands:

- approved => execute tool
- denied/abort => reject tool

The resumed flow needs a third behavior:

- satisfied externally => skip tool execution but continue turn

That semantic must exist below the TUI boundary.

## 8. Proposed Architecture

### 8.1 TUI Layer

Keep `ManualApply` as a TUI-facing user action in the approval overlay.

The TUI should:

1. preserve the full patch payload
2. launch the external editor
3. wait for an editor completion artifact
4. convert that completion into the new patch-approval outcome

The TUI must no longer immediately submit `ReviewDecision::Abort` after editor exit.

### 8.2 App Layer

The app layer should continue to own:

- payload creation
- temporary file management
- external editor launch
- terminal leave/restore behavior
- completion artifact detection

The app layer should add:

- a pending manual-apply state keyed by approval id
- a completion path that submits the new patch-approval outcome
- cancellation/failure handling that still resolves the pending approval

### 8.3 Core Approval Layer

The core patch approval flow should be extended so the pending approval can resolve as:

- approved
- approved for session
- denied
- abort
- externally applied

Recommended approach:

- use a patch-specific internal result first, to avoid widening non-patch approval paths unnecessarily

### 8.4 Orchestrator / Runtime Layer

The patch runtime and orchestrator should treat the new outcome as:

- approval succeeded
- skip `apply_patch` execution
- return success to the agent

This is the key behavior change.

## 9. Minimal Main-Repo Change Set

The smallest implementation that supports real manual edits should touch only the patch approval path.

### 9.1 TUI / xcodex

- `codex-rs/xcodex/tui2/src/bottom_pane/approval_overlay.rs`
- `codex-rs/xcodex/tui2/src/app.rs`
- `codex-rs/xcodex/tui2/src/app_event.rs`

Expected changes:

- keep `ManualApply`
- stop converting manual apply directly to abort
- add completion handling after editor exit
- add pending-state tracking

### 9.2 Core

- `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- `codex-rs/core/src/tools/orchestrator.rs`
- `codex-rs/core/src/codex.rs`

Expected changes:

- allow patch approvals to resolve as externally applied
- teach the orchestrator to continue without execution
- ensure pending approvals are always resolved exactly once

## 10. Suggested Control Flow

### 10.1 Launch

1. `ApprovalOverlay` emits `AppEvent::OpenManualPatchApply`.
2. App writes payload JSON and launches editor.
3. App records `approval_id` as pending manual apply.
4. Neovim opens the real target file and shows the overlay guidance.

### 10.2 Completion

When the user types the full suggested content and the overlay reaches the fully matched state:

1. Neovim writes a completion artifact
2. user exits the editor
3. `xcodex` reads that completion artifact
4. app submits patch approval with `ExternallyApplied`
5. core resolves the pending patch approval
6. orchestrator skips `apply_patch` execution
7. tool call is treated as successful
8. turn continues

### 10.3 Cancellation / Failure

When manual apply is not completed successfully:

1. Neovim may write `cancelled` or `failed`
2. or no completion artifact may be produced
3. `xcodex` resolves the approval as non-approved
4. the turn stops safely

Recommended v1 mapping:

- `cancelled` => `Abort`
- `failed` => `Abort`
- missing completion artifact after editor exit => `Abort`

## 11. Editor Callback Contract

The resumed flow needs a minimal editor-to-`xcodex` completion signal.

### 11.1 Required Result States

The first version only needs:

- `completed`
- `cancelled`
- `failed`

### 11.2 Recommended Mechanism

Use a sibling result artifact written next to the request payload, for example:

- request: `/tmp/xcodex-manual-apply-<id>.json`
- result: `/tmp/xcodex-manual-apply-<id>.result.json`

Suggested result shape:

```json
{
  "schema_version": 1,
  "kind": "manual_patch_apply_result",
  "approval_id": "call_123",
  "status": "completed"
}
```

Why this is preferred:

- easy for Neovim to write
- easy for `xcodex` to detect
- keeps the initial implementation editor-agnostic
- avoids introducing a new IPC transport in v1

## 12. Minimum Neovim Changes

The current Neovim implementation already knows when the manual apply is complete. The minimum change is to persist that completion in a form `xcodex` can observe.

### 12.1 Buffer State

When opening the payload, Neovim should retain at least:

- `payload_path`
- `approval_id`

alongside the existing overlay state.

### 12.2 Completion Write

When the overlay reaches the fully matched state, Neovim should:

1. compute the sibling result path
2. write the result JSON with `status = "completed"`
3. then clear the overlay

### 12.3 Optional Cancellation Write

If desired in v1, Neovim may also write:

- `status = "cancelled"` when the user explicitly clears the overlay with `<leader>m`

This is optional. `xcodex` can safely treat a missing result artifact as an aborted manual apply.

### 12.4 Payload Pattern Guard

The current pattern:

- `^/tmp/xcodex%-manual%-apply%-.+%.json$`

also matches:

- `...result.json`

The Neovim side should tighten this so result files are not mistaken for request payloads.

## 13. Tool Output Semantics

When manual apply completes successfully, the agent should observe behavior equivalent to a successful patch tool call.

At minimum:

- the tool must not be reported as rejected
- the turn must continue

Recommended v1 behavior:

- return a success text like `Patch applied manually in external editor.`

Optional later enhancement:

- emit synthetic patch-apply begin/end events so transcript summaries remain aligned with normal patch application

## 14. State Model

Conceptually, patch approval gains a new branch:

- pending
- approved
- approved_for_session
- externally_applied
- denied
- aborted

This document recommends modeling `externally_applied` in the narrowest layer possible while still allowing the orchestrator to skip execution.

## 15. Failure Modes

### 15.1 Editor Launch Fails

If editor launch fails:

- show an error
- resolve the pending approval as `Abort`

### 15.2 Result File Never Appears

If Neovim never reports completion:

- do not continue the turn implicitly
- resolve the pending approval as `Abort` after editor exit

The simplest first version is:

- wait for editor exit
- inspect the result file once

### 15.3 Result File Is Malformed

If the result artifact is unreadable or invalid:

- show an error
- resolve as `Abort`

### 15.4 Double Resolution

Manual apply must not be able to resolve the same pending approval more than once.

The app and core should both defensively guard against duplicate completion.

## 16. Testing Requirements

### 16.1 TUI Tests

Add or update tests for:

- selecting `Manual apply in editor`
- launch path creates the payload
- resumed flow does not immediately abort
- completion path submits the external-applied outcome

### 16.2 Core Behavioral Tests

Add tests that verify:

- externally applied does not execute `apply_patch`
- externally applied is treated as success, not rejection
- the turn continues after manual completion
- cancelled/failed still stop the turn

### 16.3 Neovim Behavioral Checks

Verify on the editor side that:

- payload files still open the real target file
- overlay behavior is unchanged until completion
- fully matched overlay writes the completion artifact
- result files are not intercepted as payload files

## 17. Acceptance Criteria

This feature is complete when:

- real manual file edits can be performed in Neovim
- the existing overlay UX still guides the edit
- completion of the overlay-backed manual edit resumes the same turn
- `apply_patch` is not re-run after manual completion
- the agent can continue with additional edits, commands, and review steps
- missing or invalid completion artifacts still fail safely
- the change is confined to the patch approval path plus minimal Neovim callback wiring


## Copy of the current neovim.lua file used:
local M = {}

local PAYLOAD_PATTERN = '^/tmp/xcodex%-manual%-apply%-.+%.json$'
local NS = vim.api.nvim_create_namespace 'CodexManualApply'
local ACTIVE = {}
local LAST_CLEARED = {}
local render_overlay

local function ensure_highlights()
  vim.api.nvim_set_hl(0, 'CodexManualApplyPending', { link = 'Comment', default = true })
  vim.api.nvim_set_hl(0, 'CodexManualApplyMatch', { link = 'DiffAdd', default = true })
  vim.api.nvim_set_hl(0, 'CodexManualApplyMismatch', { link = 'DiagnosticError', default = true })
end

local function notify(msg, level)
  vim.notify(msg, level or vim.log.levels.INFO, { title = 'Codex Manual Apply' })
end

local function read_payload(payload_path)
  local ok, lines = pcall(vim.fn.readfile, payload_path)
  if not ok then
    return nil, 'Failed to read payload file: ' .. payload_path
  end

  local content = table.concat(lines, '\n')
  local decode_ok, data = pcall(vim.fn.json_decode, content)
  if not decode_ok or type(data) ~= 'table' then
    return nil, 'Invalid JSON payload: ' .. payload_path
  end

  return data
end

local function split_lines(text)
  if text == '' then
    return {}
  end

  return vim.split(text, '\n', { plain = true })
end

local function parse_diff(diff)
  local start_line = tonumber(diff:match('@@ %-%d+,?%d* %+(%d+),?%d* @@')) or 1
  local added_lines = {}

  for _, line in ipairs(split_lines(diff)) do
    if vim.startswith(line, '+++') then
    elseif vim.startswith(line, '+') then
      table.insert(added_lines, line:sub(2))
    end
  end

  if #added_lines > 0 then
    return start_line, added_lines
  end

  return start_line, split_lines(diff)
end

local function gen_diff_chunks(expected, typed)
  local chunks = {}
  local typed_len = #typed
  local matched = true
  local mismatch_ranges = {}

  if typed_len > 0 then
    local typed_chunks = {}

    for i = 1, math.min(#expected, typed_len) do
      local expected_char = expected:sub(i, i)
      local typed_char = typed:sub(i, i)
      local hl = expected_char == typed_char and 'CodexManualApplyMatch' or 'CodexManualApplyMismatch'
      local display_char = expected_char == typed_char and expected_char or typed_char

      if expected_char ~= typed_char then
        matched = false
        table.insert(mismatch_ranges, { i - 1, i })
        if display_char == ' ' then
          display_char = '_'
        end
      end

      local last = typed_chunks[#typed_chunks]
      if last and last[2] == hl then
        last[1] = last[1] .. display_char
      else
        table.insert(typed_chunks, { display_char, hl })
      end
    end

    chunks = typed_chunks
  end

  if typed_len < #expected then
    table.insert(chunks, { expected:sub(typed_len + 1), 'CodexManualApplyPending' })
    matched = false
  elseif typed_len > #expected then
    matched = false
    table.insert(mismatch_ranges, { #expected, typed_len })
  end

  return chunks, mismatch_ranges, matched and typed == expected
end

local function clear_overlay(buf)
  local state = ACTIVE[buf]
  if vim.api.nvim_buf_is_valid(buf) then
    vim.api.nvim_buf_clear_namespace(buf, NS, 0, -1)
  end
  if state then
    LAST_CLEARED[buf] = vim.deepcopy(state)
  end
  ACTIVE[buf] = nil
end

function M.clear_current()
  clear_overlay(vim.api.nvim_get_current_buf())
end

function M.restore_current()
  local buf = vim.api.nvim_get_current_buf()
  local state = LAST_CLEARED[buf]

  if not state or not vim.api.nvim_buf_is_valid(buf) then
    return false
  end

  ACTIVE[buf] = vim.deepcopy(state)
  ensure_highlights()
  render_overlay(buf)
  return true
end

render_overlay = function(buf)
  local state = ACTIVE[buf]
  if not state or not vim.api.nvim_buf_is_valid(buf) then
    return
  end

  local line_count = vim.api.nvim_buf_line_count(buf)
  local all_matched = true

  vim.api.nvim_buf_clear_namespace(buf, NS, 0, -1)

  for i, expected in ipairs(state.lines) do
    local row = state.start_row + i - 1
    if row >= line_count then
      all_matched = false
      break
    end

    local typed = vim.api.nvim_buf_get_lines(buf, row, row + 1, false)[1] or ''
    local chunks, mismatch_ranges, matched = gen_diff_chunks(expected, typed)
    all_matched = all_matched and matched

    if #chunks > 0 then
      vim.api.nvim_buf_set_extmark(buf, NS, row, 0, {
        virt_text = chunks,
        virt_text_pos = 'overlay',
        hl_mode = 'combine',
        priority = 200,
      })
    end

    for _, range in ipairs(mismatch_ranges) do
      vim.api.nvim_buf_set_extmark(buf, NS, row, range[1], {
        end_row = row,
        end_col = range[2],
        hl_group = 'CodexManualApplyMismatch',
        priority = 201,
      })
    end
  end

  if all_matched then
    clear_overlay(buf)
  end
end

local function attach_overlay(buf, start_row, lines)
  clear_overlay(buf)

  ACTIVE[buf] = {
    start_row = start_row,
    lines = lines,
  }

  ensure_highlights()
  render_overlay(buf)

  if vim.b[buf].codex_manual_apply_attached then
    return
  end

  vim.b[buf].codex_manual_apply_attached = true
  vim.keymap.set('n', '<leader>m', function()
    M.clear_current()
  end, {
    buffer = buf,
    silent = true,
    desc = 'Clear Codex manual apply overlay',
  })
  vim.keymap.set('n', 'u', function()
    if not ACTIVE[buf] and M.restore_current() then
      return
    end

    vim.cmd.normal { 'u', bang = true }
  end, {
    buffer = buf,
    silent = true,
    desc = 'Undo or restore Codex manual apply overlay',
  })
  vim.api.nvim_buf_attach(buf, false, {
    on_lines = function(_, changed_buf)
      vim.schedule(function()
        render_overlay(changed_buf)
      end)
    end,
    on_detach = function(_, detached_buf)
      clear_overlay(detached_buf)
      vim.b[detached_buf].codex_manual_apply_attached = nil
    end,
  })
end

function M.is_payload_path(path)
  return type(path) == 'string' and path:match(PAYLOAD_PATTERN) ~= nil
end

function M.run(payload_path)
  if not M.is_payload_path(payload_path) then
    return false
  end

  local data, err = read_payload(payload_path)
  if not data then
    notify(err, vim.log.levels.ERROR)
    return false
  end

  if data.kind ~= 'manual_patch_apply_request' or type(data.changes) ~= 'table' or type(data.changes[1]) ~= 'table' then
    notify('Payload is missing required manual-apply fields', vim.log.levels.ERROR)
    return false
  end

  local change = data.changes[1]
  if type(change.path) ~= 'string' or change.path == '' then
    notify('Payload change is missing a valid target path', vim.log.levels.ERROR)
    return false
  end

  if vim.fn.filereadable(change.path) ~= 1 then
    notify('Target file does not exist: ' .. change.path, vim.log.levels.ERROR)
    return false
  end

  local diff = type(change.diff) == 'string' and change.diff or ''
  local start_line, added_lines = parse_diff(diff)
  if #added_lines == 0 then
    added_lines = { '' }
  end

  vim.cmd('edit ' .. vim.fn.fnameescape(change.path))

  local buf = vim.api.nvim_get_current_buf()
  local line_count = vim.api.nvim_buf_line_count(0)
  start_line = math.max(1, math.min(start_line, line_count + 1))
  vim.api.nvim_buf_set_lines(buf, start_line - 1, start_line - 1, false, vim.fn['repeat']({ '' }, #added_lines))
  attach_overlay(buf, start_line - 1, added_lines)
  vim.api.nvim_win_set_cursor(0, { start_line, 0 })

  return true
end

return M
