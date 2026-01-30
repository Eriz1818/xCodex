use std::path::Path;

use async_channel::Sender;

use crate::config::Config;
use crate::hooks::UserHooks;
use crate::mcp_connection_manager::McpHookContext;
use crate::protocol::DeprecationNoticeEvent;
use crate::protocol::Event;
use crate::protocol::EventMsg;
use crate::user_notification::UserNotifier;

pub mod config;
pub mod git_info;
pub mod hooks;
pub mod themes;

pub(crate) fn maybe_push_notify_deprecation(
    submit_id: &str,
    config: &Config,
    events: &mut Vec<Event>,
) {
    if matches!(config.xcodex.notify.as_ref(), Some(notify) if !notify.is_empty()) {
        events.push(Event {
            id: submit_id.to_owned(),
            msg: EventMsg::DeprecationNotice(DeprecationNoticeEvent {
                summary: "`notify` is deprecated. Use `[hooks].agent_turn_complete` instead."
                    .to_string(),
                details: Some(
                    "See docs/xcodex/hooks.md for the hooks contract and examples.".to_string(),
                ),
            }),
        });
    }
}

pub(crate) fn build_user_hooks(config: &Config, tx_event: Sender<Event>) -> UserHooks {
    UserHooks::new(
        config.codex_home.clone(),
        config.xcodex.hooks.clone(),
        Some(tx_event),
        config.sandbox_policy.get().clone(),
        config.codex_linux_sandbox_exe.clone(),
    )
}

pub(crate) fn build_user_notifier(config: &Config) -> UserNotifier {
    UserNotifier::new(config.xcodex.notify.clone())
}

pub(crate) fn mcp_hook_context(
    user_hooks: UserHooks,
    conversation_id: &str,
    cwd: &Path,
) -> McpHookContext {
    McpHookContext::new(
        user_hooks,
        conversation_id.to_string(),
        cwd.display().to_string(),
    )
}
