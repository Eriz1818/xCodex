use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tree_sitter_highlight::Highlight;
use tree_sitter_highlight::HighlightConfiguration;
use tree_sitter_highlight::HighlightEvent;
use tree_sitter_highlight::Highlighter;

static SYNTAX_HIGHLIGHTING_ENABLED: AtomicBool = AtomicBool::new(true);

pub(crate) fn set_syntax_highlighting_enabled(enabled: bool) {
    SYNTAX_HIGHLIGHTING_ENABLED.store(enabled, Ordering::Relaxed);
}

pub(crate) fn syntax_highlighting_enabled() -> bool {
    SYNTAX_HIGHLIGHTING_ENABLED.load(Ordering::Relaxed)
}

const GENERIC_HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "field",
    "function",
    "function.builtin",
    "keyword",
    "label",
    "method",
    "module",
    "namespace",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.escape",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

// Ref: https://github.com/tree-sitter/tree-sitter-bash/blob/master/queries/highlights.scm
#[derive(Copy, Clone)]
enum BashHighlight {
    Comment,
    Constant,
    Embedded,
    Function,
    Keyword,
    Number,
    Operator,
    Property,
    String,
}

impl BashHighlight {
    const ALL: [Self; 9] = [
        Self::Comment,
        Self::Constant,
        Self::Embedded,
        Self::Function,
        Self::Keyword,
        Self::Number,
        Self::Operator,
        Self::Property,
        Self::String,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Constant => "constant",
            Self::Embedded => "embedded",
            Self::Function => "function",
            Self::Keyword => "keyword",
            Self::Number => "number",
            Self::Operator => "operator",
            Self::Property => "property",
            Self::String => "string",
        }
    }

    fn style(self) -> Style {
        match self {
            Self::Comment => crate::theme::code_comment_style(),
            Self::Constant => crate::theme::code_constant_style(),
            Self::Embedded => crate::theme::code_embedded_style(),
            Self::Function => crate::theme::code_function_style(),
            Self::Keyword => crate::theme::code_keyword_style(),
            Self::Number => crate::theme::code_number_style(),
            Self::Operator => crate::theme::code_operator_style(),
            Self::Property => crate::theme::code_property_style(),
            Self::String => crate::theme::code_string_style(),
        }
    }
}

static HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();

fn highlight_names() -> &'static [&'static str] {
    static NAMES: OnceLock<[&'static str; BashHighlight::ALL.len()]> = OnceLock::new();
    NAMES
        .get_or_init(|| BashHighlight::ALL.map(BashHighlight::as_str))
        .as_slice()
}

fn highlight_config() -> &'static HighlightConfiguration {
    HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_bash::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        )
        .expect("load bash highlight query");
        config.configure(highlight_names());
        config
    })
}

fn highlight_for(highlight: Highlight) -> BashHighlight {
    BashHighlight::ALL[highlight.0]
}

fn style_for_generic_capture(name: &str) -> Style {
    let base = name.split('.').next().unwrap_or(name);
    // Prefer making macro-ish items read like keywords/builtins.
    if base == "function" && name.contains(".macro") {
        return crate::theme::code_macro_style();
    }
    if base == "macro" {
        return crate::theme::code_macro_style();
    }

    match base {
        "attribute" => crate::theme::code_attribute_style(),
        "comment" => crate::theme::code_comment_style(),
        "string" | "character" => crate::theme::code_string_style(),
        "number" | "float" => crate::theme::code_number_style(),
        "keyword" => crate::theme::code_keyword_style(),
        "function" | "method" | "constructor" => crate::theme::code_function_style(),
        "type" => crate::theme::code_type_style(),
        "constant" => crate::theme::code_constant_style(),
        "variable" => crate::theme::code_variable_style(),
        "property" | "field" => crate::theme::code_property_style(),
        "tag" => crate::theme::code_tag_style(),
        "module" | "namespace" => crate::theme::code_module_style(),
        "label" => crate::theme::code_label_style(),
        "operator" => crate::theme::code_operator_style(),
        "punctuation" => crate::theme::code_punctuation_style(),
        "embedded" => crate::theme::code_embedded_style(),
        _ => Style::default(),
    }
}

fn push_segment(lines: &mut Vec<Line<'static>>, segment: &str, style: Option<Style>) {
    for (i, part) in segment.split('\n').enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        if part.is_empty() {
            continue;
        }
        let span = match style {
            Some(style) => Span::styled(part.to_string(), style),
            None => part.to_string().into(),
        };
        if let Some(last) = lines.last_mut() {
            last.spans.push(span);
        }
    }
}

fn highlight_generic_to_lines(config: &HighlightConfiguration, script: &str) -> Vec<Line<'static>> {
    let mut highlighter = Highlighter::new();
    let iterator = match highlighter.highlight(config, script.as_bytes(), None, |_| None) {
        Ok(iter) => iter,
        Err(_) => return vec![script.to_string().into()],
    };

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    let mut highlight_stack: Vec<Highlight> = Vec::new();

    for event in iterator {
        match event {
            Ok(HighlightEvent::HighlightStart(highlight)) => highlight_stack.push(highlight),
            Ok(HighlightEvent::HighlightEnd) => {
                highlight_stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                if start == end {
                    continue;
                }
                let style = highlight_stack
                    .last()
                    .and_then(|h| GENERIC_HIGHLIGHT_NAMES.get(h.0).copied())
                    .map(style_for_generic_capture);
                push_segment(&mut lines, &script[start..end], style);
            }
            Err(_) => return vec![script.to_string().into()],
        }
    }

    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

static RUST_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static PYTHON_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static JAVASCRIPT_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static TYPESCRIPT_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static RUBY_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static GO_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static C_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static CPP_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static JAVA_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static HTML_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static CSS_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static JSON_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static YAML_HIGHLIGHT_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
const TYPESCRIPT_EXTRA_HIGHLIGHTS: &str = r#"
(comment) @comment

[
  (string)
  (template_string)
] @string

(regex) @string.special
(number) @number
"#;

fn rust_highlight_config() -> &'static HighlightConfiguration {
    RUST_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_rust::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load rust highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn python_highlight_config() -> &'static HighlightConfiguration {
    PYTHON_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_python::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load python highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn javascript_highlight_config() -> &'static HighlightConfiguration {
    JAVASCRIPT_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_javascript::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            "",
            "",
        )
        .expect("load javascript highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn typescript_highlight_config() -> &'static HighlightConfiguration {
    TYPESCRIPT_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let query = format!(
            "{}\n{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            TYPESCRIPT_EXTRA_HIGHLIGHTS
        );
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(language, "typescript", &query, "", "")
            .expect("load typescript highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn ruby_highlight_config() -> &'static HighlightConfiguration {
    RUBY_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_ruby::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "ruby",
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load ruby highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn go_highlight_config() -> &'static HighlightConfiguration {
    GO_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_go::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config =
            HighlightConfiguration::new(language, "go", tree_sitter_go::HIGHLIGHTS_QUERY, "", "")
                .expect("load go highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn c_highlight_config() -> &'static HighlightConfiguration {
    C_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_c::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config =
            HighlightConfiguration::new(language, "c", tree_sitter_c::HIGHLIGHT_QUERY, "", "")
                .expect("load c highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn cpp_highlight_config() -> &'static HighlightConfiguration {
    CPP_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_cpp::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config =
            HighlightConfiguration::new(language, "cpp", tree_sitter_cpp::HIGHLIGHT_QUERY, "", "")
                .expect("load c++ highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn java_highlight_config() -> &'static HighlightConfiguration {
    JAVA_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_java::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load java highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn html_highlight_config() -> &'static HighlightConfiguration {
    HTML_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_html::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load html highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn css_highlight_config() -> &'static HighlightConfiguration {
    CSS_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_css::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config =
            HighlightConfiguration::new(language, "css", tree_sitter_css::HIGHLIGHTS_QUERY, "", "")
                .expect("load css highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn json_highlight_config() -> &'static HighlightConfiguration {
    JSON_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_json::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load json highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn yaml_highlight_config() -> &'static HighlightConfiguration {
    YAML_HIGHLIGHT_CONFIG.get_or_init(|| {
        let language = tree_sitter_yaml::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load yaml highlight query");
        config.configure(GENERIC_HIGHLIGHT_NAMES);
        config
    })
}

fn highlight_config_for(lang: &str) -> Option<&'static HighlightConfiguration> {
    match lang {
        "rust" | "rs" => Some(rust_highlight_config()),
        "python" | "py" => Some(python_highlight_config()),
        "javascript" | "js" => Some(javascript_highlight_config()),
        "typescript" | "ts" => Some(typescript_highlight_config()),
        "ruby" | "rb" => Some(ruby_highlight_config()),
        "go" | "golang" => Some(go_highlight_config()),
        "c" => Some(c_highlight_config()),
        "cpp" | "c++" | "cxx" | "cc" => Some(cpp_highlight_config()),
        "java" => Some(java_highlight_config()),
        "html" | "htm" => Some(html_highlight_config()),
        "css" => Some(css_highlight_config()),
        "json" => Some(json_highlight_config()),
        "yaml" | "yml" => Some(yaml_highlight_config()),
        _ => None,
    }
}

pub(crate) fn supports_highlighting(lang: &str) -> bool {
    match lang.trim().to_ascii_lowercase().as_str() {
        "bash" | "sh" | "zsh" => true,
        other => highlight_config_for(other).is_some(),
    }
}

pub(crate) fn highlight_code_block_to_lines(lang: Option<&str>, code: &str) -> Vec<Line<'static>> {
    fn plain_lines(code: &str) -> Vec<Line<'static>> {
        if code.is_empty() {
            vec![Line::from("")]
        } else {
            code.lines().map(|l| Line::from(l.to_string())).collect()
        }
    }

    let Some(lang) = lang else {
        return plain_lines(code);
    };

    let lang = lang.trim().to_ascii_lowercase();
    match lang.as_str() {
        "bash" | "sh" | "zsh" => highlight_bash_to_lines(code),
        _ => highlight_config_for(lang.as_str())
            .map(|config| highlight_generic_to_lines(config, code))
            .unwrap_or_else(|| plain_lines(code)),
    }
}

#[derive(Clone, Copy)]
struct HeredocSpan {
    start_line: usize,
    end_line: usize,
    lang: &'static str,
}

fn heredoc_lang_for(tag: &str) -> Option<&'static str> {
    match tag.trim().to_ascii_uppercase().as_str() {
        "PY" | "PYTHON" => Some("python"),
        "JS" | "JAVASCRIPT" => Some("javascript"),
        "TS" | "TYPESCRIPT" => Some("typescript"),
        "SH" | "BASH" | "ZSH" => Some("bash"),
        "RB" | "RUBY" => Some("ruby"),
        "GO" | "GOLANG" => Some("go"),
        "C" => Some("c"),
        "CPP" | "CXX" | "C++" | "CC" => Some("cpp"),
        "JAVA" => Some("java"),
        "HTML" | "HTM" => Some("html"),
        "CSS" => Some("css"),
        "JSON" => Some("json"),
        "YAML" | "YML" => Some("yaml"),
        _ => None,
    }
}

fn parse_heredoc_spans(script: &str) -> Vec<HeredocSpan> {
    let mut spans = Vec::new();
    let lines: Vec<&str> = script.split('\n').collect();
    let mut idx = 0;

    while idx < lines.len() {
        let line = lines[idx];
        let Some(heredoc_pos) = line.find("<<") else {
            idx += 1;
            continue;
        };

        let mut rest = &line[heredoc_pos + 2..];
        let mut strip_tabs = false;
        if let Some(after) = rest.strip_prefix('-') {
            strip_tabs = true;
            rest = after;
        }
        rest = rest.trim_start();
        if rest.is_empty() {
            idx += 1;
            continue;
        }

        let (delimiter, _) =
            if let Some(quote) = rest.chars().next().filter(|c| *c == '\'' || *c == '"') {
                let after_quote = &rest[1..];
                if let Some(end) = after_quote.find(quote) {
                    (&after_quote[..end], &after_quote[end + 1..])
                } else {
                    idx += 1;
                    continue;
                }
            } else {
                let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
                (&rest[..end], &rest[end..])
            };

        let Some(lang) = heredoc_lang_for(delimiter) else {
            idx += 1;
            continue;
        };

        let mut end_idx = None;
        for (offset, body_line) in lines.iter().enumerate().skip(idx + 1) {
            let candidate = if strip_tabs {
                body_line.trim_start_matches('\t')
            } else {
                body_line
            };
            if candidate.trim_end() == delimiter {
                end_idx = Some(offset);
                break;
            }
        }

        if let Some(end_idx) = end_idx {
            if end_idx > idx + 1 {
                spans.push(HeredocSpan {
                    start_line: idx + 1,
                    end_line: end_idx,
                    lang,
                });
            }
            idx = end_idx + 1;
        } else {
            idx += 1;
        }
    }

    spans
}

pub(crate) fn highlight_bash_with_heredoc_overrides(script: &str) -> Vec<Line<'static>> {
    let mut lines = highlight_bash_to_lines(script);
    let script_lines: Vec<&str> = script.split('\n').collect();
    let spans = parse_heredoc_spans(script);

    for span in spans {
        if span.end_line > script_lines.len() {
            continue;
        }
        let body = script_lines[span.start_line..span.end_line].join("\n");
        let highlighted = highlight_code_block_to_lines(Some(span.lang), &body);
        if highlighted.len() != span.end_line.saturating_sub(span.start_line) {
            continue;
        }
        for (idx, line) in highlighted.into_iter().enumerate() {
            if let Some(target) = lines.get_mut(span.start_line + idx) {
                *target = line;
            }
        }
    }

    lines
}

/// Convert a bash script into per-line styled content using tree-sitter's
/// bash highlight query. The highlighter is streamed so multi-line content is
/// split into `Line`s while preserving style boundaries.
pub(crate) fn highlight_bash_to_lines(script: &str) -> Vec<Line<'static>> {
    let mut highlighter = Highlighter::new();
    let iterator =
        match highlighter.highlight(highlight_config(), script.as_bytes(), None, |_| None) {
            Ok(iter) => iter,
            Err(_) => return vec![script.to_string().into()],
        };

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    let mut highlight_stack: Vec<Highlight> = Vec::new();

    for event in iterator {
        match event {
            Ok(HighlightEvent::HighlightStart(highlight)) => highlight_stack.push(highlight),
            Ok(HighlightEvent::HighlightEnd) => {
                highlight_stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                if start == end {
                    continue;
                }
                let style = highlight_stack.last().map(|h| highlight_for(*h).style());
                push_segment(&mut lines, &script[start..end], style);
            }
            Err(_) => return vec![script.to_string().into()],
        }
    }

    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Modifier;

    fn reconstructed(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|sp| sp.content.clone())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn dimmed_tokens(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|sp| sp.style.add_modifier.contains(Modifier::DIM))
            .map(|sp| sp.content.clone().into_owned())
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
            .collect()
    }

    #[test]
    fn dims_expected_bash_operators() {
        let s = "echo foo && bar || baz | qux & (echo hi)";
        let lines = highlight_bash_to_lines(s);
        assert_eq!(reconstructed(&lines), s);

        let dimmed = dimmed_tokens(&lines);
        assert!(dimmed.contains(&"&&".to_string()));
        assert!(dimmed.contains(&"|".to_string()));
        assert!(!dimmed.contains(&"echo".to_string()));
    }

    #[test]
    fn dims_redirects_and_strings() {
        let s = "echo \"hi\" > out.txt; echo 'ok'";
        let lines = highlight_bash_to_lines(s);
        assert_eq!(reconstructed(&lines), s);

        let dimmed = dimmed_tokens(&lines);
        assert!(dimmed.contains(&">".to_string()));
    }

    #[test]
    fn highlights_command_and_strings() {
        let s = "echo \"hi\"";
        let lines = highlight_bash_to_lines(s);
        let mut echo_style = None;
        let mut string_style = None;
        for span in &lines[0].spans {
            let text = span.content.as_ref();
            if text == "echo" {
                echo_style = Some(span.style);
            }
            if text == "\"hi\"" {
                string_style = Some(span.style);
            }
        }
        let echo_style = echo_style.expect("echo span missing");
        let string_style = string_style.expect("string span missing");
        assert!(echo_style.fg.is_some());
        assert!(!echo_style.add_modifier.contains(Modifier::DIM));
        assert_eq!(string_style.fg, crate::theme::code_string_style().fg);
    }

    #[test]
    fn highlights_heredoc_body_as_string() {
        let s = "cat <<EOF\nheredoc body\nEOF";
        let lines = highlight_bash_to_lines(s);
        let body_line = &lines[1];
        let mut body_style = None;
        for span in &body_line.spans {
            if span.content.as_ref() == "heredoc body" {
                body_style = Some(span.style);
            }
        }
        let body_style = body_style.expect("missing heredoc span");
        assert_eq!(body_style.fg, crate::theme::code_string_style().fg);
    }

    #[test]
    fn highlights_heredoc_body_with_language_override() {
        let s = "python3 - <<'PY'\nfor i in range(3):\n  print(i)\nPY";
        let lines = highlight_bash_with_heredoc_overrides(s);
        let body_line = &lines[1];
        let mut keyword_style = None;
        for span in &body_line.spans {
            if span.content.as_ref() == "for" {
                keyword_style = Some(span.style);
            }
        }
        let keyword_style = keyword_style.expect("missing keyword span");
        assert_eq!(keyword_style.fg, crate::theme::code_keyword_style().fg);
    }
}
