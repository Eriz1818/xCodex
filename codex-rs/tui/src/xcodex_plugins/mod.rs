pub(crate) mod app;
mod app_state;
pub(crate) mod command_popup;
pub(crate) mod exclusions;
pub(crate) mod help;
pub(crate) mod history_cell;
mod hook_process_state;
pub(crate) mod hooks;
mod mcp;
mod mcp_startup_state;
mod ramp_status_state;
pub(crate) mod ramps;
pub(crate) mod settings;
pub(crate) mod status;
pub(crate) mod theme;
mod thoughts;
pub(crate) mod worktree;
mod worktree_list_state;
mod xtreme;

use crate::chatwidget::ChatWidget;
use crate::slash_command::SlashCommand;
pub(crate) use app_state::XcodexAppState;
use codex_core::config;
pub(crate) use hook_process_state::HookProcessState;
pub(crate) use mcp_startup_state::McpStartupState;
pub(crate) use ramp_status_state::RampStatusController;
use rand::Rng;
pub(crate) use worktree_list_state::WorktreeListState;

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
        "exclusion" => exclusions::handle_exclusions_command(chat, rest),
        "settings" => settings::handle_settings_command(chat, rest),
        "help" => {
            help::handle_help_command(chat, rest);
            true
        }
        "hooks" => {
            hooks::handle_hooks_command(chat, rest);
            true
        }
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

#[allow(dead_code)]
pub(crate) fn placeholder_text<R: Rng + ?Sized>(rng: &mut R, placeholders: &[&str]) -> String {
    maybe_override_placeholder_text(
        placeholders[rng.random_range(0..placeholders.len())].to_string(),
    )
}

pub(crate) fn maybe_override_placeholder_text(text: String) -> String {
    if config::is_xcodex_invocation() {
        "Ask xcodex to do anything".to_string()
    } else {
        text
    }
}

pub(crate) fn full_access_warning_prefix() -> &'static str {
    "When xcodex runs with full access, it can edit any file on your computer and run commands with network, without your approval. "
}

pub(crate) fn ramps_unavailable_message() -> &'static str {
    "Ramps are only available in xcodex."
}

pub(crate) fn ramps_rotation_description() -> &'static str {
    "When enabled, xcodex picks one eligible ramp per turn. The chosen ramp stays stable for the entire turn."
}

pub(crate) fn ramps_rotation_hint() -> &'static str {
    "Pick which ramp flows xcodex can rotate through."
}

pub(crate) fn format_edit_approval_message(target: String) -> String {
    format!("xcodex wants to edit {target}")
}
