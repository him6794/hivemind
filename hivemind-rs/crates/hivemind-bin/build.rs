#[cfg(feature = "local-ui")]
fn main() {
    // tauri-build embeds this icon into the exe resource but does not declare it as a build input.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build();
}

#[cfg(not(feature = "local-ui"))]
fn main() {}
