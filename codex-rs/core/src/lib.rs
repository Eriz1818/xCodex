//! Root of the `codex-core` library.

// Prevent accidental direct writes to stdout/stderr in library code. All
// user-visible output must go through the appropriate abstraction (e.g.,
// the TUI or the tracing stack).
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod apply_patch;
mod apps;
mod arc_monitor;
pub mod auth;
pub mod build_info;
mod client;
mod client_common;
pub(crate) mod codex;
mod realtime_context;
mod realtime_conversation;
mod realtime_prompt;
pub use codex::SteerInputError;
mod codex_thread;
mod compact_remote;
pub use codex_thread::CodexThread;
pub use codex_thread::ThreadConfigSnapshot;
mod agent;
mod codex_delegate;
mod command_canonicalization;
mod commit_attribution;
pub mod config;
pub mod config_loader;
pub mod connectors;
pub mod content_gateway;
mod context_manager;
mod contextual_user_message;
mod environment_context;
pub mod error;
mod exclusion_counters;
mod exclusion_log;
mod exclusion_preflight;
pub mod exec;
pub mod exec_env;
mod exec_policy;
pub mod external_agent_config;
pub mod file_watcher;
mod flags;
pub mod git_info;
#[cfg(test)]
mod git_info_tests;
mod guardian;
mod hook_runtime;
pub mod hooks_test;
mod installation_id;
pub mod instructions;
pub mod landlock;
pub use landlock::spawn_command_under_linux_sandbox;
pub mod mcp;
mod mcp_connection_manager {
    pub(crate) use codex_mcp::ToolInfo;

    use crate::xcodex::hooks::UserHooks;

    pub(crate) struct McpHookContext {
        pub(crate) user_hooks: UserHooks,
        pub(crate) conversation_id: String,
        pub(crate) cwd: String,
    }

    impl McpHookContext {
        pub(crate) fn new(user_hooks: UserHooks, conversation_id: String, cwd: String) -> Self {
            Self {
                user_hooks,
                conversation_id,
                cwd,
            }
        }
    }
}
mod mcp_skill_dependencies;
mod mcp_tool_approval_templates;
mod mcp_tool_exposure;
mod network_policy_decision;
pub mod network_proxy_loader;
pub use mcp::McpManager;
pub use network_proxy_loader::MtimeConfigReloader;
pub use network_proxy_loader::build_network_proxy_state;
pub use network_proxy_loader::build_network_proxy_state_and_reloader;
mod original_image_detail;
pub use codex_mcp::MCP_SANDBOX_STATE_CAPABILITY;
pub use codex_mcp::MCP_SANDBOX_STATE_METHOD;
pub use codex_mcp::SandboxState;
mod mcp_openai_file;
mod mcp_tool_call;
mod memories;
pub(crate) mod mention_syntax;
pub(crate) mod message_history;
pub(crate) mod utils;
pub mod xcodex;
pub use mention_syntax::PLUGIN_TEXT_MENTION_SIGIL;
pub use mention_syntax::TOOL_MENTION_SIGIL;
pub use message_history::HistoryEntry as MessageHistoryEntry;
pub use message_history::append_entry as append_message_history_entry;
pub use message_history::history_metadata as message_history_metadata;
pub use message_history::lookup as lookup_message_history_entry;
pub use utils::path_utils;
pub mod personality_migration;
pub mod plan_file;
pub mod plugins;
#[doc(hidden)]
pub(crate) mod prompt_debug;
#[doc(hidden)]
pub use prompt_debug::build_prompt_input;
pub(crate) mod mentions {
    pub(crate) use crate::plugins::build_connector_slug_counts;
    pub(crate) use crate::plugins::build_skill_name_counts;
    pub(crate) use crate::plugins::collect_explicit_app_ids;
    pub(crate) use crate::plugins::collect_explicit_plugin_mentions;
    pub(crate) use crate::plugins::collect_tool_mentions_from_messages;
}
mod sandbox_tags;
pub mod sandboxing;
pub(crate) mod sensitive_paths;
mod session_prefix;
mod session_startup_prewarm;
mod shell_detect;
pub mod skills;
pub(crate) use skills::SkillError;
pub(crate) use skills::SkillInjections;
pub(crate) use skills::SkillLoadOutcome;
pub(crate) use skills::SkillMetadata;
pub(crate) use skills::SkillsLoadInput;
pub(crate) use skills::SkillsManager;
pub(crate) use skills::build_skill_injections;
pub(crate) use skills::build_skill_name_counts;
pub(crate) use skills::collect_env_var_dependencies;
pub(crate) use skills::collect_explicit_skill_mentions;
pub(crate) use skills::config_rules;
pub(crate) use skills::injection;
pub(crate) use skills::loader;
pub(crate) use skills::manager;
pub(crate) use skills::maybe_emit_implicit_skill_invocation;
pub(crate) use skills::render_skills_section;
pub(crate) use skills::resolve_skill_dependencies_for_turn;
pub(crate) use skills::skills_load_input_from_config;
mod skills_watcher;
mod stream_events_utils;
pub mod test_support;
pub mod themes;
pub mod token_data {
    pub use codex_protocol::auth::KnownPlan;
    pub use codex_protocol::auth::PlanType;
}
mod truncate {
    pub(crate) use codex_utils_output_truncation::TruncationPolicy;
    pub(crate) use codex_utils_output_truncation::approx_token_count;
    pub(crate) use codex_utils_output_truncation::truncate_text;
}
mod unified_exec;
pub mod windows_sandbox;
pub use client::X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER;
pub use codex_protocol::config_types::ModelProviderAuthInfo;
mod event_mapping;
pub mod review_format;
pub mod review_prompts;
mod thread_manager;
pub(crate) mod web_search;
pub(crate) mod windows_sandbox_read_grants;
pub use thread_manager::ForkSnapshot;
pub use thread_manager::NewThread;
pub use thread_manager::ThreadManager;
pub use web_search::web_search_action_detail;
pub use web_search::web_search_detail;
pub use windows_sandbox_read_grants::grant_read_root_non_elevated;
#[deprecated(note = "use ThreadManager")]
pub type ConversationManager = ThreadManager;
#[deprecated(note = "use NewThread")]
pub type NewConversation = NewThread;
#[deprecated(note = "use CodexThread")]
pub type CodexConversation = CodexThread;
pub(crate) mod project_doc;
pub use project_doc::DEFAULT_PROJECT_DOC_FILENAME;
pub use project_doc::LOCAL_PROJECT_DOC_FILENAME;
pub use project_doc::discover_project_doc_paths;
pub use project_doc::read_project_docs;
mod rollout;
pub(crate) mod safety;
pub mod seatbelt;
mod session_rollout_init_error;
pub mod shell;
pub(crate) mod shell_snapshot;
pub mod spawn;
pub(crate) mod state_db_bridge;
pub use state_db_bridge::StateDbHandle;
pub use state_db_bridge::get_state_db;
mod thread_rollout_truncation;
mod tools;
pub(crate) mod turn_diff_tracker;
mod turn_metadata;
mod turn_timing;
mod user_notification;
pub use rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use rollout::Cursor;
pub use rollout::EventPersistenceMode;
pub use rollout::INTERACTIVE_SESSION_SOURCES;
pub use rollout::RolloutRecorder;
pub use rollout::RolloutRecorderParams;
pub use rollout::SESSIONS_SUBDIR;
pub use rollout::SessionMeta;
pub use rollout::ThreadItem;
pub use rollout::ThreadSortKey;
pub use rollout::ThreadsPage;
pub use rollout::append_thread_name;
pub use rollout::find_archived_thread_path_by_id_str;
#[deprecated(note = "use find_thread_path_by_id_str")]
pub use rollout::find_conversation_path_by_id_str;
pub use rollout::find_thread_meta_by_name_str;
pub use rollout::find_thread_name_by_id;
pub use rollout::find_thread_names_by_ids;
pub use rollout::find_thread_path_by_id_str;
pub use rollout::parse_cursor;
pub use rollout::read_head_for_summary;
pub use rollout::read_session_meta_line;
pub use rollout::rollout_date_parts;
mod function_tool;
pub mod prefs;
mod state;
mod tasks;
mod user_shell_command;
pub mod util;
pub use codex_shell_command::bash;
pub use codex_shell_command::is_dangerous_command;
pub use codex_shell_command::is_safe_command;
pub use codex_shell_command::parse_command;
pub use codex_shell_command::powershell;

pub use codex_apply_patch::CODEX_CORE_APPLY_PATCH_ARG1 as CODEX_APPLY_PATCH_ARG1;
pub use codex_sandboxing::get_platform_sandbox;
pub use codex_tools::parse_tool_input_schema;
#[cfg(target_os = "windows")]
pub use safety::set_windows_elevated_sandbox_enabled;
#[cfg(target_os = "windows")]
pub use safety::set_windows_sandbox_enabled;
// Re-export the protocol types from the standalone `codex-protocol` crate so existing
// `codex_core::protocol::...` references continue to work across the workspace.
pub use codex_protocol::protocol;
// Re-export protocol config enums to ensure call sites can use the same types
// as those in the protocol crate when constructing protocol messages.
pub use codex_protocol::config_types as protocol_config_types;

pub use client::ModelClient;
pub use client::ModelClientSession;
pub use client::X_CODEX_INSTALLATION_ID_HEADER;
pub use client::X_CODEX_TURN_METADATA_HEADER;
pub use client_common::Prompt;
pub use client_common::REVIEW_PROMPT;
pub use client_common::ResponseEvent;
pub use client_common::ResponseStream;
pub use compact::content_items_to_text;
pub use event_mapping::parse_turn_item;
pub use exec_policy::ExecPolicyError;
pub use exec_policy::check_execpolicy_for_warnings;
pub use exec_policy::format_exec_policy_error_with_source;
pub use exec_policy::load_exec_policy;
pub use file_watcher::FileWatcherEvent;
pub use turn_metadata::build_turn_metadata_header;
pub mod compact;
pub(crate) mod memory_trace;
pub use memory_trace::BuiltMemory;
pub use memory_trace::build_memories_from_trace_files;
pub mod otel_init;
