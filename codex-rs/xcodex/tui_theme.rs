use crate::color::blend;
use crate::color::is_light;
use crate::terminal_palette::best_color;
use codex_core::config::Config;
use codex_core::themes::ThemeCatalog;
use codex_core::themes::ThemeColorResolved;
use codex_core::themes::ThemeVariant;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize as _;
use std::sync::OnceLock;
use std::sync::RwLock;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(crate) struct ThemeStyles {
    transcript: Style,
    transcript_bg_rgb: Option<(u8, u8, u8)>,
    composer: Style,
    user_prompt_highlight: Style,
    status: Style,
    status_ramp_fg: Option<(u8, u8, u8)>,
    status_ramp_highlight: Option<(u8, u8, u8)>,
    selection: Style,
    dim: Style,
    border: Style,
    accent: Style,
    brand: Style,
    command: Style,
    success: Style,
    warning: Style,
    error: Style,
    link: Style,
    code_keyword: Style,
    code_operator: Style,
    code_comment: Style,
    code_string: Style,
    code_number: Style,
    code_type: Style,
    code_function: Style,
    code_constant: Style,
    code_macro: Style,
    code_punctuation: Style,
    code_variable: Style,
    code_property: Style,
    code_attribute: Style,
    code_module: Style,
    code_label: Style,
    code_tag: Style,
    code_embedded: Style,
    diff_add: Style,
    diff_del: Style,
    diff_hunk: Style,
    diff_add_highlight: Style,
    diff_del_highlight: Style,
    diff_hunk_highlight: Style,
    diff_add_text: Style,
    diff_del_text: Style,
    diff_hunk_text: Style,
}

static THEME_STYLES: OnceLock<RwLock<ThemeStyles>> = OnceLock::new();
#[cfg(test)]
static THEME_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn init(config: &Config, terminal_bg: Option<(u8, u8, u8)>) {
    apply_from_config(config, terminal_bg);
}

pub(crate) fn apply_from_config(config: &Config, terminal_bg: Option<(u8, u8, u8)>) {
    let terminal_background_is_light = terminal_bg.is_some_and(is_light);
    let auto_variant = os_theme_variant();
    let terminal_fg = crate::terminal_palette::default_fg();

    let styles = match ThemeCatalog::load(config) {
        Ok(catalog) => {
            let theme =
                catalog.resolve_active(&config.xcodex.themes, auto_variant, terminal_background_is_light);
            styles_for(theme, terminal_fg, terminal_bg)
        }
        Err(_err) => fallback_styles(),
    };

    set_styles(styles);
}

pub(crate) fn preview(config: &Config, terminal_bg: Option<(u8, u8, u8)>, theme_name: &str) {
    let terminal_background_is_light = terminal_bg.is_some_and(is_light);
    let auto_variant = os_theme_variant();
    let terminal_fg = crate::terminal_palette::default_fg();

    let styles = match ThemeCatalog::load(config) {
        Ok(catalog) => {
            let theme = catalog.get(theme_name).unwrap_or_else(|| {
                catalog.resolve_active(&config.xcodex.themes, auto_variant, terminal_background_is_light)
            });
            styles_for(theme, terminal_fg, terminal_bg)
        }
        Err(_err) => fallback_styles(),
    };

    set_styles(styles);
}

pub(crate) fn preview_definition(theme: &codex_core::themes::ThemeDefinition) {
    set_styles(styles_for(theme, None, None));
}

#[cfg(test)]
pub(crate) fn test_style_guard() -> MutexGuard<'static, ()> {
    THEME_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

pub(crate) fn active_variant(config: &Config, terminal_bg: Option<(u8, u8, u8)>) -> ThemeVariant {
    let terminal_background_is_light = terminal_bg.is_some_and(is_light);
    let auto_variant = os_theme_variant();
    match config.xcodex.themes.theme_mode {
        codex_core::config::types::ThemeMode::Light => ThemeVariant::Light,
        codex_core::config::types::ThemeMode::Dark => ThemeVariant::Dark,
        codex_core::config::types::ThemeMode::Auto => auto_variant.unwrap_or({
            if terminal_background_is_light {
                ThemeVariant::Light
            } else {
                ThemeVariant::Dark
            }
        }),
    }
}

pub(crate) fn option_style(is_selected: bool, dim: bool) -> Style {
    let styles = get_styles();

    if is_selected {
        styles.selection
    } else if dim {
        styles.transcript.patch(styles.dim)
    } else {
        styles.transcript
    }
}

pub(crate) fn dim_style() -> Style {
    get_styles().dim
}

pub(crate) fn transcript_dim_style() -> Style {
    transcript_style().patch(dim_style())
}

pub(crate) fn status_dim_style() -> Style {
    status_style().patch(dim_style())
}

pub(crate) fn border_style() -> Style {
    get_styles().border
}

pub(crate) fn transcript_style() -> Style {
    get_styles().transcript
}

pub(crate) fn transcript_bg_rgb() -> Option<(u8, u8, u8)> {
    get_styles().transcript_bg_rgb
}

pub(crate) fn composer_style() -> Style {
    get_styles().composer
}

pub(crate) fn user_prompt_highlight_style() -> Style {
    get_styles().user_prompt_highlight
}

pub(crate) fn status_style() -> Style {
    get_styles().status
}

pub(crate) fn status_ramp_palette() -> ((u8, u8, u8), (u8, u8, u8)) {
    let styles = get_styles();
    let base = styles.status_ramp_fg.unwrap_or((40, 40, 40));
    let highlight = styles.status_ramp_highlight.unwrap_or((245, 245, 245));
    (base, highlight)
}

pub(crate) fn accent_style() -> Style {
    get_styles().accent
}

#[allow(dead_code)]
pub(crate) fn brand_style() -> Style {
    get_styles().brand
}

pub(crate) fn command_style() -> Style {
    get_styles().command
}

pub(crate) fn success_style() -> Style {
    get_styles().success
}

pub(crate) fn warning_style() -> Style {
    get_styles().warning
}

pub(crate) fn error_style() -> Style {
    get_styles().error
}

pub(crate) fn link_style() -> Style {
    get_styles().link
}

pub(crate) fn code_keyword_style() -> Style {
    get_styles().code_keyword
}

pub(crate) fn code_operator_style() -> Style {
    get_styles().code_operator
}

pub(crate) fn code_comment_style() -> Style {
    get_styles().code_comment
}

pub(crate) fn code_string_style() -> Style {
    get_styles().code_string
}

pub(crate) fn code_number_style() -> Style {
    get_styles().code_number
}

pub(crate) fn code_type_style() -> Style {
    get_styles().code_type
}

pub(crate) fn code_function_style() -> Style {
    get_styles().code_function
}

pub(crate) fn code_constant_style() -> Style {
    get_styles().code_constant
}

pub(crate) fn code_macro_style() -> Style {
    get_styles().code_macro
}

pub(crate) fn code_punctuation_style() -> Style {
    get_styles().code_punctuation
}

pub(crate) fn code_variable_style() -> Style {
    get_styles().code_variable
}

pub(crate) fn code_property_style() -> Style {
    get_styles().code_property
}

pub(crate) fn code_attribute_style() -> Style {
    get_styles().code_attribute
}

pub(crate) fn code_module_style() -> Style {
    get_styles().code_module
}

pub(crate) fn code_label_style() -> Style {
    get_styles().code_label
}

pub(crate) fn code_tag_style() -> Style {
    get_styles().code_tag
}

pub(crate) fn code_embedded_style() -> Style {
    get_styles().code_embedded
}

#[allow(dead_code)]
pub(crate) fn diff_add_style() -> Style {
    get_styles().diff_add
}

#[allow(dead_code)]
pub(crate) fn diff_del_style() -> Style {
    get_styles().diff_del
}

#[allow(dead_code)]
pub(crate) fn diff_hunk_style() -> Style {
    get_styles().diff_hunk
}

pub(crate) fn diff_add_highlight_style() -> Style {
    get_styles().diff_add_highlight
}

pub(crate) fn diff_del_highlight_style() -> Style {
    get_styles().diff_del_highlight
}

pub(crate) fn diff_hunk_highlight_style() -> Style {
    get_styles().diff_hunk_highlight
}

pub(crate) fn diff_add_text_style() -> Style {
    get_styles().diff_add_text
}

pub(crate) fn diff_del_text_style() -> Style {
    get_styles().diff_del_text
}

pub(crate) fn diff_hunk_text_style() -> Style {
    get_styles().diff_hunk_text
}

#[allow(dead_code)]
pub(crate) fn preview_lines(
    config: &Config,
    terminal_bg: Option<(u8, u8, u8)>,
) -> Vec<ratatui::text::Line<'static>> {
    use codex_core::themes::ThemeCatalog;
    use codex_core::themes::ThemeColorResolved;
    use codex_core::themes::ThemeDefinition;
    use ratatui::text::Line;
    use ratatui::text::Span;

    let terminal_background_is_light = terminal_bg.is_some_and(is_light);
    let auto_variant = os_theme_variant();

    let catalog = match ThemeCatalog::load(config) {
        Ok(catalog) => catalog,
        Err(err) => {
            return vec![
                Line::from("Failed to load themes".red().bold()),
                Line::from(err.to_string().red()),
            ];
        }
    };
    let theme = catalog.resolve_active(&config.xcodex.themes, auto_variant, terminal_background_is_light);

    fn color_span(label: &str, resolved: ThemeColorResolved) -> Span<'static> {
        match resolved {
            ThemeColorResolved::Rgb(rgb) => {
                let c = best_color((rgb.0, rgb.1, rgb.2));
                Span::styled(format!(" {label} "), Style::default().fg(c).bg(c))
            }
            ThemeColorResolved::Inherit => Span::from(format!(" {label} ")).dim(),
        }
    }

    fn role_style(
        theme: &ThemeDefinition,
        fg_field: &'static str,
        fg: &codex_core::themes::ThemeColor,
        bg_field: &'static str,
        bg: &codex_core::themes::ThemeColor,
    ) -> Style {
        let fg = theme
            .resolve_role(fg_field, fg)
            .ok()
            .and_then(|resolved| to_color(resolved, None));
        let bg = theme
            .resolve_role(bg_field, bg)
            .ok()
            .and_then(|resolved| to_color(resolved, None));
        let mut style = Style::default();
        if let Some(fg) = fg {
            style = style.fg(fg);
        }
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        style
    }

    fn role_style_with_resolved_bg(theme: &ThemeDefinition, bg: ThemeColorResolved) -> Style {
        let fg = theme
            .resolve_role("roles.fg", &theme.roles.fg)
            .ok()
            .and_then(|resolved| to_color(resolved, None));
        let bg = to_color(bg, None);
        let mut style = Style::default();
        if let Some(fg) = fg {
            style = style.fg(fg);
        }
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        style
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from("Active theme".bold()));
    lines.push(Line::from(format!(
        "name: {} ({:?})",
        theme.name, theme.variant
    )));
    lines.push(Line::from(format!(
        "dir: {}",
        codex_core::themes::themes_dir(&config.codex_home, &config.xcodex.themes).display()
    )));
    lines.push(Line::from(format!(
        "mode: {:?} (effective: {:?})",
        config.xcodex.themes.theme_mode,
        active_variant(config, terminal_bg)
    )));
    if !catalog.load_warnings().is_empty() {
        lines.push(Line::from(vec![
            "warning: ".red().bold(),
            format!(
                "{} theme file(s) failed to load",
                catalog.load_warnings().len()
            )
            .red(),
        ]));
    }
    lines.push(Line::from(""));

    lines.push(Line::from("Palette".bold()));
    let row0: Vec<Span<'static>> = vec![
        "0-7: ".dim(),
        color_span(
            "0",
            theme
                .palette
                .get("black")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "1",
            theme
                .palette
                .get("red")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "2",
            theme
                .palette
                .get("green")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "3",
            theme
                .palette
                .get("yellow")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "4",
            theme
                .palette
                .get("blue")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "5",
            theme
                .palette
                .get("magenta")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "6",
            theme
                .palette
                .get("cyan")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "7",
            theme
                .palette
                .get("white")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
    ];
    lines.push(Line::from(row0));
    let row1: Vec<Span<'static>> = vec![
        "8-15:".dim(),
        " ".into(),
        color_span(
            "8",
            theme
                .palette
                .get("bright_black")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "9",
            theme
                .palette
                .get("bright_red")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "10",
            theme
                .palette
                .get("bright_green")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "11",
            theme
                .palette
                .get("bright_yellow")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "12",
            theme
                .palette
                .get("bright_blue")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "13",
            theme
                .palette
                .get("bright_magenta")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "14",
            theme
                .palette
                .get("bright_cyan")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
        color_span(
            "15",
            theme
                .palette
                .get("bright_white")
                .unwrap_or(ThemeColorResolved::Inherit),
        ),
    ];
    lines.push(Line::from(row1));
    lines.push(Line::from(""));

    lines.push(Line::from("Roles".bold()));
    lines.push(Line::from(vec![
        "fg/bg ".dim(),
        Span::styled(
            " Sample text ",
            role_style(
                theme,
                "roles.fg",
                &theme.roles.fg,
                "roles.bg",
                &theme.roles.bg,
            ),
        ),
    ]));
    lines.push(Line::from(vec![
        "selection ".dim(),
        Span::styled(
            " Selected item ",
            role_style(
                theme,
                "roles.selection_fg",
                &theme.roles.selection_fg,
                "roles.selection_bg",
                &theme.roles.selection_bg,
            ),
        ),
    ]));
    lines.push(Line::from(vec![
        "border ".dim(),
        Span::styled(
            " ╭─╮ ",
            role_style(
                theme,
                "roles.border",
                &theme.roles.border,
                "roles.bg",
                &theme.roles.bg,
            ),
        ),
    ]));
    lines.push(Line::from(vec![
        "surfaces ".dim(),
        Span::styled(
            " transcript ",
            role_style_with_resolved_bg(theme, theme.resolve_transcript_bg()),
        ),
        " ".into(),
        Span::styled(
            " composer ",
            role_style_with_resolved_bg(theme, theme.resolve_composer_bg()),
        ),
        " ".into(),
        Span::styled(
            " status ",
            role_style_with_resolved_bg(theme, theme.resolve_status_bg()),
        ),
    ]));
    lines.push(Line::from(vec![
        "accent ".dim(),
        Span::styled(
            " LINK ",
            link_style().add_modifier(ratatui::style::Modifier::UNDERLINED),
        ),
        " ".into(),
        "brand ".dim(),
        Span::styled(" xcodex ", brand_style()),
    ]));
    lines.push(Line::from(vec![
        "success ".dim(),
        Span::styled(" OK ", success_style()),
        " ".into(),
        "warning ".dim(),
        Span::styled(" WARN ", warning_style()),
        " ".into(),
        "error ".dim(),
        Span::styled(" ERR ", error_style()),
    ]));
    lines.push(Line::from(vec![
        "diff ".dim(),
        Span::styled(
            " +add ",
            role_style(
                theme,
                "roles.diff_add_fg",
                &theme.roles.diff_add_fg,
                "roles.diff_add_bg",
                &theme.roles.diff_add_bg,
            ),
        ),
        " ".into(),
        Span::styled(
            " -del ",
            role_style(
                theme,
                "roles.diff_del_fg",
                &theme.roles.diff_del_fg,
                "roles.diff_del_bg",
                &theme.roles.diff_del_bg,
            ),
        ),
        " ".into(),
        Span::styled(
            " ⋮ ",
            role_style(
                theme,
                "roles.diff_hunk_fg",
                &theme.roles.diff_hunk_fg,
                "roles.diff_hunk_bg",
                &theme.roles.diff_hunk_bg,
            ),
        ),
    ]));

    lines
}

fn to_color(
    resolved: ThemeColorResolved,
    inherit: Option<(u8, u8, u8)>,
) -> Option<ratatui::style::Color> {
    match resolved {
        ThemeColorResolved::Inherit => inherit.map(best_color),
        ThemeColorResolved::Rgb(rgb) => Some(best_color((rgb.0, rgb.1, rgb.2))),
    }
}

fn styles_for(
    theme: &codex_core::themes::ThemeDefinition,
    terminal_fg: Option<(u8, u8, u8)>,
    terminal_bg: Option<(u8, u8, u8)>,
) -> ThemeStyles {
    fn resolve_color(
        theme: &codex_core::themes::ThemeDefinition,
        field: &'static str,
        color: &codex_core::themes::ThemeColor,
    ) -> Option<ratatui::style::Color> {
        theme
            .resolve_role(field, color)
            .ok()
            .and_then(|resolved| to_color(resolved, None))
    }

    fn style_from_roles(
        fg: Option<ratatui::style::Color>,
        bg: Option<ratatui::style::Color>,
        fallback: Style,
    ) -> Style {
        if fg.is_none() && bg.is_none() {
            return fallback;
        }
        let mut style = Style::default();
        if let Some(fg) = fg {
            style = style.fg(fg);
        }
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        style
    }

    // If the theme uses `inherit` for fg/bg (the built-in `default` theme), keep these unset so
    // ANSI-colored output (diffs, logs) can render using the terminal palette without being
    // overridden by an explicit "best guess" fg/bg.
    let base_fg = resolve_color(theme, "roles.fg", &theme.roles.fg);
    let base_bg = resolve_color(theme, "roles.bg", &theme.roles.bg);
    let base = style_from_roles(base_fg, base_bg, Style::default());
    let base_rgb = theme
        .resolve_role("roles.fg", &theme.roles.fg)
        .ok()
        .and_then(|resolved| match resolved {
            ThemeColorResolved::Rgb(rgb) => Some((rgb.0, rgb.1, rgb.2)),
            ThemeColorResolved::Inherit => terminal_fg,
        })
        .unwrap_or((40, 40, 40));

    let transcript_bg_rgb = match theme.resolve_transcript_bg() {
        ThemeColorResolved::Rgb(rgb) => Some((rgb.0, rgb.1, rgb.2)),
        ThemeColorResolved::Inherit => terminal_bg,
    };
    let transcript_bg = transcript_bg_rgb.map(best_color);
    let transcript = style_from_roles(base_fg, transcript_bg, base);

    fn lifted_bg(rgb: (u8, u8, u8)) -> ratatui::style::Color {
        let top = if is_light(rgb) {
            (0, 0, 0)
        } else {
            (255, 255, 255)
        };
        best_color(blend(top, rgb, 0.1))
    }

    let composer_bg = to_color(theme.resolve_composer_bg(), None).or_else(|| {
        let base_rgb = transcript_bg_rgb?;
        Some(lifted_bg(base_rgb))
    });
    let composer = style_from_roles(base_fg, composer_bg, base);

    fn highlight_fg(rgb: (u8, u8, u8)) -> ratatui::style::Color {
        if is_light(rgb) {
            best_color((0, 0, 0))
        } else {
            best_color((255, 255, 255))
        }
    }

    fn shimmer_highlight_rgb(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
        let top = if is_light(rgb) {
            (0, 0, 0)
        } else {
            (255, 255, 255)
        };
        blend(top, rgb, 0.7)
    }

    let user_prompt_highlight_bg = theme
        .roles
        .user_prompt_highlight_bg
        .as_ref()
        .and_then(|color| {
            theme
                .resolve_role("roles.user_prompt_highlight_bg", color)
                .ok()
        })
        .and_then(|resolved| match resolved {
            ThemeColorResolved::Rgb(rgb) => Some((rgb.0, rgb.1, rgb.2)),
            ThemeColorResolved::Inherit => None,
        });

    let user_prompt_highlight = user_prompt_highlight_bg.map_or_else(
        || match theme.resolve_composer_bg() {
            ThemeColorResolved::Rgb(rgb) => {
                let rgb = (rgb.0, rgb.1, rgb.2);
                let top = if is_light(rgb) {
                    (0, 0, 0)
                } else {
                    (255, 255, 255)
                };
                let bg_rgb = blend(top, rgb, 0.18);
                Style::default()
                    .bg(best_color(bg_rgb))
                    .fg(highlight_fg(bg_rgb))
            }
            ThemeColorResolved::Inherit => {
                transcript_bg_rgb.map_or_else(Style::default, |rgb| {
                    let top = if is_light(rgb) {
                        (0, 0, 0)
                    } else {
                        (255, 255, 255)
                    };
                    let bg_rgb = blend(top, rgb, 0.18);
                    Style::default()
                        .bg(best_color(bg_rgb))
                        .fg(highlight_fg(bg_rgb))
                })
            }
        },
        |bg_rgb| {
            Style::default()
                .bg(best_color(bg_rgb))
                .fg(highlight_fg(bg_rgb))
        },
    );

    let status_bg = to_color(theme.resolve_status_bg(), terminal_bg);
    let status = style_from_roles(base_fg, status_bg, base);
    let status_ramp_fg = theme
        .roles
        .status_ramp_fg
        .as_ref()
        .and_then(|color| theme.resolve_role("roles.status_ramp_fg", color).ok())
        .and_then(|resolved| match resolved {
            ThemeColorResolved::Rgb(rgb) => Some((rgb.0, rgb.1, rgb.2)),
            ThemeColorResolved::Inherit => None,
        })
        .or(Some(base_rgb));
    let status_ramp_highlight = theme
        .roles
        .status_ramp_highlight
        .as_ref()
        .and_then(|color| {
            theme
                .resolve_role("roles.status_ramp_highlight", color)
                .ok()
        })
        .and_then(|resolved| match resolved {
            ThemeColorResolved::Rgb(rgb) => Some((rgb.0, rgb.1, rgb.2)),
            ThemeColorResolved::Inherit => None,
        })
        .or(Some(shimmer_highlight_rgb(base_rgb)));

    let selection_fg = resolve_color(theme, "roles.selection_fg", &theme.roles.selection_fg);
    let selection_bg = resolve_color(theme, "roles.selection_bg", &theme.roles.selection_bg);
    let selection = style_from_roles(selection_fg, selection_bg, Style::default().cyan());

    let dim = match theme.resolve_dim() {
        ThemeColorResolved::Rgb(rgb) => Style::default().fg(best_color((rgb.0, rgb.1, rgb.2))),
        ThemeColorResolved::Inherit => Style::default().dim(),
    };

    let border_fg = resolve_color(theme, "roles.border", &theme.roles.border);
    let border = style_from_roles(border_fg, None, Style::default().dim());

    let accent_fg = resolve_color(theme, "roles.accent", &theme.roles.accent);
    let accent = style_from_roles(accent_fg, None, Style::default().cyan());

    let brand_fg = resolve_color(theme, "roles.brand", &theme.roles.brand);
    let brand = style_from_roles(brand_fg, None, Style::default().magenta());

    let command_fg = resolve_color(theme, "roles.command", &theme.roles.command);
    let command = style_from_roles(command_fg, None, Style::default().magenta());

    let success_fg = resolve_color(theme, "roles.success", &theme.roles.success);
    let success = style_from_roles(success_fg, None, Style::default().green());

    let warning_fg = resolve_color(theme, "roles.warning", &theme.roles.warning);
    let warning = style_from_roles(warning_fg, None, Style::default().yellow());

    let error_fg = resolve_color(theme, "roles.error", &theme.roles.error);
    let error = style_from_roles(error_fg, None, Style::default().red());

    let link = match theme.roles.link.as_ref() {
        Some(link) => {
            let link_fg = resolve_color(theme, "roles.link", link);
            style_from_roles(link_fg, None, accent)
        }
        None => accent,
    };

    let code_keyword_fg = resolve_color(theme, "roles.code_keyword", &theme.roles.code_keyword);
    let code_keyword = style_from_roles(code_keyword_fg, None, Style::default().magenta());
    let code_operator_fg = resolve_color(theme, "roles.code_operator", &theme.roles.code_operator);
    let code_operator = style_from_roles(code_operator_fg, None, Style::default().magenta().dim());
    let code_comment_fg = resolve_color(theme, "roles.code_comment", &theme.roles.code_comment);
    let code_comment = style_from_roles(code_comment_fg, None, Style::default().dim());
    let code_string_fg = resolve_color(theme, "roles.code_string", &theme.roles.code_string);
    let code_string = style_from_roles(code_string_fg, None, Style::default().green());
    let code_number_fg = resolve_color(theme, "roles.code_number", &theme.roles.code_number);
    let code_number = style_from_roles(code_number_fg, None, Style::default().blue());
    let code_type_fg = resolve_color(theme, "roles.code_type", &theme.roles.code_type);
    let code_type = style_from_roles(code_type_fg, None, Style::default().cyan());
    let code_function_fg = resolve_color(theme, "roles.code_function", &theme.roles.code_function);
    let code_function = style_from_roles(code_function_fg, None, Style::default().green());
    let code_constant_fg = resolve_color(theme, "roles.code_constant", &theme.roles.code_constant);
    let code_constant = style_from_roles(code_constant_fg, None, Style::default().cyan());
    let code_macro_fg = resolve_color(theme, "roles.code_macro", &theme.roles.code_macro);
    let code_macro = style_from_roles(code_macro_fg, None, Style::default().magenta());
    let code_punctuation_fg = resolve_color(
        theme,
        "roles.code_punctuation",
        &theme.roles.code_punctuation,
    );
    let code_punctuation = style_from_roles(code_punctuation_fg, None, Style::default());
    let code_variable_fg = resolve_color(theme, "roles.code_variable", &theme.roles.code_variable);
    let code_variable = style_from_roles(code_variable_fg, None, Style::default());
    let code_property_fg = resolve_color(theme, "roles.code_property", &theme.roles.code_property);
    let code_property = style_from_roles(code_property_fg, None, Style::default());
    let code_attribute_fg =
        resolve_color(theme, "roles.code_attribute", &theme.roles.code_attribute);
    let code_attribute = style_from_roles(code_attribute_fg, None, Style::default().yellow());
    let code_module_fg = resolve_color(theme, "roles.code_module", &theme.roles.code_module);
    let code_module = style_from_roles(code_module_fg, None, Style::default().cyan());
    let code_label_fg = resolve_color(theme, "roles.code_label", &theme.roles.code_label);
    let code_label = style_from_roles(code_label_fg, None, Style::default().yellow());
    let code_tag_fg = resolve_color(theme, "roles.code_tag", &theme.roles.code_tag);
    let code_tag = style_from_roles(code_tag_fg, None, Style::default().magenta());
    let code_embedded_fg = resolve_color(theme, "roles.code_embedded", &theme.roles.code_embedded);
    let code_embedded = style_from_roles(code_embedded_fg, None, Style::default().red());

    let diff_add_fg = resolve_color(theme, "roles.diff_add_fg", &theme.roles.diff_add_fg);
    let diff_add_bg = resolve_color(theme, "roles.diff_add_bg", &theme.roles.diff_add_bg);
    let diff_add = style_from_roles(diff_add_fg, diff_add_bg, Style::default().green());

    let diff_del_fg = resolve_color(theme, "roles.diff_del_fg", &theme.roles.diff_del_fg);
    let diff_del_bg = resolve_color(theme, "roles.diff_del_bg", &theme.roles.diff_del_bg);
    let diff_del = style_from_roles(diff_del_fg, diff_del_bg, Style::default().red());

    let diff_hunk_fg = resolve_color(theme, "roles.diff_hunk_fg", &theme.roles.diff_hunk_fg);
    let diff_hunk_bg = resolve_color(theme, "roles.diff_hunk_bg", &theme.roles.diff_hunk_bg);
    let diff_hunk = style_from_roles(diff_hunk_fg, diff_hunk_bg, Style::default().cyan());

    fn fallback_diff_add_bg(variant: ThemeVariant) -> Color {
        match variant {
            ThemeVariant::Light => best_color((0xdf, 0xf5, 0xd8)),
            ThemeVariant::Dark => best_color((0x1f, 0x3a, 0x24)),
        }
    }

    fn fallback_diff_del_bg(variant: ThemeVariant) -> Color {
        match variant {
            ThemeVariant::Light => best_color((0xf6, 0xd6, 0xd6)),
            ThemeVariant::Dark => best_color((0x3a, 0x1f, 0x1f)),
        }
    }

    let diff_add_highlight = style_from_roles(
        base_fg,
        diff_add_bg.or_else(|| Some(fallback_diff_add_bg(theme.variant))),
        Style::default(),
    );
    let diff_del_highlight = style_from_roles(
        base_fg,
        diff_del_bg.or_else(|| Some(fallback_diff_del_bg(theme.variant))),
        Style::default(),
    );
    let diff_hunk_highlight = style_from_roles(base_fg, diff_hunk_bg, Style::default());

    let diff_add_text = style_from_roles(diff_add_fg, None, Style::default().green());
    let diff_del_text = style_from_roles(diff_del_fg, None, Style::default().red());
    let diff_hunk_text = style_from_roles(diff_hunk_fg, None, Style::default().cyan());

    ThemeStyles {
        transcript,
        transcript_bg_rgb,
        composer,
        user_prompt_highlight,
        status,
        status_ramp_fg,
        status_ramp_highlight,
        selection,
        dim,
        border,
        accent,
        brand,
        command,
        success,
        warning,
        error,
        link,
        code_keyword,
        code_operator,
        code_comment,
        code_string,
        code_number,
        code_type,
        code_function,
        code_constant,
        code_macro,
        code_punctuation,
        code_variable,
        code_property,
        code_attribute,
        code_module,
        code_label,
        code_tag,
        code_embedded,
        diff_add,
        diff_del,
        diff_hunk,
        diff_add_highlight,
        diff_del_highlight,
        diff_hunk_highlight,
        diff_add_text,
        diff_del_text,
        diff_hunk_text,
    }
}

fn fallback_styles() -> ThemeStyles {
    ThemeStyles {
        transcript: Style::default(),
        transcript_bg_rgb: None,
        composer: Style::default(),
        user_prompt_highlight: Style::default(),
        status: Style::default(),
        status_ramp_fg: None,
        status_ramp_highlight: None,
        selection: Style::default().cyan(),
        dim: Style::default().dim(),
        border: Style::default().dim(),
        accent: Style::default().cyan(),
        brand: Style::default().magenta(),
        command: Style::default().magenta(),
        success: Style::default().green(),
        warning: Style::default().cyan(),
        error: Style::default().red(),
        link: Style::default().cyan(),
        code_keyword: Style::default().magenta(),
        code_operator: Style::default().magenta().dim(),
        code_comment: Style::default().dim(),
        code_string: Style::default().green(),
        code_number: Style::default().blue(),
        code_type: Style::default().cyan(),
        code_function: Style::default().green(),
        code_constant: Style::default().cyan(),
        code_macro: Style::default().magenta(),
        code_punctuation: Style::default(),
        code_variable: Style::default(),
        code_property: Style::default(),
        code_attribute: Style::default().yellow(),
        code_module: Style::default().cyan(),
        code_label: Style::default().yellow(),
        code_tag: Style::default().magenta(),
        code_embedded: Style::default().red(),
        diff_add: Style::default().green(),
        diff_del: Style::default().red(),
        diff_hunk: Style::default().cyan(),
        diff_add_highlight: Style::default().bg(Color::Green),
        diff_del_highlight: Style::default().bg(Color::Red),
        diff_hunk_highlight: Style::default(),
        diff_add_text: Style::default().green(),
        diff_del_text: Style::default().red(),
        diff_hunk_text: Style::default().cyan(),
    }
}

fn set_styles(styles: ThemeStyles) {
    let lock = THEME_STYLES.get_or_init(|| RwLock::new(fallback_styles()));
    if let Ok(mut guard) = lock.write() {
        *guard = styles;
    }
}

fn get_styles() -> ThemeStyles {
    THEME_STYLES
        .get()
        .and_then(|lock| lock.read().ok().map(|guard| *guard))
        .unwrap_or_else(fallback_styles)
}

fn os_theme_variant() -> Option<ThemeVariant> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("defaults")
            .arg("read")
            .arg("-g")
            .arg("AppleInterfaceStyle")
            .output()
            .ok()?;
        if !output.status.success() {
            return Some(ThemeVariant::Light);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().eq_ignore_ascii_case("dark") {
            Some(ThemeVariant::Dark)
        } else {
            Some(ThemeVariant::Light)
        }
    }

    #[cfg(windows)]
    {
        use std::ptr;
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
        use windows_sys::Win32::System::Registry::RRF_RT_REG_DWORD;
        use windows_sys::Win32::System::Registry::RegGetValueW;

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let value = wide("AppsUseLightTheme");
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        // 0 => dark, 1 => light
        Some(if data == 0 {
            ThemeVariant::Dark
        } else {
            ThemeVariant::Light
        })
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}
