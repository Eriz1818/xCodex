use codex_tui::AppExitInfo;
use codex_tui2 as tui2;
use std::path::PathBuf;

pub async fn run_tui2(
    tui2_cli: codex_tui::Cli,
    codex_linux_sandbox_exe: Option<PathBuf>,
) -> anyhow::Result<AppExitInfo> {
    let result = tui2::run_main(tui2_cli.into(), codex_linux_sandbox_exe).await?;
    let mut exit_info: AppExitInfo = result.into();
    exit_info.token_usage = Default::default();
    exit_info.thread_id = None;
    Ok(exit_info)
}
