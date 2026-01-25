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

pub(crate) fn try_handle_subcommand(chat: &mut ChatWidget, args: &str) -> bool {
    match args.trim().to_ascii_lowercase().as_str() {
        "help" => {
            chat.open_theme_help();
            true
        }
        "template" => {
            chat.write_theme_templates();
            true
        }
        _ => false,
    }
}
