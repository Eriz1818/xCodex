use crate::auth::AuthProvider;
use crate::common::CompactionInput;
use crate::endpoint::client_version::append_upstream_client_version_for_openai_or_chatgpt;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use codex_client::HttpTransport;
use codex_client::RequestTelemetry;
use codex_protocol::models::ResponseItem;
use http::HeaderMap;
use http::Method;
use serde::Deserialize;
use serde_json::to_value;
use std::sync::Arc;

pub struct CompactClient<T: HttpTransport, A: AuthProvider> {
    session: EndpointSession<T, A>,
}

impl<T: HttpTransport, A: AuthProvider> CompactClient<T, A> {
    pub fn new(transport: T, provider: Provider, auth: A) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
        }
    }

    pub fn with_telemetry(self, request: Option<Arc<dyn RequestTelemetry>>) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
        }
    }

    fn path() -> &'static str {
        "responses/compact"
    }

    pub async fn compact(
        &self,
        body: serde_json::Value,
        extra_headers: HeaderMap,
    ) -> Result<Vec<ResponseItem>, ApiError> {
        let resp = self
            .session
            .execute_with(
                Method::POST,
                Self::path(),
                extra_headers,
                Some(body),
                |req| {
                    append_upstream_client_version_for_openai_or_chatgpt(
                        req,
                        &self.session.provider().base_url,
                    );
                },
            )
            .await?;
        let parsed: CompactHistoryResponse =
            serde_json::from_slice(&resp.body).map_err(|e| ApiError::Stream(e.to_string()))?;
        Ok(parsed.output)
    }

    pub async fn compact_input(
        &self,
        input: &CompactionInput<'_>,
        extra_headers: HeaderMap,
    ) -> Result<Vec<ResponseItem>, ApiError> {
        let body = to_value(input)
            .map_err(|e| ApiError::Stream(format!("failed to encode compaction input: {e}")))?;
        self.compact(body, extra_headers).await
    }
}

#[derive(Debug, Deserialize)]
struct CompactHistoryResponse {
    output: Vec<ResponseItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use codex_client::Request;
    use codex_client::RequestCompression;
    use codex_client::Response;
    use codex_client::StreamResponse;
    use codex_client::TransportError;
    use http::HeaderMap;
    use http::StatusCode;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Clone)]
    struct CapturingTransport {
        last_request: Arc<Mutex<Option<Request>>>,
    }

    impl Default for CapturingTransport {
        fn default() -> Self {
            Self {
                last_request: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait]
    impl HttpTransport for CapturingTransport {
        async fn execute(&self, req: Request) -> Result<Response, TransportError> {
            *self.last_request.lock().unwrap() = Some(req);
            Ok(Response {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: br#"{"output":[]}"#.to_vec().into(),
            })
        }

        async fn stream(&self, _req: Request) -> Result<StreamResponse, TransportError> {
            Err(TransportError::Build("stream should not run".to_string()))
        }
    }

    #[derive(Clone, Default)]
    struct DummyAuth;

    impl AuthProvider for DummyAuth {
        fn bearer_token(&self) -> Option<String> {
            None
        }
    }

    fn provider(base_url: &str) -> Provider {
        Provider {
            name: "test".to_string(),
            base_url: base_url.to_string(),
            query_params: None,
            headers: HeaderMap::new(),
            retry: crate::provider::RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn path_is_responses_compact() {
        assert_eq!(
            CompactClient::<CapturingTransport, DummyAuth>::path(),
            "responses/compact"
        );
    }

    #[tokio::test]
    async fn compact_appends_client_version_for_chatgpt_backend() {
        let transport = CapturingTransport::default();
        let client = CompactClient::new(
            transport.clone(),
            provider("https://chatgpt.com/backend-api/codex"),
            DummyAuth,
        );

        client
            .compact(serde_json::json!({ "input": [] }), HeaderMap::new())
            .await
            .expect("compact should succeed");

        let request = transport
            .last_request
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .expect("request should be captured");
        assert_eq!(
            request.url,
            "https://chatgpt.com/backend-api/codex/responses/compact?client_version=0.98.0"
        );
        assert_eq!(
            request
                .headers
                .get("version")
                .and_then(|value| value.to_str().ok()),
            Some("0.98.0")
        );
        assert_eq!(request.method, http::Method::POST);
        assert_eq!(request.compression, RequestCompression::None);
    }

    #[tokio::test]
    async fn compact_does_not_append_client_version_for_custom_backend() {
        let transport = CapturingTransport::default();
        let client = CompactClient::new(
            transport.clone(),
            provider("http://127.0.0.1:1234/v1"),
            DummyAuth,
        );

        client
            .compact(serde_json::json!({ "input": [] }), HeaderMap::new())
            .await
            .expect("compact should succeed");

        let request = transport
            .last_request
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .expect("request should be captured");
        assert_eq!(request.url, "http://127.0.0.1:1234/v1/responses/compact");
        assert_eq!(request.headers.get("version"), None);
    }
}
