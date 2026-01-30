use crate::app::App;
use crate::app_event::AppEvent;
use crate::chatwidget::ChatWidget;
use crate::pager_overlay::Overlay;
use crate::slash_command::SlashCommand;
use crate::terminal_palette;
use crate::tui::Tui;
use codex_core::config::edit::ConfigEdit;
use codex_core::config::edit::ConfigEditsBuilder;
use codex_core::themes::ThemeVariant;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::PluginSubcommandNode;
use super::PluginSubcommandRoot;

const THEME_SUBCOMMANDS: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "help",
        full_name: "theme help",
        description: "show theme role mapping and format details",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "template",
        full_name: "theme template",
        description: "write example theme YAML files to themes.dir",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

pub(crate) const THEME_SUBCOMMAND_ROOT: PluginSubcommandRoot = PluginSubcommandRoot {
    root: "theme",
    anchor: SlashCommand::Theme,
    children: THEME_SUBCOMMANDS,
    list_hint_order: None,
};

pub(crate) fn handle_theme_command(chat: &mut ChatWidget, rest: &str) {
    let trimmed = rest.trim();
    if !trimmed.is_empty() && try_handle_subcommand(chat, trimmed) {
        return;
    }
    open_theme_selector_event(chat);
}

pub(crate) fn try_handle_subcommand(chat: &mut ChatWidget, args: &str) -> bool {
    match args.trim().to_ascii_lowercase().as_str() {
        "help" => {
            open_theme_help_event(chat);
            true
        }
        "template" => {
            write_theme_templates(chat);
            true
        }
        _ => false,
    }
}

pub(crate) fn preview_theme(app: &mut App, tui: &mut Tui, theme: &str) {
    crate::theme::preview(&app.config, terminal_palette::default_bg(), theme);
    tui.frame_requester().schedule_frame();
}

pub(crate) fn cancel_theme_preview(app: &mut App, tui: &mut Tui) {
    crate::theme::apply_from_config(&app.config, terminal_palette::default_bg());
    tui.frame_requester().schedule_frame();
}

pub(crate) async fn persist_theme_selection(
    app: &mut App,
    tui: &mut Tui,
    variant: ThemeVariant,
    theme: String,
) {
    let profile = app.active_profile.as_deref();
    let (config_key, label) = match variant {
        ThemeVariant::Light => ("light", "Light"),
        ThemeVariant::Dark => ("dark", "Dark"),
    };

    let edit = if theme == "default" {
        ConfigEdit::ClearPath {
            segments: vec!["themes".to_string(), config_key.to_string()],
        }
    } else {
        ConfigEdit::SetPath {
            segments: vec!["themes".to_string(), config_key.to_string()],
            value: toml_edit::value(theme.clone()),
        }
    };

    let result = ConfigEditsBuilder::new(&app.config.codex_home)
        .with_profile(profile)
        .with_edits([edit])
        .apply()
        .await;

    match result {
        Ok(()) => {
            match variant {
                ThemeVariant::Light => {
                    app.config.xcodex.themes.light = (theme != "default").then(|| theme.clone());
                }
                ThemeVariant::Dark => {
                    app.config.xcodex.themes.dark = (theme != "default").then(|| theme.clone());
                }
            }
            app.chat_widget
                .set_themes_config(app.config.xcodex.themes.clone());
            crate::theme::apply_from_config(&app.config, terminal_palette::default_bg());

            let mut message = format!("Theme changed to `{theme}` for {label} mode.");
            if let Some(profile) = profile {
                message.push_str(" (profile: ");
                message.push_str(profile);
                message.push(')');
            }
            app.chat_widget.add_info_message(message, None);
        }
        Err(err) => {
            crate::theme::apply_from_config(&app.config, terminal_palette::default_bg());
            tracing::error!(error = %err, "failed to persist theme selection");
            if let Some(profile) = profile {
                app.chat_widget.add_error_message(format!(
                    "Failed to save theme for profile `{profile}`: {err}"
                ));
            } else {
                app.chat_widget
                    .add_error_message(format!("Failed to save theme: {err}"));
            }
        }
    }
    tui.frame_requester().schedule_frame();
}

pub(crate) fn open_theme_selector(app: &mut App, tui: &mut Tui) {
    let _ = tui.enter_alt_screen();
    let terminal_bg = terminal_palette::default_bg();
    app.overlay = Some(Overlay::new_theme_selector(
        app.app_event_tx.clone(),
        app.config.clone(),
        terminal_bg,
    ));
    tui.frame_requester().schedule_frame();
}

pub(crate) fn open_theme_help(app: &mut App, tui: &mut Tui) {
    let _ = tui.enter_alt_screen();
    let lines: Vec<Line<'static>> = vec![
        "Theme keys".bold().into(),
        "".into(),
        "roles.fg / roles.bg — primary app text + surfaces".into(),
        "roles.transcript_bg / roles.composer_bg / roles.status_bg — transcript, composer, and status bar backgrounds (derived by default)".into(),
        "roles.user_prompt_highlight_bg — background for highlighting past user prompts in the transcript (derived by default)".into(),
        "roles.selection_fg / roles.selection_bg — selection highlight in pickers".into(),
        "roles.cursor_fg / roles.cursor_bg — (reserved for future)".dim().into(),
        "roles.border — box borders and tree chrome (status cards, tool blocks)".into(),
        "roles.dim — secondary text (derived from fg/bg)".into(),
        "".into(),
        "Diff roles".bold().into(),
        "roles.diff_add_fg / roles.diff_add_bg — additions in /diff overlay".into(),
        "roles.diff_del_fg / roles.diff_del_bg — deletions in /diff overlay".into(),
        "roles.diff_hunk_fg / roles.diff_hunk_bg — hunk separators in /diff overlay".into(),
        "Tip: set roles.diff_*_bg to `inherit` for text-only diffs.".dim().into(),
        "".into(),
        "Palette keys".bold().into(),
        "palette.* defines ANSI slots (0–15). They matter for legacy ANSI-colored UI and external tool output; xcodex themes do not swap the terminal palette.".into(),
        "".into(),
        "Tip: run `/theme` to preview + save; press Ctrl+T to edit colors (palette/roles). `/theme template` writes example YAML files.".into(),
    ];
    app.overlay = Some(Overlay::new_static_with_lines(
        lines,
        "T H E M E".to_string(),
    ));
    tui.frame_requester().schedule_frame();
}

fn open_theme_selector_event(chat: &mut ChatWidget) {
    chat.send_app_event(AppEvent::OpenThemeSelector);
    chat.request_redraw();
}

fn open_theme_help_event(chat: &mut ChatWidget) {
    chat.send_app_event(AppEvent::OpenThemeHelp);
}

fn write_theme_templates(chat: &mut ChatWidget) {
    use codex_core::themes::ThemeCatalog;
    use codex_core::themes::ThemeVariant;

    let dir = chat.themes_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        chat.add_error_message(format!(
            "Failed to create themes directory `{}`: {err}",
            dir.display()
        ));
        return;
    }

    let templates = [
        (ThemeVariant::Light, "example-light.yaml"),
        (ThemeVariant::Dark, "example-dark.yaml"),
    ];

    let mut created: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for (variant, filename) in templates {
        let path = dir.join(filename);
        if path.exists() {
            skipped.push(path.display().to_string());
            continue;
        }
        let yaml = ThemeCatalog::example_theme_yaml(variant);
        if let Err(err) = std::fs::write(&path, yaml) {
            chat.add_error_message(format!("Failed to write `{}`: {err}", path.display()));
            return;
        }
        created.push(path.display().to_string());
    }

    if created.is_empty() {
        chat.add_info_message(
            format!("Theme templates already exist in `{}`.", dir.display()),
            None,
        );
        return;
    }

    let mut message = String::from("Wrote theme template(s):\n");
    for path in created {
        message.push_str("- ");
        message.push_str(&path);
        message.push('\n');
    }
    if !skipped.is_empty() {
        message.push_str("\nSkipped existing file(s):\n");
        for path in skipped {
            message.push_str("- ");
            message.push_str(&path);
            message.push('\n');
        }
    }
    message.push_str("\nSelect a theme with `/theme`.");

    chat.add_info_message(message, None);
}
