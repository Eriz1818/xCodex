use crate::app_event::AppEvent;
use crate::chatwidget::ChatWidget;
use crate::slash_command::SlashCommand;

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
    open_theme_selector(chat);
}

pub(crate) fn try_handle_subcommand(chat: &mut ChatWidget, args: &str) -> bool {
    match args.trim().to_ascii_lowercase().as_str() {
        "help" => {
            open_theme_help(chat);
            true
        }
        "template" => {
            write_theme_templates(chat);
            true
        }
        _ => false,
    }
}

fn open_theme_selector(chat: &mut ChatWidget) {
    chat.send_app_event(AppEvent::OpenThemeSelector);
    chat.request_redraw();
}

fn open_theme_help(chat: &mut ChatWidget) {
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
