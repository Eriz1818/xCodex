use codex_client::Request;
use http::HeaderValue;
use http::header::HeaderName;

pub(crate) const UPSTREAM_CODEX_CLIENT_VERSION: &str = "0.98.0";

pub(crate) fn append_client_version_query(req: &mut Request, client_version: &str) {
    let separator = if req.url.contains('?') { '&' } else { '?' };
    req.url = format!("{}{}client_version={client_version}", req.url, separator);
}

pub(crate) fn append_upstream_client_version_for_openai_or_chatgpt(
    req: &mut Request,
    base_url: &str,
) {
    if should_pin_upstream_client_version(base_url) {
        append_client_version_query(req, UPSTREAM_CODEX_CLIENT_VERSION);
        set_upstream_version_header_for_openai_or_chatgpt(req, base_url);
    }
}

pub(crate) fn set_upstream_version_header_for_openai_or_chatgpt(req: &mut Request, base_url: &str) {
    if should_pin_upstream_client_version(base_url) {
        set_version_header(req, UPSTREAM_CODEX_CLIENT_VERSION);
    }
}

pub(crate) fn set_version_header(req: &mut Request, version: &str) {
    let Ok(value) = HeaderValue::from_str(version) else {
        return;
    };
    req.headers
        .insert(HeaderName::from_static("version"), value);
}

fn should_pin_upstream_client_version(base_url: &str) -> bool {
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
    use super::UPSTREAM_CODEX_CLIENT_VERSION;
    use super::append_upstream_client_version_for_openai_or_chatgpt;
    use super::set_upstream_version_header_for_openai_or_chatgpt;
    use super::set_version_header;
    use codex_client::Request;
    use codex_client::RequestCompression;
    use http::HeaderMap;
    use http::Method;
    use pretty_assertions::assert_eq;

    fn request(url: &str) -> Request {
        Request {
            method: Method::GET,
            url: url.to_string(),
            headers: HeaderMap::new(),
            body: None,
            compression: RequestCompression::None,
            timeout: None,
        }
    }

    #[test]
    fn appends_query_for_chatgpt_backend() {
        let mut req = request("https://chatgpt.com/backend-api/codex/responses");
        append_upstream_client_version_for_openai_or_chatgpt(
            &mut req,
            "https://chatgpt.com/backend-api/codex",
        );

        assert_eq!(
            req.url,
            format!(
                "https://chatgpt.com/backend-api/codex/responses?client_version={UPSTREAM_CODEX_CLIENT_VERSION}"
            )
        );
        assert_eq!(
            req.headers
                .get("version")
                .and_then(|value| value.to_str().ok()),
            Some(UPSTREAM_CODEX_CLIENT_VERSION)
        );
    }

    #[test]
    fn appends_query_for_openai_backend() {
        let mut req = request("https://api.openai.com/v1/responses");
        append_upstream_client_version_for_openai_or_chatgpt(&mut req, "https://api.openai.com/v1");

        assert_eq!(
            req.url,
            format!(
                "https://api.openai.com/v1/responses?client_version={UPSTREAM_CODEX_CLIENT_VERSION}"
            )
        );
        assert_eq!(
            req.headers
                .get("version")
                .and_then(|value| value.to_str().ok()),
            Some(UPSTREAM_CODEX_CLIENT_VERSION)
        );
    }

    #[test]
    fn does_not_append_query_for_custom_backend() {
        let mut req = request("http://127.0.0.1:1234/v1/responses");
        append_upstream_client_version_for_openai_or_chatgpt(&mut req, "http://127.0.0.1:1234/v1");

        assert_eq!(req.url, "http://127.0.0.1:1234/v1/responses");
        assert_eq!(req.headers.get("version"), None);
    }

    #[test]
    fn set_version_header_is_noop_for_invalid_value() {
        let mut req = request("http://127.0.0.1:1234/v1/responses");
        set_version_header(&mut req, "a\nb");
        assert_eq!(req.headers.get("version"), None);
    }

    #[test]
    fn set_upstream_version_header_is_noop_for_custom_backend() {
        let mut req = request("http://127.0.0.1:1234/v1/responses");
        set_upstream_version_header_for_openai_or_chatgpt(&mut req, "http://127.0.0.1:1234/v1");
        assert_eq!(req.headers.get("version"), None);
    }
}
