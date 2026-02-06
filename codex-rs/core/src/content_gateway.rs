use crate::config::types::ExclusionConfig;
use crate::config::types::ExclusionOnMatch;
use crate::sensitive_paths::SensitivePathDecision;
use crate::sensitive_paths::SensitivePathPolicy;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
use once_cell::sync::Lazy;
use regex::Regex;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanLayer {
    L1PathProvenance,
    L2ContentScan,
    L3FingerprintCache,
    L4FullPayloadScan,
}

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub enabled: bool,
    pub content_hashing: bool,
    pub substring_matching: bool,
    pub secret_patterns: bool,
    pub secret_patterns_builtin: bool,
    pub secret_patterns_allowlist: Vec<String>,
    pub secret_patterns_blocklist: Vec<String>,
    pub on_match: ExclusionOnMatch,
    pub log_redactions: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            content_hashing: true,
            substring_matching: true,
            secret_patterns: true,
            secret_patterns_builtin: true,
            secret_patterns_allowlist: Vec::new(),
            secret_patterns_blocklist: Vec::new(),
            on_match: ExclusionOnMatch::Redact,
            log_redactions: false,
        }
    }
}

impl GatewayConfig {
    pub fn from_exclusion(exclusion: &ExclusionConfig) -> Self {
        Self {
            enabled: exclusion.enabled,
            content_hashing: exclusion.content_hashing,
            substring_matching: exclusion.substring_matching,
            secret_patterns: exclusion.secret_patterns,
            secret_patterns_builtin: exclusion.secret_patterns_builtin,
            secret_patterns_allowlist: exclusion.secret_patterns_allowlist.clone(),
            secret_patterns_blocklist: exclusion.secret_patterns_blocklist.clone(),
            on_match: exclusion.on_match,
            log_redactions: exclusion.log_redactions_mode()
                != crate::config::types::LogRedactionsMode::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedDecision {
    Safe,
    Redacted,
    Blocked,
}

#[derive(Debug, Default)]
pub struct GatewayCache {
    state: Mutex<GatewayCacheState>,
}

#[derive(Debug, Default)]
struct GatewayCacheState {
    epoch: u64,
    decisions: HashMap<[u8; 16], CachedDecision>,
}

impl GatewayCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_or_reset_epoch(&self, epoch: u64) -> std::sync::MutexGuard<'_, GatewayCacheState> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.epoch != epoch {
            guard.epoch = epoch;
            guard.decisions.clear();
        }
        guard
    }
}

#[derive(Debug, Clone)]
pub struct ScanReport {
    pub layers: Vec<ScanLayer>,
    pub redacted: bool,
    pub blocked: bool,
    pub reasons: Vec<RedactionReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionReason {
    FingerprintCache,
    IgnoredPath,
    SecretPattern,
}

type RedactionCallback<'a> = Option<&'a mut dyn FnMut(&str, &str, &ScanReport)>;

impl ScanReport {
    pub(crate) fn safe() -> Self {
        Self {
            layers: Vec::new(),
            redacted: false,
            blocked: false,
            reasons: Vec::new(),
        }
    }
}

static RE_PATHLIKE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
            (?P<path>
                # Windows drive-letter path, e.g. C:\foo\bar or C:/foo/bar (mixed slashes allowed).
                (?:[A-Za-z]:[\\/][A-Za-z0-9._-]+(?:[\\/][A-Za-z0-9._-]+)*)
                |
                # UNC path, e.g. \\server\share\path (mixed slashes allowed).
                (?:\\\\[A-Za-z0-9._-]+[\\/][A-Za-z0-9._-]+(?:[\\/][A-Za-z0-9._-]+)*)
                |
                # Relative / repo-like path, optionally prefixed with ./, ../, .\\, ..\\ (repeatable).
                (?:\.{1,2}[\\/])*
                (?:
                    \.[A-Za-z0-9._-]+
                    |
                    [A-Za-z0-9._-]+(?:[\\/][A-Za-z0-9._-]+)+
                )
            )
        ",
    )
    .unwrap_or_else(|err| panic!("invalid path regex: {err}"))
});

static RE_SECRET_PATTERNS_BUILTIN: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        // AWS access key id.
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap_or_else(|err| panic!("aws key regex: {err}")),
        // GitHub classic token.
        Regex::new(r"\bghp_[A-Za-z0-9]{36}\b")
            .unwrap_or_else(|err| panic!("github token regex: {err}")),
        // Private keys (PEM blocks).
        Regex::new(r"-----BEGIN[ A-Z0-9_-]*PRIVATE KEY-----")
            .unwrap_or_else(|err| panic!("private key regex: {err}")),
        // Generic key-value labels.
        Regex::new(r"(?i)\b(password|secret|api[_-]?key|token)\b\s*[:=]\s*\S+")
            .unwrap_or_else(|err| panic!("generic secret kv regex: {err}")),
    ]
});

pub struct ContentGateway {
    cfg: GatewayConfig,
    secret_patterns: Vec<Regex>,
    secret_patterns_blocklist: Vec<Regex>,
}

impl ContentGateway {
    pub fn new(cfg: GatewayConfig) -> Self {
        let secret_patterns = if cfg.secret_patterns_builtin {
            RE_SECRET_PATTERNS_BUILTIN.clone()
        } else {
            Vec::new()
        };
        let secret_patterns =
            extend_secret_patterns(secret_patterns, &cfg.secret_patterns_allowlist, "allowlist");
        let secret_patterns_blocklist =
            compile_secret_patterns(&cfg.secret_patterns_blocklist, "blocklist");
        Self {
            cfg,
            secret_patterns,
            secret_patterns_blocklist,
        }
    }

    pub fn scan_text(
        &self,
        text: &str,
        sensitive_paths: &SensitivePathPolicy,
        cache: &GatewayCache,
        epoch: u64,
    ) -> (String, ScanReport) {
        if text.is_empty() {
            return (String::new(), ScanReport::safe());
        }

        if !self.cfg.enabled {
            return (text.to_string(), ScanReport::safe());
        }

        if self.cfg.content_hashing {
            let key = content_fingerprint(text);
            let guard = cache.get_or_reset_epoch(epoch);
            if let Some(decision) = guard.decisions.get(&key) {
                match decision {
                    CachedDecision::Safe => return (text.to_string(), ScanReport::safe()),
                    CachedDecision::Redacted => {
                        return (
                            "[REDACTED]".to_string(),
                            ScanReport {
                                layers: vec![ScanLayer::L3FingerprintCache],
                                redacted: true,
                                blocked: false,
                                reasons: vec![RedactionReason::FingerprintCache],
                            },
                        );
                    }
                    CachedDecision::Blocked => {
                        return (
                            "[BLOCKED]".to_string(),
                            ScanReport {
                                layers: vec![ScanLayer::L3FingerprintCache],
                                redacted: false,
                                blocked: true,
                                reasons: vec![RedactionReason::FingerprintCache],
                            },
                        );
                    }
                }
            }
        }

        let mut out = text.to_string();
        let mut report = ScanReport::safe();

        if self.cfg.substring_matching || self.cfg.secret_patterns {
            let (next, r) = self.l2_scan_and_redact(&out, sensitive_paths);
            out = next;
            report.layers.extend(r.layers);
            report.redacted |= r.redacted;
            report.blocked |= r.blocked;
            report.reasons.extend(r.reasons);
        }

        if self.cfg.content_hashing {
            let decision = if report.blocked {
                CachedDecision::Blocked
            } else if report.redacted {
                CachedDecision::Redacted
            } else {
                CachedDecision::Safe
            };
            let key = content_fingerprint(text);
            let mut guard = cache.get_or_reset_epoch(epoch);
            guard.decisions.insert(key, decision);
        }

        if (report.redacted || report.blocked) && self.cfg.log_redactions {
            tracing::warn!(
                redacted = report.redacted,
                blocked = report.blocked,
                layers = ?report.layers,
                "sensitive content gateway applied",
            );
        }

        (out, report)
    }

    fn l2_scan_and_redact(
        &self,
        text: &str,
        sensitive_paths: &SensitivePathPolicy,
    ) -> (String, ScanReport) {
        let mut report = ScanReport::safe();
        let mut out = text.to_string();

        let mut matched_any = false;
        let mut matched_path = false;
        let mut matched_secret = false;

        if self.cfg.substring_matching {
            let matches = pathlike_candidates_in_text(text);
            for candidate in matches {
                if is_candidate_ignored(&candidate, sensitive_paths) {
                    matched_any = true;
                    matched_path = true;
                    if self.cfg.on_match == ExclusionOnMatch::Redact {
                        out = out.replace(&candidate, "[IGNORED-PATH: redacted]");
                    }
                }
            }
        }

        if self.cfg.secret_patterns {
            for re in self.secret_patterns.iter() {
                let has_unblocked_match =
                    re.find_iter(&out).any(|m| !self.is_blocklisted(m.as_str()));
                if has_unblocked_match {
                    matched_any = true;
                    matched_secret = true;
                    if self.cfg.on_match == ExclusionOnMatch::Redact {
                        let replacement = |caps: &regex::Captures<'_>| {
                            let matched = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
                            if self.is_blocklisted(matched) {
                                matched.to_string()
                            } else {
                                "[REDACTED]".to_string()
                            }
                        };
                        out = re.replace_all(&out, replacement).to_string();
                    }
                }
            }
        }

        if matched_any {
            report.layers.push(ScanLayer::L2ContentScan);
            if matched_path {
                report.reasons.push(RedactionReason::IgnoredPath);
            }
            if matched_secret {
                report.reasons.push(RedactionReason::SecretPattern);
            }
            match self.cfg.on_match {
                ExclusionOnMatch::Warn => {}
                ExclusionOnMatch::Redact => report.redacted = true,
                ExclusionOnMatch::Block => {
                    out = "[BLOCKED]".to_string();
                    report.blocked = true;
                }
            }
        }

        (out, report)
    }

    pub fn scan_response_item_text_fields(
        &self,
        item: &mut ResponseItem,
        sensitive_paths: &SensitivePathPolicy,
        cache: &GatewayCache,
        epoch: u64,
        mut on_redaction: RedactionCallback<'_>,
    ) -> ScanReport {
        let mut combined = ScanReport::safe();

        let mut scan_string = |s: &mut String| {
            let original = s.as_str();
            let (new, report) = self.scan_text(original, sensitive_paths, cache, epoch);
            if (report.redacted || report.blocked)
                && let Some(callback) = on_redaction.as_deref_mut()
            {
                callback(original, &new, &report);
            }
            *s = new;
            combined.layers.extend(report.layers);
            combined.redacted |= report.redacted;
            combined.blocked |= report.blocked;
            combined.reasons.extend(report.reasons);
        };

        match item {
            ResponseItem::Message { content, .. } => {
                for c in content {
                    if let ContentItem::InputText { text } | ContentItem::OutputText { text } = c {
                        scan_string(text);
                    }
                }
            }
            ResponseItem::FunctionCallOutput { output, .. } => {
                if let Some(text) = output.text_content_mut() {
                    scan_string(text);
                }
                if let Some(items) = output.content_items_mut() {
                    for c in items {
                        if let FunctionCallOutputContentItem::InputText { text } = c {
                            scan_string(text);
                        }
                    }
                }
            }
            ResponseItem::CustomToolCallOutput { output, .. } => {
                scan_string(output);
            }
            _ => {}
        }

        combined
    }

    fn is_blocklisted(&self, candidate: &str) -> bool {
        self.secret_patterns_blocklist
            .iter()
            .any(|re| re.is_match(candidate))
    }
}

fn extend_secret_patterns(mut base: Vec<Regex>, patterns: &[String], label: &str) -> Vec<Regex> {
    let extra = compile_secret_patterns(patterns, label);
    base.extend(extra);
    base
}

fn compile_secret_patterns(patterns: &[String], label: &str) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| {
            Regex::new(pattern)
                .map_err(|err| {
                    tracing::warn!(
                        pattern = pattern.as_str(),
                        error = %err,
                        "{label} secret pattern ignored (invalid regex)",
                    );
                })
                .ok()
        })
        .collect()
}

fn content_fingerprint(text: &str) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

fn is_candidate_ignored(candidate: &str, sensitive_paths: &SensitivePathPolicy) -> bool {
    // Avoid treating protocol-like strings as file paths.
    if candidate.contains("://") {
        return false;
    }

    normalized_candidate_variants(candidate)
        .iter()
        .filter(|variant| !variant.is_empty())
        .any(|variant| {
            let relative = variant.trim_start_matches('/');
            if relative.is_empty() {
                return false;
            }
            sensitive_paths.decision_send_relative(Path::new(relative))
                == SensitivePathDecision::Deny
        })
}

pub(crate) fn pathlike_candidates_in_text(text: &str) -> Vec<String> {
    RE_PATHLIKE
        .captures_iter(text)
        .filter_map(|c| c.name("path").map(|m| m.as_str().to_string()))
        .collect()
}

pub(crate) fn normalized_candidate_variants(candidate: &str) -> Vec<String> {
    let normalized = candidate.replace('\\', "/");
    let mut out = Vec::new();

    fn push_unique(out: &mut Vec<String>, candidate: &str) {
        if candidate.is_empty() {
            return;
        }
        if !out.iter().any(|existing| existing == candidate) {
            out.push(candidate.to_string());
        }
    }

    fn add_stripped_relative(out: &mut Vec<String>, candidate: &str) {
        let mut cur = candidate;
        loop {
            if let Some(rest) = cur.strip_prefix("./") {
                cur = rest;
                continue;
            }
            if let Some(rest) = cur.strip_prefix("../") {
                cur = rest;
                continue;
            }
            break;
        }
        push_unique(out, cur);
    }

    push_unique(&mut out, &normalized);
    add_stripped_relative(&mut out, &normalized);

    // Drive letter: C:/...
    let bytes = normalized.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        add_stripped_relative(&mut out, &normalized[3..]);
    }

    // UNC: //server/share/...
    if let Some(rest) = normalized.strip_prefix("//") {
        add_stripped_relative(&mut out, rest);
        let mut parts = rest.splitn(3, '/');
        let _server = parts.next();
        let _share = parts.next();
        if let Some(after_share) = parts.next() {
            add_stripped_relative(&mut out, after_share);
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
    fn redacts_ignored_path_mentions() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        std::fs::write(tmp.path().join(".aiexclude"), "secrets/\n").expect("write ignore");

        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let gateway = ContentGateway::new(GatewayConfig::default());
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) = gateway.scan_text(
            "please open secrets/hidden.txt and summarize",
            &policy,
            &cache,
            epoch,
        );
        assert_eq!(out, "please open [IGNORED-PATH: redacted] and summarize");
        assert!(report.redacted);
        assert!(!report.blocked);
    }

    #[test]
    fn redacts_windows_drive_path_mentions() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        std::fs::write(tmp.path().join(".aiexclude"), "secrets/\n").expect("write ignore");

        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let gateway = ContentGateway::new(GatewayConfig::default());
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) = gateway.scan_text(
            r"please open C:\secrets\hidden.txt and summarize",
            &policy,
            &cache,
            epoch,
        );
        assert_eq!(out, "please open [IGNORED-PATH: redacted] and summarize");
        assert!(report.redacted);
        assert!(!report.blocked);
    }

    #[test]
    fn redacts_unc_path_mentions() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        std::fs::write(tmp.path().join(".aiexclude"), "secrets/\n").expect("write ignore");

        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let gateway = ContentGateway::new(GatewayConfig::default());
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) = gateway.scan_text(
            r"do not share \\server\share\secrets\hidden.txt",
            &policy,
            &cache,
            epoch,
        );
        assert_eq!(out, "do not share [IGNORED-PATH: redacted]");
        assert!(report.redacted);
        assert!(!report.blocked);
    }

    #[test]
    fn redacts_dotdot_backslash_relative_path_mentions() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        std::fs::write(tmp.path().join(".aiexclude"), "secrets/\n").expect("write ignore");

        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let gateway = ContentGateway::new(GatewayConfig::default());
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) =
            gateway.scan_text(r"please open ..\secrets\hidden.txt", &policy, &cache, epoch);
        assert_eq!(out, "please open [IGNORED-PATH: redacted]");
        assert!(report.redacted);
        assert!(!report.blocked);
    }

    #[test]
    fn redacts_common_secret_patterns() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let gateway = ContentGateway::new(GatewayConfig::default());
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) = gateway.scan_text(
            "token=ghp_0123456789abcdef0123456789abcdef0123",
            &policy,
            &cache,
            epoch,
        );
        assert_eq!(out, "[REDACTED]");
        assert!(report.redacted);
    }

    #[test]
    fn allowlist_secret_patterns_redacts_when_builtins_disabled() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let mut cfg = GatewayConfig::default();
        cfg.secret_patterns_builtin = false;
        cfg.secret_patterns_allowlist = vec![String::from(r"foo\d+")];
        let gateway = ContentGateway::new(cfg);
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) = gateway.scan_text("token=foo123", &policy, &cache, epoch);
        assert_eq!(out, "token=[REDACTED]");
        assert!(report.redacted);
    }

    #[test]
    fn blocklist_secret_patterns_suppresses_match() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let mut cfg = GatewayConfig::default();
        cfg.secret_patterns_builtin = false;
        cfg.secret_patterns_allowlist = vec![String::from(r"foo\d+")];
        cfg.secret_patterns_blocklist = vec![String::from(r"foo123")];
        let gateway = ContentGateway::new(cfg);
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let (out, report) = gateway.scan_text("token=foo123", &policy, &cache, epoch);
        assert_eq!(out, "token=foo123");
        assert!(!report.redacted);
    }

    #[test]
    fn fingerprint_cache_skips_rescan_for_safe_content() {
        let tmp = tempdir().expect("tempdir");
        init_repo(tmp.path());
        let policy = SensitivePathPolicy::new(tmp.path().to_path_buf());
        let gateway = ContentGateway::new(GatewayConfig::default());
        let cache = GatewayCache::new();
        let epoch = policy.ignore_epoch();

        let input = "safe content";
        let (out1, report1) = gateway.scan_text(input, &policy, &cache, epoch);
        assert_eq!(out1, input);
        assert!(!report1.redacted);

        let (out2, report2) = gateway.scan_text(input, &policy, &cache, epoch);
        assert_eq!(out2, input);
        assert!(!report2.redacted);
    }
}
