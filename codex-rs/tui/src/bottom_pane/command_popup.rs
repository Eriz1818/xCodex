use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;
use super::slash_arg_hints;
use super::slash_subcommands::SubcommandMatch;
use super::slash_subcommands::build_subcommand_matches;
use super::slash_subcommands::slash_command_supports_subcommands as subcommands_supported;
use super::slash_subcommands::subcommand_list_hint;
use crate::render::Insets;
use crate::render::RectExt;
use crate::slash_command::SlashCommand;
use crate::slash_command::built_in_slash_commands;
use codex_common::fuzzy_match::fuzzy_match;
use codex_protocol::custom_prompts::CustomPrompt;
use codex_protocol::custom_prompts::PROMPTS_CMD_PREFIX;
use std::collections::HashSet;

fn windows_degraded_sandbox_active() -> bool {
    cfg!(target_os = "windows")
        && codex_core::windows_sandbox::ELEVATED_SANDBOX_NUX_ENABLED
        && codex_core::get_platform_sandbox().is_some()
        && !codex_core::is_windows_elevated_sandbox_enabled()
}

/// A selectable item in the popup: either a built-in command or a user prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommandItem {
    Builtin(SlashCommand),
    BuiltinText {
        /// Command string without the leading '/' (may include spaces).
        name: &'static str,
        description: &'static str,
        /// When true, pressing Enter on this suggestion runs it immediately.
        run_on_enter: bool,
        /// When true, completion appends a trailing space to invite additional args.
        insert_trailing_space: bool,
    },
    ArgValue {
        display: String,
        insert: String,
        description: Option<String>,
        insert_trailing_space: bool,
    },
    // Index into `prompts`
    UserPrompt(usize),
}

pub(crate) fn slash_command_supports_subcommands(name: &str) -> bool {
    subcommands_supported(name)
}

pub(crate) fn slash_command_supports_popup(name: &str) -> bool {
    slash_command_supports_subcommands(name)
        || slash_arg_hints::slash_command_supports_arg_hints(name)
}

pub(crate) struct CommandPopup {
    command_filter: String,
    command_line: String,
    builtins: Vec<(&'static str, SlashCommand)>,
    prompts: Vec<CustomPrompt>,
    slash_completion_branches: Vec<String>,
    current_git_branch: Option<String>,
    state: ScrollState,
}

impl CommandPopup {
    pub(crate) fn new(mut prompts: Vec<CustomPrompt>, skills_enabled: bool) -> Self {
        let allow_elevate_sandbox = windows_degraded_sandbox_active();
        let builtins: Vec<(&'static str, SlashCommand)> = built_in_slash_commands()
            .into_iter()
            .filter(|(_, cmd)| skills_enabled || *cmd != SlashCommand::Skills)
            .filter(|(_, cmd)| allow_elevate_sandbox || *cmd != SlashCommand::ElevateSandbox)
            .collect();
        // Exclude prompts that collide with builtin command names and sort by name.
        let exclude: HashSet<String> = builtins.iter().map(|(n, _)| (*n).to_string()).collect();
        prompts.retain(|p| !exclude.contains(&p.name));
        prompts.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            command_filter: String::new(),
            command_line: String::new(),
            builtins,
            prompts,
            slash_completion_branches: Vec::new(),
            current_git_branch: None,
            state: ScrollState::new(),
        }
    }

    pub(crate) fn set_prompts(&mut self, mut prompts: Vec<CustomPrompt>) {
        let exclude: HashSet<String> = self
            .builtins
            .iter()
            .map(|(n, _)| (*n).to_string())
            .collect();
        prompts.retain(|p| !exclude.contains(&p.name));
        prompts.sort_by(|a, b| a.name.cmp(&b.name));
        self.prompts = prompts;
    }

    pub(crate) fn set_slash_completion_branches(&mut self, branches: Vec<String>) {
        self.slash_completion_branches = branches;
    }

    pub(crate) fn set_current_git_branch(&mut self, branch: Option<String>) {
        self.current_git_branch = branch;
    }

    pub(crate) fn prompt(&self, idx: usize) -> Option<&CustomPrompt> {
        self.prompts.get(idx)
    }

    /// Update the filter string based on the current composer text. The text
    /// passed in is expected to start with a leading '/'. Everything after the
    /// *first* '/" on the *first* line becomes the active filter that is used
    /// to narrow down the list of available commands.
    pub(crate) fn on_composer_text_change(&mut self, text: String) {
        let first_line = text.lines().next().unwrap_or("");

        if let Some(stripped) = first_line.strip_prefix('/') {
            // Extract the *first* token (sequence of non-whitespace
            // characters) after the slash so that `/clear something` still
            // shows the help for `/clear`.
            let token = stripped.trim_start();
            let cmd_token = token.split_whitespace().next().unwrap_or("");

            // Update the filter keeping the original case (commands are all
            // lower-case for now but this may change in the future).
            self.command_filter = cmd_token.to_string();
            self.command_line = token.to_string();
        } else {
            // The composer no longer starts with '/'. Reset the filter so the
            // popup shows the *full* command list if it is still displayed
            // for some reason.
            self.command_filter.clear();
            self.command_line.clear();
        }

        // Reset or clamp selected index based on new filtered list.
        let matches = self.filtered();
        let matches_len = matches.len();
        self.state.clamp_selection(matches_len);
        if let Some(idx) = matches
            .iter()
            .position(|(item, _, _)| matches!(item, CommandItem::ArgValue { .. }))
        {
            self.state.selected_idx = Some(idx);
        } else if self.should_default_select_subcommand()
            && let Some(idx) = matches
                .iter()
                .position(|(item, _, _)| matches!(item, CommandItem::BuiltinText { .. }))
        {
            self.state.selected_idx = Some(idx);
        }
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    fn should_default_select_subcommand(&self) -> bool {
        !build_subcommand_matches(&self.command_filter, &self.command_line).is_empty()
    }

    /// Determine the preferred height of the popup for a given width.
    /// Accounts for wrapped descriptions so that long tooltips don't overflow.
    pub(crate) fn calculate_required_height(&self, width: u16) -> u16 {
        use super::selection_popup_common::measure_rows_height;
        let rows = self.rows_from_matches(self.filtered());

        measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, width)
    }

    /// Compute fuzzy-filtered matches over built-in commands and user prompts,
    /// paired with optional highlight indices and score. Sorted by ascending
    /// score, then by name for stability.
    fn filtered(&self) -> Vec<(CommandItem, Option<Vec<usize>>, i32)> {
        let filter = self.command_filter.trim();
        let mut out: Vec<(CommandItem, Option<Vec<usize>>, i32)> = Vec::new();
        let mut subcommand_matches_by_anchor: Vec<(SlashCommand, Vec<SubcommandMatch>)> =
            Vec::new();
        if filter.is_empty() {
            // Built-ins first, in presentation order.
            for (_, cmd) in self.builtins.iter() {
                out.push((CommandItem::Builtin(*cmd), None, 0));
            }
            // Then prompts, already sorted by name.
            for idx in 0..self.prompts.len() {
                out.push((CommandItem::UserPrompt(idx), None, 0));
            }
            return out;
        }

        for (_, cmd) in self.builtins.iter() {
            if let Some((indices, score)) = fuzzy_match(cmd.command(), filter) {
                out.push((CommandItem::Builtin(*cmd), Some(indices), score));
            }
        }

        for (anchor, matches) in
            build_subcommand_matches(&self.command_filter, &self.command_line).into_iter()
        {
            // When the user has entered a subcommand context (e.g. `/worktree ...`),
            // prefer showing subcommands over the root command to reduce confusion.
            out.retain(|(item, _, _)| !matches!(item, CommandItem::Builtin(cmd) if *cmd == anchor));
            subcommand_matches_by_anchor.push((anchor, matches));
        }
        // Support both search styles:
        // - Typing "name" should surface "/prompts:name" results.
        // - Typing "prompts:name" should also work.
        for (idx, p) in self.prompts.iter().enumerate() {
            let display = format!("{PROMPTS_CMD_PREFIX}:{}", p.name);
            let display_match = fuzzy_match(&display, filter);
            let name_match = fuzzy_match(&p.name, filter).map(|(indices, score)| {
                let offset = PROMPTS_CMD_PREFIX.len() + 1;
                (indices.into_iter().map(|idx| idx + offset).collect(), score)
            });

            let best = match (display_match, name_match) {
                (Some((indices, score)), Some((indices2, score2))) => {
                    if score2 < score {
                        Some((indices2, score2))
                    } else {
                        Some((indices, score))
                    }
                }
                (Some((indices, score)), None) => Some((indices, score)),
                (None, Some((indices, score))) => Some((indices, score)),
                (None, None) => None,
            };

            if let Some((indices, score)) = best {
                out.push((CommandItem::UserPrompt(idx), Some(indices), score));
            }
        }
        // When filtering, sort by ascending score and then by name for stability.
        out.sort_by(|a, b| {
            a.2.cmp(&b.2).then_with(|| {
                let an = match &a.0 {
                    CommandItem::Builtin(c) => c.command(),
                    CommandItem::BuiltinText { name, .. } => *name,
                    CommandItem::ArgValue { display, .. } => display.as_str(),
                    CommandItem::UserPrompt(i) => self.prompts[*i].name.as_str(),
                };
                let bn = match &b.0 {
                    CommandItem::Builtin(c) => c.command(),
                    CommandItem::BuiltinText { name, .. } => *name,
                    CommandItem::ArgValue { display, .. } => display.as_str(),
                    CommandItem::UserPrompt(i) => self.prompts[*i].name.as_str(),
                };
                an.cmp(bn)
            })
        });

        out.extend(self.arg_value_completions());

        for (_anchor, mut matches) in subcommand_matches_by_anchor {
            if matches.len() > 1 {
                matches.sort_by(|a, b| {
                    a.score
                        .cmp(&b.score)
                        .then_with(|| a.full_name.cmp(b.full_name))
                });
            }

            out.extend(matches.into_iter().map(|m| {
                (
                    CommandItem::BuiltinText {
                        name: m.full_name,
                        description: m.description,
                        run_on_enter: m.run_on_enter,
                        insert_trailing_space: m.insert_trailing_space,
                    },
                    m.indices,
                    m.score,
                )
            }));
        }
        out
    }

    fn filtered_items(&self) -> Vec<CommandItem> {
        self.filtered().into_iter().map(|(c, _, _)| c).collect()
    }

    fn rows_from_matches(
        &self,
        matches: Vec<(CommandItem, Option<Vec<usize>>, i32)>,
    ) -> Vec<GenericDisplayRow> {
        matches
            .into_iter()
            .map(|(item, indices, _)| {
                let (name, description) = match item {
                    CommandItem::Builtin(cmd) => {
                        (format!("/{}", cmd.command()), self.builtin_description(cmd))
                    }
                    CommandItem::BuiltinText {
                        name, description, ..
                    } => {
                        let description = if let Some(hint) = self.subcommand_hint(name) {
                            format!("{description}  {hint}")
                        } else {
                            description.to_string()
                        };
                        (format!("/{name}"), description)
                    }
                    CommandItem::ArgValue {
                        display,
                        description,
                        ..
                    } => (display, description.unwrap_or_default()),
                    CommandItem::UserPrompt(i) => {
                        let prompt = &self.prompts[i];
                        let description = prompt
                            .description
                            .clone()
                            .unwrap_or_else(|| "send saved prompt".to_string());
                        (
                            format!("/{PROMPTS_CMD_PREFIX}:{}", prompt.name),
                            description,
                        )
                    }
                };
                GenericDisplayRow {
                    name,
                    match_indices: indices.map(|v| v.into_iter().map(|i| i + 1).collect()),
                    display_shortcut: None,
                    description: Some(description),
                    wrap_indent: None,
                    disabled_reason: None,
                }
            })
            .collect()
    }

    fn builtin_description(&self, cmd: SlashCommand) -> String {
        let mut description = cmd.description().to_string();
        if self.command_line.trim_end() == cmd.command() && subcommands_supported(cmd.command()) {
            description.push_str("  ");
            if let Some(hint) = subcommand_list_hint(cmd.command()) {
                description.push_str(&hint);
            }
        }
        description
    }

    fn subcommand_hint(&self, full_name: &str) -> Option<String> {
        slash_arg_hints::hint_for_subcommand(full_name, &self.command_line)
    }

    fn arg_value_completions(&self) -> Vec<(CommandItem, Option<Vec<usize>>, i32)> {
        let tokens: Vec<&str> = self.command_line.split_whitespace().collect();
        if tokens.get(0..2) != Some(["worktree", "init"].as_slice()) {
            return Vec::new();
        }

        let has_trailing_space = self.command_line.ends_with(char::is_whitespace);
        let args = tokens.get(2..).unwrap_or_default();

        let (arg_index, partial) = match (args.len(), has_trailing_space) {
            (1, true) => (1, ""),
            (2, false) => (1, args[1]),
            (2, true) => (2, ""),
            (3, false) => (2, args[2]),
            _ => return Vec::new(),
        };

        match arg_index {
            1 => self.worktree_init_branch_completions(partial),
            2 => self.worktree_init_path_completions(args[0], partial),
            _ => Vec::new(),
        }
    }

    fn worktree_init_branch_completions(
        &self,
        partial: &str,
    ) -> Vec<(CommandItem, Option<Vec<usize>>, i32)> {
        let mut candidates: Vec<String> = Vec::new();
        if let Some(branch) = self.current_git_branch.as_deref()
            && !branch.is_empty()
            && branch != "(detached)"
        {
            candidates.push(branch.to_string());
        }

        if let Some(base) = self.slash_completion_branches.first() {
            candidates.push(base.clone());
        }

        candidates.extend(self.slash_completion_branches.iter().take(12).cloned());
        candidates.push(String::from("main"));
        candidates.push(String::from("master"));

        let mut seen: HashSet<String> = HashSet::new();
        candidates.retain(|c| seen.insert(c.clone()));

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
            .map(|(candidate, indices, score)| {
                (
                    CommandItem::ArgValue {
                        display: candidate.clone(),
                        insert: candidate,
                        description: Some(String::from("insert branch")),
                        insert_trailing_space: true,
                    },
                    indices,
                    score,
                )
            })
            .collect()
    }

    fn worktree_init_path_completions(
        &self,
        name: &str,
        partial: &str,
    ) -> Vec<(CommandItem, Option<Vec<usize>>, i32)> {
        let slug = sanitize_worktree_path_slug(name);
        let candidate = format!(
            ".worktrees/{}",
            if slug.is_empty() { "worktree" } else { &slug }
        );

        if partial.is_empty() {
            return vec![(
                CommandItem::ArgValue {
                    display: candidate.clone(),
                    insert: candidate,
                    description: Some(String::from("default path")),
                    insert_trailing_space: false,
                },
                None,
                0,
            )];
        }

        fuzzy_match(&candidate, partial)
            .map(|(indices, score)| {
                vec![(
                    CommandItem::ArgValue {
                        display: candidate.clone(),
                        insert: candidate,
                        description: Some(String::from("default path")),
                        insert_trailing_space: false,
                    },
                    Some(indices),
                    score,
                )]
            })
            .unwrap_or_default()
    }

    /// Move the selection cursor one step up.
    pub(crate) fn move_up(&mut self) {
        let len = self.filtered_items().len();
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    /// Move the selection cursor one step down.
    pub(crate) fn move_down(&mut self) {
        let matches_len = self.filtered_items().len();
        self.state.move_down_wrap(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    /// Return currently selected command, if any.
    pub(crate) fn selected_item(&self) -> Option<CommandItem> {
        let matches = self.filtered_items();
        self.state
            .selected_idx
            .and_then(|idx| matches.get(idx).cloned())
    }
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

impl WidgetRef for CommandPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let rows = self.rows_from_matches(self.filtered());
        render_rows(
            area.inset(Insets::tlbr(0, 2, 0, 0)),
            buf,
            &rows,
            &self.state,
            MAX_POPUP_ROWS,
            "no matches",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn filter_includes_init_when_typing_prefix() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        // Simulate the composer line starting with '/in' so the popup filters
        // matching commands by prefix.
        popup.on_composer_text_change("/in".to_string());

        // Access the filtered list via the selected command and ensure that
        // one of the matches is the new "init" command.
        let matches = popup.filtered_items();
        let has_init = matches.iter().any(|item| match item {
            CommandItem::Builtin(cmd) => cmd.command() == "init",
            CommandItem::BuiltinText { .. } => false,
            CommandItem::ArgValue { .. } => false,
            CommandItem::UserPrompt(_) => false,
        });
        assert!(
            has_init,
            "expected '/init' to appear among filtered commands"
        );
    }

    #[test]
    fn selecting_init_by_exact_match() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/init".to_string());

        // When an exact match exists, the selected command should be that
        // command by default.
        let selected = popup.selected_item();
        match selected {
            Some(CommandItem::Builtin(cmd)) => assert_eq!(cmd.command(), "init"),
            Some(CommandItem::BuiltinText { .. }) => {
                panic!("unexpected builtin-text suggestion selected for '/init'")
            }
            Some(CommandItem::ArgValue { .. }) => {
                panic!("unexpected arg-value suggestion selected for '/init'")
            }
            Some(CommandItem::UserPrompt(_)) => panic!("unexpected prompt selected for '/init'"),
            None => panic!("expected a selected command for exact match"),
        }
    }

    #[test]
    fn model_is_first_suggestion_for_mo() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/mo".to_string());
        let matches = popup.filtered_items();
        match matches.first() {
            Some(CommandItem::Builtin(cmd)) => assert_eq!(cmd.command(), "model"),
            Some(CommandItem::BuiltinText { .. }) => {
                panic!("unexpected builtin-text suggestion ranked before '/model' for '/mo'")
            }
            Some(CommandItem::ArgValue { .. }) => {
                panic!("unexpected arg-value suggestion ranked before '/model' for '/mo'")
            }
            Some(CommandItem::UserPrompt(_)) => {
                panic!("unexpected prompt ranked before '/model' for '/mo'")
            }
            None => panic!("expected at least one match for '/mo'"),
        }
    }

    #[test]
    fn prompt_discovery_lists_custom_prompts() {
        let prompts = vec![
            CustomPrompt {
                name: "foo".to_string(),
                path: "/tmp/foo.md".to_string().into(),
                content: "hello from foo".to_string(),
                description: None,
                argument_hint: None,
            },
            CustomPrompt {
                name: "bar".to_string(),
                path: "/tmp/bar.md".to_string().into(),
                content: "hello from bar".to_string(),
                description: None,
                argument_hint: None,
            },
        ];
        let popup = CommandPopup::new(prompts, false);
        let items = popup.filtered_items();
        let mut prompt_names: Vec<String> = items
            .into_iter()
            .filter_map(|it| match it {
                CommandItem::UserPrompt(i) => popup.prompt(i).map(|p| p.name.clone()),
                _ => None,
            })
            .collect();
        prompt_names.sort();
        assert_eq!(prompt_names, vec!["bar".to_string(), "foo".to_string()]);
    }

    #[test]
    fn prompt_name_collision_with_builtin_is_ignored() {
        // Create a prompt named like a builtin (e.g. "init").
        let popup = CommandPopup::new(
            vec![CustomPrompt {
                name: "init".to_string(),
                path: "/tmp/init.md".to_string().into(),
                content: "should be ignored".to_string(),
                description: None,
                argument_hint: None,
            }],
            false,
        );
        let items = popup.filtered_items();
        let has_collision_prompt = items.into_iter().any(|it| match it {
            CommandItem::UserPrompt(i) => popup.prompt(i).is_some_and(|p| p.name == "init"),
            _ => false,
        });
        assert!(
            !has_collision_prompt,
            "prompt with builtin name should be ignored"
        );
    }

    #[test]
    fn prompt_description_uses_frontmatter_metadata() {
        let popup = CommandPopup::new(
            vec![CustomPrompt {
                name: "draftpr".to_string(),
                path: "/tmp/draftpr.md".to_string().into(),
                content: "body".to_string(),
                description: Some("Create feature branch, commit and open draft PR.".to_string()),
                argument_hint: None,
            }],
            false,
        );
        let rows = popup.rows_from_matches(vec![(CommandItem::UserPrompt(0), None, 0)]);
        let description = rows.first().and_then(|row| row.description.as_deref());
        assert_eq!(
            description,
            Some("Create feature branch, commit and open draft PR.")
        );
    }

    #[test]
    fn prompt_description_falls_back_when_missing() {
        let popup = CommandPopup::new(
            vec![CustomPrompt {
                name: "foo".to_string(),
                path: "/tmp/foo.md".to_string().into(),
                content: "body".to_string(),
                description: None,
                argument_hint: None,
            }],
            false,
        );
        let rows = popup.rows_from_matches(vec![(CommandItem::UserPrompt(0), None, 0)]);
        let description = rows.first().and_then(|row| row.description.as_deref());
        assert_eq!(description, Some("send saved prompt"));
    }

    #[test]
    fn prompt_is_suggested_when_filter_matches_prompt_name() {
        let mut popup = CommandPopup::new(
            vec![CustomPrompt {
                name: "my-prompt".to_string(),
                path: "/tmp/my-prompt.md".to_string().into(),
                content: "hello from prompt".to_string(),
                description: None,
                argument_hint: None,
            }],
            false,
        );
        popup.on_composer_text_change("/my".to_string());
        let items = popup.filtered_items();
        let has_prompt = items
            .into_iter()
            .any(|item| matches!(item, CommandItem::UserPrompt(_)));
        assert!(has_prompt, "expected /my to suggest the custom prompt");
    }

    #[test]
    fn fuzzy_filter_matches_subsequence_for_ac() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/ac".to_string());

        let cmds: Vec<&str> = popup
            .filtered_items()
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::Builtin(cmd) => Some(cmd.command()),
                CommandItem::BuiltinText { .. } => None,
                CommandItem::ArgValue { .. } => None,
                CommandItem::UserPrompt(_) => None,
            })
            .collect();
        assert!(
            cmds.contains(&"compact") && cmds.contains(&"feedback"),
            "expected fuzzy search for '/ac' to include compact and feedback, got {cmds:?}"
        );
    }

    #[test]
    fn worktree_subcommands_are_suggested_under_worktree() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree ".to_string());

        let items = popup.filtered_items();
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, CommandItem::Builtin(SlashCommand::Worktree))),
            "expected /worktree root command to be hidden in subcommand context"
        );
        assert!(
            items
                .iter()
                .any(|item| matches!(item, CommandItem::BuiltinText { .. })),
            "expected at least one /worktree subcommand suggestion under /worktree"
        );
    }

    #[test]
    fn settings_subcommands_are_suggested_under_settings() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/settings ".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"settings status-bar")
                && subcommands.contains(&"settings worktrees"),
            "expected /settings to suggest subcommands, got {subcommands:?}"
        );
    }

    #[test]
    fn settings_nested_subcommands_are_suggested_under_status_bar() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/settings status-bar ".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"settings status-bar git-branch")
                && subcommands.contains(&"settings status-bar worktree"),
            "expected /settings status-bar to suggest nested subcommands, got {subcommands:?}"
        );
    }

    #[test]
    fn worktree_subcommands_are_hidden_until_space() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree".to_string());

        let items = popup.filtered_items();
        assert!(
            !items.iter().any(|item| {
                matches!(item, CommandItem::BuiltinText { name, .. } if name.starts_with("worktree "))
            }),
            "expected no /worktree subcommand suggestions without a trailing space"
        );
    }

    #[test]
    fn worktree_subcommands_filter_by_prefix() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree d".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"worktree detect") && subcommands.contains(&"worktree doctor"),
            "expected /worktree d to suggest detect/doctor, got {subcommands:?}"
        );
    }

    #[test]
    fn worktree_nested_subcommands_are_suggested_under_shared() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree shared ".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"worktree shared add")
                && subcommands.contains(&"worktree shared rm")
                && subcommands.contains(&"worktree shared list"),
            "expected /worktree shared to suggest nested subcommands, got {subcommands:?}"
        );
        assert!(
            !subcommands.contains(&"worktree detect"),
            "expected /worktree shared suggestions to be scoped (no detect), got {subcommands:?}"
        );
    }

    #[test]
    fn worktree_leaf_subcommand_stays_visible_while_typing_args() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree shared add docs/impl-plans".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"worktree shared add"),
            "expected leaf subcommand to stay visible while typing args, got {subcommands:?}"
        );
    }

    #[test]
    fn worktree_leaf_subcommand_stays_visible_after_trailing_space_and_args() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree init foo ".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"worktree init"),
            "expected leaf subcommand to stay visible after a trailing space and args, got {subcommands:?}"
        );
    }

    #[test]
    fn worktree_init_description_includes_next_arg_hint() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree init foo ".to_string());

        let rows = popup.rows_from_matches(popup.filtered());
        let init = rows
            .iter()
            .find(|row| row.name == "/worktree init")
            .and_then(|row| row.description.as_deref())
            .unwrap_or_default();

        assert!(
            init.contains("Next: <branch>"),
            "expected /worktree init row to include next-arg hint, got {init:?}"
        );
    }

    #[test]
    fn worktree_init_branch_arg_suggests_branches() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.set_slash_completion_branches(vec!["main".to_string(), "feature".to_string()]);
        popup.set_current_git_branch(Some("feature".to_string()));
        popup.on_composer_text_change("/worktree init foo ".to_string());

        let items = popup.filtered_items();
        let values: Vec<String> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::ArgValue { display, .. } => Some(display),
                _ => None,
            })
            .collect();

        assert!(
            values.contains(&"feature".to_string()) && values.contains(&"main".to_string()),
            "expected branch suggestions to include current and default branches, got {values:?}"
        );
    }

    #[test]
    fn worktree_init_path_arg_suggests_default_path() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree init feat/x main ".to_string());

        let items = popup.filtered_items();
        let values: Vec<String> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::ArgValue { display, .. } => Some(display),
                _ => None,
            })
            .collect();

        assert!(
            values.contains(&".worktrees/feat-x".to_string()),
            "expected path suggestions to include default .worktrees slug, got {values:?}"
        );
    }

    #[test]
    fn default_selection_prefers_subcommands_in_worktree_context() {
        let mut popup = CommandPopup::new(Vec::new(), false);
        popup.on_composer_text_change("/worktree shar".to_string());
        assert!(
            matches!(popup.selected_item(), Some(CommandItem::BuiltinText { .. })),
            "expected subcommand to be selected by default for /worktree context"
        );
    }
}
