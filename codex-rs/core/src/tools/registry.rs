use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::client_common::tools::ToolSpec;
use crate::function_tool::FunctionCallError;
use crate::protocol::EventMsg;
use crate::protocol::ReviewDecision;
use crate::protocol::WarningEvent;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::ToolProvenance;
use async_trait::async_trait;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_utils_readiness::Readiness;
use tracing::warn;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Function,
    Mcp,
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolKind;

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            (self.kind(), payload),
            (ToolKind::Function, ToolPayload::Function { .. })
                | (ToolKind::Mcp, ToolPayload::Mcp { .. })
        )
    }

    /// Returns `true` if the [ToolInvocation] *might* mutate the environment of the
    /// user (through file system, OS operations, ...).
    /// This function must remains defensive and return `true` if a doubt exist on the
    /// exact effect of a ToolInvocation.
    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    /// Perform the actual [ToolInvocation] and returns a [ToolOutput] containing
    /// the final output to return to the model.
    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError>;
}

pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    pub fn new(handlers: HashMap<String, Arc<dyn ToolHandler>>) -> Self {
        Self { handlers }
    }

    pub fn handler(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        if let Some(handler) = self.handlers.get(name) {
            return Some(Arc::clone(handler));
        }
        if name.starts_with("mcp__") {
            return self.handlers.get("mcp__").cloned();
        }
        None
    }

    // TODO(jif) for dynamic tools.
    // pub fn register(&mut self, name: impl Into<String>, handler: Arc<dyn ToolHandler>) {
    //     let name = name.into();
    //     if self.handlers.insert(name.clone(), handler).is_some() {
    //         warn!("overwriting handler for tool {name}");
    //     }
    // }

    pub async fn dispatch(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ResponseInputItem, FunctionCallError> {
        let tool_name = invocation.tool_name.clone();
        let call_id_owned = invocation.call_id.clone();
        let session = invocation.session.clone();
        let turn = invocation.turn.clone();
        let otel = invocation.turn.client.get_otel_manager();
        let payload_for_response = invocation.payload.clone();
        let log_payload = payload_for_response.log_payload();

        let handler = match self.handler(tool_name.as_ref()) {
            Some(handler) => handler,
            None => {
                let message =
                    unsupported_tool_call_message(&invocation.payload, tool_name.as_ref());
                otel.tool_result(
                    tool_name.as_ref(),
                    &call_id_owned,
                    log_payload.as_ref(),
                    Duration::ZERO,
                    false,
                    &message,
                );
                return Err(FunctionCallError::RespondToModel(message));
            }
        };

        if !handler.matches_kind(&invocation.payload) {
            let message = format!("tool {tool_name} invoked with incompatible payload");
            otel.tool_result(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                false,
                &message,
            );
            return Err(FunctionCallError::Fatal(message));
        }

        let output_cell = tokio::sync::Mutex::new(None);

        let result = otel
            .log_tool_result(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                || {
                    let handler = handler.clone();
                    let output_cell = &output_cell;
                    let invocation = invocation;
                    async move {
                        if handler.is_mutating(&invocation).await {
                            tracing::trace!("waiting for tool gate");
                            invocation.turn.tool_call_gate.wait_ready().await;
                            tracing::trace!("tool gate released");
                        }
                        match handler.handle(invocation).await {
                            Ok(output) => {
                                let preview = output.log_preview();
                                let success = output.success_for_logging();
                                let mut guard = output_cell.lock().await;
                                *guard = Some(output);
                                Ok((preview, success))
                            }
                            Err(err) => Err(err),
                        }
                    }
                },
            )
            .await;

        match result {
            Ok(_) => {
                let mut guard = output_cell.lock().await;
                let output = guard.take().ok_or_else(|| {
                    FunctionCallError::Fatal("tool produced no output".to_string())
                })?;
                let output = enforce_sensitive_send_policy(
                    output,
                    session.as_ref(),
                    &turn,
                    &tool_name,
                    &call_id_owned,
                )
                .await;
                Ok(output.into_response(&call_id_owned, &payload_for_response))
            }
            Err(err) => Err(err),
        }
    }
}

async fn enforce_sensitive_send_policy(
    output: ToolOutput,
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    tool_name: &str,
    call_id: &str,
) -> ToolOutput {
    let output = match output {
        ToolOutput::Function {
            content: _,
            content_items: _,
            success: _,
            provenance: ToolProvenance::Filesystem { ref path },
        } if turn.exclusion.layer_send_firewall_enabled()
            && turn.sensitive_paths.decision_send(path)
                == crate::sensitive_paths::SensitivePathDecision::Deny =>
        {
            if turn.exclusion.prompt_on_blocked
                && maybe_prompt_for_send(session, turn, call_id, path).await
            {
                output
            } else {
                let mut counters = turn
                    .exclusion_counters
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                counters.record(
                    crate::exclusion_counters::ExclusionLayer::Layer3SendFirewall,
                    crate::exclusion_counters::ExclusionSource::Filesystem,
                    tool_name,
                    /* redacted */ false,
                    /* blocked */ true,
                );
                ToolOutput::Function {
                    content: turn.sensitive_paths.format_denied_message(),
                    content_items: None,
                    success: Some(false),
                    provenance: ToolProvenance::Filesystem { path: path.clone() },
                }
            }
        }
        other => other,
    };

    let output = if turn.exclusion.layer_output_sanitization_enabled() {
        enforce_sensitive_content_gateway(output, session, turn, tool_name)
    } else {
        output
    };

    if !is_unattested_output(&output) {
        return output;
    }

    enforce_unattested_output_policy(
        output,
        turn.unattested_output_policy,
        tool_name,
        call_id,
        |message| async {
            session
                .send_event(turn, EventMsg::Warning(WarningEvent { message }))
                .await;
        },
        |command| async {
            session
                .request_command_approval(
                    turn,
                    call_id.to_string(),
                    command,
                    turn.cwd.clone(),
                    Some("unattested MCP output would be sent to the model".to_string()),
                    None,
                )
                .await
        },
    )
    .await
}

async fn maybe_prompt_for_send(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    path: &std::path::Path,
) -> bool {
    let display = path.display().to_string();
    let question = RequestUserInputQuestion {
        header: "Exclusions".to_string(),
        id: "exclusions_send".to_string(),
        question: format!("Allow xcodex to send this excluded output?\n{display}"),
        is_other: false,
        is_secret: false,
        options: Some(vec![
            RequestUserInputQuestionOption {
                label: "Allow once".to_string(),
                description: "Permit this output for the current request.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Block".to_string(),
                description: "Keep exclusions blocking this output.".to_string(),
            },
        ]),
    };
    let args = RequestUserInputArgs {
        questions: vec![question],
    };
    let response = session
        .request_user_input(turn, call_id.to_string(), args)
        .await;
    response
        .and_then(|response| response.answers.get("exclusions_send").cloned())
        .and_then(|answer| answer.answers.first().cloned())
        .is_some_and(|value| value == "Allow once")
}

fn enforce_sensitive_content_gateway(
    output: ToolOutput,
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    tool_name: &str,
) -> ToolOutput {
    let epoch = turn.sensitive_paths.ignore_epoch();
    let gateway = crate::content_gateway::ContentGateway::new(
        crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion),
    );

    match output {
        ToolOutput::Function {
            content,
            mut content_items,
            mut success,
            provenance,
        } => {
            let source = match &provenance {
                ToolProvenance::Filesystem { .. } => {
                    crate::exclusion_counters::ExclusionSource::Filesystem
                }
                ToolProvenance::Mcp { .. } => crate::exclusion_counters::ExclusionSource::Mcp,
                ToolProvenance::Shell { .. } => crate::exclusion_counters::ExclusionSource::Shell,
                ToolProvenance::Unattested { .. } => {
                    crate::exclusion_counters::ExclusionSource::Other
                }
            };
            let origin_type = provenance.origin_type();
            let origin_path = provenance.origin_path();
            let should_log = turn.exclusion.log_redactions_mode()
                != crate::config::types::LogRedactionsMode::Off;
            let log_context = crate::exclusion_log::RedactionLogContext {
                codex_home: &turn.codex_home,
                layer: crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                source,
                tool_name,
                origin_type,
                origin_path: origin_path.as_deref(),
                log_mode: turn.exclusion.log_redactions_mode(),
                max_bytes: turn.exclusion.log_redactions_max_bytes,
                max_files: turn.exclusion.log_redactions_max_files,
            };
            let original_content = content;
            let (content, report) = gateway.scan_text(
                &original_content,
                &turn.sensitive_paths,
                &session.content_gateway_cache,
                epoch,
            );
            if should_log && (report.redacted || report.blocked) {
                crate::exclusion_log::log_redaction_event(
                    &log_context,
                    &report,
                    &original_content,
                    &content,
                );
            }
            if report.redacted || report.blocked {
                let mut counters = turn
                    .exclusion_counters
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                counters.record(
                    crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                    source,
                    tool_name,
                    report.redacted,
                    report.blocked,
                );
            }

            if let Some(items) = &mut content_items {
                for item in items.iter_mut() {
                    if let FunctionCallOutputContentItem::InputText { text } = item {
                        let original_text = text.clone();
                        let (next, r) = gateway.scan_text(
                            &original_text,
                            &turn.sensitive_paths,
                            &session.content_gateway_cache,
                            epoch,
                        );
                        *text = next;
                        if should_log && (r.redacted || r.blocked) {
                            crate::exclusion_log::log_redaction_event(
                                &log_context,
                                &r,
                                &original_text,
                                text.as_str(),
                            );
                        }
                        if r.redacted || r.blocked {
                            let mut counters = turn
                                .exclusion_counters
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            counters.record(
                                crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                                source,
                                tool_name,
                                r.redacted,
                                r.blocked,
                            );
                        }
                        if r.redacted {
                            success = Some(false);
                        }
                    }
                }
            }

            if report.redacted {
                success = Some(false);
            }

            ToolOutput::Function {
                content,
                content_items,
                success,
                provenance,
            }
        }
        ToolOutput::Mcp { result, provenance } => {
            let mut report = crate::content_gateway::ScanReport::safe();
            let origin_type = provenance.origin_type();
            let origin_path = provenance.origin_path();
            let should_log = turn.exclusion.log_redactions_mode()
                != crate::config::types::LogRedactionsMode::Off;
            let log_context = crate::exclusion_log::RedactionLogContext {
                codex_home: &turn.codex_home,
                layer: crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                source: crate::exclusion_counters::ExclusionSource::Mcp,
                tool_name,
                origin_type,
                origin_path: origin_path.as_deref(),
                log_mode: turn.exclusion.log_redactions_mode(),
                max_bytes: turn.exclusion.log_redactions_max_bytes,
                max_files: turn.exclusion.log_redactions_max_files,
            };
            let result = result.map(|mut ok| {
                let mut scan_string = |s: &mut String| {
                    let original = s.clone();
                    let (next, r) = gateway.scan_text(
                        &original,
                        &turn.sensitive_paths,
                        &session.content_gateway_cache,
                        epoch,
                    );
                    *s = next;
                    report.layers.extend(r.layers.iter().copied());
                    report.redacted |= r.redacted;
                    report.blocked |= r.blocked;
                    report.reasons.extend(r.reasons.iter().copied());
                    if should_log && (r.redacted || r.blocked) {
                        crate::exclusion_log::log_redaction_event(
                            &log_context,
                            &r,
                            &original,
                            s.as_str(),
                        );
                    }
                };

                for block in ok.content.iter_mut() {
                    match block {
                        mcp_types::ContentBlock::TextContent(text) => scan_string(&mut text.text),
                        mcp_types::ContentBlock::ResourceLink(link) => {
                            if let Some(desc) = &mut link.description {
                                scan_string(desc);
                            }
                            if let Some(title) = &mut link.title {
                                scan_string(title);
                            }
                            scan_string(&mut link.name);
                            scan_string(&mut link.uri);
                        }
                        mcp_types::ContentBlock::EmbeddedResource(resource) => {
                            match &mut resource.resource {
                                mcp_types::EmbeddedResourceResource::TextResourceContents(text) => {
                                    if let Some(mime) = &mut text.mime_type {
                                        scan_string(mime);
                                    }
                                    scan_string(&mut text.text);
                                    scan_string(&mut text.uri);
                                }
                                mcp_types::EmbeddedResourceResource::BlobResourceContents(blob) => {
                                    if let Some(mime) = &mut blob.mime_type {
                                        scan_string(mime);
                                    }
                                    scan_string(&mut blob.uri);
                                }
                            }
                        }
                        mcp_types::ContentBlock::ImageContent(_)
                        | mcp_types::ContentBlock::AudioContent(_) => {}
                    }
                }
                ok
            });
            if report.redacted || report.blocked {
                let mut counters = turn
                    .exclusion_counters
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                counters.record(
                    crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                    crate::exclusion_counters::ExclusionSource::Mcp,
                    tool_name,
                    report.redacted,
                    report.blocked,
                );
            }
            ToolOutput::Mcp { result, provenance }
        }
    }
}

fn is_unattested_output(output: &ToolOutput) -> bool {
    match output {
        ToolOutput::Mcp { provenance, .. } => matches!(provenance, ToolProvenance::Mcp { .. }),
        ToolOutput::Function { provenance, .. } => matches!(
            provenance,
            ToolProvenance::Shell { .. }
                | ToolProvenance::Mcp { .. }
                | ToolProvenance::Unattested { .. }
        ),
    }
}

async fn enforce_unattested_output_policy<WarnFut, WarnFn, ApprovalFut, ApprovalFn>(
    output: ToolOutput,
    policy: crate::config::types::UnattestedOutputPolicy,
    tool_name: &str,
    call_id: &str,
    mut warn: WarnFn,
    mut request_approval: ApprovalFn,
) -> ToolOutput
where
    WarnFn: FnMut(String) -> WarnFut,
    WarnFut: std::future::Future<Output = ()>,
    ApprovalFn: FnMut(Vec<String>) -> ApprovalFut,
    ApprovalFut: std::future::Future<Output = ReviewDecision>,
{
    match policy {
        crate::config::types::UnattestedOutputPolicy::Allow => output,
        crate::config::types::UnattestedOutputPolicy::Warn => {
            warn(unattested_output_warning_message(
                &output, policy, tool_name, call_id,
            ))
            .await;
            output
        }
        crate::config::types::UnattestedOutputPolicy::Confirm => {
            warn(unattested_output_warning_message(
                &output, policy, tool_name, call_id,
            ))
            .await;

            let provenance = match &output {
                ToolOutput::Function { provenance, .. } => provenance,
                ToolOutput::Mcp { provenance, .. } => provenance,
            };

            let mut command = vec!["send_unattested_output".to_string(), tool_name.to_string()];
            command.push(provenance.origin_type().to_string());
            if let Some(path) = provenance.origin_path() {
                command.push(path);
            }

            let decision = request_approval(command).await;
            match decision {
                ReviewDecision::Approved
                | ReviewDecision::ApprovedForSession
                | ReviewDecision::ApprovedExecpolicyAmendment { .. } => output,
                ReviewDecision::Denied | ReviewDecision::Abort => block_unattested_output(output),
            }
        }
        crate::config::types::UnattestedOutputPolicy::Block => block_unattested_output(output),
    }
}

fn unattested_output_warning_message(
    output: &ToolOutput,
    policy: crate::config::types::UnattestedOutputPolicy,
    tool_name: &str,
    call_id: &str,
) -> String {
    let provenance = match output {
        ToolOutput::Function { provenance, .. } => provenance,
        ToolOutput::Mcp { provenance, .. } => provenance,
    };
    let origin = provenance
        .origin_path()
        .unwrap_or_else(|| String::from("<unknown>"));
    format!(
        "unattested tool output ({tool_name}, call_id={call_id}, origin={origin}) may contain sensitive data; policy={policy:?}"
    )
}

fn block_unattested_output(output: ToolOutput) -> ToolOutput {
    let message = "unattested tool output blocked by policy".to_string();
    match output {
        ToolOutput::Function { provenance, .. } => ToolOutput::Function {
            content: message,
            content_items: None,
            success: Some(false),
            provenance,
        },
        ToolOutput::Mcp { provenance, .. } => ToolOutput::Mcp {
            result: Err(message),
            provenance,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::UnattestedOutputPolicy;
    use pretty_assertions::assert_eq;

    fn unattested_output() -> ToolOutput {
        ToolOutput::Function {
            content: "payload".to_string(),
            content_items: None,
            success: Some(true),
            provenance: ToolProvenance::Unattested {
                origin_type: "mcp",
                origin_path: Some("server/tool".to_string()),
            },
        }
    }

    #[test]
    fn is_unattested_output_matches_expected_provenance() {
        let output = unattested_output();
        assert_eq!(true, super::is_unattested_output(&output));

        let output = ToolOutput::Function {
            content: "payload".to_string(),
            content_items: None,
            success: Some(true),
            provenance: ToolProvenance::Filesystem {
                path: std::path::PathBuf::from("/tmp/file"),
            },
        };
        assert_eq!(false, super::is_unattested_output(&output));

        let output = ToolOutput::Mcp {
            result: Err("boom".to_string()),
            provenance: ToolProvenance::Mcp {
                server: "server".to_string(),
                tool: "tool".to_string(),
            },
        };
        assert_eq!(true, super::is_unattested_output(&output));
    }

    #[test]
    fn block_unattested_output_replaces_payload_with_policy_message() {
        let output = unattested_output();
        let blocked = super::block_unattested_output(output);
        match blocked {
            ToolOutput::Function {
                content,
                content_items: None,
                success: Some(false),
                provenance:
                    ToolProvenance::Unattested {
                        origin_type: "mcp",
                        origin_path: Some(origin),
                    },
            } => {
                assert_eq!(content, "unattested tool output blocked by policy");
                assert_eq!(origin, "server/tool");
            }
            _ => panic!("unexpected output variant"),
        }
    }

    #[tokio::test]
    async fn enforce_unattested_output_policy_warn_emits_warning() {
        let output = unattested_output();
        let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let output = super::enforce_unattested_output_policy(
            output,
            UnattestedOutputPolicy::Warn,
            "mcp__server__tool",
            "call-1",
            {
                let warnings = std::sync::Arc::clone(&warnings);
                move |message| {
                    let warnings = std::sync::Arc::clone(&warnings);
                    async move {
                        warnings
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(message);
                    }
                }
            },
            |_command| async { ReviewDecision::Abort },
        )
        .await;

        let warnings = warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0],
            "unattested tool output (mcp__server__tool, call_id=call-1, origin=server/tool) may contain sensitive data; policy=Warn"
        );
        match output {
            ToolOutput::Function {
                content,
                content_items: None,
                success: Some(true),
                provenance:
                    ToolProvenance::Unattested {
                        origin_type: "mcp",
                        origin_path: Some(origin),
                    },
            } => {
                assert_eq!(content, "payload");
                assert_eq!(origin, "server/tool");
            }
            _ => panic!("unexpected output variant"),
        }
    }

    #[tokio::test]
    async fn enforce_unattested_output_policy_confirm_requests_approval_and_blocks_on_denied() {
        let output = unattested_output();
        let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let approval_commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let output = super::enforce_unattested_output_policy(
            output,
            UnattestedOutputPolicy::Confirm,
            "mcp__server__tool",
            "call-1",
            {
                let warnings = std::sync::Arc::clone(&warnings);
                move |message| {
                    let warnings = std::sync::Arc::clone(&warnings);
                    async move {
                        warnings
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(message);
                    }
                }
            },
            {
                let approval_commands = std::sync::Arc::clone(&approval_commands);
                move |command| {
                    let approval_commands = std::sync::Arc::clone(&approval_commands);
                    async move {
                        approval_commands
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(command);
                        ReviewDecision::Denied
                    }
                }
            },
        )
        .await;

        let warnings = warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let approval_commands = approval_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(warnings.len(), 1);
        assert_eq!(approval_commands.len(), 1);
        assert_eq!(
            approval_commands[0],
            vec![
                "send_unattested_output".to_string(),
                "mcp__server__tool".to_string(),
                "mcp".to_string(),
                "server/tool".to_string(),
            ]
        );

        match output {
            ToolOutput::Function {
                content,
                content_items: None,
                success: Some(false),
                provenance:
                    ToolProvenance::Unattested {
                        origin_type: "mcp",
                        origin_path: Some(origin),
                    },
            } => {
                assert_eq!(content, "unattested tool output blocked by policy");
                assert_eq!(origin, "server/tool");
            }
            _ => panic!("unexpected output variant"),
        }
    }

    #[tokio::test]
    async fn enforce_unattested_output_policy_confirm_allows_on_approved() {
        let output = unattested_output();
        let approvals = std::sync::Arc::new(std::sync::Mutex::new(0_u64));

        let output = super::enforce_unattested_output_policy(
            output,
            UnattestedOutputPolicy::Confirm,
            "mcp__server__tool",
            "call-1",
            |_message| async {},
            {
                let approvals = std::sync::Arc::clone(&approvals);
                move |_command| {
                    let approvals = std::sync::Arc::clone(&approvals);
                    async move {
                        let mut guard = approvals
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        *guard += 1;
                        ReviewDecision::Approved
                    }
                }
            },
        )
        .await;

        assert_eq!(
            *approvals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            1
        );
        match output {
            ToolOutput::Function {
                content,
                content_items: None,
                success: Some(true),
                provenance:
                    ToolProvenance::Unattested {
                        origin_type: "mcp",
                        origin_path: Some(origin),
                    },
            } => {
                assert_eq!(content, "payload");
                assert_eq!(origin, "server/tool");
            }
            _ => panic!("unexpected output variant"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredToolSpec {
    pub spec: ToolSpec,
    pub supports_parallel_tool_calls: bool,
}

impl ConfiguredToolSpec {
    pub fn new(spec: ToolSpec, supports_parallel_tool_calls: bool) -> Self {
        Self {
            spec,
            supports_parallel_tool_calls,
        }
    }
}

pub struct ToolRegistryBuilder {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
    specs: Vec<ConfiguredToolSpec>,
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            specs: Vec::new(),
        }
    }

    pub fn push_spec(&mut self, spec: ToolSpec) {
        self.push_spec_with_parallel_support(spec, false);
    }

    pub fn push_spec_with_parallel_support(
        &mut self,
        spec: ToolSpec,
        supports_parallel_tool_calls: bool,
    ) {
        self.specs
            .push(ConfiguredToolSpec::new(spec, supports_parallel_tool_calls));
    }

    pub fn register_handler(&mut self, name: impl Into<String>, handler: Arc<dyn ToolHandler>) {
        let name = name.into();
        if self
            .handlers
            .insert(name.clone(), handler.clone())
            .is_some()
        {
            warn!("overwriting handler for tool {name}");
        }
    }

    // TODO(jif) for dynamic tools.
    // pub fn register_many<I>(&mut self, names: I, handler: Arc<dyn ToolHandler>)
    // where
    //     I: IntoIterator,
    //     I::Item: Into<String>,
    // {
    //     for name in names {
    //         let name = name.into();
    //         if self
    //             .handlers
    //             .insert(name.clone(), handler.clone())
    //             .is_some()
    //         {
    //             warn!("overwriting handler for tool {name}");
    //         }
    //     }
    // }

    pub fn build(self) -> (Vec<ConfiguredToolSpec>, ToolRegistry) {
        let registry = ToolRegistry::new(self.handlers);
        (self.specs, registry)
    }
}

fn unsupported_tool_call_message(payload: &ToolPayload, tool_name: &str) -> String {
    match payload {
        ToolPayload::Custom { .. } => format!("unsupported custom tool call: {tool_name}"),
        _ => format!("unsupported call: {tool_name}"),
    }
}
