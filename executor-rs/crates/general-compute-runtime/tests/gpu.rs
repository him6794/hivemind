use general_compute_runtime::gpu::{
    GpuCapability, GpuFallbackReason, GpuRequirement, GpuRuntime, GpuSelection, GpuVendor,
    negotiate_gpu,
};

fn image(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn capability(device_id: &str, image_digest: String) -> GpuCapability {
    GpuCapability::new(
        GpuVendor::Nvidia,
        device_id,
        "sm_80",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        24 * 1024 * 1024 * 1024,
        32,
        image_digest,
    )
    .expect("valid GPU capability")
}

fn requirement(allow_cpu_fallback: bool) -> GpuRequirement {
    GpuRequirement::new(
        GpuVendor::Nvidia,
        "sm_80",
        GpuRuntime::Cuda,
        "550.54",
        16 * 1024 * 1024 * 1024,
        8,
        image('a'),
        allow_cpu_fallback,
    )
    .expect("valid GPU requirement")
}

#[test]
fn gpu_negotiation_selects_a_deterministic_compatible_device() {
    let first = capability("gpu-b", image('a'));
    let second = capability("gpu-a", image('a'));

    let selected = negotiate_gpu(&requirement(false), &[first, second])
        .expect("a compatible GPU should be selected");
    assert_eq!(selected, GpuSelection::Gpu(capability("gpu-a", image('a'))));
}

#[test]
fn gpu_negotiation_rejects_runtime_driver_image_and_capacity_mismatches() {
    let requirement = requirement(false);

    let mut runtime_mismatch = capability("gpu-a", image('a'));
    runtime_mismatch.runtime = GpuRuntime::Rocm;
    assert!(negotiate_gpu(&requirement, &[runtime_mismatch]).is_err());

    let mut driver_mismatch = capability("gpu-a", image('a'));
    driver_mismatch.driver_abi = "550.55".into();
    assert!(negotiate_gpu(&requirement, &[driver_mismatch]).is_err());

    let mut image_mismatch = capability("gpu-a", image('b'));
    image_mismatch.image_digest = image('b');
    assert!(negotiate_gpu(&requirement, &[image_mismatch]).is_err());

    let mut capacity_mismatch = capability("gpu-a", image('a'));
    capacity_mismatch.vram_bytes = 8 * 1024 * 1024 * 1024;
    assert!(negotiate_gpu(&requirement, &[capacity_mismatch]).is_err());
}

#[test]
fn gpu_negotiation_requires_explicit_cpu_fallback() {
    let no_fallback = negotiate_gpu(&requirement(false), &[])
        .expect_err("missing GPU must not silently fall back to CPU");
    assert_eq!(
        no_fallback.to_string(),
        "no compatible GPU capability is registered"
    );

    let fallback = negotiate_gpu(&requirement(true), &[]).expect("explicit fallback is allowed");
    assert_eq!(
        fallback,
        GpuSelection::CpuFallback {
            reason: GpuFallbackReason::NoCompatibleDevice,
        }
    );
}

#[test]
fn gpu_contract_rejects_invalid_identity_and_unknown_json_fields() {
    assert!(
        GpuCapability::new(
            GpuVendor::Nvidia,
            "gpu-a",
            "sm_80",
            GpuRuntime::Rocm,
            "12.4",
            "550.54",
            24 * 1024 * 1024 * 1024,
            32,
            image('a'),
        )
        .is_err()
    );
    assert!(
        GpuRequirement::new(
            GpuVendor::Nvidia,
            "sm_80",
            GpuRuntime::Cuda,
            "550.54",
            0,
            8,
            image('a'),
            false,
        )
        .is_err()
    );

    let json = r#"{
        "protocol_version":"general-compute-gpu-v1alpha1",
        "vendor":"nvidia",
        "device_id":"gpu-a",
        "compute_capability":"sm_80",
        "runtime":"cuda",
        "runtime_version":"12.4",
        "driver_abi":"550.54",
        "vram_bytes":25769803776,
        "max_streams":32,
        "image_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "unexpected":true
    }"#;
    assert!(serde_json::from_str::<GpuCapability>(json).is_err());
}
