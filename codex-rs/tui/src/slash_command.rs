use strum::IntoEnumIterator;
use strum_macros::AsRefStr;
use strum_macros::EnumIter;
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;

/// Commands that can be invoked by starting a message with a leading slash.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    // DO NOT ALPHA-SORT! Enum order is presentation order in the popup, so
    // more frequently used commands should be listed first.
    Model,
    Fast,
    Approvals,
    Permissions,
    #[strum(serialize = "setup-default-sandbox")]
    ElevateSandbox,
    #[strum(serialize = "sandbox-add-read-dir")]
    SandboxReadRoot,
    Experimental,
    Skills,
    Help,
    Review,
    Rename,
    New,
    Resume,
    Fork,
    Init,
    Compact,
    Autocompact,
    Plan,
    Collab,
    Agent,
    Copy,
    Diff,
    Mention,
    Status,
    Settings,
    Theme,
    StatusMenu,
    Worktree,
    #[strum(serialize = "exclusion")]
    Exclusions,
    Hooks,
    DebugConfig,
    Title,
    Statusline,
    Mcp,
    Apps,
    Plugins,
    Logout,
    Quit,
    Exit,
    Feedback,
    Rollout,
    Ps,
    PsKill,
    #[strum(serialize = "clean")]
    Stop,
    #[strum(disabled)]
    Clean,
    Clear,
    Personality,
    Realtime,
    TestApproval,
    #[strum(serialize = "subagents")]
    MultiAgents,
    // Debugging commands.
    #[strum(serialize = "debug-m-drop")]
    MemoryDrop,
    #[strum(serialize = "debug-m-update")]
    MemoryUpdate,
}

impl SlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Feedback => "save a local report for troubleshooting",
            SlashCommand::New => "start a new chat during a conversation",
            SlashCommand::Init => "create an AGENTS.md file with instructions for xcodex",
            SlashCommand::Compact => "summarize conversation to prevent hitting the context limit",
            SlashCommand::Autocompact => "toggle automatic conversation compaction (persists)",
            SlashCommand::Review => "review my current changes and find issues",
            SlashCommand::Rename => "rename the current thread",
            SlashCommand::Resume => "resume a saved chat",
            SlashCommand::Clear => "clear the terminal and start a new chat",
            SlashCommand::Fork => "fork the current chat",
            SlashCommand::Quit | SlashCommand::Exit => "exit xcodex",
            SlashCommand::Copy => "copy last response as markdown",
            SlashCommand::Diff => "show git diff (including untracked files)",
            SlashCommand::Mention => "mention a file",
            SlashCommand::Skills => "use skills to improve how xcodex performs specific tasks",
            SlashCommand::Help => "show help for a topic (e.g. /help xcodex)",
            SlashCommand::Status => "open the status/settings menu",
            SlashCommand::Settings => "open the status/settings menu",
            SlashCommand::Theme => "choose a theme (persists)",
            SlashCommand::StatusMenu => "open the status/settings menu (alias)",
            SlashCommand::Worktree => "switch this session to a different git worktree",
            SlashCommand::Exclusions => "review and update exclusions for this session",
            SlashCommand::Hooks => "learn how to automate xcodex with hooks",
            SlashCommand::DebugConfig => "show config layers and requirement sources for debugging",
            SlashCommand::Title => "configure which items appear in the terminal title",
            SlashCommand::Statusline => "configure which items appear in the status line",
            SlashCommand::Ps => "list background terminals",
            SlashCommand::PsKill => "terminate background terminals",
            SlashCommand::Stop | SlashCommand::Clean => "stop all background terminals",
            SlashCommand::MemoryDrop => "DO NOT USE",
            SlashCommand::MemoryUpdate => "DO NOT USE",
            SlashCommand::Model => "choose what model and reasoning effort to use",
            SlashCommand::Fast => "toggle Fast mode to enable fastest inference at 2X plan usage",
            SlashCommand::Approvals => "choose what xcodex can do without approval",
            SlashCommand::Permissions => "choose what xcodex is allowed to do",
            SlashCommand::Personality => "choose a communication style for xcodex",
            SlashCommand::Realtime => "toggle realtime voice mode (experimental)",
            SlashCommand::Plan => "open the Plan menu (plans + settings)",
            SlashCommand::Collab => "change collaboration mode (experimental)",
            SlashCommand::Agent | SlashCommand::MultiAgents => "switch the active agent thread",
            SlashCommand::ElevateSandbox => "set up elevated agent sandbox",
            SlashCommand::SandboxReadRoot => {
                "let sandbox read a directory: /sandbox-add-read-dir <absolute_path>"
            }
            SlashCommand::Experimental => "toggle experimental features",
            SlashCommand::Mcp => "list configured MCP tools",
            SlashCommand::Apps => "manage apps",
            SlashCommand::Plugins => "browse plugins",
            SlashCommand::Logout => "log out of xcodex",
            SlashCommand::Rollout => "print the rollout file path",
            SlashCommand::TestApproval => "test approval request",
        }
    }

    /// Command string without the leading '/'. Provided for compatibility with
    /// existing code that expects a method named `command()`.
    pub fn command(self) -> &'static str {
        self.into()
    }

    /// Whether this command supports inline args (for example `/review ...`).
    pub fn supports_inline_args(self) -> bool {
        matches!(
            self,
            SlashCommand::Review
                | SlashCommand::Rename
                | SlashCommand::Plan
                | SlashCommand::Theme
                | SlashCommand::Worktree
                | SlashCommand::Fast
                | SlashCommand::Resume
                | SlashCommand::SandboxReadRoot
        )
    }

    /// Whether this command can be run while a task is in progress.
    pub fn available_during_task(self) -> bool {
        match self {
            SlashCommand::New
            | SlashCommand::Resume
            | SlashCommand::Fork
            | SlashCommand::Init
            | SlashCommand::Compact
            | SlashCommand::Autocompact
            | SlashCommand::Model
            | SlashCommand::Fast
            | SlashCommand::Personality
            | SlashCommand::Approvals
            | SlashCommand::Permissions
            | SlashCommand::ElevateSandbox
            | SlashCommand::SandboxReadRoot
            | SlashCommand::Experimental
            | SlashCommand::Review
            | SlashCommand::Logout
            | SlashCommand::MemoryDrop
            | SlashCommand::MemoryUpdate
            | SlashCommand::Clear => false,
            SlashCommand::Diff
            | SlashCommand::Plan
            | SlashCommand::Copy
            | SlashCommand::Rename
            | SlashCommand::Mention
            | SlashCommand::Skills
            | SlashCommand::Help
            | SlashCommand::Status
            | SlashCommand::Settings
            | SlashCommand::Theme
            | SlashCommand::StatusMenu
            | SlashCommand::Worktree
            | SlashCommand::Exclusions
            | SlashCommand::Hooks
            | SlashCommand::DebugConfig
            | SlashCommand::Ps
            | SlashCommand::PsKill
            | SlashCommand::Stop
            | SlashCommand::Clean
            | SlashCommand::Mcp
            | SlashCommand::Apps
            | SlashCommand::Plugins
            | SlashCommand::Feedback
            | SlashCommand::Quit
            | SlashCommand::Exit
            | SlashCommand::Rollout
            | SlashCommand::TestApproval
            | SlashCommand::Realtime
            | SlashCommand::Collab
            | SlashCommand::Agent
            | SlashCommand::MultiAgents => true,
            SlashCommand::Statusline | SlashCommand::Title => false,
        }
    }

    fn is_visible(self) -> bool {
        match self {
            SlashCommand::SandboxReadRoot => cfg!(target_os = "windows"),
            SlashCommand::Copy => !cfg!(target_os = "android"),
            SlashCommand::Rollout | SlashCommand::TestApproval => cfg!(debug_assertions),
            SlashCommand::Clean => false,
            _ => true,
        }
    }
}

/// Return all built-in commands in a Vec paired with their command string.
pub fn built_in_slash_commands() -> Vec<(&'static str, SlashCommand)> {
    SlashCommand::iter()
        .filter(|command| command.is_visible())
        .map(|c| (c.command(), c))
        .collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    use super::SlashCommand;

    #[test]
    fn stop_command_is_canonical_name() {
        assert_eq!(SlashCommand::Stop.command(), "stop");
    }

    #[test]
    fn clean_alias_parses_to_stop_command() {
        assert_eq!(SlashCommand::from_str("clean"), Ok(SlashCommand::Stop));
    }
}
