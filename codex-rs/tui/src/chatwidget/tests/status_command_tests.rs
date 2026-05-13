use super::*;

#[tokio::test]
async fn status_command_opens_status_menu_view() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);
    chat.instruction_source_paths = vec![chat.config.cwd.join("AGENTS.md").to_path_buf()];

    chat.dispatch_command(SlashCommand::Status);

    assert!(chat.bottom_pane.has_active_view());
    let rendered = render_bottom_popup(&chat, /*width*/ 80);
    assert!(
        rendered.contains("[ Status ]") && rendered.contains("Tip:"),
        "expected /status to open the xcodex status menu, got:\n{rendered}"
    );
    assert!(
        rendered.contains("Agents.md"),
        "expected /status menu to include upstream instruction-source status, got:\n{rendered}"
    );
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .all(|event| !matches!(event, AppEvent::InsertHistoryCell(_))),
        "/status should not append an upstream-style status card to history"
    );
}

#[tokio::test]
async fn settings_command_opens_settings_menu_view() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.dispatch_command(SlashCommand::Settings);

    assert!(chat.bottom_pane.has_active_view());
    let rendered = render_bottom_popup(&chat, /*width*/ 80);
    assert!(
        rendered.contains("[ Settings ]") && rendered.contains("Toggles apply immediately"),
        "expected /settings to open the xcodex settings menu, got:\n{rendered}"
    );
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .all(|event| !matches!(event, AppEvent::InsertHistoryCell(_))),
        "/settings should not append the legacy settings card to history"
    );
}

#[tokio::test]
async fn settings_command_with_args_keeps_legacy_xcodex_settings_commands() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.dispatch_command_with_args(
        SlashCommand::Settings,
        "status-bar model status".to_string(),
        Vec::new(),
    );

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one settings status card");
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("/settings") && rendered.contains("Model"),
        "expected /settings args to keep xcodex settings output, got:\n{rendered}"
    );
}
