use crate::custom_prompts::CustomPrompt;
use crate::custom_prompts::PROMPTS_CMD_PREFIX;
use codex_protocol::user_input::ByteRange;
use codex_protocol::user_input::TextElement;
use lazy_static::lazy_static;
use regex_lite::Regex;
use shlex::Shlex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;

lazy_static! {
    static ref PROMPT_ARG_REGEX: Regex =
        Regex::new(r"\$[A-Z][A-Z0-9_]*").unwrap_or_else(|_| std::process::abort());
}

pub(crate) struct ExpandedPrompt {
    pub(crate) text: String,
    pub(crate) text_elements: Vec<TextElement>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PromptExpansionError {
    InvalidArg { token: String },
    MissingRequiredArgs { args: Vec<String> },
}

#[derive(Clone, Debug)]
struct NamedArgValue {
    value: String,
    source_range: Range<usize>,
}

/// Parse a first-line slash command of the form `/name <rest>`.
/// Returns `(name, rest_after_name, rest_offset)` if the line begins with `/`
/// and contains a non-empty name; otherwise returns `None`.
///
/// `rest_offset` is the byte index into the original line where `rest_after_name`
/// starts after trimming leading whitespace (so `line[rest_offset..] == rest_after_name`).
pub fn parse_slash_name(line: &str) -> Option<(&str, &str, usize)> {
    let stripped = line.strip_prefix('/')?;
    let mut name_end_in_stripped = stripped.len();
    for (idx, ch) in stripped.char_indices() {
        if ch.is_whitespace() {
            name_end_in_stripped = idx;
            break;
        }
    }
    let name = &stripped[..name_end_in_stripped];
    if name.is_empty() {
        return None;
    }
    let rest_untrimmed = &stripped[name_end_in_stripped..];
    let rest = rest_untrimmed.trim_start();
    let rest_start_in_stripped = name_end_in_stripped + (rest_untrimmed.len() - rest.len());
    let rest_offset = rest_start_in_stripped + 1;
    Some((name, rest, rest_offset))
}

pub fn parse_positional_args(rest: &str) -> Vec<String> {
    Shlex::new(rest).collect()
}

pub fn prompt_argument_names(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for m in PROMPT_ARG_REGEX.find_iter(content) {
        if m.start() > 0 && content.as_bytes()[m.start() - 1] == b'$' {
            continue;
        }
        let name = &content[m.start() + 1..m.end()];
        if name == "ARGUMENTS" {
            continue;
        }
        let name = name.to_string();
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }
    names
}

pub fn prompt_has_numeric_placeholders(content: &str) -> bool {
    if content.contains("$ARGUMENTS") {
        return true;
    }
    let bytes = content.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$' && (b'1'..=b'9').contains(&bytes[i + 1]) {
            return true;
        }
        i += 1;
    }
    false
}

pub fn extract_positional_args_for_prompt_line(line: &str, prompt_name: &str) -> Vec<String> {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return Vec::new();
    };
    let Some(after_prefix) = rest.strip_prefix(&format!("{PROMPTS_CMD_PREFIX}:")) else {
        return Vec::new();
    };
    let mut parts = after_prefix.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    if cmd != prompt_name {
        return Vec::new();
    }
    let args_str = parts.next().unwrap_or("").trim();
    if args_str.is_empty() {
        return Vec::new();
    }
    parse_positional_args(args_str)
}

pub fn expand_if_numeric_with_positional_args(
    prompt: &CustomPrompt,
    first_line: &str,
    _text_elements: &[TextElement],
) -> Option<ExpandedPrompt> {
    if !prompt_argument_names(&prompt.content).is_empty() {
        return None;
    }
    if !prompt_has_numeric_placeholders(&prompt.content) {
        return None;
    }
    let args = extract_positional_args_for_prompt_line(first_line, &prompt.name);
    if args.is_empty() {
        return None;
    }
    Some(ExpandedPrompt {
        text: expand_numeric_placeholders(&prompt.content, &args),
        text_elements: Vec::new(),
    })
}

pub(crate) fn expand_custom_prompt_submission(
    prompt: &CustomPrompt,
    first_line: &str,
    text_elements: &[TextElement],
) -> Result<Option<ExpandedPrompt>, PromptExpansionError> {
    let Some((name, rest, rest_offset)) = parse_slash_name(first_line) else {
        return Ok(None);
    };
    let Some(prompt_name) = name.strip_prefix(&format!("{PROMPTS_CMD_PREFIX}:")) else {
        return Ok(None);
    };
    if prompt_name != prompt.name {
        return Ok(None);
    }

    let named_args = prompt_argument_names(&prompt.content);
    if named_args.is_empty() {
        if prompt_has_numeric_placeholders(&prompt.content) {
            return Ok(expand_if_numeric_with_positional_args(
                prompt,
                first_line,
                text_elements,
            ));
        }
        return Ok(Some(ExpandedPrompt {
            text: prompt.content.clone(),
            text_elements: Vec::new(),
        }));
    }

    let provided_args = parse_named_arg_values(rest, rest_offset, text_elements)?;
    let missing_args = named_args
        .iter()
        .filter(|arg| !provided_args.contains_key(arg.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_args.is_empty() {
        return Err(PromptExpansionError::MissingRequiredArgs { args: missing_args });
    }

    Ok(Some(expand_named_placeholders(
        &prompt.content,
        &provided_args,
        text_elements,
    )))
}

fn parse_named_arg_values(
    rest: &str,
    rest_offset: usize,
    text_elements: &[TextElement],
) -> Result<HashMap<String, NamedArgValue>, PromptExpansionError> {
    let mut args = HashMap::new();
    let bytes = rest.as_bytes();
    let mut i = 0usize;

    while i < rest.len() {
        while i < rest.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= rest.len() {
            break;
        }

        let token_start = i;
        while i < rest.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= rest.len() || bytes[i] != b'=' || i == token_start {
            let token_end = rest[i..]
                .find(char::is_whitespace)
                .map_or(rest.len(), |offset| i + offset);
            return Err(PromptExpansionError::InvalidArg {
                token: rest[token_start..token_end].to_string(),
            });
        }

        let key = rest[token_start..i].to_string();
        i += 1;

        let (value_start, value_end) = if i < rest.len() && matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            i += 1;
            let value_start = i;
            while i < rest.len() && bytes[i] != quote {
                i += 1;
            }
            let value_end = i;
            if i < rest.len() {
                i += 1;
            }
            (value_start, value_end)
        } else if let Some(element) = text_elements
            .iter()
            .find(|element| element.byte_range.start == rest_offset + i)
        {
            let value_start = i;
            let value_end = element.byte_range.end.saturating_sub(rest_offset);
            i = value_end.min(rest.len());
            (value_start, i)
        } else {
            let value_start = i;
            while i < rest.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            (value_start, i)
        };

        let source_range = rest_offset + value_start..rest_offset + value_end;
        let value = rest[value_start..value_end].trim_end().to_string();
        args.insert(
            key,
            NamedArgValue {
                value,
                source_range,
            },
        );
    }

    Ok(args)
}

fn expand_named_placeholders(
    content: &str,
    args: &HashMap<String, NamedArgValue>,
    text_elements: &[TextElement],
) -> ExpandedPrompt {
    let mut out = String::with_capacity(content.len());
    let mut out_elements = Vec::new();
    let mut i = 0;

    while let Some(off) = content[i..].find('$') {
        let j = i + off;
        out.push_str(&content[i..j]);
        let rest = &content[j..];
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b'$' {
            out.push_str("$$");
            i = j + 2;
            continue;
        }

        let name_end = rest[1..]
            .find(|ch: char| !(ch == '_' || ch.is_ascii_uppercase() || ch.is_ascii_digit()))
            .map_or(rest.len(), |offset| offset + 1);
        if name_end <= 1 {
            out.push('$');
            i = j + 1;
            continue;
        }

        let name = &rest[1..name_end];
        if let Some(arg) = args.get(name) {
            let out_start = out.len();
            out.push_str(&arg.value);
            rebase_text_elements_for_arg(arg, out_start, text_elements, &mut out_elements);
            i = j + name_end;
        } else {
            out.push_str(&rest[..name_end]);
            i = j + name_end;
        }
    }

    out.push_str(&content[i..]);
    ExpandedPrompt {
        text: out,
        text_elements: out_elements,
    }
}

fn rebase_text_elements_for_arg(
    arg: &NamedArgValue,
    out_start: usize,
    text_elements: &[TextElement],
    out_elements: &mut Vec<TextElement>,
) {
    for element in text_elements {
        if element.byte_range.start < arg.source_range.start
            || element.byte_range.end > arg.source_range.end
        {
            continue;
        }
        let start = out_start + element.byte_range.start - arg.source_range.start;
        let end = out_start + element.byte_range.end - arg.source_range.start;
        out_elements.push(element.map_range(|_| ByteRange { start, end }));
    }
}

pub fn expand_numeric_placeholders(content: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    let mut cached_joined_args: Option<String> = None;
    while let Some(off) = content[i..].find('$') {
        let j = i + off;
        out.push_str(&content[i..j]);
        let rest = &content[j..];
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 {
            match bytes[1] {
                b'$' => {
                    out.push_str("$$");
                    i = j + 2;
                    continue;
                }
                b'1'..=b'9' => {
                    let idx = (bytes[1] - b'1') as usize;
                    if let Some(val) = args.get(idx) {
                        out.push_str(val);
                    }
                    i = j + 2;
                    continue;
                }
                _ => {}
            }
        }
        if rest.len() > "ARGUMENTS".len() && rest[1..].starts_with("ARGUMENTS") {
            if !args.is_empty() {
                let joined = cached_joined_args.get_or_insert_with(|| args.join(" "));
                out.push_str(joined);
            }
            i = j + 1 + "ARGUMENTS".len();
            continue;
        }
        out.push('$');
        i = j + 1;
    }
    out.push_str(&content[i..]);
    out
}

pub fn prompt_command_with_arg_placeholders(name: &str, args: &[String]) -> (String, usize) {
    let mut text = format!("/{PROMPTS_CMD_PREFIX}:{name}");
    let mut cursor: usize = text.len();
    for (i, arg) in args.iter().enumerate() {
        text.push_str(format!(" {arg}=\"\"").as_str());
        if i == 0 {
            cursor = text.len() - 1;
        }
    }
    (text, cursor)
}
