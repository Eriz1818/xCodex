use ignore::gitignore::Gitignore;
use ignore::gitignore::GitignoreBuilder;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

use crate::config::types::ExclusionConfig;
use crate::git_info::get_git_repo_root;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitivePathDecision {
    Allow,
    Deny,
}

#[derive(Debug)]
pub struct SensitivePathPolicy {
    repo_root: Option<PathBuf>,
    enabled: bool,
    path_matching: bool,
    ignore_files: Vec<String>,
    ignores: OnceLock<Option<Gitignore>>,
}

impl SensitivePathPolicy {
    pub fn new(cwd: PathBuf) -> Self {
        Self::new_with_exclusion(cwd, ExclusionConfig::default())
    }

    pub fn new_with_exclusion(cwd: PathBuf, exclusion: ExclusionConfig) -> Self {
        let repo_root = get_git_repo_root(&cwd);
        Self {
            repo_root,
            enabled: exclusion.enabled,
            path_matching: exclusion.path_matching,
            ignore_files: exclusion.files,
            ignores: OnceLock::new(),
        }
    }

    pub fn decision_discover(&self, path: &Path) -> SensitivePathDecision {
        self.decision_discover_with_is_dir(path, None)
    }

    pub fn decision_discover_with_is_dir(
        &self,
        path: &Path,
        is_dir: Option<bool>,
    ) -> SensitivePathDecision {
        if self.is_ignore_file(path) {
            return SensitivePathDecision::Deny;
        }
        if self.is_ignore_match(path, is_dir) {
            SensitivePathDecision::Deny
        } else {
            SensitivePathDecision::Allow
        }
    }

    pub fn decision_send(&self, path: &Path) -> SensitivePathDecision {
        if self.is_ignore_file(path) {
            return SensitivePathDecision::Deny;
        }
        self.decision_discover(path)
    }

    pub fn decision_send_relative(&self, path: &Path) -> SensitivePathDecision {
        if self.is_ignore_file(path) {
            return SensitivePathDecision::Deny;
        }
        self.decision_discover_relative(path, None)
    }

    pub fn is_exclusion_control_path(&self, path: &Path) -> bool {
        self.is_ignore_file(path)
    }

    pub fn ignore_file_paths(&self) -> Vec<PathBuf> {
        if !self.enabled {
            return Vec::new();
        }

        let Some(root) = self.repo_root.as_ref() else {
            return Vec::new();
        };

        self.ignore_files
            .iter()
            .map(|name| root.join(name))
            .filter(|path| path.is_file())
            .collect()
    }

    pub fn ignore_epoch(&self) -> u64 {
        // Keep this stable and cheap: combine metadata for the ignore files only.
        let mut acc: u64 = 0x9e3779b97f4a7c15;
        for path in self.ignore_file_paths() {
            let meta = match path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let len = meta.len();
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let since_epoch = modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = since_epoch.as_secs();
            let nanos = u64::from(since_epoch.subsec_nanos());

            // Mix: xorshift-like.
            acc ^= secs.wrapping_mul(0x9e3779b97f4a7c15);
            acc = acc.rotate_left(13);
            acc ^= nanos.wrapping_mul(0xc2b2ae3d27d4eb4f);
            acc = acc.rotate_left(17);
            acc ^= len.wrapping_mul(0x165667b19e3779f9);
            acc = acc.rotate_left(11);
        }
        acc
    }

    fn is_ignore_match(&self, path: &Path, is_dir: Option<bool>) -> bool {
        if !self.enabled || !self.path_matching {
            return false;
        }

        let Some(root) = self.repo_root.as_ref() else {
            return false;
        };

        let Some(relative) = path.strip_prefix(root).ok() else {
            return false;
        };

        if self.is_ignore_file(relative) {
            return true;
        }

        let Some(matcher) = self.ignore_matcher() else {
            return false;
        };

        let is_dir = is_dir.unwrap_or_else(|| path.is_dir());
        matcher
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
    }

    fn decision_discover_relative(
        &self,
        relative: &Path,
        is_dir: Option<bool>,
    ) -> SensitivePathDecision {
        if self.is_ignore_file(relative) {
            return SensitivePathDecision::Deny;
        }

        if !self.enabled || !self.path_matching {
            return SensitivePathDecision::Allow;
        }

        let Some(_root) = self.repo_root.as_ref() else {
            return SensitivePathDecision::Allow;
        };

        let Some(matcher) = self.ignore_matcher() else {
            return SensitivePathDecision::Allow;
        };

        let is_dir = is_dir.unwrap_or(false);
        if matcher
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
        {
            SensitivePathDecision::Deny
        } else {
            SensitivePathDecision::Allow
        }
    }

    fn is_ignore_file(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|p| p.to_str());
        let Some(name) = name else {
            return false;
        };
        self.ignore_files.iter().any(|candidate| candidate == name)
    }

    fn ignore_matcher(&self) -> Option<&Gitignore> {
        self.ignores
            .get_or_init(|| {
                if !self.enabled || !self.path_matching {
                    return None;
                }
                let root = self.repo_root.as_ref()?;
                let ignore_paths = self
                    .ignore_files
                    .iter()
                    .map(|name| root.join(name))
                    .filter(|path| path.is_file())
                    .collect::<Vec<_>>();
                if ignore_paths.is_empty() {
                    return None;
                }
                let mut builder = GitignoreBuilder::new(root);
                for ignore_path in ignore_paths {
                    builder.add(ignore_path);
                }
                builder.build().ok()
            })
            .as_ref()
    }

    pub fn format_denied_message(&self) -> String {
        // Keep the message stable and avoid leaking paths.
        "denied by sensitive-path policy".to_string()
    }
}
