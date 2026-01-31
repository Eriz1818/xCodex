#![cfg(not(target_os = "windows"))]

use codex_core::protocol::DeprecationNoticeEvent;
use codex_core::protocol::EventMsg;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notify_emits_deprecation_notice() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = core_test_support::responses::start_mock_server().await;
    let TestCodex { codex, .. } = test_codex()
        .with_config(|cfg| cfg.xcodex.notify = Some(vec!["/bin/false".to_string()]))
        .build(&server)
        .await?;

    let EventMsg::DeprecationNotice(DeprecationNoticeEvent { summary, .. }) =
        wait_for_event(&codex, |ev| matches!(ev, EventMsg::DeprecationNotice(_))).await
    else {
        unreachable!("wait_for_event filters for DeprecationNotice")
    };
    assert!(summary.contains("`notify` is deprecated"));
    Ok(())
}
