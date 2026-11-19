//! Cross-platform hardware probe for CPU SIMD capabilities, GPU backends,
//! VRAM/RAM budgets, and NPU descriptors (Milestone 5.1 #1 & #3).

use serde::{Deserialize, Serialize};
use sysinfo::System;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuSimdProfile {
    pub sse4_2: bool,
    pub avx2: bool,
    pub avx512f: bool,
    pub neon: bool,
    pub logical_cores: usize,
    pub physical_cores: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuBackendKind {
    Cuda,
    Vulkan,
    Metal,
    DirectMl,
    Rocm,
    CpuOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpuVendor {
    AppleNeuralEngine,
    QualcommHexagon,
    IntelNpu,
    AmdXdna,
    RockchipRknn,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuDescriptor {
    pub name: String,
    pub backend: GpuBackendKind,
    pub vram_mb: u64,
    pub unified_memory: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpuDescriptor {
    pub name: String,
    pub vendor: NpuVendor,
    pub tops: u32,
    pub supported_formats: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareInventory {
    pub os: String,
    pub arch: String,
    pub cpu: CpuSimdProfile,
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub gpus: Vec<GpuDescriptor>,
    pub npus: Vec<NpuDescriptor>,
}

impl HardwareInventory {
    /// Preferred model artifact formats ordered from fastest hardware match to fallback.
    #[must_use]
    pub fn recommended_artifact_formats(&self) -> Vec<&'static str> {
        let mut formats = Vec::new();
        for npu in &self.npus {
            match npu.vendor {
                NpuVendor::AppleNeuralEngine => push_unique(&mut formats, "coreml_package"),
                NpuVendor::QualcommHexagon => push_unique(&mut formats, "qnn_context_binary"),
                NpuVendor::IntelNpu => push_unique(&mut formats, "openvino_ir"),
                NpuVendor::RockchipRknn => push_unique(&mut formats, "rknn"),
                NpuVendor::AmdXdna => push_unique(&mut formats, "onnx"),
                NpuVendor::None => {}
            }
        }
        if self
            .gpus
            .iter()
            .any(|g| g.backend != GpuBackendKind::CpuOnly)
        {
            push_unique(&mut formats, "gguf");
            push_unique(&mut formats, "onnx");
        } else {
            push_unique(&mut formats, "gguf");
            push_unique(&mut formats, "onnx");
            push_unique(&mut formats, "litert");
        }
        formats
    }

    #[must_use]
    pub fn max_vram_mb(&self) -> u64 {
        self.gpus.iter().map(|g| g.vram_mb).max().unwrap_or(0)
    }

    #[must_use]
    pub fn has_dedicated_accelerator(&self) -> bool {
        self.gpus
            .iter()
            .any(|g| g.backend != GpuBackendKind::CpuOnly)
            || !self.npus.is_empty()
    }
}

fn push_unique(list: &mut Vec<&'static str>, item: &'static str) {
    if !list.contains(&item) {
        list.push(item);
    }
}

#[must_use]
pub fn probe_cpu_simd() -> CpuSimdProfile {
    let mut sys = System::new();
    sys.refresh_cpu();
    let logical_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or_else(|_| sys.cpus().len().max(1));
    let physical_cores = sys.physical_core_count().unwrap_or(logical_cores).max(1);

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let (sse4_2, avx2, avx512f, neon) = (
        std::arch::is_x86_feature_detected!("sse4.2"),
        std::arch::is_x86_feature_detected!("avx2"),
        std::arch::is_x86_feature_detected!("avx512f"),
        false,
    );

    #[cfg(target_arch = "aarch64")]
    let (sse4_2, avx2, avx512f, neon) = (false, false, false, true);

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    let (sse4_2, avx2, avx512f, neon) = (false, false, false, false);

    CpuSimdProfile {
        sse4_2,
        avx2,
        avx512f,
        neon,
        logical_cores,
        physical_cores,
    }
}

/// Probe local hardware inventory safely without shelling out to untrusted binaries.
/// Accepts optional caller-detected GPU/NPU descriptors (e.g. from `wgpu` or platform FFI).
#[must_use]
pub fn probe_local_hardware(
    detected_gpus: Vec<GpuDescriptor>,
    detected_npus: Vec<NpuDescriptor>,
) -> HardwareInventory {
    let mut sys = System::new();
    sys.refresh_memory();

    let total_ram_mb = sys.total_memory() / (1024 * 1024);
    let available_ram_mb = sys.available_memory() / (1024 * 1024);

    #[allow(unused_mut)]
    let mut npus = detected_npus;
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if npus.is_empty() {
        npus.push(NpuDescriptor {
            name: "Apple Neural Engine".to_string(),
            vendor: NpuVendor::AppleNeuralEngine,
            tops: 18,
            supported_formats: vec!["coreml_package".to_string()],
        });
    }

    HardwareInventory {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu: probe_cpu_simd(),
        total_ram_mb,
        available_ram_mb,
        gpus: detected_gpus,
        npus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hardware_probe_reports_positive_cores_and_ram() {
        let inv = probe_local_hardware(vec![], vec![]);
        assert!(inv.cpu.logical_cores >= 1);
        assert!(inv.cpu.physical_cores >= 1);
        assert!(inv.total_ram_mb > 0);
        assert!(!inv.os.is_empty());
        assert!(!inv.arch.is_empty());
        assert!(inv.recommended_artifact_formats().contains(&"gguf"));
    }

    #[test]
    fn npu_and_gpu_prioritize_native_formats() {
        let inv = HardwareInventory {
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
            cpu: CpuSimdProfile {
                sse4_2: false,
                avx2: false,
                avx512f: false,
                neon: true,
                logical_cores: 10,
                physical_cores: 10,
            },
            total_ram_mb: 32768,
            available_ram_mb: 24576,
            gpus: vec![GpuDescriptor {
                name: "Apple M3 Pro".to_string(),
                backend: GpuBackendKind::Metal,
                vram_mb: 24576,
                unified_memory: true,
            }],
            npus: vec![NpuDescriptor {
                name: "Apple Neural Engine".to_string(),
                vendor: NpuVendor::AppleNeuralEngine,
                tops: 18,
                supported_formats: vec!["coreml_package".to_string()],
            }],
        };

        assert!(inv.has_dedicated_accelerator());
        assert_eq!(inv.max_vram_mb(), 24576);
        assert_eq!(inv.recommended_artifact_formats()[0], "coreml_package");
        assert!(inv.recommended_artifact_formats().contains(&"gguf"));
    }
}
