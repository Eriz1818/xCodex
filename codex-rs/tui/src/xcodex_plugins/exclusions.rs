use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::chatwidget::ChatWidget;
use crate::render::renderable::ColumnRenderable;
use codex_core::config::types::ExclusionConfig;
use codex_core::config::types::ExclusionOnMatch;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) fn handle_exclusions_command(chat: &mut ChatWidget, _rest: &str) -> bool {
    open_exclusions_menu(chat);
    true
}

pub(crate) fn open_exclusions_menu(chat: &mut ChatWidget) {
    let config = chat.config_ref();
    let exclusion = config.exclusion.clone();
    let hooks_sanitize_payloads = config.xcodex.hooks.sanitize_payloads;

    let mut items: Vec<SelectionItem> = Vec::new();

    let preset_items = build_preset_items(&exclusion, hooks_sanitize_payloads);
    items.extend(preset_items);

    items.push(SelectionItem {
        name: "".to_string(),
        is_disabled: true,
        ..Default::default()
    });

    let toggles = build_toggle_items(&exclusion, hooks_sanitize_payloads);
    items.extend(toggles);

    let mut header = ColumnRenderable::new();
    header.push(Line::from("Exclusions".bold()));
    header.push(Line::from("Adjust exclusion layers and prompts.".dim()));

    chat.show_selection_view(SelectionViewParams {
        header: Box::new(header),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}

fn build_preset_items(
    current: &ExclusionConfig,
    hooks_sanitize_payloads: bool,
) -> Vec<SelectionItem> {
    let presets = vec![
        (
            "Allow all (exclusions off)",
            Preset::AllowAll,
            "Disable exclusion matching and redaction.",
        ),
        (
            "Block all (no prompts)",
            Preset::BlockAll,
            "Block excluded paths and content without asking.",
        ),
        (
            "Ask and allow",
            Preset::AskAndAllow,
            "Prompt before accessing excluded paths.",
        ),
    ];

    presets
        .into_iter()
        .map(|(name, preset, description)| {
            let next = preset.apply(current, hooks_sanitize_payloads);
            let is_current = preset.matches(current, hooks_sanitize_payloads);
            let mut actions: Vec<SelectionAction> = Vec::new();
            if !is_current {
                actions.push(Box::new(move |tx: &AppEventSender| {
                    tx.send(AppEvent::UpdateExclusionSettings {
                        exclusion: next.exclusion.clone(),
                        hooks_sanitize_payloads: next.hooks_sanitize_payloads,
                    });
                    tx.send(AppEvent::PersistExclusionSettings {
                        exclusion: next.exclusion.clone(),
                        hooks_sanitize_payloads: next.hooks_sanitize_payloads,
                    });
                    tx.send(AppEvent::OpenToolsCommand {
                        command: "/exclusion".to_string(),
                    });
                }));
            }
            SelectionItem {
                name: name.to_string(),
                description: Some(description.to_string()),
                is_current,
                dismiss_on_select: true,
                actions,
                ..Default::default()
            }
        })
        .collect()
}

fn build_toggle_items(
    current: &ExclusionConfig,
    hooks_sanitize_payloads: bool,
) -> Vec<SelectionItem> {
    let mut items: Vec<SelectionItem> = Vec::new();

    let toggles = vec![
        (
            "Enabled",
            Toggle::Enabled,
            "Enable exclusions for this session.",
        ),
        (
            "Path matching (L1)",
            Toggle::PathMatching,
            "Block excluded file paths on reads and listings.",
        ),
        (
            "Output sanitization (L2)",
            Toggle::LayerOutput,
            "Redact tool outputs before they reach the model.",
        ),
        (
            "Send firewall (L3)",
            Toggle::LayerSend,
            "Block excluded file payloads from being sent.",
        ),
        (
            "Request interceptor (L4)",
            Toggle::LayerRequest,
            "Scan the full prompt payload before sending.",
        ),
        (
            "Hook payload sanitizer (L5)",
            Toggle::HooksPayloads,
            "Redact hook payload strings before dispatch.",
        ),
        (
            "Prompt when blocked",
            Toggle::PromptOnBlocked,
            "Ask before allowing excluded paths and payloads.",
        ),
    ];

    for (label, toggle, description) in toggles {
        let is_on = toggle.is_enabled(current, hooks_sanitize_payloads);
        let next = toggle.apply(current, hooks_sanitize_payloads);
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx: &AppEventSender| {
            tx.send(AppEvent::UpdateExclusionSettings {
                exclusion: next.exclusion.clone(),
                hooks_sanitize_payloads: next.hooks_sanitize_payloads,
            });
            tx.send(AppEvent::PersistExclusionSettings {
                exclusion: next.exclusion.clone(),
                hooks_sanitize_payloads: next.hooks_sanitize_payloads,
            });
            tx.send(AppEvent::OpenToolsCommand {
                command: "/exclusion".to_string(),
            });
        })];

        items.push(SelectionItem {
            name: format!("[{}] {label}", if is_on { "x" } else { " " }),
            description: Some(description.to_string()),
            dismiss_on_select: true,
            actions,
            ..Default::default()
        });
    }

    items
}

#[derive(Clone, Copy)]
enum Toggle {
    Enabled,
    PathMatching,
    LayerOutput,
    LayerSend,
    LayerRequest,
    HooksPayloads,
    PromptOnBlocked,
}

impl Toggle {
    fn is_enabled(self, current: &ExclusionConfig, hooks_sanitize_payloads: bool) -> bool {
        match self {
            Toggle::Enabled => current.enabled,
            Toggle::PathMatching => current.path_matching,
            Toggle::LayerOutput => current.layer_output_sanitization_enabled(),
            Toggle::LayerSend => current.layer_send_firewall_enabled(),
            Toggle::LayerRequest => current.layer_request_interceptor_enabled(),
            Toggle::HooksPayloads => hooks_sanitize_payloads,
            Toggle::PromptOnBlocked => current.prompt_on_blocked,
        }
    }

    fn apply(self, current: &ExclusionConfig, hooks_sanitize_payloads: bool) -> NextSettings {
        let mut exclusion = current.clone();
        let mut hooks_sanitize_payloads = hooks_sanitize_payloads;
        match self {
            Toggle::Enabled => exclusion.enabled = !exclusion.enabled,
            Toggle::PathMatching => exclusion.path_matching = !exclusion.path_matching,
            Toggle::LayerOutput => {
                exclusion.layer_output_sanitization =
                    Some(!exclusion.layer_output_sanitization_enabled());
            }
            Toggle::LayerSend => {
                exclusion.layer_send_firewall = Some(!exclusion.layer_send_firewall_enabled());
            }
            Toggle::LayerRequest => {
                exclusion.layer_request_interceptor =
                    Some(!exclusion.layer_request_interceptor_enabled());
            }
            Toggle::HooksPayloads => hooks_sanitize_payloads = !hooks_sanitize_payloads,
            Toggle::PromptOnBlocked => exclusion.prompt_on_blocked = !exclusion.prompt_on_blocked,
        }
        NextSettings {
            exclusion,
            hooks_sanitize_payloads,
        }
    }
}

#[derive(Clone)]
struct NextSettings {
    exclusion: ExclusionConfig,
    hooks_sanitize_payloads: bool,
}

#[derive(Clone, Copy)]
enum Preset {
    AllowAll,
    BlockAll,
    AskAndAllow,
}

impl Preset {
    fn apply(self, current: &ExclusionConfig, _hooks_sanitize_payloads: bool) -> NextSettings {
        let mut exclusion = current.clone();
        let hooks_sanitize_payloads = match self {
            Preset::AllowAll => {
                exclusion.enabled = false;
                exclusion.prompt_on_blocked = false;
                exclusion.on_match = ExclusionOnMatch::Warn;
                exclusion.layer_output_sanitization = Some(false);
                exclusion.layer_send_firewall = Some(false);
                exclusion.layer_request_interceptor = Some(false);
                false
            }
            Preset::BlockAll => {
                exclusion.enabled = true;
                exclusion.on_match = ExclusionOnMatch::Block;
                exclusion.prompt_on_blocked = false;
                exclusion.layer_output_sanitization = Some(true);
                exclusion.layer_send_firewall = Some(true);
                exclusion.layer_request_interceptor = Some(true);
                true
            }
            Preset::AskAndAllow => {
                exclusion.enabled = true;
                exclusion.on_match = ExclusionOnMatch::Redact;
                exclusion.prompt_on_blocked = true;
                exclusion.layer_output_sanitization = Some(true);
                exclusion.layer_send_firewall = Some(true);
                exclusion.layer_request_interceptor = Some(true);
                true
            }
        };

        NextSettings {
            exclusion,
            hooks_sanitize_payloads,
        }
    }

    fn matches(self, current: &ExclusionConfig, hooks_sanitize_payloads: bool) -> bool {
        let next = self.apply(current, hooks_sanitize_payloads);
        next.exclusion.enabled == current.enabled
            && next.exclusion.prompt_on_blocked == current.prompt_on_blocked
            && next.exclusion.on_match == current.on_match
            && next.exclusion.layer_output_sanitization == current.layer_output_sanitization
            && next.exclusion.layer_send_firewall == current.layer_send_firewall
            && next.exclusion.layer_request_interceptor == current.layer_request_interceptor
    }
}
