use std::env;
use std::path::{Path, PathBuf};

const GNU_TARGET: &str = "x86_64-pc-windows-gnu";
const MSVC_TARGET: &str = "x86_64-pc-windows-msvc";
const ARM64_MSVC_TARGET: &str = "aarch64-pc-windows-msvc";

fn artifact_dir(root: &Path, target: &str) -> Result<PathBuf, String> {
    let relative = match target {
        GNU_TARGET => "vendor/libtailscale/windows-x86_64",
        MSVC_TARGET => "vendor/libtailscale/windows-x86_64-msvc",
        ARM64_MSVC_TARGET => "vendor/libtailscale/windows-aarch64-msvc",
        _ if target.contains("windows") => {
            return Err(format!(
                "unsupported Windows target '{target}'; supported targets are {GNU_TARGET}, {MSVC_TARGET}, and {ARM64_MSVC_TARGET}"
            ));
        }
        _ => return Err(format!("target '{target}' is not a Windows target")),
    };
    Ok(root.join(relative))
}

fn validate_artifact_dir(dir: &Path, target: &str) -> Result<(), String> {
    let artifact = if target == MSVC_TARGET || target == ARM64_MSVC_TARGET {
        dir.join("libtailscale.dll")
    } else {
        dir.join("libtailscale.a")
    };
    let header = dir.join("tailscale.h");
    if !artifact.is_file() {
        return Err(format!(
            "missing libtailscale native artifact for {target}: {}\nPrepare the ABI-specific Windows artifact before building",
            artifact.display()
        ));
    }
    if !header.is_file() {
        return Err(format!(
            "missing libtailscale header for {target}: {}\nPrepare the ABI-specific Windows artifact before building",
            header.display()
        ));
    }
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TARGET");
    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let root = manifest
            .join("../../..")
            .canonicalize()
            .expect("client-runtime repository root must exist");
        let target = env::var("TARGET").expect("Cargo must provide TARGET to build scripts");
        let lib_dir = artifact_dir(&root, &target).unwrap_or_else(|error| panic!("{error}"));
        validate_artifact_dir(&lib_dir, &target).unwrap_or_else(|error| panic!("{error}"));
        let artifact = if target == GNU_TARGET {
            lib_dir.join("libtailscale.a")
        } else {
            lib_dir.join("libtailscale.dll")
        };
        println!("cargo:rerun-if-changed={}", artifact.display());
        println!(
            "cargo:rerun-if-changed={}",
            lib_dir.join("tailscale.h").display()
        );
        if target == GNU_TARGET {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            println!("cargo:rustc-link-lib=static=tailscale");
            println!("cargo:rustc-link-lib=dylib=ws2_32");
            println!("cargo:rustc-link-lib=dylib=advapi32");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{artifact_dir, validate_artifact_dir, ARM64_MSVC_TARGET, GNU_TARGET, MSVC_TARGET};
    use std::fs;

    #[test]
    fn selects_abi_specific_artifact_directories() {
        let root = std::path::Path::new("repo");
        assert_eq!(
            artifact_dir(root, GNU_TARGET).unwrap(),
            root.join("vendor/libtailscale/windows-x86_64")
        );
        assert_eq!(
            artifact_dir(root, MSVC_TARGET).unwrap(),
            root.join("vendor/libtailscale/windows-x86_64-msvc")
        );
        assert_eq!(
            artifact_dir(root, ARM64_MSVC_TARGET).unwrap(),
            root.join("vendor/libtailscale/windows-aarch64-msvc")
        );
    }

    #[test]
    fn rejects_unsupported_windows_targets() {
        let error =
            artifact_dir(std::path::Path::new("repo"), "aarch64-pc-windows-gnu").unwrap_err();
        assert!(error.contains("unsupported Windows target"));
    }

    #[test]
    fn requires_the_archive_and_header_for_the_selected_abi() {
        let temp = std::env::temp_dir().join(format!(
            "hivemind-client-runtime-build-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        let artifact = temp.join("libtailscale.dll");
        let header = temp.join("tailscale.h");
        assert!(validate_artifact_dir(&temp, MSVC_TARGET).is_err());
        fs::write(&artifact, b"artifact").unwrap();
        assert!(validate_artifact_dir(&temp, MSVC_TARGET).is_err());
        fs::write(&header, b"header").unwrap();
        assert!(validate_artifact_dir(&temp, MSVC_TARGET).is_ok());
        fs::remove_dir_all(temp).unwrap();
    }
}
