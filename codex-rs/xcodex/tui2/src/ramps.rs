use codex_core::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RampId {
    Hardware,
    Build,
    DevOps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RampStage {
    Waiting,
    Warmup,
    Active,
    Stabilizing,
}

pub(crate) fn ramps_enabled() -> bool {
    codex_core::config::is_xcodex_invocation()
}

pub(crate) fn baseline_ramp() -> RampId {
    RampId::Hardware
}

pub(crate) fn select_ramp(config: &Config, turn_index: u64) -> RampId {
    let mut eligible = Vec::with_capacity(3);
    eligible.push(RampId::Hardware);
    if config.xcodex.tui_ramps_build {
        eligible.push(RampId::Build);
    }
    if config.xcodex.tui_ramps_devops {
        eligible.push(RampId::DevOps);
    }

    if !config.xcodex.tui_ramps_rotate || eligible.len() == 1 {
        return RampId::Hardware;
    }

    // Deterministic rotation keeps tests/snapshots stable.
    let idx = (turn_index.saturating_sub(1) as usize) % eligible.len();
    eligible[idx]
}

pub(crate) fn stage_label(ramp: RampId, stage: RampStage) -> &'static str {
    match (ramp, stage) {
        (RampId::Hardware, RampStage::Waiting) => "Charging",
        (RampId::Hardware, RampStage::Warmup) => "Spooling",
        (RampId::Hardware, RampStage::Active) => "Overclocking",
        (RampId::Hardware, RampStage::Stabilizing) => "Stabilizing",

        (RampId::Build, RampStage::Waiting) => "Staging",
        (RampId::Build, RampStage::Warmup) => "Compiling",
        (RampId::Build, RampStage::Active) => "Linking",
        (RampId::Build, RampStage::Stabilizing) => "Verifying",

        (RampId::DevOps, RampStage::Waiting) => "Provisioning",
        (RampId::DevOps, RampStage::Warmup) => "Deploying",
        (RampId::DevOps, RampStage::Active) => "Reconciling",
        (RampId::DevOps, RampStage::Stabilizing) => "Verifying",
    }
}

pub(crate) fn completion_label(ramp: RampId) -> &'static str {
    match ramp {
        RampId::Hardware => "Overclocked",
        RampId::Build => "Built",
        RampId::DevOps => "Deployed",
    }
}

pub(crate) fn preview_flow(ramp: RampId) -> &'static str {
    match ramp {
        RampId::Hardware => "Charging → Spooling → Overclocking → Stabilizing → Overclocked in …",
        RampId::Build => "Staging → Compiling → Linking → Verifying → Built in …",
        RampId::DevOps => "Provisioning → Deploying → Reconciling → Verifying → Deployed in …",
    }
}
