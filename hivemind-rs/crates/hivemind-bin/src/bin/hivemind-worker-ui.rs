#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
use std::io::Write;

#[cfg(windows)]
#[path = "../local_ui_webview.rs"]
mod local_ui_webview;

#[cfg(windows)]
fn main() {
    if let Err(error) = local_ui_webview::run("Hivemind Worker") {
        let _ = writeln!(std::io::stderr().lock(), "hivemind-worker-ui: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hivemind-worker-ui is only available on Windows");
    std::process::exit(1);
}
