pub(crate) mod cache;
pub mod collaboration_mode_presets;
pub(crate) mod config;
pub mod manager;
pub mod model_info;
pub mod model_presets;

pub use codex_app_server_protocol::AuthMode;
pub use codex_login::AuthManager;
pub use codex_login::CodexAuth;
pub use codex_model_provider_info::ModelProviderInfo;
pub use codex_model_provider_info::WireApi;
pub use config::ModelsManagerConfig;
use std::ffi::OsStr;
use std::path::Path;

const UPSTREAM_CODEX_CLIENT_VERSION: &str = "0.130.0";

/// Load the bundled model catalog shipped with `codex-models-manager`.
pub fn bundled_models_response()
-> std::result::Result<codex_protocol::openai_models::ModelsResponse, serde_json::Error> {
    serde_json::from_str(include_str!("../models.json"))
}

/// Convert the client version string to a whole version string (e.g. "1.2.3-alpha.4" -> "1.2.3").
pub fn client_version_to_whole() -> String {
    client_version_to_whole_impl(is_xcodex_invocation())
}

/// Returns the client version sent to the models endpoint.
///
/// We intentionally pin `client_version` to upstream Codex when
/// querying ChatGPT/OpenAI model catalogs so backend model visibility matches
/// Codex behavior.
pub fn models_client_version(base_url: &str) -> String {
    models_client_version_impl(base_url, is_xcodex_invocation())
}

fn client_version_to_whole_impl(is_xcodex_invocation: bool) -> String {
    if is_xcodex_invocation {
        return UPSTREAM_CODEX_CLIENT_VERSION.to_string();
    }

    package_version_to_whole()
}

fn models_client_version_impl(base_url: &str, is_xcodex_invocation: bool) -> String {
    if is_xcodex_invocation || is_openai_or_chatgpt_models_endpoint(base_url) {
        return UPSTREAM_CODEX_CLIENT_VERSION.to_string();
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
    let Ok(uri) = base_url.parse::<http::Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    let host = authority.host();
    if host.eq_ignore_ascii_case("api.openai.com") {
        return true;
    }
    if !host.eq_ignore_ascii_case("chatgpt.com") {
        return false;
    }
    let path = uri.path().trim_end_matches('/');
    path == "/backend-api/codex" || path.starts_with("/backend-api/codex/")
}

fn is_xcodex_exe_name(name: &OsStr) -> bool {
    let Some(stem) = Path::new(name).file_stem().and_then(OsStr::to_str) else {
        return false;
    };
    stem == "xcodex" || stem.starts_with("xcodex-")
}

fn is_xcodex_invocation() -> bool {
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

#[cfg(test)]
mod tests {
    use super::UPSTREAM_CODEX_CLIENT_VERSION;
    use super::client_version_to_whole_impl;
    use super::models_client_version_impl;
    use pretty_assertions::assert_eq;

    #[test]
    fn client_version_to_whole_uses_upstream_version_for_xcodex_invocation() {
        assert_eq!(
            UPSTREAM_CODEX_CLIENT_VERSION.to_string(),
            client_version_to_whole_impl(true)
        );
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
            UPSTREAM_CODEX_CLIENT_VERSION.to_string(),
            models_client_version_impl("https://chatgpt.com/backend-api/codex", false)
        );
    }

    #[test]
    fn models_client_version_uses_upstream_version_for_openai_backend() {
        assert_eq!(
            UPSTREAM_CODEX_CLIENT_VERSION.to_string(),
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
