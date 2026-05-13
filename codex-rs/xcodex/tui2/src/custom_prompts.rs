use codex_protocol::models::ContentItem;
use std::path::PathBuf;

pub(crate) const PROMPTS_CMD_PREFIX: &str = "prompts";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CustomPrompt {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) description: Option<String>,
    pub(crate) content: String,
    pub(crate) argument_hint: Option<String>,
    pub(crate) text_elements: Vec<ContentItem>,
}
