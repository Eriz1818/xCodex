use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

/// Returns the path to the Codex configuration directory, which can be
/// specified by the `CODEX_HOME` environment variable. If not set, defaults to
/// `~/.codex` (or `~/.xcodex` when invoked as `xcodex`).
///
/// - If `CODEX_HOME` is set, the value must exist and be a directory. The
///   value will be canonicalized and this function will Err otherwise.
/// - If `CODEX_HOME` is not set, this function does not verify that the
///   directory exists.
pub(crate) fn find_codex_home() -> std::io::Result<PathBuf> {
    let codex_home_env = std::env::var("CODEX_HOME")
        .ok()
        .filter(|value| !value.is_empty());
    let mut path = codex_utils_home_dir::find_codex_home()?;

    if codex_home_env.is_none() {
        path = apply_default_home_dirname(path, default_home_dirname());
    }

    Ok(path)
}

const CODEX_DEFAULT_HOME_DIRNAME: &str = ".codex";
const XCODEX_DEFAULT_HOME_DIRNAME: &str = ".xcodex";
const XCODEX_EXE_STEM: &str = "xcodex";

fn is_xcodex_exe_name(name: &OsStr) -> bool {
    let Some(stem) = Path::new(name).file_stem().and_then(OsStr::to_str) else {
        return false;
    };
    stem == XCODEX_EXE_STEM || stem.starts_with("xcodex-")
}

pub(crate) fn is_xcodex_invocation() -> bool {
    if let Some(argv0) = std::env::args_os().next()
        && is_xcodex_exe_name(&argv0)
    {
        return true;
    }

    if let Ok(exe) = std::env::current_exe()
        && is_xcodex_exe_name(exe.as_os_str())
    {
        return true;
    }

    false
}

fn default_home_dirname_impl(is_xcodex: bool) -> &'static str {
    if is_xcodex {
        XCODEX_DEFAULT_HOME_DIRNAME
    } else {
        CODEX_DEFAULT_HOME_DIRNAME
    }
}

fn default_home_dirname() -> &'static str {
    default_home_dirname_impl(is_xcodex_invocation())
}

fn apply_default_home_dirname(mut path: PathBuf, default_home_dirname: &str) -> PathBuf {
    if default_home_dirname != CODEX_DEFAULT_HOME_DIRNAME && path.ends_with(".codex") {
        path.set_file_name(default_home_dirname);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_home_dirname_switches_for_xcodex_invocation() {
        assert_eq!(CODEX_DEFAULT_HOME_DIRNAME, default_home_dirname_impl(false));
        assert_eq!(XCODEX_DEFAULT_HOME_DIRNAME, default_home_dirname_impl(true));
    }

    #[test]
    fn xcodex_exe_name_matches_prefixed_names() {
        assert_eq!(true, is_xcodex_exe_name(OsStr::new("xcodex")));
        assert_eq!(
            true,
            is_xcodex_exe_name(OsStr::new("xcodex-x86_64-unknown-linux-musl"))
        );
        assert_eq!(false, is_xcodex_exe_name(OsStr::new("codex")));
    }

    #[test]
    fn apply_default_home_dirname_swaps_codex_suffix_for_xcodex() {
        let codex_home = PathBuf::from("tmp").join(".codex");
        let expected = PathBuf::from("tmp").join(".xcodex");
        assert_eq!(
            expected,
            apply_default_home_dirname(codex_home, XCODEX_DEFAULT_HOME_DIRNAME)
        );
    }

    #[test]
    fn apply_default_home_dirname_keeps_path_when_suffix_differs() {
        let home = PathBuf::from("tmp").join("custom-home");
        let expected = home.clone();
        assert_eq!(
            expected,
            apply_default_home_dirname(home, XCODEX_DEFAULT_HOME_DIRNAME)
        );
    }
}
