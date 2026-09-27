#[cfg(feature = "local-ui")]
fn main() {
    tauri_build::build();
}

#[cfg(not(feature = "local-ui"))]
fn main() {}
