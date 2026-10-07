use super::workload::{self, GPU_DIMENSIONS};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use hivemind_models::observations::*;
use std::time::{Duration, Instant};
use wgpu::{
    Backends, BindGroupDescriptor, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
    ComputePipelineDescriptor, DeviceDescriptor, Instance, InstanceDescriptor, InstanceFlags,
    MapMode, PollType, ShaderSource,
};

const WORKGROUP: u32 = 16;
const TARGET_NANOS: u64 = 200_000_000;

fn instance(backends: Backends) -> Instance {
    Instance::new(InstanceDescriptor {
        backends,
        flags: InstanceFlags::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    })
}

/// List the supported hardware adapters visible on the active native backends.
/// Software renderers and non-GPU adapter classes are rejected; at most four
/// devices are returned, deduplicated by PCI identity where present.
pub fn inventory() -> Result<Vec<BenchmarkGpuIdentity>> {
    let backends = Backends::DX12 | Backends::VULKAN;
    let instance = instance(backends);
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let mut result = Vec::new();
    for adapter in adapters {
        let info = adapter.get_info();
        let (device_type, backend) = match info.device_type {
            wgpu::DeviceType::DiscreteGpu => ("discrete-gpu", backend_name(info.backend)?),
            wgpu::DeviceType::IntegratedGpu => ("integrated-gpu", backend_name(info.backend)?),
            _ => continue,
        };
        let identity = BenchmarkGpuIdentity {
            backend: backend.to_owned(),
            vendor: info.vendor,
            device: info.device,
            name: bounded(&info.name),
            driver: bounded(&info.driver),
            driver_info: bounded(&info.driver_info),
            device_type: device_type.to_owned(),
            pci_bus_id: bounded_pci(&info.device_pci_bus_id),
        };
        if identity.validate().is_err() {
            continue;
        }
        if result
            .iter()
            .any(|existing| same_device(existing, &identity))
        {
            continue;
        }
        result.push(identity);
        if result.len() == 4 {
            break;
        }
    }
    Ok(result)
}

fn backend_name(backend: wgpu::Backend) -> Result<&'static str> {
    match backend {
        wgpu::Backend::Dx12 => Ok("dx12"),
        wgpu::Backend::Vulkan => Ok("vulkan"),
        _ => bail!("unsupported GPU backend"),
    }
}

/// Run the fixed FP32 matmul suite for one specific, previously enumerated
/// device. The shader and pipeline are compiled once (untimed) and reused for
/// both workload dimensions.
pub fn run(identity: &BenchmarkGpuIdentity) -> Result<GpuBenchmark> {
    let backends = match identity.backend.as_str() {
        "dx12" => Backends::DX12,
        "vulkan" => Backends::VULKAN,
        _ => bail!("unsupported GPU backend"),
    };
    let instance = instance(backends);
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let adapter = adapters
        .into_iter()
        .find(|candidate| candidate_matches(&candidate.get_info(), identity));
    let Some(adapter) = adapter else {
        bail!("requested GPU device is no longer present");
    };

    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
        label: Some("hivemind-gpu-benchmark"),
        ..Default::default()
    }))
    .context("request GPU device")?;

    let wgsl = include_str!("matmul.wgsl");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("hivemind-gpu-benchmark-shader"),
        source: ShaderSource::Wgsl(wgsl.into()),
    });
    // `layout: None` lets the backend derive the layout from the shader's
    // declared bindings; the derived layout is then bound explicitly.
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some("hivemind-gpu-benchmark-pipeline"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let layout = pipeline.get_bind_group_layout(0);

    let mut cases = Vec::with_capacity(GPU_DIMENSIONS.len());
    for dimension in GPU_DIMENSIONS {
        cases.push(run_case_gpu(
            &device, &queue, &pipeline, &layout, dimension,
        )?);
    }

    let measurement = BenchmarkMeasurement {
        measurement_id: Some(uuid::Uuid::new_v4()),
        state: BenchmarkState::Ready,
        measured_at: Some(Utc::now()),
        identity: format!(
            "backend={};vendor={:#x};device={:#x};name={}",
            identity.backend, identity.vendor, identity.device, identity.name
        ),
        threads: 1,
        correctness_passed: true,
        cases,
        reason: None,
    };
    Ok(GpuBenchmark {
        device: identity.clone(),
        measurement,
    })
}

fn run_case_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    layout: &wgpu::BindGroupLayout,
    dimension: u32,
) -> Result<BenchmarkCase> {
    let element_bytes = u64::from(dimension) * u64::from(dimension) * 4;
    let (left_values, right_values) = workload::matrix_values::<f32>(dimension)?;

    let left = device.create_buffer(&buffer_desc(
        "gpu-bench-lhs",
        element_bytes,
        BufferUsages::STORAGE,
        true,
    ));
    fill_buffer(device, &left, &f32_to_bytes(&left_values))?;
    let right = device.create_buffer(&buffer_desc(
        "gpu-bench-rhs",
        element_bytes,
        BufferUsages::STORAGE,
        true,
    ));
    fill_buffer(device, &right, &f32_to_bytes(&right_values))?;
    let dim_uniform = device.create_buffer(&buffer_desc(
        "gpu-bench-dim",
        4,
        BufferUsages::STORAGE,
        true,
    ));
    fill_buffer(device, &dim_uniform, &dimension.to_le_bytes())?;
    let output = device.create_buffer(&buffer_desc(
        "gpu-bench-out",
        element_bytes,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        false,
    ));
    let staging = device.create_buffer(&buffer_desc(
        "gpu-bench-staging",
        element_bytes,
        BufferUsages::COPY_DST,
        false,
    ));
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("gpu-bench-bind"),
        layout,
        entries: &[
            buffer_entry(0, &left),
            buffer_entry(1, &right),
            buffer_entry(2, &output),
            buffer_entry(3, &dim_uniform),
        ],
    });

    // Verify the kernel before any timing: one dispatch must reproduce the
    // analytical product.
    let buffers = (&output, &staging);
    measure_batch(device, queue, pipeline, &bind_group, buffers, dimension, 1)?;
    let repetitions = calibrate_gpu(device, queue, pipeline, &bind_group, buffers, dimension)?;

    // Warm up once at the calibrated count, then record five measured batches.
    let _ = measure_batch(
        device,
        queue,
        pipeline,
        &bind_group,
        buffers,
        dimension,
        repetitions,
    )?;
    let mut batches = Vec::with_capacity(BENCHMARK_BATCHES);
    for _ in 0..BENCHMARK_BATCHES {
        let elapsed_ns = measure_batch(
            device,
            queue,
            pipeline,
            &bind_group,
            buffers,
            dimension,
            repetitions,
        )?;
        batches.push(BenchmarkBatch {
            repetitions,
            elapsed_ns,
        });
    }

    Ok(BenchmarkCase {
        name: format!("gpu-f32-{dimension}"),
        dimension,
        threads: 1,
        batches,
    })
}

fn buffer_desc(
    label: &'static str,
    size: u64,
    usage: BufferUsages,
    mapped: bool,
) -> BufferDescriptor<'static> {
    BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: mapped,
    }
}

fn buffer_entry<'a>(binding: u32, buffer: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: None,
        }),
    }
}

fn fill_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer, data: &[u8]) -> Result<()> {
    let size = u64::try_from(data.len()).unwrap_or(u64::MAX);
    let mapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_flag = std::sync::Arc::clone(&mapped);
    buffer.map_async(MapMode::Write, 0..size, move |result: Result<(), _>| {
        if result.is_ok() {
            callback_flag.store(true, std::sync::atomic::Ordering::Release);
        }
    });
    poll_until_queue_empty(device);
    if !mapped.load(std::sync::atomic::Ordering::Acquire) {
        bail!("GPU input buffer did not map in time");
    }
    let mut view = buffer
        .get_mapped_range_mut(0..size)
        .context("mapped input range is available")?;
    view.copy_from_slice(data);
    drop(view);
    buffer.unmap();
    Ok(())
}

fn poll_until_queue_empty(device: &wgpu::Device) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(status) = device.poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_millis(50)),
        }) {
            if status.wait_finished() {
                return;
            }
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Execute `repetitions` full matmul dispatches, copy the result into the
/// staging buffer, and verify it. The timer spans command encoding, submit,
/// GPU completion sync and the readback; it excludes input upload and shader
/// compilation. Returns elapsed nanoseconds.
fn measure_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    buffers: (&wgpu::Buffer, &wgpu::Buffer),
    dimension: u32,
    repetitions: u32,
) -> Result<u64> {
    let (output, staging) = buffers;
    let dispatch = dimension / WORKGROUP;
    let size = u64::from(dimension) * u64::from(dimension) * 4;
    let start = Instant::now();
    for _ in 0..repetitions {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("gpu-bench-dispatch"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(dispatch, dispatch, 1);
        }
        queue.submit([encoder.finish()]);
    }
    let mut copy_encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu-bench-readback"),
    });
    copy_encoder.copy_buffer_to_buffer(output, 0, staging, 0, Some(size));
    let copy_index = queue.submit([copy_encoder.finish()]);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = device
            .poll(PollType::Wait {
                submission_index: Some(copy_index.clone()),
                timeout: Some(Duration::from_millis(100)),
            })
            .context("GPU completion poll failed")?;
        if status.wait_finished() {
            break;
        }
        if Instant::now() >= deadline {
            bail!("GPU benchmark readback timed out");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let mapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_flag = std::sync::Arc::clone(&mapped);
    staging.map_async(MapMode::Read, 0..size, move |result: Result<(), _>| {
        if result.is_ok() {
            callback_flag.store(true, std::sync::atomic::Ordering::Release);
        }
    });
    let map_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = device.poll(PollType::Poll);
        if mapped.load(std::sync::atomic::Ordering::Acquire) || Instant::now() >= map_deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if !mapped.load(std::sync::atomic::Ordering::Acquire) {
        bail!("GPU output buffer did not map in time");
    }
    let bytes = staging
        .get_mapped_range(0..size)
        .expect("staging buffer is mapped")
        .to_vec();
    staging.unmap();
    let elapsed_ns = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let values = bytes_to_f32(&bytes)?;
    workload::validate_values::<f32>(dimension, &values).context("GPU output mismatch")?;
    Ok(elapsed_ns)
}

fn calibrate_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    buffers: (&wgpu::Buffer, &wgpu::Buffer),
    dimension: u32,
) -> Result<u32> {
    let mut repetitions = 1u32;
    for _ in 0..8 {
        let elapsed_ns = measure_batch(
            device,
            queue,
            pipeline,
            bind_group,
            buffers,
            dimension,
            repetitions,
        )?;
        if elapsed_ns >= TARGET_NANOS {
            return Ok(repetitions);
        }
        repetitions = repetitions.saturating_mul(2).min(128);
    }
    Ok(repetitions)
}

fn bytes_to_f32(bytes: &[u8]) -> Result<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        bail!("GPU readback is not word aligned");
    }
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        bail!("GPU readback is not word aligned");
    }
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        out.push(f32::from_le_bytes(*chunk));
    }
    Ok(out)
}

fn f32_to_bytes(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn bounded(text: &str) -> String {
    let mut trimmed = text.trim().to_owned();
    if trimmed.len() > 255 {
        let mut end = 255;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed.truncate(end);
    }
    trimmed
}

fn bounded_pci(text: &str) -> Option<String> {
    let trimmed = text.trim().to_owned();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    Some(trimmed)
}

fn same_device(a: &BenchmarkGpuIdentity, b: &BenchmarkGpuIdentity) -> bool {
    if let (Some(left), Some(right)) = (&a.pci_bus_id, &b.pci_bus_id) {
        return left == right;
    }
    a.backend == b.backend && a.vendor == b.vendor && a.device == b.device
}

fn candidate_matches(info: &wgpu::AdapterInfo, identity: &BenchmarkGpuIdentity) -> bool {
    info.vendor == identity.vendor
        && info.device == identity.device
        && backend_matches(info.backend, &identity.backend)
        && match &identity.pci_bus_id {
            Some(expected) => info.device_pci_bus_id == *expected,
            None => true,
        }
}

fn backend_matches(backend: wgpu::Backend, expected: &str) -> bool {
    matches!(
        (backend, expected),
        (wgpu::Backend::Dx12, "dx12") | (wgpu::Backend::Vulkan, "vulkan")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_round_trip_is_word_aligned() {
        let bytes = f32_to_bytes(&[1.0f32, -2.5, 0.125]);
        assert_eq!(bytes_to_f32(&bytes).unwrap(), vec![1.0f32, -2.5, 0.125]);
        assert!(bytes_to_f32(&[1, 2, 3]).is_err());
    }

    #[test]
    fn identity_matching_uses_pci_when_present() {
        let a = BenchmarkGpuIdentity {
            backend: "dx12".into(),
            vendor: 0x10de,
            device: 0x1,
            name: "gpu".into(),
            driver: "d".into(),
            driver_info: "i".into(),
            device_type: "discrete-gpu".into(),
            pci_bus_id: Some("0000:01:00.0".into()),
        };
        let mut b = a.clone();
        b.backend = "vulkan".into();
        assert!(
            same_device(&a, &b),
            "PCI identity supersedes backend for dedupe"
        );
    }

    #[test]
    fn dimension_bytes_are_exact() {
        assert_eq!(512 * 512 * 4, 1_048_576);
        assert_eq!(1024 * 1024 * 4, 4_194_304);
    }
}
