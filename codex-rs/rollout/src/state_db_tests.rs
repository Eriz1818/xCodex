#![allow(warnings, clippy::all)]

use super::*;
use crate::list::parse_cursor;
use chrono::DateTime;
use chrono::NaiveDateTime;
use chrono::Timelike;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TurnContextItem;
use codex_state::ThreadMetadataBuilder;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::fs;
use uuid::Uuid;

#[test]
fn cursor_to_anchor_normalizes_timestamp_format() {
    let uuid = Uuid::new_v4();
    let ts_str = "2026-01-27T12-34-56";
    let token = format!("{ts_str}|{uuid}");
    let cursor = parse_cursor(token.as_str()).expect("cursor should parse");
    let anchor = cursor_to_anchor(Some(&cursor)).expect("anchor should parse");

    let naive =
        NaiveDateTime::parse_from_str(ts_str, "%Y-%m-%dT%H-%M-%S").expect("ts should parse");
    let expected_ts = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
        .with_nanosecond(0)
        .expect("nanosecond");

    assert_eq!(anchor.id, uuid);
    assert_eq!(anchor.ts, expected_ts);
}

#[tokio::test]
async fn apply_rollout_items_turn_context_refreshes_git_metadata() {
    let temp = tempdir().expect("tempdir");
    let codex_home = temp.path().to_path_buf();
    let runtime = codex_state::StateRuntime::init(codex_home.clone(), "test-provider".into())
        .await
        .expect("state db init");

    let thread_id = ThreadId::new();
    let rollout_path = codex_home.join("sessions/2026/02/15/rollout-test.jsonl");
    fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
        .await
        .expect("create sessions dir");
    fs::write(&rollout_path, "")
        .await
        .expect("create rollout file");

    let created_at = chrono::Utc::now()
        .with_nanosecond(0)
        .expect("truncate nanos");
    let mut seeded_builder = ThreadMetadataBuilder::new(
        thread_id,
        rollout_path.clone(),
        created_at,
        SessionSource::Cli,
    );
    seeded_builder.model_provider = Some("test-provider".to_string());
    seeded_builder.cwd = codex_home.join("seed-cwd");
    seeded_builder.git_sha = Some("deadbeef".to_string());
    seeded_builder.git_branch = Some("stale-branch".to_string());
    seeded_builder.git_origin_url = Some("https://example.invalid/repo.git".to_string());
    runtime
        .upsert_thread(&seeded_builder.build("test-provider"))
        .await
        .expect("seed thread");

    let new_cwd = codex_home.join("not-a-git-repo");
    fs::create_dir_all(&new_cwd).await.expect("create new cwd");

    let mut builder = ThreadMetadataBuilder::new(
        thread_id,
        rollout_path.clone(),
        created_at,
        SessionSource::Cli,
    );
    builder.model_provider = Some("test-provider".to_string());
    builder.cwd = new_cwd.clone();

    let items = vec![RolloutItem::TurnContext(TurnContextItem {
        turn_id: Some("turn-1".to_string()),
        cwd: new_cwd.clone(),
        approval_policy: AskForApproval::OnRequest,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        network: None,
        model: "gpt-5".to_string(),
        personality: None,
        collaboration_mode: None,
        effort: None,
        summary: ReasoningSummary::Auto,
        user_instructions: None,
        developer_instructions: None,
        final_output_json_schema: None,
        truncation_policy: None,
    })];

    apply_rollout_items(
        Some(runtime.as_ref()),
        &rollout_path,
        "test-provider",
        Some(&builder),
        &items,
        "test_apply_rollout_items_turn_context_refreshes_git_metadata",
        /*new_thread_memory_mode*/ None,
        /*updated_at_override*/ None,
    )
    .await;

    let updated = runtime
        .get_thread(thread_id)
        .await
        .expect("get thread")
        .expect("thread should exist");
    assert_eq!(
        normalize_for_path_comparison(&updated.cwd).expect("normalized updated cwd"),
        normalize_for_path_comparison(&new_cwd).expect("normalized expected cwd")
    );
    assert_eq!(updated.git_sha, None);
    assert_eq!(updated.git_branch, None);
    assert_eq!(updated.git_origin_url, None);
}
