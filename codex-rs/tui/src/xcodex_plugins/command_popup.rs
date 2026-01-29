use crate::slash_command::SlashCommand;
use crate::xcodex_plugins::PluginSlashCommand;
use crate::xcodex_plugins::plugin_slash_commands;
use codex_common::fuzzy_match::fuzzy_match;
use codex_protocol::custom_prompts::CustomPrompt;
use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct ArgCompletion {
    pub(crate) display: String,
    pub(crate) insert: String,
    pub(crate) description: Option<String>,
    pub(crate) insert_trailing_space: bool,
    pub(crate) indices: Option<Vec<usize>>,
}

pub(crate) fn popup_plugin_commands() -> Vec<PluginSlashCommand> {
    plugin_slash_commands().to_vec()
}

pub(crate) fn filter_prompts_for_popup(
    prompts: &mut Vec<CustomPrompt>,
    builtins: &[(&'static str, SlashCommand)],
    plugin_commands: &[PluginSlashCommand],
) {
    let exclude: HashSet<String> = builtins
        .iter()
        .map(|(name, _)| (*name).to_string())
        .chain(
            plugin_commands
                .iter()
                .map(|command| command.name.to_string()),
        )
        .collect();
    prompts.retain(|prompt| !exclude.contains(&prompt.name));
    prompts.sort_by(|a, b| a.name.cmp(&b.name));
}

pub(crate) fn worktree_init_completions(
    command_line: &str,
    current_git_branch: Option<&str>,
    slash_completion_branches: &[String],
) -> Vec<ArgCompletion> {
    let tokens: Vec<&str> = command_line.split_whitespace().collect();
    if tokens.get(0..2) != Some(["worktree", "init"].as_slice()) {
        return Vec::new();
    }

    let has_trailing_space = command_line.ends_with(|ch: char| ch.is_whitespace());
    let args = tokens.get(2..).unwrap_or_default();

    let (arg_index, partial) = match (args.len(), has_trailing_space) {
        (1, true) => (1, ""),
        (2, false) => (1, args[1]),
        (2, true) => (2, ""),
        (3, false) => (2, args[2]),
        _ => return Vec::new(),
    };

    match arg_index {
        1 => {
            worktree_init_branch_completions(current_git_branch, slash_completion_branches, partial)
        }
        2 => worktree_init_path_completions(args[0], partial),
        _ => Vec::new(),
    }
}

fn worktree_init_branch_completions(
    current_git_branch: Option<&str>,
    slash_completion_branches: &[String],
    partial: &str,
) -> Vec<ArgCompletion> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(branch) = current_git_branch
        && !branch.is_empty()
        && branch != "(detached)"
    {
        candidates.push(branch.to_string());
    }

    if let Some(base) = slash_completion_branches.first() {
        candidates.push(base.clone());
    }

    candidates.extend(slash_completion_branches.iter().take(12).cloned());
    candidates.push(String::from("main"));
    candidates.push(String::from("master"));

    let mut seen: HashSet<String> = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));

    let mut matches: Vec<(String, Option<Vec<usize>>, i32)> = Vec::new();
    for candidate in candidates {
        if partial.is_empty() {
            matches.push((candidate, None, 0));
        } else if let Some((indices, score)) = fuzzy_match(&candidate, partial) {
            matches.push((candidate, Some(indices), score));
        }
    }

    matches.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    matches
        .into_iter()
        .take(5)
        .map(|(candidate, indices, _score)| ArgCompletion {
            display: candidate.clone(),
            insert: candidate,
            description: Some(String::from("insert branch")),
            insert_trailing_space: true,
            indices,
        })
        .collect()
}

fn worktree_init_path_completions(name: &str, partial: &str) -> Vec<ArgCompletion> {
    let slug = sanitize_worktree_path_slug(name);
    let candidate = format!(
        ".worktrees/{}",
        if slug.is_empty() { "worktree" } else { &slug }
    );

    if partial.is_empty() {
        return vec![ArgCompletion {
            display: candidate.clone(),
            insert: candidate,
            description: Some(String::from("default path")),
            insert_trailing_space: false,
            indices: None,
        }];
    }

    fuzzy_match(&candidate, partial)
        .map(|(indices, _score)| {
            vec![ArgCompletion {
                display: candidate.clone(),
                insert: candidate,
                description: Some(String::from("default path")),
                insert_trailing_space: false,
                indices: Some(indices),
            }]
        })
        .unwrap_or_default()
}

fn sanitize_worktree_path_slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        match ch {
            '/' | '\\' => out.push('-'),
            ' ' | '\t' | '\n' | '\r' => out.push('-'),
            other => out.push(other),
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}
