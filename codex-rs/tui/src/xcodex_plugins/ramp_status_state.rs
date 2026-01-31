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

#[derive(Debug, Default)]
pub(crate) struct RampStatusController {
    turn_index: u64,
    state: RampStatusState,
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

impl RampStatusController {
    pub(crate) fn is_enabled(&self) -> bool {
        self.state.is_enabled()
    }

    pub(crate) fn is_active(&self, task_running: bool) -> bool {
        self.state.is_active(task_running)
    }

    pub(crate) fn header_if_active(&self, task_running: bool) -> Option<String> {
        self.is_active(task_running)
            .then(|| self.state.header_string())
    }

    pub(crate) fn start_turn(&mut self, config: &Config, task_running: bool) -> Option<String> {
        if !self.is_enabled() {
            return None;
        }
        self.turn_index = self.turn_index.saturating_add(1);
        self.state.reset_for_turn(config, self.turn_index);
        self.header_if_active(task_running)
    }

    pub(crate) fn set_stage(
        &mut self,
        task_running: bool,
        stage: crate::ramps::RampStage,
    ) -> Option<String> {
        self.state.set_stage(task_running, stage)
    }

    pub(crate) fn set_context(
        &mut self,
        task_running: bool,
        context: Option<String>,
    ) -> Option<String> {
        self.state.set_context(task_running, context)
    }

    pub(crate) fn stage(&self) -> crate::ramps::RampStage {
        self.state.stage()
    }

    pub(crate) fn completion_label(&self) -> String {
        self.state.completion_label()
    }
}
