use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::client_common::tools::ToolSpec;
use crate::function_tool::FunctionCallError;
use crate::protocol::EventMsg;
use crate::protocol::ReviewDecision;
use crate::protocol::SandboxPolicy;
use crate::protocol::WarningEvent;
use crate::sandbox_tags::sandbox_tag;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::ToolProvenance;
use async_trait::async_trait;
use codex_hooks::HookEvent;
use codex_hooks::HookEventAfterToolUse;
use codex_hooks::HookPayload;
use codex_hooks::HookToolInput;
use codex_hooks::HookToolInputLocalShell;
use codex_hooks::HookToolKind;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_utils_readiness::Readiness;
use sha2::Digest as _;
use sha2::Sha256;
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
        let otel = invocation.turn.otel_manager.clone();
        let payload_for_response = invocation.payload.clone();
        let log_payload = payload_for_response.log_payload();
        let metric_tags = [
            (
                "sandbox",
                sandbox_tag(
                    &invocation.turn.sandbox_policy,
                    invocation.turn.windows_sandbox_level,
                ),
            ),
            (
                "sandbox_policy",
                sandbox_policy_tag(&invocation.turn.sandbox_policy),
            ),
        ];

        if let Some(message) = plan_mode_tool_block_message(
            invocation.turn.collaboration_mode.mode,
            tool_name.as_ref(),
        ) {
            otel.tool_result_with_tags(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                false,
                &message,
                &metric_tags,
            );
            return Err(FunctionCallError::RespondToModel(message));
        }

        let handler = match self.handler(tool_name.as_ref()) {
            Some(handler) => handler,
            None => {
                let message =
                    unsupported_tool_call_message(&invocation.payload, tool_name.as_ref());
                otel.tool_result_with_tags(
                    tool_name.as_ref(),
                    &call_id_owned,
                    log_payload.as_ref(),
                    Duration::ZERO,
                    false,
                    &message,
                    &metric_tags,
                );
                return Err(FunctionCallError::RespondToModel(message));
            }
        };

        if !handler.matches_kind(&invocation.payload) {
            let message = format!("tool {tool_name} invoked with incompatible payload");
            otel.tool_result_with_tags(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                false,
                &message,
                &metric_tags,
            );
            return Err(FunctionCallError::Fatal(message));
        }

        let is_mutating = handler.is_mutating(&invocation).await;
        let output_cell = tokio::sync::Mutex::new(None);
        let invocation_for_tool = invocation.clone();

        let started = Instant::now();
        let result = otel
            .log_tool_result_with_tags(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                &metric_tags,
                || {
                    let handler = handler.clone();
                    let output_cell = &output_cell;
                    async move {
                        if is_mutating {
                            tracing::trace!("waiting for tool gate");
                            invocation_for_tool.turn.tool_call_gate.wait_ready().await;
                            tracing::trace!("tool gate released");
                        }
                        match handler.handle(invocation_for_tool).await {
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
        let duration = started.elapsed();
        let (output_preview, success) = match &result {
            Ok((preview, success)) => (preview.clone(), *success),
            Err(err) => (err.to_string(), false),
        };
        dispatch_after_tool_use_hook(AfterToolUseHookDispatch {
            invocation: &invocation,
            output_preview,
            success,
            executed: true,
            duration,
            mutating: is_mutating,
        })
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
            body,
            success,
            provenance: ToolProvenance::Filesystem { path },
        } if turn.exclusion.layer_send_firewall_enabled()
            && turn.sensitive_paths.decision_send(&path)
                == crate::sensitive_paths::SensitivePathDecision::Deny =>
        {
            if turn.exclusion.prompt_on_blocked
                && maybe_prompt_for_send(session, turn, call_id, &path).await
            {
                ToolOutput::Function {
                    body,
                    success,
                    provenance: ToolProvenance::Filesystem { path },
                }
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
                    body: FunctionCallOutputBody::Text(
                        turn.sensitive_paths.format_denied_message(),
                    ),
                    success: Some(false),
                    provenance: ToolProvenance::Filesystem { path },
                }
            }
        }
        other => other,
    };

    let output = if turn.exclusion.layer_output_sanitization_enabled() {
        enforce_sensitive_content_gateway(output, session, turn, tool_name, call_id).await
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

enum RedactionDecision {
    AllowOnce,
    AllowForSession,
    Redact,
    Block,
    AddAllowlistLiteral(String),
    AddAllowlistRegex(String),
    AddBlocklist(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RedactionPromptAnswer {
    AllowOnce,
    AllowForSession,
    Redact,
    Block,
    AddToAllowlist,
    AddToBlocklist,
    RevealMatches,
    HideMatches,
}

fn parse_redaction_prompt_answer(answer: &str) -> Option<RedactionPromptAnswer> {
    match answer {
        "Allow once" => Some(RedactionPromptAnswer::AllowOnce),
        "Allow for this session" => Some(RedactionPromptAnswer::AllowForSession),
        "Redact" => Some(RedactionPromptAnswer::Redact),
        "Block" => Some(RedactionPromptAnswer::Block),
        "Add to allowlist" => Some(RedactionPromptAnswer::AddToAllowlist),
        "Add to blocklist" => Some(RedactionPromptAnswer::AddToBlocklist),
        "Reveal matched values" => Some(RedactionPromptAnswer::RevealMatches),
        "Hide matched values" => Some(RedactionPromptAnswer::HideMatches),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RedactionMatchSummary {
    reason: crate::content_gateway::RedactionReason,
    value: String,
    count: usize,
}

fn redaction_reason_label(reason: crate::content_gateway::RedactionReason) -> &'static str {
    match reason {
        crate::content_gateway::RedactionReason::FingerprintCache => "Fingerprint cache",
        crate::content_gateway::RedactionReason::IgnoredPath => "Ignored path",
        crate::content_gateway::RedactionReason::SecretPattern => "Secret pattern",
    }
}

fn short_sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        use std::fmt::Write as _;
        write!(out, "{byte:02x}").ok();
    }
    out
}

fn truncate_match_value(mut value: String) -> String {
    if value.len() <= 200 {
        return value;
    }
    let mut boundary = 200.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value.truncate(boundary);
    value.push_str("...");
    value
}

fn redaction_match_value_display(
    summary: &RedactionMatchSummary,
    reveal_secret_matches: bool,
) -> String {
    match summary.reason {
        crate::content_gateway::RedactionReason::SecretPattern => {
            let hash = short_sha256_hex(&summary.value);
            if reveal_secret_matches {
                return format!("{} (sha256:{hash})", summary.value);
            }
            let char_len = summary.value.chars().count();
            if char_len <= 8 {
                format!("[REDACTED sha256:{hash}]")
            } else {
                let prefix = summary
                    .value
                    .char_indices()
                    .nth(4)
                    .map(|(idx, _)| &summary.value[..idx])
                    .unwrap_or(summary.value.as_str());
                let suffix_start_chars = char_len.saturating_sub(4);
                let suffix_idx = summary
                    .value
                    .char_indices()
                    .nth(suffix_start_chars)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0);
                let suffix = &summary.value[suffix_idx..];
                format!("[REDACTED {prefix}...{suffix} sha256:{hash}]")
            }
        }
        _ => truncate_match_value(summary.value.clone()),
    }
}

fn redaction_match_label(summary: &RedactionMatchSummary, reveal_secret_matches: bool) -> String {
    let reason = redaction_reason_label(summary.reason);
    let value = redaction_match_value_display(summary, reveal_secret_matches);
    let mut label = format!("{value} (reason: {reason})");
    if summary.count > 1 {
        label.push_str(&format!(" x{}", summary.count));
    }
    label
}

fn summarize_redaction_matches(
    report: &crate::content_gateway::ScanReport,
) -> Vec<RedactionMatchSummary> {
    let mut out: Vec<RedactionMatchSummary> = Vec::new();
    let mut seen: HashMap<(crate::content_gateway::RedactionReason, &str), usize> = HashMap::new();

    for match_info in &report.matches {
        let key = (match_info.reason, match_info.value.as_str());
        if let Some(idx) = seen.get(&key) {
            out[*idx].count = out[*idx].count.saturating_add(1);
        } else {
            seen.insert(key, out.len());
            out.push(RedactionMatchSummary {
                reason: match_info.reason,
                value: match_info.value.clone(),
                count: 1,
            });
        }
    }

    out
}

fn format_redaction_matches(
    report: &crate::content_gateway::ScanReport,
    layer_label: &str,
    reveal_secret_matches: bool,
) -> Option<String> {
    if report.matches.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    lines.push(format!("Matched content ({layer_label}):"));
    for match_info in summarize_redaction_matches(report) {
        lines.push(format!(
            "- {}",
            redaction_match_label(&match_info, reveal_secret_matches)
        ));
    }
    Some(lines.join("\n"))
}

async fn prompt_for_redaction_match_selection(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    prompt: &str,
    question_id: &str,
    options: Vec<RequestUserInputQuestionOption>,
) -> Option<String> {
    let question = RequestUserInputQuestion {
        header: "Exclusions".to_string(),
        id: question_id.to_string(),
        question: prompt.to_string(),
        is_other: false,
        is_secret: false,
        options: Some(options),
    };
    let args = RequestUserInputArgs {
        questions: vec![question],
    };
    let response = session
        .request_user_input(turn, call_id.to_string(), args)
        .await;
    response
        .and_then(|response| response.answers.get(question_id).cloned())
        .and_then(|answer| answer.answers.first().cloned())
}

async fn maybe_prompt_for_redaction(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    context_label: &str,
    report: &crate::content_gateway::ScanReport,
) -> Option<RedactionDecision> {
    if !turn.exclusion.prompt_on_blocked
        || (!report.redacted && !report.blocked && report.matches.is_empty())
    {
        return None;
    }

    let match_summaries = summarize_redaction_matches(report);
    let has_secret_matches = match_summaries.iter().any(|summary| {
        matches!(
            summary.reason,
            crate::content_gateway::RedactionReason::SecretPattern
        )
    });

    let mut reveal_secret_matches =
        has_secret_matches && turn.exclusion.prompt_reveal_secret_matches;
    let answer = loop {
        let mut question_text =
            format!("Exclusions matched content in {context_label}. How should xcodex proceed?");
        if reveal_secret_matches {
            question_text.push_str("\n(Showing full matched values.)");
        }
        if let Some(summary) =
            format_redaction_matches(report, "L2-output_sanitization", reveal_secret_matches)
        {
            question_text.push('\n');
            question_text.push_str(&summary);
        }

        let mut options = vec![
            RequestUserInputQuestionOption {
                label: "Allow once".to_string(),
                description: "Permit this content for the current request.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Allow for this session".to_string(),
                description: "Permit this exact content for this xcodex session.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Redact".to_string(),
                description: "Redact matching content.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Block".to_string(),
                description: "Block matching content.".to_string(),
            },
        ];

        if has_secret_matches {
            options.push(RequestUserInputQuestionOption {
                label: if reveal_secret_matches {
                    "Hide matched values".to_string()
                } else {
                    "Reveal matched values".to_string()
                },
                description: if reveal_secret_matches {
                    "Return to redacted previews for secret matches.".to_string()
                } else {
                    "Show the full matched values in this prompt (may display secrets).".to_string()
                },
            });
        }

        if match_summaries.iter().any(|summary| {
            matches!(
                summary.reason,
                crate::content_gateway::RedactionReason::SecretPattern
                    | crate::content_gateway::RedactionReason::IgnoredPath
            )
        }) {
            options.push(RequestUserInputQuestionOption {
                label: "Add to allowlist".to_string(),
                description: "Allow this matched value through exclusions going forward."
                    .to_string(),
            });
        }

        if has_secret_matches {
            options.push(RequestUserInputQuestionOption {
                label: "Add to blocklist".to_string(),
                description: "Add this value to extra secret patterns to scan.".to_string(),
            });
        }

        let question = RequestUserInputQuestion {
            header: "Exclusions".to_string(),
            id: "exclusions_redaction".to_string(),
            question: question_text,
            is_other: false,
            is_secret: false,
            options: Some(options),
        };
        let args = RequestUserInputArgs {
            questions: vec![question],
        };
        let response = session
            .request_user_input(turn, call_id.to_string(), args)
            .await;
        let answer = response
            .and_then(|response| response.answers.get("exclusions_redaction").cloned())
            .and_then(|answer| answer.answers.first().cloned())?;

        let Some(answer) = parse_redaction_prompt_answer(&answer) else {
            return None;
        };

        match answer {
            RedactionPromptAnswer::RevealMatches => {
                reveal_secret_matches = true;
                continue;
            }
            RedactionPromptAnswer::HideMatches => {
                reveal_secret_matches = false;
                continue;
            }
            other => break other,
        }
    };

    match answer {
        RedactionPromptAnswer::AllowOnce => Some(RedactionDecision::AllowOnce),
        RedactionPromptAnswer::AllowForSession => Some(RedactionDecision::AllowForSession),
        RedactionPromptAnswer::Redact => Some(RedactionDecision::Redact),
        RedactionPromptAnswer::Block => Some(RedactionDecision::Block),
        RedactionPromptAnswer::AddToAllowlist => {
            let candidates: Vec<RedactionMatchSummary> = match_summaries
                .iter()
                .filter(|summary| {
                    matches!(
                        summary.reason,
                        crate::content_gateway::RedactionReason::SecretPattern
                            | crate::content_gateway::RedactionReason::IgnoredPath
                    )
                })
                .cloned()
                .collect();
            let selected = if candidates.len() == 1 {
                candidates.first().cloned()
            } else {
                let prompt = "Select a matched value to add to the allowlist.";
                let options = candidates
                    .iter()
                    .map(|summary| RequestUserInputQuestionOption {
                        label: redaction_match_label(summary, reveal_secret_matches),
                        description: String::new(),
                    })
                    .collect::<Vec<_>>();

                let answer = prompt_for_redaction_match_selection(
                    session,
                    turn,
                    call_id,
                    prompt,
                    "exclusions_allowlist_match",
                    options,
                )
                .await?;

                candidates
                    .into_iter()
                    .find(|summary| redaction_match_label(summary, reveal_secret_matches) == answer)
            }?;

            match selected.reason {
                crate::content_gateway::RedactionReason::IgnoredPath => {
                    Some(RedactionDecision::AddAllowlistLiteral(selected.value))
                }
                _ => Some(RedactionDecision::AddAllowlistRegex(selected.value)),
            }
        }
        RedactionPromptAnswer::AddToBlocklist => {
            let candidates: Vec<RedactionMatchSummary> = match_summaries
                .iter()
                .filter(|summary| {
                    matches!(
                        summary.reason,
                        crate::content_gateway::RedactionReason::SecretPattern
                    )
                })
                .cloned()
                .collect();
            let selected = if candidates.len() == 1 {
                candidates.first().cloned()
            } else {
                let prompt = "Select a matched value to add to the blocklist.";
                let options = candidates
                    .iter()
                    .map(|summary| RequestUserInputQuestionOption {
                        label: redaction_match_label(summary, reveal_secret_matches),
                        description: String::new(),
                    })
                    .collect::<Vec<_>>();

                let answer = prompt_for_redaction_match_selection(
                    session,
                    turn,
                    call_id,
                    prompt,
                    "exclusions_blocklist_match",
                    options,
                )
                .await?;

                candidates
                    .into_iter()
                    .find(|summary| redaction_match_label(summary, reveal_secret_matches) == answer)
            }?;

            Some(RedactionDecision::AddBlocklist(selected.value))
        }
        RedactionPromptAnswer::RevealMatches | RedactionPromptAnswer::HideMatches => None,
    }
}

async fn resolve_redaction_decision(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    context_label: &str,
    original: String,
    sanitized: String,
    mut report: crate::content_gateway::ScanReport,
) -> (String, crate::content_gateway::ScanReport) {
    let Some(decision) =
        maybe_prompt_for_redaction(session, turn, call_id, context_label, &report).await
    else {
        return (sanitized, report);
    };

    match decision {
        RedactionDecision::AllowOnce => (original, crate::content_gateway::ScanReport::safe()),
        RedactionDecision::AllowForSession => {
            crate::content_gateway::remember_safe_report_matches_for_epoch(
                &session.content_gateway_cache,
                &report,
                turn.sensitive_paths.ignore_epoch(),
            );
            session
                .content_gateway_cache
                .remember_safe_text_for_epoch(&original, turn.sensitive_paths.ignore_epoch());
            (original, crate::content_gateway::ScanReport::safe())
        }
        RedactionDecision::Redact => {
            if report.redacted || report.blocked || report.matches.is_empty() {
                return (sanitized, report);
            }

            let mut redact_cfg =
                crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion);
            redact_cfg.on_match = crate::config::types::ExclusionOnMatch::Redact;
            let redact_gateway = crate::content_gateway::ContentGateway::new(redact_cfg);
            let redact_cache = crate::content_gateway::GatewayCache::new();
            let epoch = turn.sensitive_paths.ignore_epoch();

            redact_gateway.scan_text(&original, &turn.sensitive_paths, &redact_cache, epoch)
        }
        RedactionDecision::Block => {
            report.redacted = false;
            report.blocked = true;
            ("[BLOCKED]".to_string(), report)
        }
        RedactionDecision::AddAllowlistLiteral(value) => {
            session
                .add_exclusion_secret_pattern(regex::escape(&value), true)
                .await;
            (original, crate::content_gateway::ScanReport::safe())
        }
        RedactionDecision::AddAllowlistRegex(value) => {
            session.add_exclusion_secret_pattern(value, true).await;
            (original, crate::content_gateway::ScanReport::safe())
        }
        RedactionDecision::AddBlocklist(value) => {
            session.add_exclusion_secret_pattern(value, false).await;
            report.redacted = false;
            report.blocked = true;
            ("[BLOCKED]".to_string(), report)
        }
    }
}

async fn enforce_sensitive_content_gateway(
    output: ToolOutput,
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    tool_name: &str,
    call_id: &str,
) -> ToolOutput {
    let epoch = turn.sensitive_paths.ignore_epoch();

    match output {
        ToolOutput::Function {
            body,
            mut success,
            provenance,
        } => {
            let mut gateway_cfg =
                crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion);
            if is_trusted_local_code_output(&provenance) {
                gateway_cfg.secret_patterns = false;
            }
            let gateway = crate::content_gateway::ContentGateway::new(gateway_cfg);
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
            let context_label = format!("{tool_name} output");

            let record_report = |report: &crate::content_gateway::ScanReport| {
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
            };

            let body = match body {
                FunctionCallOutputBody::Text(content) => {
                    let original_content = content;
                    let (sanitized, report) = gateway.scan_text(
                        &original_content,
                        &turn.sensitive_paths,
                        &session.content_gateway_cache,
                        epoch,
                    );
                    let (content, report) = resolve_redaction_decision(
                        session,
                        turn,
                        call_id,
                        &context_label,
                        original_content.clone(),
                        sanitized,
                        report,
                    )
                    .await;
                    if should_log && (report.redacted || report.blocked) {
                        crate::exclusion_log::log_redaction_event(
                            &log_context,
                            &report,
                            &original_content,
                            &content,
                        );
                    }
                    record_report(&report);
                    if report.redacted {
                        success = Some(false);
                    }
                    FunctionCallOutputBody::Text(content)
                }
                FunctionCallOutputBody::ContentItems(mut items) => {
                    for item in &mut items {
                        if let FunctionCallOutputContentItem::InputText { text } = item {
                            let original_text = text.clone();
                            let (sanitized, report) = gateway.scan_text(
                                &original_text,
                                &turn.sensitive_paths,
                                &session.content_gateway_cache,
                                epoch,
                            );
                            let (next, report) = resolve_redaction_decision(
                                session,
                                turn,
                                call_id,
                                &context_label,
                                original_text.clone(),
                                sanitized,
                                report,
                            )
                            .await;
                            *text = next;
                            if should_log && (report.redacted || report.blocked) {
                                crate::exclusion_log::log_redaction_event(
                                    &log_context,
                                    &report,
                                    &original_text,
                                    text.as_str(),
                                );
                            }
                            record_report(&report);
                            if report.redacted {
                                success = Some(false);
                            }
                        }
                    }
                    FunctionCallOutputBody::ContentItems(items)
                }
            };

            ToolOutput::Function {
                body,
                success,
                provenance,
            }
        }
        ToolOutput::Mcp { result, provenance } => {
            let gateway = crate::content_gateway::ContentGateway::new(
                crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion),
            );
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

            fn scan_json_value(
                value: &mut serde_json::Value,
                scan_string: &mut impl FnMut(&mut String),
            ) {
                match value {
                    serde_json::Value::String(s) => scan_string(s),
                    serde_json::Value::Array(items) => {
                        for item in items {
                            scan_json_value(item, scan_string);
                        }
                    }
                    serde_json::Value::Object(map) => {
                        for value in map.values_mut() {
                            scan_json_value(value, scan_string);
                        }
                    }
                    serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_) => {}
                }
            }

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

                for block in &mut ok.content {
                    scan_json_value(block, &mut scan_string);
                }
                if let Some(structured_content) = &mut ok.structured_content {
                    scan_json_value(structured_content, &mut scan_string);
                }
                if let Some(meta) = &mut ok.meta {
                    scan_json_value(meta, &mut scan_string);
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

fn is_trusted_local_code_output(provenance: &ToolProvenance) -> bool {
    let ToolProvenance::Filesystem { path } = provenance else {
        return false;
    };
    let Some(extension) = path.extension().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "h"
            | "hpp"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "kts"
            | "m"
            | "mm"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "sql"
            | "swift"
            | "toml"
            | "ts"
            | "tsx"
            | "yaml"
            | "yml"
            | "zsh"
    )
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
            body: FunctionCallOutputBody::Text(message),
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
    use codex_protocol::config_types::ModeKind;
    use pretty_assertions::assert_eq;

    fn unattested_output() -> ToolOutput {
        ToolOutput::Function {
            body: FunctionCallOutputBody::Text("payload".to_string()),
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
            body: FunctionCallOutputBody::Text("payload".to_string()),
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
    fn trusted_local_code_output_matches_only_filesystem_code_extensions() {
        let trusted = ToolProvenance::Filesystem {
            path: std::path::PathBuf::from("/tmp/src/main.rs"),
        };
        assert_eq!(true, super::is_trusted_local_code_output(&trusted));

        let markdown = ToolProvenance::Filesystem {
            path: std::path::PathBuf::from("/tmp/docs/readme.md"),
        };
        assert_eq!(false, super::is_trusted_local_code_output(&markdown));

        let shell = ToolProvenance::Shell {
            cwd: std::path::PathBuf::from("/tmp"),
        };
        assert_eq!(false, super::is_trusted_local_code_output(&shell));
    }

    #[test]
    fn block_unattested_output_replaces_payload_with_policy_message() {
        let output = unattested_output();
        let blocked = super::block_unattested_output(output);
        match blocked {
            ToolOutput::Function {
                body: FunctionCallOutputBody::Text(content),
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
                body: FunctionCallOutputBody::Text(content),
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
                body: FunctionCallOutputBody::Text(content),
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
                body: FunctionCallOutputBody::Text(content),
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

    #[test]
    fn parse_redaction_prompt_answer_maps_answers() {
        assert!(matches!(
            super::parse_redaction_prompt_answer("Allow once"),
            Some(super::RedactionPromptAnswer::AllowOnce)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Allow for this session"),
            Some(super::RedactionPromptAnswer::AllowForSession)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Redact"),
            Some(super::RedactionPromptAnswer::Redact)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Block"),
            Some(super::RedactionPromptAnswer::Block)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Add to allowlist"),
            Some(super::RedactionPromptAnswer::AddToAllowlist)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Add to blocklist"),
            Some(super::RedactionPromptAnswer::AddToBlocklist)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Reveal matched values"),
            Some(super::RedactionPromptAnswer::RevealMatches)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Hide matched values"),
            Some(super::RedactionPromptAnswer::HideMatches)
        ));
        assert_eq!(
            super::parse_redaction_prompt_answer("unknown").is_none(),
            true
        );
    }

    #[test]
    fn format_redaction_matches_returns_summary() {
        let report = crate::content_gateway::ScanReport {
            layers: Vec::new(),
            redacted: true,
            blocked: false,
            reasons: vec![crate::content_gateway::RedactionReason::SecretPattern],
            matches: vec![crate::content_gateway::RedactionMatch {
                reason: crate::content_gateway::RedactionReason::SecretPattern,
                value: "token_abc123".to_string(),
            }],
        };

        let summary = super::format_redaction_matches(&report, "L2-output_sanitization", false);
        assert_eq!(
            summary,
            Some(
                "Matched content (L2-output_sanitization):\n- [REDACTED toke...c123 sha256:424fdc9e] (reason: Secret pattern)"
                    .to_string(),
            )
        );
    }

    #[test]
    fn format_redaction_matches_can_reveal_secret_values() {
        let report = crate::content_gateway::ScanReport {
            layers: Vec::new(),
            redacted: true,
            blocked: false,
            reasons: vec![crate::content_gateway::RedactionReason::SecretPattern],
            matches: vec![crate::content_gateway::RedactionMatch {
                reason: crate::content_gateway::RedactionReason::SecretPattern,
                value: "token_abc123".to_string(),
            }],
        };

        let summary = super::format_redaction_matches(&report, "L2-output_sanitization", true);
        assert_eq!(
            summary,
            Some(
                "Matched content (L2-output_sanitization):\n- token_abc123 (sha256:424fdc9e) (reason: Secret pattern)"
                    .to_string(),
            )
        );
    }

    #[test]
    fn plan_mode_blocks_file_mutation_tools_and_allows_read_only_tools() {
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "apply_patch").is_some(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "mcp__filesystem__write_file")
                .is_some(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "mcp__filesystem__edit_file")
                .is_some(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "read_file").is_none(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Default, "apply_patch").is_none(),
            true
        );
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

fn plan_mode_tool_block_message(mode: ModeKind, tool_name: &str) -> Option<String> {
    if mode != ModeKind::Plan || !is_plan_mode_file_mutation_tool(tool_name) {
        return None;
    }

    Some(format!(
        "`{tool_name}` is blocked in Plan mode because it can mutate files. Switch to Default mode to run file edits."
    ))
}

fn is_plan_mode_file_mutation_tool(tool_name: &str) -> bool {
    let trailing = tool_name
        .rsplit_once("__")
        .map_or(tool_name, |(_, suffix)| suffix);
    let canonical = trailing
        .rsplit_once('/')
        .map_or(trailing, |(_, suffix)| suffix);
    matches!(canonical, "apply_patch" | "write_file" | "edit_file")
}
fn sandbox_policy_tag(policy: &SandboxPolicy) -> &'static str {
    match policy {
        SandboxPolicy::ReadOnly { .. } => "read-only",
        SandboxPolicy::WorkspaceWrite { .. } => "workspace-write",
        SandboxPolicy::DangerFullAccess => "danger-full-access",
        SandboxPolicy::ExternalSandbox { .. } => "external-sandbox",
    }
}

// Hooks use a separate wire-facing input type so hook payload JSON stays stable
// and decoupled from core's internal tool runtime representation.
impl From<&ToolPayload> for HookToolInput {
    fn from(payload: &ToolPayload) -> Self {
        match payload {
            ToolPayload::Function { arguments } => HookToolInput::Function {
                arguments: arguments.clone(),
            },
            ToolPayload::Custom { input } => HookToolInput::Custom {
                input: input.clone(),
            },
            ToolPayload::LocalShell { params } => HookToolInput::LocalShell {
                params: HookToolInputLocalShell {
                    command: params.command.clone(),
                    workdir: params.workdir.clone(),
                    timeout_ms: params.timeout_ms,
                    sandbox_permissions: params.sandbox_permissions,
                    prefix_rule: params.prefix_rule.clone(),
                    justification: params.justification.clone(),
                },
            },
            ToolPayload::Mcp {
                server,
                tool,
                raw_arguments,
            } => HookToolInput::Mcp {
                server: server.clone(),
                tool: tool.clone(),
                arguments: raw_arguments.clone(),
            },
        }
    }
}

fn hook_tool_kind(tool_input: &HookToolInput) -> HookToolKind {
    match tool_input {
        HookToolInput::Function { .. } => HookToolKind::Function,
        HookToolInput::Custom { .. } => HookToolKind::Custom,
        HookToolInput::LocalShell { .. } => HookToolKind::LocalShell,
        HookToolInput::Mcp { .. } => HookToolKind::Mcp,
    }
}

struct AfterToolUseHookDispatch<'a> {
    invocation: &'a ToolInvocation,
    output_preview: String,
    success: bool,
    executed: bool,
    duration: Duration,
    mutating: bool,
}

async fn dispatch_after_tool_use_hook(dispatch: AfterToolUseHookDispatch<'_>) {
    let AfterToolUseHookDispatch { invocation, .. } = dispatch;
    let session = invocation.session.as_ref();
    let turn = invocation.turn.as_ref();
    let tool_input = HookToolInput::from(&invocation.payload);
    session
        .hooks()
        .dispatch(HookPayload {
            session_id: session.conversation_id,
            cwd: turn.cwd.clone(),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterToolUse {
                event: HookEventAfterToolUse {
                    turn_id: turn.sub_id.clone(),
                    call_id: invocation.call_id.clone(),
                    tool_name: invocation.tool_name.clone(),
                    tool_kind: hook_tool_kind(&tool_input),
                    tool_input,
                    executed: dispatch.executed,
                    success: dispatch.success,
                    duration_ms: u64::try_from(dispatch.duration.as_millis()).unwrap_or(u64::MAX),
                    mutating: dispatch.mutating,
                    sandbox: sandbox_tag(&turn.sandbox_policy, turn.windows_sandbox_level)
                        .to_string(),
                    sandbox_policy: sandbox_policy_tag(&turn.sandbox_policy).to_string(),
                    output_preview: dispatch.output_preview.clone(),
                },
            },
        })
        .await;
}
