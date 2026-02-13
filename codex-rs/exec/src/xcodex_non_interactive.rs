use std::sync::Arc;

use codex_core::CodexThread;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::protocol::ReviewDecision;
use codex_protocol::approvals::ElicitationAction;

pub(crate) fn xcodex_auto_op_for_event(event: &EventMsg) -> Option<Op> {
    match event {
        EventMsg::ElicitationRequest(ev) => Some(Op::ResolveElicitation {
            server_name: ev.server_name.clone(),
            request_id: ev.id.clone(),
            decision: ElicitationAction::Cancel,
        }),
        EventMsg::ExecApprovalRequest(ev) => Some(Op::ExecApproval {
            id: ev.call_id.clone(),
            turn_id: Some(ev.turn_id.clone()),
            decision: ReviewDecision::Denied,
        }),
        EventMsg::ApplyPatchApprovalRequest(ev) => Some(Op::PatchApproval {
            id: ev.call_id.clone(),
            decision: ReviewDecision::Denied,
        }),
        _ => None,
    }
}

pub(crate) async fn handle_xcodex_non_interactive_event(
    thread: &Arc<CodexThread>,
    event: &EventMsg,
) -> anyhow::Result<()> {
    if let Some(op) = xcodex_auto_op_for_event(event) {
        thread.submit(op).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::xcodex_auto_op_for_event;
    use codex_core::protocol::EventMsg;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn xcodex_auto_op_cancels_elicitation_requests() {
        let event = serde_json::from_value::<EventMsg>(json!({
            "type": "elicitation_request",
            "server_name": "mcp-test",
            "id": "req-1",
            "message": "Need approval?",
        }))
        .expect("event should deserialize");
        let op = xcodex_auto_op_for_event(&event).expect("op should be generated");

        let op_value = serde_json::to_value(op).expect("op should serialize");
        assert_eq!(
            op_value,
            json!({
                "type": "resolve_elicitation",
                "server_name": "mcp-test",
                "request_id": "req-1",
                "decision": "cancel",
            })
        );
    }

    #[test]
    fn xcodex_auto_op_denies_exec_approval_requests() {
        let event = serde_json::from_value::<EventMsg>(json!({
            "type": "exec_approval_request",
            "call_id": "call-1",
            "turn_id": "turn-1",
            "command": ["git", "status"],
            "cwd": ".",
            "reason": null,
            "proposed_execpolicy_amendment": null,
            "parsed_cmd": [],
        }))
        .expect("event should deserialize");
        let op = xcodex_auto_op_for_event(&event).expect("op should be generated");

        let op_value = serde_json::to_value(op).expect("op should serialize");
        assert_eq!(
            op_value,
            json!({
                "type": "exec_approval",
                "id": "call-1",
                "turn_id": "turn-1",
                "decision": "denied",
            })
        );
    }

    #[test]
    fn xcodex_auto_op_denies_patch_approval_requests() {
        let event = serde_json::from_value::<EventMsg>(json!({
            "type": "apply_patch_approval_request",
            "call_id": "patch-1",
            "turn_id": "turn-1",
            "changes": {},
            "reason": null,
            "grant_root": null,
        }))
        .expect("event should deserialize");
        let op = xcodex_auto_op_for_event(&event).expect("op should be generated");

        let op_value = serde_json::to_value(op).expect("op should serialize");
        assert_eq!(
            op_value,
            json!({
                "type": "patch_approval",
                "id": "patch-1",
                "decision": "denied",
            })
        );
    }

    #[test]
    fn xcodex_auto_op_ignores_unrelated_events() {
        let event = serde_json::from_value::<EventMsg>(json!({
            "type": "shutdown_complete",
        }))
        .expect("event should deserialize");

        assert_eq!(xcodex_auto_op_for_event(&event), None);
    }
}
