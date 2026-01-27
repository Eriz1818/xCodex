use codex_core::config::Config;

#[derive(Debug)]
pub(crate) struct RampStatusState {
    selected: crate::ramps::RampId,
    stage: crate::ramps::RampStage,
    context: Option<String>,
}

impl Default for RampStatusState {
    fn default() -> Self {
        Self {
            selected: crate::ramps::baseline_ramp(),
            stage: crate::ramps::RampStage::Waiting,
            context: None,
        }
    }
}

impl RampStatusState {
    pub(crate) fn reset_for_turn(&mut self, config: &Config, turn_index: u64) {
        self.selected = crate::ramps::select_ramp(config, turn_index);
        self.stage = crate::ramps::RampStage::Waiting;
        self.context = None;
    }

    pub(crate) fn is_enabled(&self) -> bool {
        crate::ramps::ramps_enabled()
    }

    pub(crate) fn is_active(&self, task_running: bool) -> bool {
        self.is_enabled() && task_running
    }

    pub(crate) fn header_string(&self) -> String {
        let stage = crate::ramps::stage_label(self.selected, self.stage);
        if let Some(context) = self.context.as_deref()
            && !context.is_empty()
        {
            format!("{stage} · {context}")
        } else {
            stage.to_string()
        }
    }

    pub(crate) fn set_stage(
        &mut self,
        task_running: bool,
        stage: crate::ramps::RampStage,
    ) -> Option<String> {
        if !self.is_active(task_running) || self.stage == stage {
            return None;
        }
        self.stage = stage;
        Some(self.header_string())
    }

    pub(crate) fn set_context(
        &mut self,
        task_running: bool,
        context: Option<String>,
    ) -> Option<String> {
        if !self.is_active(task_running) && context.is_some() {
            return None;
        }
        if self.context == context {
            return None;
        }
        self.context = context;
        if self.is_active(task_running) {
            Some(self.header_string())
        } else {
            None
        }
    }

    pub(crate) fn stage(&self) -> crate::ramps::RampStage {
        self.stage
    }

    pub(crate) fn completion_label(&self) -> String {
        crate::ramps::completion_label(self.selected).to_string()
    }
}
