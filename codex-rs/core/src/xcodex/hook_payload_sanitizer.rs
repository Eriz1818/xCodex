use crate::config::types::ExclusionConfig;
use crate::content_gateway::ContentGateway;
use crate::content_gateway::GatewayCache;
use crate::content_gateway::GatewayConfig;
use crate::sensitive_paths::SensitivePathPolicy;
use serde_json::Value;
use std::path::PathBuf;

pub(crate) struct HookPayloadSanitizer {
    gateway: ContentGateway,
    sensitive_paths: SensitivePathPolicy,
    cache: GatewayCache,
}

impl HookPayloadSanitizer {
    pub(crate) fn new(exclusion: ExclusionConfig, cwd: PathBuf) -> Option<Self> {
        if !exclusion.enabled {
            return None;
        }
        if !exclusion.secret_patterns && !exclusion.substring_matching {
            return None;
        }
        Some(Self {
            gateway: ContentGateway::new(GatewayConfig::from_exclusion(&exclusion)),
            sensitive_paths: SensitivePathPolicy::new_with_exclusion(cwd, exclusion),
            cache: GatewayCache::new(),
        })
    }

    pub(crate) fn sanitize_text(&self, text: &str) -> String {
        let epoch = self.sensitive_paths.ignore_epoch();
        let (sanitized, _) =
            self.gateway
                .scan_text(text, &self.sensitive_paths, &self.cache, epoch);
        sanitized
    }

    pub(crate) fn sanitize_value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.sanitize_text(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.sanitize_value(item)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), self.sanitize_value(value)))
                    .collect(),
            ),
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        }
    }
}
