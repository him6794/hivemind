//! Pure-Rust C ABI binding layer (`hivemind-ffi`) for Android (JNI/NDK),
//! iOS/iPadOS (in-process Swift/XCFramework, zero external process fork),
//! macOS, Linux, and Windows.

use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

const VERSION_CSTR: &CStr = c"0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiHardwareProfile {
    pub os: String,
    pub arch: String,
    pub cpu_cores: usize,
    pub simd_features: Vec<String>,
    pub supported_backends: Vec<String>,
    pub in_process_ffi_only: bool,
}

#[derive(Debug)]
pub struct HivemindFfiClient {
    base_url: String,
    api_key: String,
}

#[must_use]
pub fn probe_hardware_profile() -> FfiHardwareProfile {
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let mut simd = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            simd.push("avx2".into());
        }
        if std::arch::is_x86_feature_detected!("fma") {
            simd.push("fma".into());
        }
        if std::arch::is_x86_feature_detected!("avx512f") {
            simd.push("avx512f".into());
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        simd.push("neon".into());
    }

    let mut backends = vec!["cpu".to_owned(), "wasm".to_owned()];
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        backends.push("metal".into());
        backends.push("coreml_ane".into());
    }
    #[cfg(target_os = "android")]
    {
        backends.push("vulkan".into());
        backends.push("qnn_hexagon".into());
        backends.push("litert".into());
    }
    #[cfg(target_os = "windows")]
    {
        backends.push("vulkan".into());
        backends.push("directml".into());
        backends.push("cuda".into());
    }
    #[cfg(target_os = "linux")]
    {
        backends.push("vulkan".into());
        backends.push("cuda".into());
        backends.push("rocm".into());
    }

    FfiHardwareProfile {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        cpu_cores,
        simd_features: simd,
        supported_backends: backends,
        in_process_ffi_only: cfg!(target_os = "ios"),
    }
}

fn into_c_string(s: String) -> *mut c_char {
    CString::new(s).map_or(std::ptr::null_mut(), CString::into_raw)
}

#[no_mangle]
pub extern "C" fn hivemind_ffi_version() -> *const c_char {
    VERSION_CSTR.as_ptr()
}

#[no_mangle]
pub extern "C" fn hivemind_ffi_probe_hardware_json() -> *mut c_char {
    let profile = probe_hardware_profile();
    match serde_json::to_string(&profile) {
        Ok(json) => into_c_string(json),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `base_url` and `api_key` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn hivemind_ffi_create_client(
    base_url: *const c_char,
    api_key: *const c_char,
) -> *mut HivemindFfiClient {
    if base_url.is_null() || api_key.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(base) = CStr::from_ptr(base_url).to_str() else {
        return std::ptr::null_mut();
    };
    let Ok(key) = CStr::from_ptr(api_key).to_str() else {
        return std::ptr::null_mut();
    };
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() || key.trim().is_empty() {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(HivemindFfiClient {
        base_url: base.to_owned(),
        api_key: key.trim().to_owned(),
    }))
}

/// # Safety
/// `client` must be null or a pointer returned by `hivemind_ffi_create_client`.
#[no_mangle]
pub unsafe extern "C" fn hivemind_ffi_destroy_client(client: *mut HivemindFfiClient) {
    if !client.is_null() {
        drop(Box::from_raw(client));
    }
}

/// # Safety
/// `client`, `model_id`, and `prompt` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn hivemind_ffi_build_chat_envelope_json(
    client: *const HivemindFfiClient,
    model_id: *const c_char,
    prompt: *const c_char,
    max_tokens: u32,
    stream: bool,
    own_devices_only: bool,
) -> *mut c_char {
    if client.is_null() || model_id.is_null() || prompt.is_null() {
        return std::ptr::null_mut();
    }
    let client = &*client;
    let Ok(model) = CStr::from_ptr(model_id).to_str() else {
        return std::ptr::null_mut();
    };
    let Ok(prompt_str) = CStr::from_ptr(prompt).to_str() else {
        return std::ptr::null_mut();
    };
    let envelope = serde_json::json!({
        "url": format!("{}/v1/chat/completions", client.base_url),
        "headers": {
            "Authorization": format!("Bearer {}", client.api_key),
            "Content-Type": "application/json",
            "X-Hivemind-Own-Devices-Only": if own_devices_only { "true" } else { "false" },
            "X-Hivemind-Trust-Tier": if own_devices_only { "tier0_private" } else { "tier1_public" }
        },
        "body": {
            "model": model,
            "stream": stream,
            "max_tokens": max_tokens.max(1),
            "messages": [
                { "role": "user", "content": prompt_str }
            ]
        }
    });
    match serde_json::to_string(&envelope) {
        Ok(json) => into_c_string(json),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn hivemind_ffi_mobile_guard_allows_sharing(
    is_charging: bool,
    is_wifi: bool,
    battery_percent: u8,
    min_battery_percent: u8,
    temperature_celsius: f32,
    max_temperature_celsius: f32,
) -> i32 {
    let allowed = is_charging
        && is_wifi
        && battery_percent >= min_battery_percent
        && temperature_celsius.is_finite()
        && temperature_celsius <= max_temperature_celsius;
    i32::from(allowed)
}

/// # Safety
/// `ptr` must be null or a pointer returned by a `hivemind_ffi_*` function returning `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn hivemind_ffi_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_abi_lifecycle_probe_and_mobile_guard_work() {
        unsafe {
            let ver = CStr::from_ptr(hivemind_ffi_version()).to_str().unwrap();
            assert_eq!(ver, "0.1.0");

            let hw_ptr = hivemind_ffi_probe_hardware_json();
            assert!(!hw_ptr.is_null());
            let hw_json = CStr::from_ptr(hw_ptr).to_str().unwrap();
            let hw: FfiHardwareProfile = serde_json::from_str(hw_json).unwrap();
            assert!(hw.cpu_cores >= 1);
            assert!(hw.supported_backends.contains(&"cpu".to_owned()));
            hivemind_ffi_free_string(hw_ptr);

            let client = hivemind_ffi_create_client(
                c"http://127.0.0.1:8080/".as_ptr(),
                c"hmk_test_secret".as_ptr(),
            );
            assert!(!client.is_null());

            let env_ptr = hivemind_ffi_build_chat_envelope_json(
                client,
                c"qwen2.5-7b-q4".as_ptr(),
                c"Hello Hivemind".as_ptr(),
                128,
                true,
                true,
            );
            assert!(!env_ptr.is_null());
            let env_val: serde_json::Value =
                serde_json::from_str(CStr::from_ptr(env_ptr).to_str().unwrap()).unwrap();
            assert_eq!(env_val["url"], "http://127.0.0.1:8080/v1/chat/completions");
            assert_eq!(env_val["headers"]["X-Hivemind-Trust-Tier"], "tier0_private");
            assert_eq!(env_val["body"]["stream"], true);
            hivemind_ffi_free_string(env_ptr);
            hivemind_ffi_destroy_client(client);
        }

        assert_eq!(
            hivemind_ffi_mobile_guard_allows_sharing(true, true, 85, 50, 36.5, 42.0),
            1
        );
        // Unplugged, cellular, low battery, or overheating all deny sharing
        assert_eq!(
            hivemind_ffi_mobile_guard_allows_sharing(false, true, 85, 50, 36.5, 42.0),
            0
        );
        assert_eq!(
            hivemind_ffi_mobile_guard_allows_sharing(true, false, 85, 50, 36.5, 42.0),
            0
        );
        assert_eq!(
            hivemind_ffi_mobile_guard_allows_sharing(true, true, 40, 50, 36.5, 42.0),
            0
        );
        assert_eq!(
            hivemind_ffi_mobile_guard_allows_sharing(true, true, 85, 50, 45.0, 42.0),
            0
        );
    }
}
