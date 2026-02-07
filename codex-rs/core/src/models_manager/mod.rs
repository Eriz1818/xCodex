pub mod cache;
pub mod collaboration_mode_presets;
pub mod manager;
pub mod model_info;
pub mod model_presets;

#[cfg(any(test, feature = "test-support"))]
pub use collaboration_mode_presets::test_builtin_collaboration_mode_presets;

/// Convert the client version string to a whole version string (e.g. "1.2.3-alpha.4" -> "1.2.3").
pub fn client_version_to_whole() -> String {
    client_version_to_whole_impl(crate::config::is_xcodex_invocation())
}

fn client_version_to_whole_impl(is_xcodex_invocation: bool) -> String {
    if is_xcodex_invocation {
        return "0.98.0".to_string();
    }

    format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    )
}

#[cfg(test)]
mod tests {
    use super::client_version_to_whole_impl;
    use pretty_assertions::assert_eq;

    #[test]
    fn client_version_to_whole_uses_upstream_version_for_xcodex_invocation() {
        assert_eq!("0.98.0".to_string(), client_version_to_whole_impl(true));
    }

    #[test]
    fn client_version_to_whole_uses_package_version_for_codex_invocation() {
        let expected = format!(
            "{}.{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        );

        assert_eq!(expected, client_version_to_whole_impl(false));
    }
}
