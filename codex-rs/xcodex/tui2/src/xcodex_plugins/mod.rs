mod mcp;
pub(crate) mod theme;
mod thoughts;
mod worktree;
mod xtreme;

use crate::chatwidget::ChatWidget;
use crate::slash_command::SlashCommand;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginSlashCommand {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) run_on_enter: bool,
    pub(crate) insert_trailing_space: bool,
}

const PLUGIN_COMMANDS: [PluginSlashCommand; 2] = [
    PluginSlashCommand {
        name: "thoughts",
        description: "toggle showing agent thoughts/reasoning (persists)",
        run_on_enter: true,
        insert_trailing_space: false,
    },
    PluginSlashCommand {
        name: "xtreme",
        description: "open the ⚡Tools control panel",
        run_on_enter: true,
        insert_trailing_space: false,
    },
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginSubcommandNode {
    pub(crate) token: &'static str,
    pub(crate) full_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) run_on_enter: bool,
    pub(crate) insert_trailing_space: bool,
    pub(crate) children: &'static [PluginSubcommandNode],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginSubcommandHintOrder {
    pub(crate) token: &'static str,
    pub(crate) order: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginSubcommandRoot {
    pub(crate) root: &'static str,
    pub(crate) anchor: SlashCommand,
    pub(crate) children: &'static [PluginSubcommandNode],
    pub(crate) list_hint_order: Option<&'static [PluginSubcommandHintOrder]>,
}

const PLUGIN_SUBCOMMAND_ROOTS: &[PluginSubcommandRoot] = &[
    mcp::MCP_SUBCOMMAND_ROOT,
    theme::THEME_SUBCOMMAND_ROOT,
    worktree::WORKTREE_SUBCOMMAND_ROOT,
];

pub(crate) fn plugin_slash_commands() -> &'static [PluginSlashCommand] {
    &PLUGIN_COMMANDS
}

pub(crate) fn plugin_subcommand_roots() -> &'static [PluginSubcommandRoot] {
    PLUGIN_SUBCOMMAND_ROOTS
}

pub(crate) fn try_handle_slash_command(chat: &mut ChatWidget, name: &str, rest: &str) -> bool {
    match name {
        "thoughts" => thoughts::handle(chat, rest),
        "xtreme" => xtreme::handle(chat, rest),
        _ => false,
    }
}

pub(crate) fn try_handle_mcp_subcommand(chat: &mut ChatWidget, args: &[&str]) -> bool {
    mcp::try_handle_subcommand(chat, args)
}

pub(crate) fn handle_theme_command(chat: &mut ChatWidget, rest: &str) {
    theme::handle_theme_command(chat, rest);
}

pub(crate) fn try_handle_worktree_subcommand(chat: &mut ChatWidget, args: &str) -> bool {
    worktree::try_handle_subcommand(chat, args)
}
