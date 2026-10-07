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
    let args: Vec<String> = std::env::args().collect();
    if let Some(done) = hivemind_bin::try_benchmark_helper(&args) {
        std::process::exit(i32::from(!done));
    }
    if let Err(error) = local_ui_webview::run("Hivemind Worker") {
        let _ = writeln!(std::io::stderr().lock(), "hivemind-worker-ui: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(not(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc")))]
fn main() {
    eprintln!("hivemind-worker-ui requires x86_64-pc-windows-msvc");
    std::process::exit(1);
}
