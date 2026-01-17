pub(crate) struct SlashArgHintProvider {
    pub(crate) root: &'static str,
    pub(crate) hint_for_subcommand: fn(full_name: &str, command_line: &str) -> Option<String>,
}

const PROVIDERS: &[SlashArgHintProvider] = &[SlashArgHintProvider {
    root: "worktree",
    hint_for_subcommand: worktree_hint_for_subcommand,
}];

pub(crate) fn slash_command_supports_arg_hints(root: &str) -> bool {
    PROVIDERS.iter().any(|p| p.root == root)
}

pub(crate) fn hint_for_subcommand(full_name: &str, command_line: &str) -> Option<String> {
    let root = full_name.split_whitespace().next()?;
    let provider = PROVIDERS.iter().find(|p| p.root == root)?;
    (provider.hint_for_subcommand)(full_name, command_line)
}

fn worktree_hint_for_subcommand(full_name: &str, command_line: &str) -> Option<String> {
    match full_name {
        "worktree init" => {
            let usage = "Usage: /worktree init <name> <branch> [<path>]";
            let next = worktree_init_next_arg(command_line);
            let example = match next {
                Some("<name>") => Some("e.g. feat-singularity"),
                Some("<branch>") => Some("e.g. main"),
                Some("[<path>]") => Some("e.g. .worktrees/<slug>"),
                _ => None,
            };

            Some(match (next, example) {
                (Some(next), Some(example)) => format!("{usage}  Next: {next} ({example})"),
                (Some(next), None) => format!("{usage}  Next: {next}"),
                (None, _) => usage.to_string(),
            })
        }
        "worktree shared add" => Some("Usage: /worktree shared add <dir>".to_string()),
        "worktree shared rm" => Some("Usage: /worktree shared rm <dir>".to_string()),
        _ => None,
    }
}

fn worktree_init_next_arg(command_line: &str) -> Option<&'static str> {
    let tokens: Vec<&str> = command_line.split_whitespace().collect();
    if tokens.get(0..2) != Some(["worktree", "init"].as_slice()) {
        return None;
    }

    let has_trailing_space = command_line.ends_with(char::is_whitespace);
    let args = tokens.get(2..).unwrap_or_default();

    match (args.len(), has_trailing_space) {
        (0, _) => Some("<name>"),
        (1, true) => Some("<branch>"),
        (2, true) => Some("[<path>]"),
        _ => None,
    }
}
