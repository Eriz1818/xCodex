use std::path::Path;
use std::path::PathBuf;

use codex_core::parse_command::extract_shell_command;
use dirs::home_dir;
use shlex::try_join;

pub(crate) fn escape_command(command: &[String]) -> String {
    try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}

pub(crate) fn strip_bash_lc_and_escape(command: &[String]) -> String {
    if let Some((_, script)) = extract_shell_command(command) {
        return script.to_string();
    }
    escape_command(command)
}

pub(crate) fn build_copy_command_snippet(pretty_command: &str) -> String {
    let trimmed = pretty_command.trim_end_matches('\n');
    if !trimmed.contains('\n') {
        return trimmed.to_string();
    }

    let delimiter_base = "XCXCODEX_EOF";
    let mut delimiter = delimiter_base.to_string();
    let mut suffix = 0usize;
    while trimmed.lines().any(|line| line == delimiter) {
        suffix += 1;
        delimiter = format!("{delimiter_base}_{suffix}");
    }

    format!("bash -lc \"$(cat <<'{delimiter}'\n{trimmed}\n{delimiter}\n)\"")
}

/// If `path` is absolute and inside $HOME, return the part *after* the home
/// directory; otherwise, return the path as-is. Note if `path` is the homedir,
/// this will return and empty path.
pub(crate) fn relativize_to_home<P>(path: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if !path.is_absolute() {
        // If the path is not absolute, we can’t do anything with it.
        return None;
    }

    let home_dir = home_dir()?;
    let rel = path.strip_prefix(&home_dir).ok()?;
    Some(rel.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_command() {
        let args = vec!["foo".into(), "bar baz".into(), "weird&stuff".into()];
        let cmdline = escape_command(&args);
        assert_eq!(cmdline, "foo 'bar baz' 'weird&stuff'");
    }

    #[test]
    fn test_strip_bash_lc_and_escape() {
        // Test bash
        let args = vec!["bash".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test zsh
        let args = vec!["zsh".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test absolute path to zsh
        let args = vec!["/usr/bin/zsh".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test absolute path to bash
        let args = vec!["/bin/bash".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");
    }

    #[test]
    fn build_copy_command_snippet_wraps_multiline_script() {
        let snippet = build_copy_command_snippet("echo one\necho two\n");
        assert!(snippet.contains("bash -lc"));
        assert!(snippet.contains("echo one"));
        assert!(snippet.contains("echo two"));
        assert!(snippet.contains("XCXCODEX_EOF"));
    }

    #[test]
    fn build_copy_command_snippet_avoids_delimiter_collision() {
        let snippet = build_copy_command_snippet("echo one\nXCXCODEX_EOF\n");
        assert!(snippet.contains("XCXCODEX_EOF_1"));
    }
}
