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

    package_version_to_whole()
}

/// Returns the client version sent to the models endpoint.
///
/// We intentionally pin `client_version` to upstream Codex (`0.98.0`) when
/// querying ChatGPT/OpenAI model catalogs so backend model visibility matches
/// Codex behavior.
pub fn models_client_version(base_url: &str) -> String {
    models_client_version_impl(base_url, crate::config::is_xcodex_invocation())
}

fn models_client_version_impl(base_url: &str, is_xcodex_invocation: bool) -> String {
    if is_xcodex_invocation || is_openai_or_chatgpt_models_endpoint(base_url) {
        return "0.98.0".to_string();
    }

    package_version_to_whole()
}

fn package_version_to_whole() -> String {
    format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    )
}

fn is_openai_or_chatgpt_models_endpoint(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("api.openai.com") {
        return true;
    }
    if !host.eq_ignore_ascii_case("chatgpt.com") {
        return false;
    }
    let path = url.path().trim_end_matches('/');
    path == "/backend-api/codex" || path.starts_with("/backend-api/codex/")
}

#[cfg(test)]
mod tests {
    use super::client_version_to_whole_impl;
    use super::models_client_version_impl;
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

    #[test]
    fn models_client_version_uses_upstream_version_for_chatgpt_backend() {
        assert_eq!(
            "0.98.0".to_string(),
            models_client_version_impl("https://chatgpt.com/backend-api/codex", false)
        );
    }

    #[test]
    fn models_client_version_uses_upstream_version_for_openai_backend() {
        assert_eq!(
            "0.98.0".to_string(),
            models_client_version_impl("https://api.openai.com/v1", false)
        );
    }

    #[test]
    fn models_client_version_uses_package_version_for_non_openai_backend() {
        let expected = format!(
            "{}.{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        );
        assert_eq!(
            expected,
            models_client_version_impl("http://127.0.0.1:12345/v1", false)
        );
    }

    #[test]
    fn models_client_version_uses_package_version_for_non_openai_host_with_openai_path_segment() {
        let expected = format!(
            "{}.{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        );
        assert_eq!(
            expected,
            models_client_version_impl("https://example.com/api.openai.com/v1", false)
        );
    }
}
