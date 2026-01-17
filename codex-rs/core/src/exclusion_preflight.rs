use crate::content_gateway::normalized_candidate_variants;
use crate::content_gateway::pathlike_candidates_in_text;
use crate::sensitive_paths::SensitivePathDecision;
use crate::sensitive_paths::SensitivePathPolicy;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

pub(crate) fn shell_command_references_excluded_paths(
    command: &[String],
    cwd: &Path,
    sensitive_paths: &SensitivePathPolicy,
) -> bool {
    for arg in command {
        for candidate in pathlike_candidates_in_text(arg) {
            if candidate.contains("://") {
                continue;
            }

            for variant in normalized_candidate_variants(&candidate) {
                if variant.contains("://") {
                    continue;
                }
                let path = if Path::new(&variant).is_absolute() {
                    PathBuf::from(&variant)
                } else {
                    cwd.join(&variant)
                };
                let path = normalize_path_without_fs(&path);
                if sensitive_paths.decision_send(&path) == SensitivePathDecision::Deny {
                    return true;
                }
            }
        }
    }
    false
}

fn normalize_path_without_fs(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::process::Command;
    use tempfile::tempdir;

    fn init_repo(dir: &std::path::Path) {
        let status = Command::new("git")
            .arg("init")
            .current_dir(dir)
            .status()
            .expect("git init");
        assert!(status.success(), "git init failed");
    }

    #[test]
    fn blocks_shell_command_when_args_reference_ignored_paths() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        std::fs::write(tmp.path().join(".aiexclude"), "secrets/\n").expect("write ignore");

        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());

        let command = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "cat secrets/.env".to_string(),
        ];
        assert_eq!(
            shell_command_references_excluded_paths(&command, tmp.path(), &policy),
            true
        );

        let ok = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "cat public.txt".to_string(),
        ];
        assert_eq!(
            shell_command_references_excluded_paths(&ok, tmp.path(), &policy),
            false
        );
    }

    #[test]
    fn blocks_shell_command_for_windows_style_paths_that_map_to_repo_relative() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        std::fs::write(tmp.path().join(".aiexclude"), "secrets/\n").expect("write ignore");

        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());

        let command = vec![
            "bash".to_string(),
            "-lc".to_string(),
            r"type ..\secrets\hidden.txt".to_string(),
        ];
        assert_eq!(
            shell_command_references_excluded_paths(&command, tmp.path(), &policy),
            true
        );
    }
}
