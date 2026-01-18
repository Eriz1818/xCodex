use crate::color::blend;
use crate::color::is_light;
use crate::terminal_palette::best_color;
use codex_core::config::Config;
use codex_core::themes::ThemeCatalog;
use codex_core::themes::ThemeColorResolved;
use codex_core::themes::ThemeVariant;
use ratatui::style::Style;
use ratatui::style::Stylize as _;
use std::sync::OnceLock;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ThemeStyles {
    transcript: Style,
    composer: Style,
    status: Style,
    selection: Style,
    dim: Style,
    border: Style,
    accent: Style,
    brand: Style,
    success: Style,
    warning: Style,
    error: Style,
    link: Style,
    diff_add: Style,
    diff_del: Style,
    diff_hunk: Style,
    diff_add_text: Style,
    diff_del_text: Style,
    diff_hunk_text: Style,
}

static THEME_STYLES: OnceLock<RwLock<ThemeStyles>> = OnceLock::new();

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
                catalog.resolve_active(&config.themes, auto_variant, terminal_background_is_light);
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
                catalog.resolve_active(&config.themes, auto_variant, terminal_background_is_light)
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

pub(crate) fn active_variant(config: &Config, terminal_bg: Option<(u8, u8, u8)>) -> ThemeVariant {
    let terminal_background_is_light = terminal_bg.is_some_and(is_light);
    let auto_variant = os_theme_variant();
    match config.themes.theme_mode {
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

pub(crate) fn composer_style() -> Style {
    get_styles().composer
}

pub(crate) fn status_style() -> Style {
    get_styles().status
}

pub(crate) fn accent_style() -> Style {
    get_styles().accent
}

pub(crate) fn brand_style() -> Style {
    get_styles().brand
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

pub(crate) fn diff_add_style() -> Style {
    get_styles().diff_add
}

pub(crate) fn diff_del_style() -> Style {
    get_styles().diff_del
}

pub(crate) fn diff_hunk_style() -> Style {
    get_styles().diff_hunk
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
    let theme = catalog.resolve_active(&config.themes, auto_variant, terminal_background_is_light);

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
        codex_core::themes::themes_dir(&config.codex_home, &config.themes).display()
    )));
    lines.push(Line::from(format!(
        "mode: {:?} (effective: {:?})",
        config.themes.theme_mode,
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
    _terminal_fg: Option<(u8, u8, u8)>,
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

    let transcript_bg = to_color(theme.resolve_transcript_bg(), terminal_bg);
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
        let base_rgb = match theme.resolve_transcript_bg() {
            ThemeColorResolved::Rgb(rgb) => Some((rgb.0, rgb.1, rgb.2)),
            ThemeColorResolved::Inherit => terminal_bg,
        }?;
        Some(lifted_bg(base_rgb))
    });
    let composer = style_from_roles(base_fg, composer_bg, base);

    let status_bg = to_color(theme.resolve_status_bg(), terminal_bg);
    let status = style_from_roles(base_fg, status_bg, base);

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

    let success_fg = resolve_color(theme, "roles.success", &theme.roles.success);
    let success = style_from_roles(success_fg, None, Style::default().green());

    let warning_fg = resolve_color(theme, "roles.warning", &theme.roles.warning);
    let warning = style_from_roles(warning_fg, None, Style::default().cyan());

    let error_fg = resolve_color(theme, "roles.error", &theme.roles.error);
    let error = style_from_roles(error_fg, None, Style::default().red());

    let link = match theme.roles.link.as_ref() {
        Some(link) => {
            let link_fg = resolve_color(theme, "roles.link", link);
            style_from_roles(link_fg, None, accent)
        }
        None => accent,
    };

    let diff_add_fg = resolve_color(theme, "roles.diff_add_fg", &theme.roles.diff_add_fg);
    let diff_add_bg = resolve_color(theme, "roles.diff_add_bg", &theme.roles.diff_add_bg);
    let diff_add = style_from_roles(diff_add_fg, diff_add_bg, Style::default().green());

    let diff_del_fg = resolve_color(theme, "roles.diff_del_fg", &theme.roles.diff_del_fg);
    let diff_del_bg = resolve_color(theme, "roles.diff_del_bg", &theme.roles.diff_del_bg);
    let diff_del = style_from_roles(diff_del_fg, diff_del_bg, Style::default().red());

    let diff_hunk_fg = resolve_color(theme, "roles.diff_hunk_fg", &theme.roles.diff_hunk_fg);
    let diff_hunk_bg = resolve_color(theme, "roles.diff_hunk_bg", &theme.roles.diff_hunk_bg);
    let diff_hunk = style_from_roles(diff_hunk_fg, diff_hunk_bg, Style::default().cyan());

    let diff_add_text = style_from_roles(diff_add_fg, None, Style::default().green());
    let diff_del_text = style_from_roles(diff_del_fg, None, Style::default().red());
    let diff_hunk_text = style_from_roles(diff_hunk_fg, None, Style::default().cyan());

    ThemeStyles {
        transcript,
        composer,
        status,
        selection,
        dim,
        border,
        accent,
        brand,
        success,
        warning,
        error,
        link,
        diff_add,
        diff_del,
        diff_hunk,
        diff_add_text,
        diff_del_text,
        diff_hunk_text,
    }
}

fn fallback_styles() -> ThemeStyles {
    ThemeStyles {
        transcript: Style::default(),
        composer: Style::default(),
        status: Style::default(),
        selection: Style::default().cyan(),
        dim: Style::default().dim(),
        border: Style::default().dim(),
        accent: Style::default().cyan(),
        brand: Style::default().magenta(),
        success: Style::default().green(),
        warning: Style::default().cyan(),
        error: Style::default().red(),
        link: Style::default().cyan(),
        diff_add: Style::default().green(),
        diff_del: Style::default().red(),
        diff_hunk: Style::default().cyan(),
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
