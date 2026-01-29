use std::collections::HashMap;
use std::collections::VecDeque;

use codex_core::protocol::Event;
use codex_protocol::ThreadId;

#[derive(Default)]
pub(crate) struct XcodexAppState {
    pub(crate) shared_dirs_write_notice_shown: bool,
    // TODO(jif) drop once new UX is here.
    // Track external agent approvals spawned via AgentControl.
    /// Map routed approval IDs to their originating external threads and original IDs.
    pub(crate) external_approval_routes: HashMap<String, (ThreadId, String)>,
    /// Buffered Codex events while external approvals are pending.
    pub(crate) paused_codex_events: VecDeque<Event>,
}

impl XcodexAppState {
    pub(crate) fn take_shared_dirs_write_notice(&mut self) -> bool {
        let show_notice = !self.shared_dirs_write_notice_shown;
        self.shared_dirs_write_notice_shown = true;
        show_notice
    }
}
