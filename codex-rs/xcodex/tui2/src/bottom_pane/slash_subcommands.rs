use crate::slash_command::SlashCommand;
use crate::xcodex_plugins::PluginSubcommandNode;
use crate::xcodex_plugins::PluginSubcommandRoot;
use crate::xcodex_plugins::plugin_subcommand_roots;
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

const SETTINGS_TRANSCRIPT_CHILDREN: &[SubcommandNode] = &[
    SubcommandNode {
        token: "diff-highlight",
        full_name: "settings transcript diff-highlight",
        description: "toggle/show diff highlight for transcript diffs",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "highlight-past-prompts",
        full_name: "settings transcript highlight-past-prompts",
        description: "toggle/show past prompt highlights in transcript",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "syntax-highlight",
        full_name: "settings transcript syntax-highlight",
        description: "toggle/show syntax highlighting for fenced code blocks",
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
        token: "transcript",
        full_name: "settings transcript",
        description: "show or update transcript settings",
        run_on_enter: false,
        insert_trailing_space: true,
        children: SETTINGS_TRANSCRIPT_CHILDREN,
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
        token: "template",
        full_name: "theme template",
        description: "write example theme YAML files to themes.dir",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const MCP_RETRY_CHILDREN: &[SubcommandNode] = &[SubcommandNode {
    token: "failed",
    full_name: "mcp retry failed",
    description: "retry failed MCP servers",
    run_on_enter: true,
    insert_trailing_space: false,
    children: &[],
}];

const MCP_SUBCOMMANDS: &[SubcommandNode] = &[
    SubcommandNode {
        token: "list",
        full_name: "mcp list",
        description: "list configured MCP tools",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    SubcommandNode {
        token: "load",
        full_name: "mcp load",
        description: "start an MCP server on demand",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    SubcommandNode {
        token: "retry",
        full_name: "mcp retry",
        description: "retry MCP server startup",
        run_on_enter: false,
        insert_trailing_space: true,
        children: MCP_RETRY_CHILDREN,
    },
    SubcommandNode {
        token: "timeout",
        full_name: "mcp timeout",
        description: "set MCP startup timeout (seconds)",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
];

const SUBCOMMAND_ROOTS: &[SubcommandRoot] = &[
    SubcommandRoot {
        root: "mcp",
        anchor: SlashCommand::Mcp,
        children: MCP_SUBCOMMANDS,
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
        || plugin_subcommand_roots()
            .iter()
            .any(|root| root.root == name)
}

pub(crate) fn subcommand_list_hint(root: &str) -> Option<String> {
    let Some(root) = SUBCOMMAND_ROOTS.iter().find(|r| r.root == root) else {
        let root = plugin_subcommand_roots().iter().find(|r| r.root == root)?;
        return plugin_subcommand_list_hint(root);
    };

    if root.children.is_empty() {
        return None;
    }
    let children = root
        .children
        .iter()
        .map(|node| node.token)
        .collect::<Vec<_>>();
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
        let Some(root) = plugin_subcommand_roots()
            .iter()
            .find(|root| root.root == command_filter)
        else {
            return Vec::new();
        };
        return build_plugin_subcommand_matches(root, command_line);
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

fn build_plugin_subcommand_matches(
    root: &PluginSubcommandRoot,
    command_line: &str,
) -> Vec<(SlashCommand, Vec<SubcommandMatch>)> {
    let show_subcommands = match command_line.strip_prefix(root.root) {
        Some(rest) => rest.starts_with(char::is_whitespace),
        None => false,
    };
    if !show_subcommands {
        return Vec::new();
    }

    let matches = build_plugin_subcommand_matches_for_root(root, command_line);
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

fn build_plugin_subcommand_matches_for_root(
    root: &PluginSubcommandRoot,
    command_line: &str,
) -> Vec<SubcommandMatch> {
    let filter = command_line.trim_end();
    let tokens: Vec<&str> = command_line.split_whitespace().collect();
    if tokens.first().copied() != Some(root.root) {
        return Vec::new();
    }

    let has_trailing_space = command_line.ends_with(char::is_whitespace);
    let mut nodes = root.children;
    let mut leaf: Option<&PluginSubcommandNode> = None;

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

fn plugin_subcommand_list_hint(root: &PluginSubcommandRoot) -> Option<String> {
    if root.children.is_empty() {
        return None;
    }
    let mut children = root
        .children
        .iter()
        .map(|node| node.token)
        .collect::<Vec<_>>();
    if let Some(order) = root.list_hint_order {
        children.sort_by_key(|token| {
            order
                .iter()
                .find(|entry| entry.token == *token)
                .map(|entry| entry.order)
                .unwrap_or(usize::MAX)
        });
    }
    let children = children.join(", ");

    Some(format!("Type space for subcommands: {children}"))
}
