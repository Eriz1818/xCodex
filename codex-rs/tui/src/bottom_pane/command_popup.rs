use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;
use super::slash_arg_hints;
use super::slash_subcommands::build_subcommand_matches;
use super::slash_subcommands::slash_command_supports_subcommands as subcommands_supported;
use super::slash_subcommands::subcommand_list_hint;
use crate::render::Insets;
use crate::render::RectExt;
use crate::slash_command::SlashCommand;
use crate::slash_command::built_in_slash_commands;
use crate::xcodex_plugins::PluginSlashCommand;
use crate::xcodex_plugins::command_popup as xcodex_command_popup;
use codex_protocol::custom_prompts::CustomPrompt;
use codex_protocol::custom_prompts::PROMPTS_CMD_PREFIX;

pub(crate) const DEFAULT_SLASH_POPUP_ROWS: usize = 8;

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
    plugin_commands: Vec<PluginSlashCommand>,
    prompts: Vec<CustomPrompt>,
    slash_completion_branches: Vec<String>,
    current_git_branch: Option<String>,
    state: ScrollState,
    selection_locked: bool,
    max_rows: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CommandPopupFlags {
    pub(crate) collaboration_modes_enabled: bool,
}

impl CommandPopup {
    pub(crate) fn new(
        mut prompts: Vec<CustomPrompt>,
        flags: CommandPopupFlags,
        max_rows: usize,
    ) -> Self {
        let allow_elevate_sandbox = windows_degraded_sandbox_active();
        let builtins: Vec<(&'static str, SlashCommand)> = built_in_slash_commands()
            .into_iter()
            .filter(|(_, cmd)| allow_elevate_sandbox || *cmd != SlashCommand::ElevateSandbox)
            .filter(|(_, cmd)| flags.collaboration_modes_enabled || *cmd != SlashCommand::Collab)
            .collect();
        let plugin_commands: Vec<PluginSlashCommand> =
            xcodex_command_popup::popup_plugin_commands();
        // Exclude prompts that collide with builtin or plugin command names.
        xcodex_command_popup::filter_prompts_for_popup(&mut prompts, &builtins, &plugin_commands);
        Self {
            command_filter: String::new(),
            command_line: String::new(),
            builtins,
            plugin_commands,
            prompts,
            slash_completion_branches: Vec::new(),
            current_git_branch: None,
            state: ScrollState::new(),
            selection_locked: false,
            max_rows: max_rows.max(1),
        }
    }

    pub(crate) fn set_prompts(&mut self, mut prompts: Vec<CustomPrompt>) {
        xcodex_command_popup::filter_prompts_for_popup(
            &mut prompts,
            &self.builtins,
            &self.plugin_commands,
        );
        self.prompts = prompts;
    }

    pub(crate) fn set_max_rows(&mut self, max_rows: usize) {
        self.max_rows = max_rows.max(1);
        self.state.ensure_visible(
            self.filtered().len(),
            self.max_rows.min(self.filtered().len()),
        );
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

        let prev_filter = self.command_filter.clone();
        let prev_line = self.command_line.clone();

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

        let command_changed = self.command_filter != prev_filter || self.command_line != prev_line;
        if command_changed {
            self.selection_locked = false;
        }

        // Reset or clamp selected index based on new filtered list.
        let matches = self.filtered();
        let matches_len = matches.len();
        let had_selection = self.state.selected_idx.is_some();
        self.state.clamp_selection(matches_len);

        if !had_selection {
            if let Some(idx) = matches
                .iter()
                .position(|(item, _)| matches!(item, CommandItem::ArgValue { .. }))
            {
                self.state.selected_idx = Some(idx);
            } else if self.should_default_select_subcommand()
                && let Some(idx) = matches
                    .iter()
                    .position(|(item, _)| matches!(item, CommandItem::BuiltinText { .. }))
            {
                self.state.selected_idx = Some(idx);
            }
        }
        self.state
            .ensure_visible(matches_len, self.max_rows.min(matches_len));
    }

    fn should_default_select_subcommand(&self) -> bool {
        !build_subcommand_matches(&self.command_filter, &self.command_line).is_empty()
    }

    /// Determine the preferred height of the popup for a given width.
    /// Accounts for wrapped descriptions so that long tooltips don't overflow.
    pub(crate) fn calculate_required_height(&self, width: u16) -> u16 {
        use super::selection_popup_common::measure_rows_height;
        let rows = self.rows_from_matches(self.filtered());

        measure_rows_height(&rows, &self.state, self.max_rows, width)
    }

    /// Compute exact/prefix matches over built-in commands and user prompts,
    /// paired with optional highlight indices. Preserves the original
    /// presentation order for built-ins and prompts.
    fn filtered(&self) -> Vec<(CommandItem, Option<Vec<usize>>)> {
        let filter = self.command_filter.trim();
        let subcommand_matches_by_anchor =
            build_subcommand_matches(&self.command_filter, &self.command_line);
        let mut out: Vec<(CommandItem, Option<Vec<usize>>)> = Vec::new();
        if filter.is_empty() {
            // Built-ins first, in presentation order.
            for (_, cmd) in self.builtins.iter() {
                out.push((CommandItem::Builtin(*cmd), None));
            }
            for command in self.plugin_commands.iter() {
                out.push((
                    CommandItem::BuiltinText {
                        name: command.name,
                        description: command.description,
                        run_on_enter: command.run_on_enter,
                        insert_trailing_space: command.insert_trailing_space,
                    },
                    None,
                ));
            }
            // Then prompts, already sorted by name.
            for idx in 0..self.prompts.len() {
                out.push((CommandItem::UserPrompt(idx), None));
            }
            return out;
        }

        if !subcommand_matches_by_anchor.is_empty() {
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
                    )
                }));
            }
            return out;
        }

        let filter_lower = filter.to_lowercase();
        let filter_chars = filter.chars().count();
        let mut exact: Vec<(CommandItem, Option<Vec<usize>>)> = Vec::new();
        let mut prefix: Vec<(CommandItem, Option<Vec<usize>>)> = Vec::new();
        let prompt_prefix_len = PROMPTS_CMD_PREFIX.chars().count() + 1;
        let indices_for = |offset| Some((offset..offset + filter_chars).collect());

        let mut push_match =
            |item: CommandItem, display: &str, name: Option<&str>, name_offset: usize| {
                let display_lower = display.to_lowercase();
                let name_lower = name.map(str::to_lowercase);
                let display_exact = display_lower == filter_lower;
                let name_exact = name_lower.as_deref() == Some(filter_lower.as_str());
                if display_exact || name_exact {
                    let offset = if display_exact { 0 } else { name_offset };
                    exact.push((item, indices_for(offset)));
                    return;
                }
                let display_prefix = display_lower.starts_with(&filter_lower);
                let name_prefix = name_lower
                    .as_ref()
                    .is_some_and(|name| name.starts_with(&filter_lower));
                if display_prefix || name_prefix {
                    let offset = if display_prefix { 0 } else { name_offset };
                    prefix.push((item, indices_for(offset)));
                }
            };

        for (_, cmd) in self.builtins.iter() {
            push_match(CommandItem::Builtin(*cmd), cmd.command(), None, 0);
        }
        for command in self.plugin_commands.iter() {
            push_match(
                CommandItem::BuiltinText {
                    name: command.name,
                    description: command.description,
                    run_on_enter: command.run_on_enter,
                    insert_trailing_space: command.insert_trailing_space,
                },
                command.name,
                Some(command.name),
                0,
            );
        }

        // Support both search styles:
        // - Typing "name" should surface "/prompts:name" results.
        // - Typing "prompts:name" should also work.
        for (idx, p) in self.prompts.iter().enumerate() {
            let display = format!("{PROMPTS_CMD_PREFIX}:{}", p.name);
            push_match(
                CommandItem::UserPrompt(idx),
                &display,
                Some(&p.name),
                prompt_prefix_len,
            );
        }

        out.extend(exact);
        out.extend(prefix);
        out.extend(self.arg_value_completions());
        out
    }

    fn filtered_items(&self) -> Vec<CommandItem> {
        self.filtered().into_iter().map(|(c, _)| c).collect()
    }

    fn rows_from_matches(
        &self,
        matches: Vec<(CommandItem, Option<Vec<usize>>)>,
    ) -> Vec<GenericDisplayRow> {
        matches
            .into_iter()
            .map(|(item, indices)| {
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

    fn arg_value_completions(&self) -> Vec<(CommandItem, Option<Vec<usize>>)> {
        xcodex_command_popup::worktree_init_completions(
            &self.command_line,
            self.current_git_branch.as_deref(),
            &self.slash_completion_branches,
        )
        .into_iter()
        .map(|completion| {
            (
                CommandItem::ArgValue {
                    display: completion.display,
                    insert: completion.insert,
                    description: completion.description,
                    insert_trailing_space: completion.insert_trailing_space,
                },
                completion.indices,
            )
        })
        .collect()
    }

    /// Move the selection cursor one step up.
    pub(crate) fn move_up(&mut self) {
        let len = self.filtered_items().len();
        self.selection_locked = true;
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, self.max_rows.min(len));
    }

    /// Move the selection cursor one step down.
    pub(crate) fn move_down(&mut self) {
        let matches_len = self.filtered_items().len();
        self.selection_locked = true;
        self.state.move_down_wrap(matches_len);
        self.state
            .ensure_visible(matches_len, self.max_rows.min(matches_len));
    }

    /// Return currently selected command, if any.
    pub(crate) fn selected_item(&self) -> Option<CommandItem> {
        let matches = self.filtered_items();
        self.state
            .selected_idx
            .and_then(|idx| matches.get(idx).cloned())
    }
}

impl WidgetRef for CommandPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let base_style = crate::theme::transcript_style();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(base_style);
            }
        }
        let rows = self.rows_from_matches(self.filtered());
        render_rows(
            area.inset(Insets::tlbr(0, 2, 0, 0)),
            buf,
            &rows,
            &self.state,
            self.max_rows,
            base_style,
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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
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
    fn filter_includes_thoughts_plugin_command() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        popup.on_composer_text_change("/tho".to_string());

        let matches = popup.filtered_items();
        let has_thoughts = matches.iter().any(|item| match item {
            CommandItem::BuiltinText { name, .. } => *name == "thoughts",
            CommandItem::Builtin(_) => false,
            CommandItem::ArgValue { .. } => false,
            CommandItem::UserPrompt(_) => false,
        });
        assert!(
            has_thoughts,
            "expected '/thoughts' to appear among filtered commands"
        );
    }

    #[test]
    fn selecting_init_by_exact_match() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
    fn filtered_commands_keep_presentation_order_for_prefix() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        popup.on_composer_text_change("/m".to_string());

        let cmds: Vec<&str> = popup
            .filtered_items()
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::Builtin(cmd) => Some(cmd.command()),
                CommandItem::BuiltinText { .. } | CommandItem::ArgValue { .. } => None,
                CommandItem::UserPrompt(_) => None,
            })
            .collect();
        assert_eq!(cmds, vec!["model", "mention", "mcp"]);
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

        let popup = CommandPopup::new(
            prompts,
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
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
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        let rows = popup.rows_from_matches(vec![(CommandItem::UserPrompt(0), None)]);
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
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        let rows = popup.rows_from_matches(vec![(CommandItem::UserPrompt(0), None)]);
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
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        popup.on_composer_text_change("/my".to_string());
        let items = popup.filtered_items();
        let has_prompt = items
            .into_iter()
            .any(|item| matches!(item, CommandItem::UserPrompt(_)));
        assert!(has_prompt, "expected /my to suggest the custom prompt");
    }

    #[test]
    fn prefix_filter_limits_matches_for_ac() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
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
            !cmds.contains(&"compact"),
            "expected prefix search for '/ac' to exclude 'compact', got {cmds:?}"
        );
    }

    #[test]
    fn worktree_subcommands_are_suggested_under_worktree() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
    fn worktree_subcommand_hint_uses_plugin_order() {
        let hint = subcommand_list_hint("worktree").expect("worktree hint");
        assert_eq!(
            hint,
            "Type space for subcommands: detect, doctor, init, shared, link-shared"
        );
    }

    #[test]
    fn settings_subcommands_are_suggested_under_settings() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
    fn mcp_subcommands_are_suggested_under_mcp() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

        popup.on_composer_text_change("/mcp ".to_string());

        let items = popup.filtered_items();
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, CommandItem::Builtin(SlashCommand::Mcp))),
            "expected /mcp root command to be hidden in subcommand context"
        );

        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"mcp retry") && subcommands.contains(&"mcp timeout"),
            "expected /mcp to suggest subcommands, got {subcommands:?}"
        );
    }

    #[test]
    fn mcp_retry_subcommands_are_suggested_under_retry() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

        popup.on_composer_text_change("/mcp retry ".to_string());

        let items = popup.filtered_items();
        let subcommands: Vec<&str> = items
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::BuiltinText { name, .. } => Some(name),
                _ => None,
            })
            .collect();

        assert!(
            subcommands.contains(&"mcp retry failed"),
            "expected /mcp retry to suggest failed, got {subcommands:?}"
        );
    }

    #[test]
    fn worktree_subcommands_are_hidden_until_space() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
    fn arrow_key_selection_is_not_reset_by_popup_sync() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        popup.on_composer_text_change("/worktree ".to_string());

        let first = popup.selected_item();
        popup.move_down();
        let moved = popup.selected_item();
        assert_ne!(first, moved, "expected move_down to change selection");

        // Simulate redundant sync calls (e.g. after an Up/Down key event).
        popup.on_composer_text_change("/worktree ".to_string());
        assert_eq!(
            popup.selected_item(),
            moved,
            "expected selection to persist across redundant sync"
        );
    }

    #[test]
    fn worktree_subcommands_filter_by_prefix() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
    fn subcommand_context_hides_other_root_suggestions() {
        let prompts = vec![CustomPrompt {
            name: "worktree-helper".to_string(),
            path: "/tmp/worktree-helper.md".to_string().into(),
            content: "hello".to_string(),
            description: None,
            argument_hint: None,
        }];

        let mut popup = CommandPopup::new(
            prompts,
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

        popup.on_composer_text_change("/worktree d".to_string());

        let items = popup.filtered_items();
        assert!(
            items.iter().all(|item| {
                matches!(
                    item,
                    CommandItem::BuiltinText { .. } | CommandItem::ArgValue { .. }
                )
            }),
            "expected subcommand context to hide other root suggestions, got {items:?}"
        );
    }

    #[test]
    fn selection_does_not_reset_when_refreshing_popup() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

        popup.on_composer_text_change("/worktree ".to_string());

        popup.move_down();
        let moved = popup.selected_item();
        assert!(moved.is_some(), "expected selection after moving down");

        popup.on_composer_text_change("/worktree ".to_string());
        assert_eq!(
            popup.selected_item(),
            moved,
            "expected selection to persist across refresh"
        );
    }

    #[test]
    fn worktree_nested_subcommands_are_suggested_under_shared() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

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
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );

        popup.on_composer_text_change("/worktree shar".to_string());
        assert!(
            matches!(popup.selected_item(), Some(CommandItem::BuiltinText { .. })),
            "expected subcommand to be selected by default for /worktree context"
        );
    }

    #[test]
    fn collab_command_hidden_when_collaboration_modes_disabled() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags::default(),
            DEFAULT_SLASH_POPUP_ROWS,
        );
        popup.on_composer_text_change("/coll".to_string());

        let cmds: Vec<&str> = popup
            .filtered_items()
            .into_iter()
            .filter_map(|item| match item {
                CommandItem::Builtin(cmd) => Some(cmd.command()),
                CommandItem::BuiltinText { .. } | CommandItem::ArgValue { .. } => None,
                CommandItem::UserPrompt(_) => None,
            })
            .collect();
        assert!(
            !cmds.contains(&"collab"),
            "expected '/collab' to be hidden when collaboration modes are disabled, got {cmds:?}"
        );
    }

    #[test]
    fn collab_command_visible_when_collaboration_modes_enabled() {
        let mut popup = CommandPopup::new(
            Vec::new(),
            CommandPopupFlags {
                collaboration_modes_enabled: true,
            },
            DEFAULT_SLASH_POPUP_ROWS,
        );
        popup.on_composer_text_change("/collab".to_string());

        match popup.selected_item() {
            Some(CommandItem::Builtin(cmd)) => assert_eq!(cmd.command(), "collab"),
            other => panic!("expected collab to be selected for exact match, got {other:?}"),
        }
    }
}
