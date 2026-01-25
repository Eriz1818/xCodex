use crate::slash_command::SlashCommand;

use super::PluginSubcommandHintOrder;
use super::PluginSubcommandNode;
use super::PluginSubcommandRoot;

const WORKTREE_SHARED_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "add",
        full_name: "worktree shared add",
        description: "add a repo-relative shared dir to config",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "rm",
        full_name: "worktree shared rm",
        description: "remove a shared dir from config",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "list",
        full_name: "worktree shared list",
        description: "show configured shared dirs",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const WORKTREE_LINK_SHARED_CHILDREN: &[PluginSubcommandNode] = &[PluginSubcommandNode {
    token: "--migrate",
    full_name: "worktree link-shared --migrate",
    description: "migrate untracked files into workspace root, then link",
    run_on_enter: true,
    insert_trailing_space: false,
    children: &[],
}];

const WORKTREE_SUBCOMMANDS: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "detect",
        full_name: "worktree detect",
        description: "refresh git worktree list and open picker",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "doctor",
        full_name: "worktree doctor",
        description: "show shared-dir + untracked status for this worktree",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "link-shared",
        full_name: "worktree link-shared",
        description: "apply shared-dir links for this worktree",
        run_on_enter: true,
        insert_trailing_space: false,
        children: WORKTREE_LINK_SHARED_CHILDREN,
    },
    PluginSubcommandNode {
        token: "init",
        full_name: "worktree init",
        description: "create a new worktree and switch to it",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "shared",
        full_name: "worktree shared",
        description: "manage `worktrees.shared_dirs` from the TUI",
        run_on_enter: false,
        insert_trailing_space: true,
        children: WORKTREE_SHARED_CHILDREN,
    },
];

const WORKTREE_HINT_ORDER: &[PluginSubcommandHintOrder] = &[
    PluginSubcommandHintOrder {
        token: "detect",
        order: 0,
    },
    PluginSubcommandHintOrder {
        token: "doctor",
        order: 1,
    },
    PluginSubcommandHintOrder {
        token: "init",
        order: 2,
    },
    PluginSubcommandHintOrder {
        token: "shared",
        order: 3,
    },
    PluginSubcommandHintOrder {
        token: "link-shared",
        order: 4,
    },
];

pub(crate) const WORKTREE_SUBCOMMAND_ROOT: PluginSubcommandRoot = PluginSubcommandRoot {
    root: "worktree",
    anchor: SlashCommand::Worktree,
    children: WORKTREE_SUBCOMMANDS,
    list_hint_order: Some(WORKTREE_HINT_ORDER),
};
