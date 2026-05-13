use crate::config::types::Personality;
use crate::context_manager::estimate_reasoning_length;
use crate::error::Result;
use crate::truncate::approx_token_count;
pub use codex_api::ResponseEvent;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::LocalShellAction;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::WebSearchAction;
use codex_tools::ToolSpec;
use futures::Stream;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tokio::sync::mpsc;

/// Review thread system prompt. Edit `core/src/review_prompt.md` to customize.
pub const REVIEW_PROMPT: &str = include_str!("../review_prompt.md");

// Centralized templates for review-related user messages
pub const REVIEW_EXIT_SUCCESS_TMPL: &str = include_str!("../templates/review/exit_success.xml");
pub const REVIEW_EXIT_INTERRUPTED_TMPL: &str =
    include_str!("../templates/review/exit_interrupted.xml");

/// API request payload for a single model turn
#[derive(Default, Debug, Clone)]
pub struct Prompt {
    /// Conversation context input items.
    pub input: Vec<ResponseItem>,

    /// Tools available to the model, including additional tools sourced from
    /// external MCP servers.
    pub(crate) tools: Vec<ToolSpec>,

    /// Whether parallel tool calls are permitted for this prompt.
    pub(crate) parallel_tool_calls: bool,

    pub base_instructions: BaseInstructions,

    /// Optionally specify the personality of the model.
    pub personality: Option<Personality>,

    /// Optional the output schema for the model's response.
    pub output_schema: Option<Value>,
}

impl Prompt {
    pub(crate) fn get_formatted_input(&self) -> Vec<ResponseItem> {
        let mut input = self.input.clone();

        // when using the *Freeform* apply_patch tool specifically, tool outputs
        // should be structured text, not json. Do NOT reserialize when using
        // the Function tool - note that this differs from the check above for
        // instructions. We declare the result as a named variable for clarity.
        let is_freeform_apply_patch_tool_present = self.tools.iter().any(|tool| match tool {
            ToolSpec::Freeform(f) => f.name == "apply_patch",
            _ => false,
        });
        if is_freeform_apply_patch_tool_present {
            reserialize_shell_outputs(&mut input);
        }

        input
    }

    pub(crate) fn estimate_token_count(&self) -> i64 {
        let instructions_tokens =
            i64::try_from(approx_token_count(&self.base_instructions.text)).unwrap_or(i64::MAX);

        // IMPORTANT: this is a heuristic. Prefer counting the actual text fields that are
        // semantically visible to the model instead of serializing entire ResponseItems to JSON,
        // which can drastically overestimate due to structural keys.
        //
        // NOTE: encrypted reasoning uses `estimate_reasoning_length` (decoded bytes after overhead)
        // and is intentionally over-counted versus tokens to keep auto-compact decisions
        // conservative.
        let estimate_item_tokens = |item: &ResponseItem| -> i64 {
            let estimate_str_tokens =
                |text: &str| -> i64 { i64::try_from(approx_token_count(text)).unwrap_or(i64::MAX) };
            let estimate_content_item_tokens = |content: &ContentItem| -> i64 {
                match content {
                    ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                        estimate_str_tokens(text)
                    }
                    ContentItem::InputImage { image_url } => estimate_str_tokens(image_url),
                }
            };
            let estimate_fco_item_tokens = |content: &FunctionCallOutputContentItem| -> i64 {
                match content {
                    FunctionCallOutputContentItem::InputText { text } => estimate_str_tokens(text),
                    FunctionCallOutputContentItem::InputImage { image_url, .. } => {
                        estimate_str_tokens(image_url)
                    }
                }
            };

            match item {
                ResponseItem::GhostSnapshot { .. } => 0,
                ResponseItem::Reasoning {
                    encrypted_content: Some(content),
                    ..
                }
                | ResponseItem::Compaction {
                    encrypted_content: content,
                } => estimate_reasoning_length(content.len()) as i64,
                ResponseItem::Message { role, content, .. } => estimate_str_tokens(role)
                    .saturating_add(content.iter().fold(0i64, |acc, item| {
                        acc.saturating_add(estimate_content_item_tokens(item))
                    })),
                ResponseItem::FunctionCall {
                    name, arguments, ..
                } => estimate_str_tokens(name).saturating_add(estimate_str_tokens(arguments)),
                ResponseItem::FunctionCallOutput { output, .. } => {
                    if let Some(items) = output.content_items() {
                        items.iter().fold(0i64, |acc, item| {
                            acc.saturating_add(estimate_fco_item_tokens(item))
                        })
                    } else {
                        estimate_str_tokens(output.text_content().unwrap_or_default())
                    }
                }
                ResponseItem::CustomToolCall { name, input, .. } => {
                    estimate_str_tokens(name).saturating_add(estimate_str_tokens(input))
                }
                ResponseItem::CustomToolCallOutput { output, .. } => {
                    estimate_str_tokens(output.text_content().unwrap_or_default())
                }
                ResponseItem::LocalShellCall { action, .. } => match action {
                    LocalShellAction::Exec(exec) => {
                        let command = exec.command.join(" ");
                        let mut total = estimate_str_tokens(&command);
                        if let Some(working_directory) = exec.working_directory.as_ref() {
                            total = total.saturating_add(estimate_str_tokens(working_directory));
                        }
                        if let Some(user) = exec.user.as_ref() {
                            total = total.saturating_add(estimate_str_tokens(user));
                        }
                        if let Some(env) = exec.env.as_ref() {
                            total = env.iter().fold(total, |acc, (k, v)| {
                                acc.saturating_add(estimate_str_tokens(k))
                                    .saturating_add(estimate_str_tokens(v))
                            });
                        }
                        total
                    }
                },
                ResponseItem::WebSearchCall { action, .. } => match action.as_ref() {
                    Some(WebSearchAction::Search { query, queries }) => {
                        let query_tokens =
                            query.as_ref().map_or(0, |query| estimate_str_tokens(query));
                        let queries_tokens = queries.as_ref().map_or(0, |queries| {
                            queries.iter().fold(0i64, |acc, query| {
                                acc.saturating_add(estimate_str_tokens(query))
                            })
                        });
                        query_tokens.saturating_add(queries_tokens)
                    }
                    Some(WebSearchAction::OpenPage { url }) => {
                        url.as_ref().map_or(0, |url| estimate_str_tokens(url))
                    }
                    Some(WebSearchAction::FindInPage { url, pattern }) => url
                        .as_ref()
                        .map_or(0, |url| estimate_str_tokens(url))
                        .saturating_add(
                            pattern
                                .as_ref()
                                .map_or(0, |pattern| estimate_str_tokens(pattern)),
                        ),
                    Some(WebSearchAction::Other) | None => 0,
                },
                ResponseItem::ToolSearchCall {
                    execution,
                    arguments,
                    ..
                } => estimate_str_tokens(execution)
                    .saturating_add(estimate_str_tokens(&arguments.to_string())),
                ResponseItem::ToolSearchOutput {
                    status,
                    execution,
                    tools,
                    ..
                } => estimate_str_tokens(status)
                    .saturating_add(estimate_str_tokens(execution))
                    .saturating_add(estimate_str_tokens(
                        &Value::Array(tools.clone()).to_string(),
                    )),
                ResponseItem::ImageGenerationCall {
                    status,
                    revised_prompt,
                    result,
                    ..
                } => estimate_str_tokens(status)
                    .saturating_add(
                        revised_prompt
                            .as_ref()
                            .map_or(0, |prompt| estimate_str_tokens(prompt)),
                    )
                    .saturating_add(estimate_str_tokens(result)),
                ResponseItem::Reasoning { .. } => 0,
                ResponseItem::Other => 0,
            }
        };

        let formatted_input = self.get_formatted_input();
        let last_user_message_index = formatted_input
            .iter()
            .rposition(|item| matches!(item, ResponseItem::Message { role, .. } if role == "user"));

        let input_tokens = formatted_input
            .iter()
            .enumerate()
            .fold(0i64, |acc, (idx, item)| {
                let should_count_reasoning = last_user_message_index
                    .map(|last_user| idx < last_user)
                    .unwrap_or(true);
                acc.saturating_add(match item {
                    ResponseItem::Reasoning {
                        encrypted_content: Some(content),
                        ..
                    } if should_count_reasoning => estimate_reasoning_length(content.len()) as i64,
                    ResponseItem::Reasoning { .. } => 0,
                    _ => estimate_item_tokens(item),
                })
            });

        let tools_tokens = if self.tools.is_empty() {
            0
        } else {
            match serde_json::to_string(&self.tools) {
                Ok(serialized) => {
                    i64::try_from(approx_token_count(&serialized)).unwrap_or(i64::MAX)
                }
                Err(_) => i64::MAX,
            }
        };

        let schema_tokens =
            self.output_schema
                .as_ref()
                .map_or(0, |schema| match serde_json::to_string(schema) {
                    Ok(serialized) => {
                        i64::try_from(approx_token_count(&serialized)).unwrap_or(i64::MAX)
                    }
                    Err(_) => i64::MAX,
                });

        instructions_tokens
            .saturating_add(input_tokens)
            .saturating_add(tools_tokens)
            .saturating_add(schema_tokens)
    }
}

fn reserialize_shell_outputs(items: &mut [ResponseItem]) {
    let mut shell_call_ids: HashSet<String> = HashSet::new();

    items.iter_mut().for_each(|item| match item {
        ResponseItem::LocalShellCall { call_id, id, .. } => {
            if let Some(identifier) = call_id.clone().or_else(|| id.clone()) {
                shell_call_ids.insert(identifier);
            }
        }
        ResponseItem::CustomToolCall {
            id: _,
            status: _,
            call_id,
            name,
            input: _,
        } if name == "apply_patch" => {
            shell_call_ids.insert(call_id.clone());
        }
        ResponseItem::FunctionCall { name, call_id, .. }
            if is_shell_tool_name(name) || name == "apply_patch" =>
        {
            shell_call_ids.insert(call_id.clone());
        }
        ResponseItem::FunctionCallOutput {
            call_id, output, ..
        }
        | ResponseItem::CustomToolCallOutput {
            call_id, output, ..
        } => {
            if shell_call_ids.remove(call_id)
                && let Some(structured) = output
                    .text_content()
                    .and_then(parse_structured_shell_output)
            {
                output.body = FunctionCallOutputBody::Text(structured);
            }
        }
        _ => {}
    })
}

fn is_shell_tool_name(name: &str) -> bool {
    matches!(name, "shell" | "container.exec")
}

#[derive(Deserialize)]
struct ExecOutputJson {
    output: String,
    metadata: ExecOutputMetadataJson,
}

#[derive(Deserialize)]
struct ExecOutputMetadataJson {
    exit_code: i32,
    duration_seconds: f32,
}

fn parse_structured_shell_output(raw: &str) -> Option<String> {
    let parsed: ExecOutputJson = serde_json::from_str(raw).ok()?;
    Some(build_structured_output(&parsed))
}

fn build_structured_output(parsed: &ExecOutputJson) -> String {
    let mut sections = Vec::new();
    sections.push(format!("Exit code: {}", parsed.metadata.exit_code));
    sections.push(format!(
        "Wall time: {} seconds",
        parsed.metadata.duration_seconds
    ));

    let mut output = parsed.output.clone();
    if let Some((stripped, total_lines)) = strip_total_output_header(&parsed.output) {
        sections.push(format!("Total output lines: {total_lines}"));
        output = stripped.to_string();
    }

    sections.push("Output:".to_string());
    sections.push(output);

    sections.join("\n")
}

fn strip_total_output_header(output: &str) -> Option<(&str, u32)> {
    let after_prefix = output.strip_prefix("Total output lines: ")?;
    let (total_segment, remainder) = after_prefix.split_once('\n')?;
    let total_lines = total_segment.parse::<u32>().ok()?;
    let remainder = remainder.strip_prefix('\n').unwrap_or(remainder);
    Some((remainder, total_lines))
}

pub struct ResponseStream {
    pub(crate) rx_event: mpsc::Receiver<Result<ResponseEvent>>,
}

impl Stream for ResponseStream {
    type Item = Result<ResponseEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx_event.poll_recv(cx)
    }
}

#[cfg(test)]
#[path = "client_common_tests.rs"]
mod tests;
