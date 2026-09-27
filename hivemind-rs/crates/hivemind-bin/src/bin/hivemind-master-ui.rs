#![cfg_attr(
    all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
    windows_subsystem = "windows"
)]

#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
use std::io::Write;

#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
#[path = "../local_ui_webview.rs"]
mod local_ui_webview;

#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
fn main() {
    if let Err(error) = local_ui_webview::run("Hivemind Master") {
        let _ = writeln!(std::io::stderr().lock(), "hivemind-master-ui: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(not(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc")))]
fn main() {
    eprintln!("hivemind-master-ui requires x86_64-pc-windows-msvc");
    std::process::exit(1);
}
