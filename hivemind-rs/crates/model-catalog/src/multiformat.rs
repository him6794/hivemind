//! Multi-format hardware artifact catalog extensions (GGUF, ONNX, CoreML, QNN,
//! LiteRT, OpenVINO, RKNN) for heterogeneous CPU / GPU / NPU scheduling.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    Gguf,
    Onnx,
    CoreMlPackage,
    QnnContextBinary,
    LiteRt,
    OpenVinoIr,
    Rknn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceleratorKind {
    Cpu,
    Cuda,
    Rocm,
    Metal,
    Vulkan,
    DirectMl,
    AppleNeuralEngine,
    QualcommHexagonNpu,
    AndroidNnapi,
    IntelNpu,
    RockchipNpu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareArtifactPin {
    pub format: ArtifactFormat,
    pub target_accelerators: Vec<AcceleratorKind>,
    pub precision: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub vram_or_npu_bytes: u64,
    pub sha256: String,
    pub swarm_infohash: String,
    pub min_npu_tops: Option<u32>,
}

/// Logical model that maps one unified model ID to multiple hardware-tailored
/// format artifacts so Nodepool can dispatch each host its optimal artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiFormatModelEntry {
    pub model_id: String,
    pub display_name: String,
    pub context_length: u32,
    pub task_kind: String, // "llm", "embedding", "whisper", "tts", "vision", "onnx"
    pub artifacts: Vec<HardwareArtifactPin>,
}

impl MultiFormatModelEntry {
    /// Selects the highest-priority compatible artifact for a host's available
    /// accelerators, NPU TOPS, and memory budget. Prefers dedicated NPU/GPU
    /// formats over CPU fallback when the host supports them.
    #[must_use]
    pub fn select_best_artifact(
        &self,
        host_accelerators: &[AcceleratorKind],
        host_npu_tops: Option<u32>,
        available_ram_bytes: u64,
        available_accel_mem_bytes: u64,
    ) -> Option<&HardwareArtifactPin> {
        self.artifacts
            .iter()
            .filter(|art| {
                let accel_ok = art
                    .target_accelerators
                    .iter()
                    .any(|req| host_accelerators.contains(req));
                let tops_ok = match (art.min_npu_tops, host_npu_tops) {
                    (Some(min_t), Some(actual_t)) => actual_t >= min_t,
                    (Some(_), None) => false,
                    (None, _) => true,
                };
                let mem_ok = available_ram_bytes >= art.ram_bytes
                    && (art.vram_or_npu_bytes == 0
                        || available_accel_mem_bytes >= art.vram_or_npu_bytes);
                accel_ok && tops_ok && mem_ok
            })
            .max_by_key(|art| format_priority(art.format))
    }
}

fn format_priority(format: ArtifactFormat) -> u8 {
    match format {
        ArtifactFormat::CoreMlPackage
        | ArtifactFormat::QnnContextBinary
        | ArtifactFormat::Rknn
        | ArtifactFormat::OpenVinoIr => 30,
        ArtifactFormat::LiteRt | ArtifactFormat::Onnx => 20,
        ArtifactFormat::Gguf => 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_multiformat() -> MultiFormatModelEntry {
        MultiFormatModelEntry {
            model_id: "qwen2.5-3b-instruct".into(),
            display_name: "Qwen2.5 3B Instruct".into(),
            context_length: 8192,
            task_kind: "llm".into(),
            artifacts: vec![
                HardwareArtifactPin {
                    format: ArtifactFormat::Gguf,
                    target_accelerators: vec![
                        AcceleratorKind::Cpu,
                        AcceleratorKind::Cuda,
                        AcceleratorKind::Vulkan,
                        AcceleratorKind::Metal,
                    ],
                    precision: "Q4_K_M".into(),
                    file_name: "qwen2.5-3b-q4_k_m.gguf".into(),
                    size_bytes: 2 << 30,
                    ram_bytes: 3 << 30,
                    vram_or_npu_bytes: 0,
                    sha256: "a".repeat(64),
                    swarm_infohash: "b".repeat(40),
                    min_npu_tops: None,
                },
                HardwareArtifactPin {
                    format: ArtifactFormat::CoreMlPackage,
                    target_accelerators: vec![AcceleratorKind::AppleNeuralEngine],
                    precision: "INT4".into(),
                    file_name: "qwen2.5-3b-int4.mlpackage".into(),
                    size_bytes: 1800 << 20,
                    ram_bytes: 2500 << 20,
                    vram_or_npu_bytes: 2 << 30,
                    sha256: "c".repeat(64),
                    swarm_infohash: "d".repeat(40),
                    min_npu_tops: Some(15),
                },
                HardwareArtifactPin {
                    format: ArtifactFormat::QnnContextBinary,
                    target_accelerators: vec![AcceleratorKind::QualcommHexagonNpu],
                    precision: "INT4".into(),
                    file_name: "qwen2.5-3b-qnn.bin".into(),
                    size_bytes: 1700 << 20,
                    ram_bytes: 2400 << 20,
                    vram_or_npu_bytes: 2 << 30,
                    sha256: "e".repeat(64),
                    swarm_infohash: "f".repeat(40),
                    min_npu_tops: Some(30),
                },
                HardwareArtifactPin {
                    format: ArtifactFormat::Rknn,
                    target_accelerators: vec![AcceleratorKind::RockchipNpu],
                    precision: "W4A16".into(),
                    file_name: "qwen2.5-3b.rkllm".into(),
                    size_bytes: 1900 << 20,
                    ram_bytes: 2600 << 20,
                    vram_or_npu_bytes: 0,
                    sha256: "1".repeat(64),
                    swarm_infohash: "2".repeat(40),
                    min_npu_tops: Some(6),
                },
            ],
        }
    }

    #[test]
    fn selects_dedicated_npu_artifact_over_gguf_when_host_meets_tops_and_memory() {
        let model = sample_multiformat();

        // Apple Silicon host with Metal + ANE (38 TOPS) selects CoreML .mlpackage
        let apple = model
            .select_best_artifact(
                &[AcceleratorKind::Metal, AcceleratorKind::AppleNeuralEngine],
                Some(38),
                16 << 30,
                12 << 30,
            )
            .expect("should match");
        assert_eq!(apple.format, ArtifactFormat::CoreMlPackage);

        // Snapdragon X / Android flagship with 45 TOPS Hexagon NPU selects QNN
        let qnn = model
            .select_best_artifact(
                &[AcceleratorKind::Vulkan, AcceleratorKind::QualcommHexagonNpu],
                Some(45),
                12 << 30,
                8 << 30,
            )
            .expect("should match");
        assert_eq!(qnn.format, ArtifactFormat::QnnContextBinary);

        // Standard CPU/CUDA host falls back cleanly to GGUF
        let pc = model
            .select_best_artifact(
                &[AcceleratorKind::Cpu, AcceleratorKind::Cuda],
                None,
                16 << 30,
                8 << 30,
            )
            .expect("should match");
        assert_eq!(pc.format, ArtifactFormat::Gguf);

        // Low-TOPS Rockchip SBC (6 TOPS) matches RKNN
        let rknn = model
            .select_best_artifact(
                &[AcceleratorKind::Cpu, AcceleratorKind::RockchipNpu],
                Some(6),
                4 << 30,
                0,
            )
            .expect("should match");
        assert_eq!(rknn.format, ArtifactFormat::Rknn);
    }
}
