// Release builds on Windows use the GUI subsystem, so starting the Master opens its window
// without a console. Their output goes to a log file instead (see main).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[cfg(all(not(debug_assertions), target_os = "windows"))]
    hivemind_common::redirect_std_streams_to_log_file("hivemind-master")?;
    hivemind_bin::run_service(hivemind_bin::ServiceRole::Master).await
}
