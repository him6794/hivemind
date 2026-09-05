use general_compute_runtime::gpu::{GpuCapability, GpuRuntime, GpuVendor};
use general_compute_runtime::gpu_tensor::{GpuTensorError, GpuTensorManifest};
use general_compute_runtime::tensor::{
    ByteOrder, TENSOR_ABI_VERSION, TensorDType, TensorLayout, TensorManifest,
};
use general_compute_runtime::{ArtifactChunk, ArtifactManifest, ArtifactRole, sha256_digest};

fn binary_artifact(bytes: &[u8]) -> ArtifactManifest {
    ArtifactManifest {
        artifact_id: "gpu-tensor-data".into(),
        role: ArtifactRole::Output,
        size_bytes: bytes.len() as u64,
        mime_type: "application/octet-stream".into(),
        sha256: sha256_digest(bytes),
        chunks: if bytes.is_empty() {
            Vec::new()
        } else {
            vec![ArtifactChunk {
                offset: 0,
                size_bytes: bytes.len() as u64,
                sha256: sha256_digest(bytes),
            }]
        },
        inline_bytes: Some(bytes.to_vec()),
    }
}

fn tensor_for(shape: Vec<u64>, bytes: &[u8]) -> TensorManifest {
    let mut tensor = TensorManifest {
        abi_version: TENSOR_ABI_VERSION.into(),
        dtype: TensorDType::Float64,
        shape,
        byte_order: ByteOrder::Little,
        layout: TensorLayout::C,
        data_artifact: binary_artifact(bytes),
        logical_sha256: String::new(),
    };
    tensor.logical_sha256 = tensor.canonical_logical_sha256();
    tensor
}

fn device(vram_bytes: u64) -> GpuCapability {
    GpuCapability::new(
        GpuVendor::Nvidia,
        "gpu-a",
        "sm_80",
        GpuRuntime::Cuda,
        "12.4",
        "550.54",
        vram_bytes,
        32,
        format!("sha256:{}", "a".to_string().repeat(64)),
    )
    .expect("valid GPU capability")
}

fn gpu_tensor(shape: Vec<u64>, bytes: &[u8], device_id: &str) -> GpuTensorManifest {
    GpuTensorManifest {
        tensor: tensor_for(shape, bytes),
        device_id: device_id.into(),
    }
}

#[test]
fn gpu_tensor_accepts_a_buffer_that_fits_the_devices_vram_budget() {
    let bytes = vec![0u8; 32];
    let manifest = gpu_tensor(vec![2, 2], &bytes, "gpu-a");

    manifest
        .validate_bytes_for_device(&bytes, &device(1024))
        .expect("a buffer within the device VRAM budget must validate");
}

#[test]
fn gpu_tensor_rejects_a_device_id_that_does_not_match_the_negotiated_capability() {
    let bytes = vec![0u8; 32];
    let manifest = gpu_tensor(vec![2, 2], &bytes, "gpu-wrong");

    let error = manifest
        .validate_for_device(&device(1024))
        .expect_err("a tensor bound to a different device must be rejected");

    assert_eq!(error, GpuTensorError::DeviceIdMismatch);
}

#[test]
fn gpu_tensor_rejects_a_buffer_that_exceeds_the_devices_vram_budget() {
    let bytes = vec![0u8; 32];
    let manifest = gpu_tensor(vec![2, 2], &bytes, "gpu-a");

    let error = manifest
        .validate_for_device(&device(16))
        .expect_err("a buffer larger than the device VRAM budget must be rejected");

    assert_eq!(
        error,
        GpuTensorError::ExceedsDeviceVram {
            size_bytes: 32,
            vram_bytes: 16,
        }
    );
}

#[test]
fn gpu_tensor_delegates_metadata_validation_to_the_wrapped_tensor() {
    let bytes = vec![0u8; 31];
    let mut manifest = gpu_tensor(vec![2, 2], &[0u8; 32], "gpu-a");
    manifest.tensor.data_artifact = binary_artifact(&bytes);

    let error = manifest
        .validate_for_device(&device(1024))
        .expect_err("a tensor whose size does not match dtype/shape must be rejected");

    assert!(matches!(error, GpuTensorError::Tensor(_)));
}

#[test]
fn gpu_tensor_validate_bytes_rejects_a_checksum_mismatch_against_materialized_bytes() {
    let declared = vec![0u8; 32];
    let manifest = gpu_tensor(vec![2, 2], &declared, "gpu-a");
    let tampered = vec![1u8; 32];

    let error = manifest
        .validate_bytes_for_device(&tampered, &device(1024))
        .expect_err("materialized bytes that differ from the declared checksum must be rejected");

    assert!(matches!(error, GpuTensorError::Tensor(_)));
}

#[test]
fn gpu_tensor_manifest_rejects_unknown_json_fields() {
    let json = r#"{
        "tensor": {
            "abi_version": "tensor-v1alpha1",
            "dtype": "float64",
            "shape": [2, 2],
            "byte_order": "little",
            "layout": "c",
            "data_artifact": {
                "artifact_id": "gpu-tensor-data",
                "role": "output",
                "size_bytes": 32,
                "mime_type": "application/octet-stream",
                "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                "chunks": []
            },
            "logical_sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        },
        "device_id": "gpu-a",
        "unexpected": true
    }"#;

    assert!(serde_json::from_str::<GpuTensorManifest>(json).is_err());
}
