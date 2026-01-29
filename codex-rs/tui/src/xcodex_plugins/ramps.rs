use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::chatwidget::ChatWidget;
use crate::ramps;
use crate::render::renderable::ColumnRenderable;
use crate::xcodex_plugins::RampStatusController;
use codex_core::config::Config;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) enum RampStatusUpdate {
    Context(String),
    Stage(ramps::RampStage),
}

pub(crate) fn status_enabled(ramps: &RampStatusController) -> bool {
    ramps.is_enabled()
}

pub(crate) fn status_active(ramps: &RampStatusController, task_running: bool) -> bool {
    ramps.is_active(task_running)
}

pub(crate) fn completion_label(ramps: &RampStatusController) -> Option<String> {
    status_enabled(ramps).then(|| ramps.completion_label())
}

pub(crate) fn show_separator(ramps: &RampStatusController) -> bool {
    status_enabled(ramps)
}

pub(crate) fn set_stage(
    ramps: &mut RampStatusController,
    task_running: bool,
    stage: crate::ramps::RampStage,
) -> Option<String> {
    ramps.set_stage(task_running, stage)
}

pub(crate) fn set_context(
    ramps: &mut RampStatusController,
    task_running: bool,
    context: Option<String>,
) -> Option<String> {
    ramps.set_context(task_running, context)
}

pub(crate) fn apply_status_header(
    ramps: &mut RampStatusController,
    task_running: bool,
    update: RampStatusUpdate,
) -> Option<String> {
    match update {
        RampStatusUpdate::Context(context) => {
            if status_active(ramps, task_running) {
                set_context(ramps, task_running, Some(context))
            } else {
                Some(context)
            }
        }
        RampStatusUpdate::Stage(stage) => {
            if status_active(ramps, task_running) {
                set_stage(ramps, task_running, stage)
            } else {
                None
            }
        }
    }
}

pub(crate) fn task_start_header(
    ramps: &mut RampStatusController,
    config: &Config,
    task_running: bool,
) -> String {
    if let Some(header) = ramps.start_turn(config, task_running) {
        return header;
    }

    if crate::xtreme::xtreme_ui_enabled(config) {
        "Charging".to_string()
    } else {
        "Working".to_string()
    }
}

pub(crate) fn open_settings_view(chat: &mut ChatWidget) {
    if !chat.ramp_status_enabled() {
        chat.add_info_message(super::ramps_unavailable_message().to_string(), None);
        return;
    }

    let (rotate, build, devops) = chat.ramps_config();
    let mut items: Vec<SelectionItem> = Vec::new();

    {
        let next_rotate = !rotate;
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateRampsConfig {
                rotate: next_rotate,
                build,
                devops,
            });
            tx.send(AppEvent::PersistRampsConfig {
                rotate: next_rotate,
                build,
                devops,
            });
            tx.send(AppEvent::OpenRampsSettingsView);
        })];

        items.push(SelectionItem {
            name: format!(
                "[{}] Rotate ramps (random per turn)",
                if rotate { "x" } else { " " }
            ),
            selected_description: Some(super::ramps_rotation_description().to_string()),
            actions,
            dismiss_on_select: true,
            ..Default::default()
        });
    }

    items.push(SelectionItem {
        name: "Hardware ramp (baseline)".to_string(),
        selected_description: Some(ramps::preview_flow(ramps::RampId::Hardware).to_string()),
        ..Default::default()
    });

    {
        let next_build = !build;
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateRampsConfig {
                rotate,
                build: next_build,
                devops,
            });
            tx.send(AppEvent::PersistRampsConfig {
                rotate,
                build: next_build,
                devops,
            });
            tx.send(AppEvent::OpenRampsSettingsView);
        })];

        items.push(SelectionItem {
            name: format!("[{}] Build ramp", if build { "x" } else { " " }),
            selected_description: Some(ramps::preview_flow(ramps::RampId::Build).to_string()),
            actions,
            dismiss_on_select: true,
            ..Default::default()
        });
    }

    {
        let next_devops = !devops;
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateRampsConfig {
                rotate,
                build,
                devops: next_devops,
            });
            tx.send(AppEvent::PersistRampsConfig {
                rotate,
                build,
                devops: next_devops,
            });
            tx.send(AppEvent::OpenRampsSettingsView);
        })];

        items.push(SelectionItem {
            name: format!("[{}] DevOps ramp", if devops { "x" } else { " " }),
            selected_description: Some(ramps::preview_flow(ramps::RampId::DevOps).to_string()),
            actions,
            dismiss_on_select: true,
            ..Default::default()
        });
    }

    let mut header = ColumnRenderable::new();
    header.push(Line::from("Ramps".bold()));
    header.push(Line::from(super::ramps_rotation_hint().dim()));

    chat.show_selection_view(SelectionViewParams {
        header: Box::new(header),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}
