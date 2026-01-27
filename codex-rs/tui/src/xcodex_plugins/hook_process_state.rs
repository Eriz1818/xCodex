use crate::history_cell::BackgroundActivityEntry;

#[derive(Debug, Default)]
pub(crate) struct HookProcessState {
    hooks: Vec<HookProcessSummary>,
}

#[derive(Debug)]
struct HookProcessSummary {
    key: String,
    command_display: String,
}

impl HookProcessState {
    pub(crate) fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    pub(crate) fn begin(&mut self, key: String, command_display: String) {
        if let Some(existing) = self.hooks.iter_mut().find(|hook| hook.key == key) {
            existing.command_display = command_display;
        } else {
            self.hooks.push(HookProcessSummary {
                key,
                command_display,
            });
        }
    }

    pub(crate) fn end(&mut self, key: &str) -> bool {
        let before = self.hooks.len();
        self.hooks.retain(|hook| hook.key != key);
        self.hooks.len() != before
    }

    pub(crate) fn clear(&mut self) {
        self.hooks.clear();
    }

    pub(crate) fn command_displays(&self) -> Vec<String> {
        self.hooks
            .iter()
            .map(|hook| hook.command_display.clone())
            .collect()
    }

    pub(crate) fn entries(&self) -> Vec<BackgroundActivityEntry> {
        self.hooks
            .iter()
            .map(|hook| {
                BackgroundActivityEntry::new(hook.key.clone(), hook.command_display.clone())
            })
            .collect()
    }
}
