#![cfg_attr(debug_assertions, allow(dead_code))]

use crate::version::CODEX_CLI_VERSION;
use codex_core::config::Config;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct WhatsNewInfo {
    pub(crate) version: String,
    pub(crate) bullets: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WhatsNewState {
    last_seen_version: String,
}

const WHATS_NEW_STATE_FILENAME: &str = "whats_new.json";

pub(crate) fn get_whats_new_on_startup(config: &Config) -> Option<WhatsNewInfo> {
    if !codex_core::config::is_xcodex_invocation() {
        return None;
    }

    let version = CODEX_CLI_VERSION;
    let bullets = codex_common::whats_new::whats_new_bullets_for_version(version)?;

    let state_path = state_filepath(config);
    if read_last_seen_version(&state_path).as_deref() == Some(version) {
        return None;
    }

    persist_last_seen_version(&state_path, version);
    Some(WhatsNewInfo {
        version: version.to_string(),
        bullets,
    })
}

fn state_filepath(config: &Config) -> PathBuf {
    config.codex_home.join(WHATS_NEW_STATE_FILENAME)
}

fn read_last_seen_version(path: &PathBuf) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let state: WhatsNewState = serde_json::from_str(&contents).ok()?;
    Some(state.last_seen_version)
}

fn persist_last_seen_version(path: &PathBuf, version: &str) {
    let state = WhatsNewState {
        last_seen_version: version.to_string(),
    };
    let json = match serde_json::to_string_pretty(&state) {
        Ok(json) => json,
        Err(err) => {
            tracing::debug!("Failed to serialize whats-new state: {err}");
            return;
        }
    };

    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        tracing::debug!("Failed to create whats-new state directory: {err}");
        return;
    }

    if let Err(err) = std::fs::write(path, format!("{json}\n")) {
        tracing::debug!("Failed to persist whats-new state: {err}");
    }
}
