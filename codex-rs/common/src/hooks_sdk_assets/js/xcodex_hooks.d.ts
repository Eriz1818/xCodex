/**
 * xCodex hooks kit: TypeScript type definitions for external hooks.
 *
 * Installed into `$CODEX_HOME/hooks/` by:
 * - `xcodex hooks install javascript`
 * - `xcodex hooks install typescript`
 *
 * These types model the JSON payload shape emitted by Codex hooks.
 *
 * Docs:
 * - Hooks overview: docs/xcodex/hooks.md
 * - Hook SDK installers: docs/xcodex/hooks-sdks.md
 * - Machine-readable schema: docs/xcodex/hooks.schema.json
 * - Authoritative config reference: docs/config.md#hooks
 */

export type ApprovalKind = "exec" | "apply-patch" | "elicitation";
export type ToolCallStatus = "completed" | "aborted";

export type HookPayloadBase = {
  "schema-version": number;
  "event-id": string;
  timestamp: string;
};

export type AgentTurnCompletePayload = HookPayloadBase & {
  type: "agent-turn-complete";
  cwd: string;
  "input-messages": string[];
  "last-assistant-message"?: null | string;
  "thread-id": string;
  "turn-id": string;
};

export type ApprovalRequestedPayload = HookPayloadBase & {
  type: "approval-requested";
  "approval-policy"?: "untrusted" | "on-failure" | "on-request" | "never" | null;
  "call-id"?: null | string;
  command?: null | string[];
  cwd?: null | string;
  "grant-root"?: null | string;
  kind: ApprovalKind;
  message?: null | string;
  paths?: null | string[];
  "proposed-execpolicy-amendment"?: null | string[];
  reason?: null | string;
  "request-id"?: null | string;
  "sandbox-policy"?: null | unknown;
  "server-name"?: null | string;
  "thread-id": string;
  "turn-id"?: null | string;
};

export type ModelRequestStartedPayload = HookPayloadBase & {
  type: "model-request-started";
  attempt: number;
  cwd: string;
  "has-output-schema": boolean;
  model: string;
  "model-request-id": string;
  "parallel-tool-calls": boolean;
  "prompt-input-item-count": number;
  provider: string;
  "thread-id": string;
  "tool-count": number;
  "turn-id": string;
};

export type ModelResponseCompletedPayload = HookPayloadBase & {
  type: "model-response-completed";
  attempt: number;
  cwd: string;
  "model-request-id": string;
  "needs-follow-up": boolean;
  "response-id": string;
  "thread-id": string;
  "token-usage"?: null | unknown;
  "turn-id": string;
};

export type SessionEndPayload = HookPayloadBase & {
  type: "session-end";
  cwd: string;
  "session-source": string;
  "thread-id": string;
};

export type SessionStartPayload = HookPayloadBase & {
  type: "session-start";
  cwd: string;
  "session-source": string;
  "thread-id": string;
};

export type ToolCallFinishedPayload = HookPayloadBase & {
  type: "tool-call-finished";
  attempt: number;
  "call-id": string;
  cwd: string;
  "duration-ms": number;
  "model-request-id": string;
  "output-bytes": number;
  "output-preview"?: null | string;
  status: ToolCallStatus;
  success: boolean;
  "thread-id": string;
  "tool-name": string;
  "turn-id": string;
};

export type ToolCallStartedPayload = HookPayloadBase & {
  type: "tool-call-started";
  attempt: number;
  "call-id": string;
  cwd: string;
  "model-request-id": string;
  "thread-id": string;
  "tool-name": string;
  "turn-id": string;
};

export type HookPayload =
  | AgentTurnCompletePayload
  | ApprovalRequestedPayload
  | ModelRequestStartedPayload
  | ModelResponseCompletedPayload
  | SessionEndPayload
  | SessionStartPayload
  | ToolCallFinishedPayload
  | ToolCallStartedPayload
  | (HookPayloadBase & { type: string; [k: string]: unknown });

/**
 * Read a hook payload (handles stdin vs payload-path envelopes).
 *
 * @param raw Optional stdin string; if omitted, reads from fd 0.
 */
export function readPayload(raw?: string): HookPayload;
