use crate::slash_command::SlashCommand;
use codex_common::fuzzy_match::fuzzy_match;

#[derive(Clone, Copy, Debug)]
struct SubcommandNode {
    token: &'static str,
    full_name: &'static str,
    description: &'static str,
    run_on_enter: bool,
    insert_trailing_space: bool,
    children: &'static [SubcommandNode],
}

#[derive(Clone, Copy, Debug)]
struct SubcommandRoot {
    root: &'static str,
    anchor: SlashCommand,
    children: &'static [SubcommandNode],
}

pub(crate) struct SubcommandMatch {
    pub(crate) full_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) run_on_enter: bool,
    pub(crate) insert_trailing_space: bool,
    pub(crate) indices: Option<Vec<usize>>,
    pub(crate) score: i32,
}

const WORKTREE_SHARED_CHILDREN: &[SubcommandNode] = &[
    SubcommandNode {
        token: "add",
        full_name: "worktree shared add",
        description: "add a repo-relative shared dir to config",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "rm",
        full_name: "worktree shared rm",
        description: "remove a shared dir from config",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "list",
        full_name: "worktree shared list",
        description: "show configured shared dirs",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const WORKTREE_LINK_SHARED_CHILDREN: &[SubcommandNode] = &[SubcommandNode {
    token: "--migrate",
    full_name: "worktree link-shared --migrate",
    description: "migrate untracked files into workspace root, then link",
    run_on_enter: true,
    insert_trailing_space: false,
    children: &[],
}];

const SETTINGS_STATUS_BAR_CHILDREN: &[SubcommandNode] = &[
    SubcommandNode {
        token: "git-branch",
        full_name: "settings status-bar git-branch",
        description: "toggle/show git branch in status bar",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "worktree",
        full_name: "settings status-bar worktree",
        description: "toggle/show worktree path in status bar",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
];

const SETTINGS_SUBCOMMANDS: &[SubcommandNode] = &[
    SubcommandNode {
        token: "status-bar",
        full_name: "settings status-bar",
        description: "show or update status bar settings",
        run_on_enter: false,
        insert_trailing_space: true,
        children: SETTINGS_STATUS_BAR_CHILDREN,
    },
    SubcommandNode {
        token: "worktrees",
        full_name: "settings worktrees",
        description: "open worktrees settings editor",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const THEME_SUBCOMMANDS: &[SubcommandNode] = &[
    SubcommandNode {
        token: "help",
        full_name: "theme help",
        description: "show theme role mapping and format details",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "create",
        full_name: "theme create",
        description: "create a new theme YAML by copying an existing theme",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "edit",
        full_name: "theme edit",
        description: "edit theme roles/palette and save as a new theme",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "preview",
        full_name: "theme preview",
        description: "show theme preview for the active theme",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "template",
        full_name: "theme template",
        description: "write example theme YAML files to themes.dir",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const WORKTREE_SUBCOMMANDS: &[SubcommandNode] = &[
    SubcommandNode {
        token: "detect",
        full_name: "worktree detect",
        description: "refresh git worktree list and open picker",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "doctor",
        full_name: "worktree doctor",
        description: "show shared-dir + untracked status for this worktree",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "link-shared",
        full_name: "worktree link-shared",
        description: "apply shared-dir links for this worktree",
        run_on_enter: true,
        insert_trailing_space: false,
        children: WORKTREE_LINK_SHARED_CHILDREN,
    },
    SubcommandNode {
        token: "init",
        full_name: "worktree init",
        description: "create a new worktree and switch to it",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "shared",
        full_name: "worktree shared",
        description: "manage `worktrees.shared_dirs` from the TUI",
        run_on_enter: false,
        insert_trailing_space: true,
        children: WORKTREE_SHARED_CHILDREN,
    },
];

const SUBCOMMAND_ROOTS: &[SubcommandRoot] = &[
    SubcommandRoot {
        root: "worktree",
        anchor: SlashCommand::Worktree,
        children: WORKTREE_SUBCOMMANDS,
    },
    SubcommandRoot {
        root: "settings",
        anchor: SlashCommand::Settings,
        children: SETTINGS_SUBCOMMANDS,
    },
    SubcommandRoot {
        root: "theme",
        anchor: SlashCommand::Theme,
        children: THEME_SUBCOMMANDS,
    },
];

pub(crate) fn slash_command_supports_subcommands(name: &str) -> bool {
    SUBCOMMAND_ROOTS.iter().any(|root| root.root == name)
}

pub(crate) fn subcommand_list_hint(root: &str) -> Option<String> {
    let root = SUBCOMMAND_ROOTS.iter().find(|r| r.root == root)?;
    if root.children.is_empty() {
        return None;
    }

    let mut children = root
        .children
        .iter()
        .map(|node| node.token)
        .collect::<Vec<_>>();
    if root.root == "worktree" {
        children.sort_by_key(|token| match *token {
            "detect" => 0,
            "doctor" => 1,
            "init" => 2,
            "shared" => 3,
            "link-shared" => 4,
            _ => 100,
        });
    }
    let children = children.join(", ");

    Some(format!("Type space for subcommands: {children}"))
}

pub(crate) fn build_subcommand_matches(
    command_filter: &str,
    command_line: &str,
) -> Vec<(SlashCommand, Vec<SubcommandMatch>)> {
    let Some(root) = SUBCOMMAND_ROOTS
        .iter()
        .find(|root| root.root == command_filter)
    else {
        return Vec::new();
    };

    let show_subcommands = match command_line.strip_prefix(root.root) {
        Some(rest) => rest.starts_with(char::is_whitespace),
        None => false,
    };
    if !show_subcommands {
        return Vec::new();
    }

    let matches = build_subcommand_matches_for_root(root, command_line);
    if matches.is_empty() {
        return Vec::new();
    }
    vec![(root.anchor, matches)]
}

fn build_subcommand_matches_for_root(
    root: &SubcommandRoot,
    command_line: &str,
) -> Vec<SubcommandMatch> {
    let filter = command_line.trim_end();
    let tokens: Vec<&str> = command_line.split_whitespace().collect();
    if tokens.first().copied() != Some(root.root) {
        return Vec::new();
    }

    let has_trailing_space = command_line.ends_with(char::is_whitespace);
    let mut nodes = root.children;
    let mut leaf: Option<&SubcommandNode> = None;

    let mut remaining = tokens.get(1..).unwrap_or(&[]);
    if !has_trailing_space && !remaining.is_empty() {
        remaining = &remaining[..remaining.len().saturating_sub(1)];
    }

    for token in remaining {
        if nodes.is_empty() {
            break;
        }
        let Some(next) = nodes.iter().find(|node| node.token == *token) else {
            return Vec::new();
        };
        leaf = Some(next);
        nodes = next.children;
    }

    if nodes.is_empty()
        && let Some(node) = leaf
    {
        return vec![SubcommandMatch {
            full_name: node.full_name,
            description: node.description,
            run_on_enter: node.run_on_enter,
            insert_trailing_space: node.insert_trailing_space,
            indices: None,
            score: 0,
        }];
    }

    nodes
        .iter()
        .filter_map(|node| {
            fuzzy_match(node.full_name, filter).map(|(indices, score)| SubcommandMatch {
                full_name: node.full_name,
                description: node.description,
                run_on_enter: node.run_on_enter,
                insert_trailing_space: node.insert_trailing_space,
                indices: Some(indices),
                score,
            })
        })
        .collect()
}
