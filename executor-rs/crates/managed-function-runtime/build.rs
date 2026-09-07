use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-env-changed=CUDA_HOME");
    println!("cargo:rerun-if-env-changed=CUDA_PATH");

    if env::var_os("CARGO_FEATURE_CUDA").is_none()
        || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux")
    {
        return;
    }

    let mut roots = vec![PathBuf::from("/usr/local/cuda")];
    for variable in ["CUDA_HOME", "CUDA_PATH"] {
        if let Some(value) = env::var_os(variable) {
            roots.push(PathBuf::from(value));
        }
    }

    for root in roots {
        for directory in [root.join("lib64"), root.join("lib64").join("stubs")] {
            if Path::new(&directory).is_dir() {
                println!("cargo:rustc-link-search=native={}", directory.display());
            }
        }
    }
}
