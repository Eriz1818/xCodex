use codex_core::git_info::GitWorktreeEntry;

#[derive(Debug, Default)]
pub(crate) struct WorktreeListState {
    list: Vec<GitWorktreeEntry>,
    error: Option<String>,
    refresh_in_progress: bool,
}

impl WorktreeListState {
    pub(crate) fn list(&self) -> &[GitWorktreeEntry] {
        &self.list
    }

    pub(crate) fn error(&self) -> Option<&String> {
        self.error.as_ref()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub(crate) fn refresh_in_progress(&self) -> bool {
        self.refresh_in_progress
    }

    pub(crate) fn mark_refreshing(&mut self) {
        self.refresh_in_progress = true;
    }

    pub(crate) fn clear_no_repo(&mut self) {
        self.list.clear();
        self.error = None;
        self.refresh_in_progress = false;
    }

    pub(crate) fn set_list(&mut self, mut worktrees: Vec<GitWorktreeEntry>) {
        worktrees.sort_by(|a, b| a.path.cmp(&b.path));
        self.list = worktrees;
        self.error = None;
        self.refresh_in_progress = false;
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.list.clear();
        self.error = Some(error);
        self.refresh_in_progress = false;
    }
}
