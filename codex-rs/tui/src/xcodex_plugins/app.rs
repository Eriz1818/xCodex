use crate::app::App;
use crate::app_event::AppEvent;
use crate::app_event::PlanSettingsCycleTarget;
use crate::tui;
use codex_core::config::edit::ConfigEdit;
use codex_core::config::edit::ConfigEditsBuilder;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;

pub(crate) async fn try_handle_event(
    app: &mut App,
    tui: &mut tui::Tui,
    event: AppEvent,
) -> Result<Option<AppEvent>> {
    match event {
        AppEvent::CodexEvent(event) => {
            if app.xcodex_state.external_approval_routes.is_empty() {
                Ok(Some(AppEvent::CodexEvent(event)))
            } else {
                app.xcodex_state.paused_codex_events.push_back(event);
                Ok(None)
            }
        }
        AppEvent::CodexOp(op) => {
            handle_codex_op(app, op).await;
            Ok(None)
        }
        AppEvent::UpdateXtremeMode(mode) => {
            app.config.xcodex.tui_xtreme_mode = mode;
            app.chat_widget.set_xtreme_mode(mode);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::PreviewTheme { theme } => {
            crate::xcodex_plugins::theme::preview_theme(app, tui, &theme);
            Ok(None)
        }
        AppEvent::CancelThemePreview => {
            crate::xcodex_plugins::theme::cancel_theme_preview(app, tui);
            Ok(None)
        }
        AppEvent::PersistThemeSelection { variant, theme } => {
            crate::xcodex_plugins::theme::persist_theme_selection(app, tui, variant, theme).await;
            Ok(None)
        }
        AppEvent::OpenThemeSelector => {
            crate::xcodex_plugins::theme::open_theme_selector(app, tui);
            Ok(None)
        }
        AppEvent::OpenThemeHelp => {
            crate::xcodex_plugins::theme::open_theme_help(app, tui);
            Ok(None)
        }
        AppEvent::UpdateRampsConfig {
            rotate,
            build,
            devops,
        } => {
            app.config.xcodex.tui_ramps_rotate = rotate;
            app.config.xcodex.tui_ramps_build = build;
            app.config.xcodex.tui_ramps_devops = devops;
            app.chat_widget.set_ramps_config(rotate, build, devops);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::WorktreeListUpdated {
            worktrees,
            open_picker,
        } => {
            crate::xcodex_plugins::worktree::set_worktree_list(
                &mut app.chat_widget,
                worktrees,
                open_picker,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::WorktreeDetect { open_picker } => {
            crate::xcodex_plugins::worktree::spawn_worktree_detection(
                &mut app.chat_widget,
                open_picker,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenWorktreeCommandMenu => {
            if app.chat_widget.composer_is_empty() {
                app.chat_widget
                    .set_composer_text("/worktree ".to_string(), Vec::new(), Vec::new());
            } else {
                app.chat_widget.add_info_message(
                    "Clear the composer to open the /worktree menu.".to_string(),
                    None,
                );
            }
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenToolsCommand { command } => {
            if app.chat_widget.composer_is_empty() {
                app.chat_widget
                    .set_composer_text(command, Vec::new(), Vec::new());
            } else {
                app.chat_widget.add_info_message(
                    "Clear the composer to open tools commands.".to_string(),
                    None,
                );
            }
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanListView { scope } => {
            crate::xcodex_plugins::plan::open_plan_list_scope(&mut app.chat_widget, &scope);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanSettingsView => {
            crate::xcodex_plugins::plan::open_plan_settings_menu(&mut app.chat_widget);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanBaseDirEditorView => {
            crate::xcodex_plugins::plan::open_plan_base_dir_editor(&mut app.chat_widget);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanModePickerView => {
            crate::xcodex_plugins::plan::open_plan_mode_picker(&mut app.chat_widget);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanModelPickerView => {
            crate::xcodex_plugins::plan::open_plan_model_picker(&mut app.chat_widget);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::ApplyPlanSettingsCommand {
            args,
            reopen_settings,
        } => {
            crate::xcodex_plugins::plan::apply_plan_settings_command(
                &mut app.chat_widget,
                &args,
                reopen_settings,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::CyclePlanSettingsValue {
            target,
            selected_idx,
        } => {
            let target = match target {
                PlanSettingsCycleTarget::BrainstormFirst => "brainstorm-first",
                PlanSettingsCycleTarget::Flowchart => "flowchart",
                PlanSettingsCycleTarget::Mode => "mode",
                PlanSettingsCycleTarget::TrackWorktree => "track-worktree",
                PlanSettingsCycleTarget::TrackBranch => "track-branch",
                PlanSettingsCycleTarget::MismatchAction => "mismatch-action",
                PlanSettingsCycleTarget::Naming => "naming",
            };
            crate::xcodex_plugins::plan::cycle_plan_settings_value(
                &mut app.chat_widget,
                target,
                selected_idx,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanFile { path } => {
            if let Some(update) =
                crate::xcodex_plugins::plan::open_plan_file_path(&mut app.chat_widget, path)
            {
                app.app_event_tx.send(AppEvent::PlanFileUiUpdated {
                    path: update.path,
                    todos_remaining: update.todos_remaining,
                    is_done: update.is_done,
                });
            }
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::MarkActivePlanDone => {
            if let Some(update) =
                crate::xcodex_plugins::plan::mark_active_plan_done_action(&mut app.chat_widget)
            {
                app.app_event_tx.send(AppEvent::PlanFileUiUpdated {
                    path: update.path,
                    todos_remaining: update.todos_remaining,
                    is_done: update.is_done,
                });
            }
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::MarkActivePlanArchived => {
            if let Some(update) =
                crate::xcodex_plugins::plan::mark_active_plan_archived_action(&mut app.chat_widget)
            {
                app.app_event_tx.send(AppEvent::PlanFileUiUpdated {
                    path: update.path,
                    todos_remaining: update.todos_remaining,
                    is_done: update.is_done,
                });
            }
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::PauseActivePlanRun => {
            if let Some(update) =
                crate::xcodex_plugins::plan::pause_active_plan_run_action(&mut app.chat_widget)
            {
                app.app_event_tx.send(AppEvent::PlanFileUiUpdated {
                    path: update.path,
                    todos_remaining: update.todos_remaining,
                    is_done: update.is_done,
                });
            }
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanLoadConfirmation { path, scope } => {
            crate::xcodex_plugins::plan::open_plan_load_confirmation(
                &mut app.chat_widget,
                path,
                &scope,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenPlanDoSomethingElsePrompt => {
            app.chat_widget.show_plan_do_something_else_prompt();
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::ReopenPlanNextStepPromptAfterTurn => {
            app.chat_widget.reopen_plan_prompt_after_turn();
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::PlanFileUiUpdated {
            path,
            todos_remaining,
            is_done,
        } => {
            tracing::debug!(
                path = %path.display(),
                todos_remaining,
                is_done,
                "plan file ui state updated"
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenWorktreesSettingsView => {
            crate::xcodex_plugins::worktree::open_worktrees_settings_view(&mut app.chat_widget);
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenWorktreeInitWizard {
            worktree_root,
            workspace_root,
            current_branch,
            shared_dirs,
            branches,
        } => {
            app.chat_widget
                .set_slash_completion_branches(branches.clone());
            crate::xcodex_plugins::worktree::open_worktree_init_wizard(
                &mut app.chat_widget,
                worktree_root,
                workspace_root,
                current_branch,
                shared_dirs,
                branches,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::OpenRampsSettingsView => {
            app.chat_widget.open_ramps_settings_view();
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::WorktreeListUpdateFailed { error, open_picker } => {
            crate::xcodex_plugins::worktree::on_worktree_list_update_failed(
                &mut app.chat_widget,
                error,
                open_picker,
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::WorktreeSwitched(cwd) => {
            let previous_cwd = app.config.cwd.clone();
            app.config.cwd = cwd.clone();
            app.chat_widget.set_session_cwd(cwd);
            tui.frame_requester().schedule_frame();

            let tx = app.app_event_tx.clone();
            let branch_cwd = app.config.cwd.clone();
            tokio::spawn(async move {
                let branches = codex_core::git_info::local_git_branches(&branch_cwd).await;
                tx.send(AppEvent::UpdateSlashCompletionBranches { branches });
            });

            let next_root = codex_core::git_info::resolve_git_worktree_head(&app.config.cwd)
                .map(|head| head.worktree_root);

            let auto_link = app.config.xcodex.worktrees_auto_link_shared_dirs
                && !app.config.worktrees_shared_dirs.is_empty();
            if auto_link
                && let Some(next_root) = next_root.clone()
                && let Some(workspace_root) =
                    codex_core::git_info::resolve_root_git_project_for_trust(&next_root)
                && next_root != workspace_root
            {
                let show_notice = app.xcodex_state.take_shared_dirs_write_notice();
                let shared_dirs = app.config.worktrees_shared_dirs.clone();
                let tx = app.app_event_tx.clone();
                tokio::spawn(async move {
                    let actions = codex_core::git_info::link_worktree_shared_dirs(
                        &next_root,
                        &workspace_root,
                        &shared_dirs,
                    )
                    .await;

                    let mut linked = 0usize;
                    let mut skipped = 0usize;
                    let mut failed = 0usize;
                    for action in &actions {
                        match action.outcome {
                            codex_core::git_info::SharedDirLinkOutcome::Linked => linked += 1,
                            codex_core::git_info::SharedDirLinkOutcome::AlreadyLinked => {}
                            codex_core::git_info::SharedDirLinkOutcome::Skipped(_) => {
                                skipped += 1;
                            }
                            codex_core::git_info::SharedDirLinkOutcome::Failed(_) => {
                                failed += 1;
                            }
                        }
                    }

                    if linked == 0 && skipped == 0 && failed == 0 {
                        return;
                    }

                    let summary = format!(
                        "Auto-linked shared dirs after worktree switch: linked={linked}, skipped={skipped}, failed={failed}."
                    );
                    let hint = if show_notice {
                        Some(String::from(
                            "Note: shared dirs are linked into the workspace root; writes under them persist across worktrees. Tip: run `/worktree doctor` for details.",
                        ))
                    } else {
                        Some(String::from("Tip: run `/worktree doctor` for details."))
                    };
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        crate::history_cell::new_info_event(summary, hint),
                    )));
                });
            }

            let previous_root = codex_core::git_info::resolve_git_worktree_head(&previous_cwd)
                .map(|head| head.worktree_root);

            let Some(previous_root) = previous_root else {
                return Ok(None);
            };
            let Some(next_root) = next_root else {
                return Ok(None);
            };

            if previous_root == next_root {
                return Ok(None);
            }

            if codex_core::git_info::resolve_root_git_project_for_trust(&previous_root)
                .is_some_and(|root| root == previous_root)
            {
                return Ok(None);
            }

            let tx = app.app_event_tx.clone();
            tokio::spawn(async move {
                let Ok(summary) =
                    codex_core::git_info::summarize_git_untracked_files(&previous_root, 5).await
                else {
                    return;
                };
                if summary.total == 0 {
                    return;
                }

                tx.send(AppEvent::WorktreeUntrackedFilesDetected {
                    previous_worktree_root: previous_root,
                    total: summary.total,
                    sample: summary.sample,
                });
            });
            Ok(None)
        }
        AppEvent::WorktreeUntrackedFilesDetected {
            previous_worktree_root,
            total,
            sample,
        } => {
            let display = crate::exec_command::relativize_to_home(&previous_worktree_root)
                .map(|path| {
                    if path.as_os_str().is_empty() {
                        String::from("~")
                    } else {
                        format!("~/{}", path.display())
                    }
                })
                .unwrap_or_else(|| previous_worktree_root.display().to_string());

            let sample_preview = if sample.is_empty() {
                String::new()
            } else {
                let preview: String = sample
                    .iter()
                    .take(3)
                    .map(|path| format!("\n  - {path}"))
                    .collect();
                let remainder = total.saturating_sub(sample.len());
                if remainder > 0 {
                    format!("{preview}\n  - … +{remainder} more")
                } else {
                    preview
                }
            };

            app.chat_widget.add_info_message(
                format!(
                    "Untracked files detected in the previous worktree ({display}). Deleting that worktree may lose them.{sample_preview}"
                ),
                Some(String::from("Tip: git stash push -u -m \"worktree scratch\"")),
            );
            tui.frame_requester().schedule_frame();
            Ok(None)
        }
        AppEvent::PersistXtremeMode(mode) => {
            let profile = app.active_profile.as_deref();
            let mode_value = match mode {
                codex_core::config::types::XtremeMode::Auto => "auto",
                codex_core::config::types::XtremeMode::On => "on",
                codex_core::config::types::XtremeMode::Off => "off",
            };
            match ConfigEditsBuilder::new(&app.config.codex_home)
                .with_profile(profile)
                .with_edits([ConfigEdit::SetPath {
                    segments: vec!["tui".to_string(), "xtreme_mode".to_string()],
                    value: toml_edit::value(mode_value),
                }])
                .apply()
                .await
            {
                Ok(()) => {}
                Err(err) => {
                    tracing::error!(error = %err, "failed to persist xtreme mode");
                    if let Some(profile) = profile {
                        app.chat_widget.add_error_message(format!(
                            "Failed to save xtreme mode for profile `{profile}`: {err}"
                        ));
                    } else {
                        app.chat_widget
                            .add_error_message(format!("Failed to save xtreme mode: {err}"));
                    }
                }
            }
            Ok(None)
        }
        AppEvent::PersistRampsConfig {
            rotate,
            build,
            devops,
        } => {
            let profile = app.active_profile.as_deref();
            match ConfigEditsBuilder::new(&app.config.codex_home)
                .with_profile(profile)
                .with_edits([
                    ConfigEdit::SetPath {
                        segments: vec!["tui".to_string(), "ramps_rotate".to_string()],
                        value: toml_edit::value(rotate),
                    },
                    ConfigEdit::SetPath {
                        segments: vec!["tui".to_string(), "ramps_build".to_string()],
                        value: toml_edit::value(build),
                    },
                    ConfigEdit::SetPath {
                        segments: vec!["tui".to_string(), "ramps_devops".to_string()],
                        value: toml_edit::value(devops),
                    },
                ])
                .apply()
                .await
            {
                Ok(()) => {}
                Err(err) => {
                    tracing::error!(error = %err, "failed to persist xcodex ramps config");
                    if let Some(profile) = profile {
                        app.chat_widget.add_error_message(format!(
                            "Failed to save ramps config for profile `{profile}`: {err}"
                        ));
                    } else {
                        app.chat_widget
                            .add_error_message(format!("Failed to save ramps config: {err}"));
                    }
                }
            }
            Ok(None)
        }
        AppEvent::UpdateExclusionSettings {
            exclusion,
            hooks_sanitize_payloads,
        } => {
            app.config.exclusion = exclusion.clone();
            app.config.xcodex.hooks.sanitize_payloads = hooks_sanitize_payloads;
            app.chat_widget
                .set_exclusion_settings(exclusion, hooks_sanitize_payloads);
            Ok(None)
        }
        AppEvent::PersistExclusionSettings {
            exclusion,
            hooks_sanitize_payloads,
        } => {
            let profile = app.active_profile.as_deref();
            let exclusion_value = match exclusion_to_item(&exclusion) {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!(error = %err, "failed to serialize exclusion settings");
                    if let Some(profile) = profile {
                        app.chat_widget.add_error_message(format!(
                            "Failed to save exclusions for profile `{profile}`: {err}"
                        ));
                    } else {
                        app.chat_widget
                            .add_error_message(format!("Failed to save exclusions: {err}"));
                    }
                    return Ok(None);
                }
            };
            match ConfigEditsBuilder::new(&app.config.codex_home)
                .with_profile(profile)
                .with_edits([
                    ConfigEdit::SetPath {
                        segments: vec!["exclusion".to_string()],
                        value: exclusion_value,
                    },
                    ConfigEdit::SetPath {
                        segments:
                            codex_core::xcodex::config::hooks_sanitize_payloads_config_segments(),
                        value: toml_edit::value(hooks_sanitize_payloads),
                    },
                ])
                .apply()
                .await
            {
                Ok(()) => {}
                Err(err) => {
                    tracing::error!(error = %err, "failed to persist exclusion settings");
                    if let Some(profile) = profile {
                        app.chat_widget.add_error_message(format!(
                            "Failed to save exclusions for profile `{profile}`: {err}"
                        ));
                    } else {
                        app.chat_widget
                            .add_error_message(format!("Failed to save exclusions: {err}"));
                    }
                }
            }
            Ok(None)
        }
        AppEvent::UpdateWorktreesSharedDirs { shared_dirs } => {
            app.config.worktrees_shared_dirs = shared_dirs.clone();
            crate::xcodex_plugins::worktree::set_worktrees_shared_dirs(
                &mut app.chat_widget,
                shared_dirs,
            );
            Ok(None)
        }
        AppEvent::UpdateWorktreesPinnedPaths { pinned_paths } => {
            app.config.worktrees_pinned_paths = pinned_paths.clone();
            crate::xcodex_plugins::worktree::set_worktrees_pinned_paths(
                &mut app.chat_widget,
                pinned_paths,
            );
            Ok(None)
        }
        AppEvent::PersistWorktreesSharedDirs { shared_dirs } => {
            let mut shared_dirs_array = toml_edit::Array::new();
            for dir in &shared_dirs {
                shared_dirs_array.push(dir.clone());
            }
            match ConfigEditsBuilder::new(&app.config.codex_home)
                .with_edits([ConfigEdit::SetPath {
                    segments: vec!["worktrees".to_string(), "shared_dirs".to_string()],
                    value: toml_edit::value(shared_dirs_array),
                }])
                .apply()
                .await
            {
                Ok(()) => {}
                Err(err) => {
                    tracing::error!(error = %err, "failed to persist worktree shared dirs");
                    app.chat_widget
                        .add_error_message(format!("Failed to save worktree shared dirs: {err}"));
                }
            }
            Ok(None)
        }
        AppEvent::PersistWorktreesPinnedPaths { pinned_paths } => {
            let mut pinned_paths_array = toml_edit::Array::new();
            for path in &pinned_paths {
                pinned_paths_array.push(path.clone());
            }
            match ConfigEditsBuilder::new(&app.config.codex_home)
                .with_edits([ConfigEdit::SetPath {
                    segments: vec!["worktrees".to_string(), "pinned_paths".to_string()],
                    value: toml_edit::value(pinned_paths_array),
                }])
                .apply()
                .await
            {
                Ok(()) => {}
                Err(err) => {
                    tracing::error!(error = %err, "failed to persist worktree pinned paths");
                    app.chat_widget
                        .add_error_message(format!("Failed to save worktree pinned paths: {err}"));
                }
            }
            Ok(None)
        }
        AppEvent::PersistMcpStartupTimeout {
            server,
            startup_timeout_sec,
        } => {
            let profile = app.active_profile.as_deref();
            match ConfigEditsBuilder::new(&app.config.codex_home)
                .with_profile(profile)
                .with_edits([ConfigEdit::SetPath {
                    segments: vec![
                        "mcp_servers".to_string(),
                        server.clone(),
                        "startup_timeout_sec".to_string(),
                    ],
                    value: toml_edit::value(i64::try_from(startup_timeout_sec).unwrap_or(i64::MAX)),
                }])
                .apply()
                .await
            {
                Ok(()) => {
                    let mut mcp_servers = app.config.mcp_servers.get().clone();
                    if let Some(cfg) = mcp_servers.get_mut(&server) {
                        cfg.startup_timeout_sec =
                            Some(std::time::Duration::from_secs(startup_timeout_sec));
                        if let Err(err) = app.config.mcp_servers.set(mcp_servers) {
                            tracing::warn!(%err, "failed to update MCP startup timeout in app config");
                        }
                    }
                }
                Err(err) => {
                    tracing::error!(error = %err, "failed to persist MCP startup timeout");
                    app.chat_widget.add_error_message(format!(
                        "Failed to save MCP startup timeout for `{server}`: {err}"
                    ));
                }
            }
            Ok(None)
        }
        other => Ok(Some(other)),
    }
}

fn exclusion_to_item(
    exclusion: &codex_core::config::types::ExclusionConfig,
) -> Result<toml_edit::Item, String> {
    let serialized = toml::to_string(exclusion).map_err(|err| err.to_string())?;
    let document = serialized
        .parse::<toml_edit::DocumentMut>()
        .map_err(|err| err.to_string())?;
    Ok(toml_edit::Item::Table(document.as_table().clone()))
}

/// Routes external approval request events through the chat widget by
/// rewriting the event id to include the originating thread.
///
/// `thread_id` is the external thread that issued the approval request.
/// `event` is the approval request event whose id is rewritten so replies
/// can be routed back to the correct thread.
#[allow(dead_code)] // Upstream parity seam: external approval routing remains available for multi-thread integrations.
fn handle_external_approval_request(app: &mut App, thread_id: ThreadId, mut event: Event) {
    match &mut event.msg {
        EventMsg::RequestUserInput(ev) => {
            let original_id = ev.turn_id.clone();
            let routing_id = format!("{thread_id}:{original_id}");
            app.xcodex_state
                .external_approval_routes
                .insert(routing_id.clone(), (thread_id, original_id));
            ev.turn_id = routing_id.clone();
            event.id = routing_id;
        }
        EventMsg::ExecApprovalRequest(_) | EventMsg::ApplyPatchApprovalRequest(_) => {
            let original_id = event.id.clone();
            let routing_id = format!("{thread_id}:{original_id}");
            app.xcodex_state
                .external_approval_routes
                .insert(routing_id.clone(), (thread_id, original_id));
            event.id = routing_id;
        }
        _ => return,
    }
    app.chat_widget.handle_codex_event(event);
}

async fn handle_codex_op(app: &mut App, op: Op) {
    match op {
        // Catch potential approvals coming from an external thread and treat them
        // directly. This support both command and patch approval. In such case
        // the approval get transferred to the corresponding thread and the external
        // approval map (`external_approval_routes`) is updated.
        Op::ExecApproval {
            id,
            turn_id,
            decision,
        } => {
            if let Some((thread_id, original_id)) =
                app.xcodex_state.external_approval_routes.remove(&id)
            {
                // Approval of a sub-agent.
                forward_external_op(
                    app,
                    thread_id,
                    Op::ExecApproval {
                        id: original_id,
                        turn_id,
                        decision,
                    },
                )
                .await;
                finish_external_approval(app);
            } else {
                // This is an approval but not external.
                app.chat_widget.submit_op(Op::ExecApproval {
                    id,
                    turn_id,
                    decision,
                });
            }
        }
        Op::PatchApproval { id, decision } => {
            if let Some((thread_id, original_id)) =
                app.xcodex_state.external_approval_routes.remove(&id)
            {
                // Approval of a sub-agent.
                forward_external_op(
                    app,
                    thread_id,
                    Op::PatchApproval {
                        id: original_id,
                        decision,
                    },
                )
                .await;
                finish_external_approval(app);
            } else {
                // This is an approval but not external.
                app.chat_widget
                    .submit_op(Op::PatchApproval { id, decision });
            }
        }
        Op::UserInputAnswer { id, response } => {
            if let Some((thread_id, original_id)) =
                app.xcodex_state.external_approval_routes.remove(&id)
            {
                forward_external_op(
                    app,
                    thread_id,
                    Op::UserInputAnswer {
                        id: original_id,
                        response,
                    },
                )
                .await;
                finish_external_approval(app);
            } else {
                app.chat_widget
                    .submit_op(Op::UserInputAnswer { id, response });
            }
        }
        // Standard path where this is not an external approval response.
        _ => app.chat_widget.submit_op(op),
    }
}

async fn forward_external_op(app: &App, thread_id: ThreadId, op: Op) {
    let thread = match app.server.get_thread(thread_id).await {
        Ok(thread) => thread,
        Err(err) => {
            tracing::warn!("failed to find thread {thread_id} for approval response: {err}");
            return;
        }
    };
    if let Err(err) = thread.submit(op).await {
        tracing::warn!("failed to submit approval response to thread {thread_id}: {err}");
    }
}

fn finish_external_approval(app: &mut App) {
    if app.xcodex_state.external_approval_routes.is_empty() {
        while let Some(event) = app.xcodex_state.paused_codex_events.pop_front() {
            app.handle_codex_event_now(event);
        }
    }
}
