//! Minimal Rust CUDA/cuBLAS adapter for the GPU-v1 operation set.
//!
//! This module is compiled only for the operator's Linux CUDA runner.  It uses
//! fixed CUDA and cuBLAS symbols; no task-controlled source, PTX, libraries,
//! pointers, streams, or kernels enter the interpreter API.

use super::sealed;
use super::{GpuBackendError, GpuOperation, GpuTensor};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr;

/// CUDA backend state is private to one interpreter invocation.
///
/// The raw handle never crosses the public `GpuBackend` boundary.
pub struct CudaGpuBackend {
    /// The operator-bound stable identity corresponding to this ordinal.
    device_id: String,
    /// The operator-bound CUDA UUID that was checked against the ordinal.
    cuda_uuid: String,
    /// Parsed UUID bytes used to re-check the ordinal before each operation.
    cuda_uuid_bytes: [u8; 16],
    pub(super) device_ordinal: i32,
    handle: *mut c_void,
}

#[repr(C)]
struct CudaUuid {
    bytes: [u8; 16],
}

const CUDA_SUCCESS: c_int = 0;
const CUDA_MEMCPY_HOST_TO_DEVICE: c_int = 1;
const CUDA_MEMCPY_DEVICE_TO_HOST: c_int = 2;
const CUBLAS_STATUS_SUCCESS: c_int = 0;
const CUBLAS_OP_N: c_int = 0;
const CUBLAS_DEFAULT_MATH: c_int = 0;

#[link(name = "cudart")]
unsafe extern "C" {
    #[link_name = "cudaSetDevice"]
    fn cuda_set_device(device: c_int) -> c_int;
    #[link_name = "cudaDeviceGetUuid"]
    fn cuda_device_get_uuid(uuid: *mut CudaUuid, device: c_int) -> c_int;
    #[link_name = "cudaMalloc"]
    fn cuda_malloc(device_pointer: *mut *mut c_void, bytes: usize) -> c_int;
    #[link_name = "cudaFree"]
    fn cuda_free(device_pointer: *mut c_void) -> c_int;
    #[link_name = "cudaMemcpy"]
    fn cuda_memcpy(
        destination: *mut c_void,
        source: *const c_void,
        bytes: usize,
        kind: c_int,
    ) -> c_int;
    #[link_name = "cudaDeviceSynchronize"]
    fn cuda_device_synchronize() -> c_int;
    #[link_name = "cudaGetErrorString"]
    fn cuda_get_error_string(error: c_int) -> *const c_char;
}

#[link(name = "cublas")]
unsafe extern "C" {
    #[link_name = "cublasCreate_v2"]
    fn cublas_create(handle: *mut *mut c_void) -> c_int;
    #[link_name = "cublasDestroy_v2"]
    fn cublas_destroy(handle: *mut c_void) -> c_int;
    #[link_name = "cublasSetMathMode"]
    fn cublas_set_math_mode(handle: *mut c_void, mode: c_int) -> c_int;
    #[link_name = "cublasSaxpy_v2"]
    fn cublas_saxpy(
        handle: *mut c_void,
        n: c_int,
        alpha: *const f32,
        x: *const c_void,
        incx: c_int,
        y: *mut c_void,
        incy: c_int,
    ) -> c_int;
    #[link_name = "cublasSscal_v2"]
    fn cublas_sscal(
        handle: *mut c_void,
        n: c_int,
        alpha: *const f32,
        x: *mut c_void,
        incx: c_int,
    ) -> c_int;
    #[link_name = "cublasSgemm_v2"]
    fn cublas_sgemm(
        handle: *mut c_void,
        transa: c_int,
        transb: c_int,
        m: c_int,
        n: c_int,
        k: c_int,
        alpha: *const f32,
        a: *const c_void,
        lda: c_int,
        b: *const c_void,
        ldb: c_int,
        beta: *const f32,
        c: *mut c_void,
        ldc: c_int,
    ) -> c_int;
}

struct DeviceBuffer {
    pointer: *mut c_void,
    bytes: usize,
}

impl DeviceBuffer {
    fn from_f32(data: &[f32]) -> Result<Self, GpuBackendError> {
        let bytes = data
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| GpuBackendError::invalid("CUDA allocation size overflow"))?;
        if bytes > super::GPU_MAX_TENSOR_BYTES {
            return Err(GpuBackendError::invalid(
                "CUDA tensor exceeds the GPU-v1 byte limit",
            ));
        }
        if bytes == 0 {
            return Err(GpuBackendError::invalid("CUDA tensors cannot be empty"));
        }
        let mut pointer = ptr::null_mut();
        // SAFETY: `pointer` is valid mutable storage for CUDA to initialize, and
        // `bytes` is nonzero and bounded by the GPU-v1 tensor limit.
        let error = unsafe { cuda_malloc(&mut pointer, bytes) };
        check_cuda(error, "cudaMalloc")?;
        // SAFETY: `pointer` is the live allocation returned by `cudaMalloc` and
        // `data` contains at least `bytes` initialized bytes for the synchronous
        // host-to-device copy.
        let error = unsafe {
            cuda_memcpy(
                pointer,
                data.as_ptr().cast(),
                bytes,
                CUDA_MEMCPY_HOST_TO_DEVICE,
            )
        };
        if let Err(error) = check_cuda(error, "cudaMemcpy host-to-device") {
            // SAFETY: `pointer` came from the successful `cudaMalloc` above and
            // is no longer used after this best-effort cleanup call.
            unsafe {
                let _ = cuda_free(pointer);
            }
            return Err(error);
        }
        Ok(Self { pointer, bytes })
    }

    fn to_f32(&self) -> Result<Vec<f32>, GpuBackendError> {
        let count = self.bytes / std::mem::size_of::<f32>();
        let mut data = Vec::<f32>::with_capacity(count);
        // SAFETY: `data` has capacity for exactly `count` f32 values, which is
        // exactly `self.bytes`; `self.pointer` is the live allocation owned by
        // this buffer and CUDA copies synchronously into the host allocation.
        let error = unsafe {
            cuda_memcpy(
                data.as_mut_ptr().cast(),
                self.pointer,
                self.bytes,
                CUDA_MEMCPY_DEVICE_TO_HOST,
            )
        };
        check_cuda(error, "cudaMemcpy device-to-host")?;
        // SAFETY: the synchronous copy above initialized exactly `count` f32
        // elements in the allocation's spare capacity.
        unsafe {
            data.set_len(count);
        }
        if data.iter().any(|value| !value.is_finite()) {
            return Err(GpuBackendError::execution(
                "CUDA returned a non-finite tensor value",
            ));
        }
        Ok(data)
    }
}

impl Drop for DeviceBuffer {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: `pointer` is an allocation owned by this buffer and Drop
            // is the only path that releases it.
            unsafe {
                let _ = cuda_free(self.pointer);
            }
        }
    }
}

impl sealed::GpuBackendImpl for CudaGpuBackend {
    fn execute_unchecked(
        &mut self,
        operation: GpuOperation,
        inputs: &[GpuTensor],
    ) -> Result<GpuTensor, GpuBackendError> {
        self.bind_device()?;
        match operation {
            GpuOperation::AddF32 => self.add(inputs),
            GpuOperation::ScaleF32 => self.scale(inputs),
            GpuOperation::MatmulF32 => self.matmul(inputs),
        }
    }
}

impl super::GpuBackend for CudaGpuBackend {}

impl CudaGpuBackend {
    /// Bind one operator-selected stable device identity to one CUDA ordinal.
    ///
    /// The caller must obtain all three values from the trusted admission
    /// snapshot; task input is never accepted by the interpreter as a device
    /// selector. The UUID check prevents a changed CUDA ordinal mapping from
    /// silently running on a different physical GPU.
    pub fn new(
        device_id: impl Into<String>,
        device_ordinal: i32,
        expected_cuda_uuid: impl Into<String>,
    ) -> Result<Self, GpuBackendError> {
        let device_id = device_id.into();
        if device_id.is_empty()
            || device_id.len() > 128
            || !device_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
            })
        {
            return Err(GpuBackendError::invalid(
                "CUDA device identity is not a valid operator token",
            ));
        }
        let expected_cuda_uuid = expected_cuda_uuid.into();
        let expected_cuda_uuid_bytes = parse_cuda_uuid(&expected_cuda_uuid).ok_or_else(|| {
            GpuBackendError::invalid("CUDA device UUID is not a valid operator token")
        })?;
        let expected_cuda_uuid = canonical_cuda_uuid_bytes(&expected_cuda_uuid_bytes);
        if device_ordinal < 0 {
            return Err(GpuBackendError::invalid(
                "CUDA device ordinal must not be negative",
            ));
        }
        // SAFETY: `device_ordinal` is nonnegative and is supplied by the
        // operator's trusted device binding rather than task input.
        let error = unsafe { cuda_set_device(device_ordinal) };
        check_cuda_initialization(error, "cudaSetDevice")?;
        let mut actual_uuid = CudaUuid { bytes: [0; 16] };
        // SAFETY: `actual_uuid` is valid writable storage for CUDA, and the
        // ordinal was selected successfully immediately above.
        let error = unsafe { cuda_device_get_uuid(&mut actual_uuid, device_ordinal) };
        check_cuda_initialization(error, "cudaDeviceGetUuid")?;
        let actual_cuda_uuid = canonical_cuda_uuid(&actual_uuid);
        if actual_uuid.bytes != expected_cuda_uuid_bytes {
            return Err(GpuBackendError::unavailable(format!(
                "CUDA ordinal {device_ordinal} is bound to {actual_cuda_uuid}, expected {expected_cuda_uuid}"
            )));
        }
        let mut handle = ptr::null_mut();
        // SAFETY: `handle` is valid mutable storage for cuBLAS to initialize.
        let status = unsafe { cublas_create(&mut handle) };
        check_cublas_initialization(status, "cublasCreate_v2")?;
        // SAFETY: `handle` was returned by the successful cuBLAS create call.
        let status = unsafe { cublas_set_math_mode(handle, CUBLAS_DEFAULT_MATH) };
        if let Err(error) = check_cublas_initialization(status, "cublasSetMathMode") {
            // SAFETY: `handle` was returned by cublasCreate_v2 and is not used
            // again after this cleanup call.
            unsafe {
                let _ = cublas_destroy(handle);
            }
            return Err(error);
        }
        Ok(Self {
            device_id,
            cuda_uuid: expected_cuda_uuid,
            cuda_uuid_bytes: expected_cuda_uuid_bytes,
            device_ordinal,
            handle,
        })
    }

    fn bind_device(&self) -> Result<(), GpuBackendError> {
        // SAFETY: `device_ordinal` is validated as nonnegative at construction,
        // and selecting it occurs before any operation uses this backend's CUDA
        // or cuBLAS resources.
        let error = unsafe { cuda_set_device(self.device_ordinal) };
        check_cuda(error, "cudaSetDevice")?;
        let mut actual_uuid = CudaUuid { bytes: [0; 16] };
        // SAFETY: `actual_uuid` is valid writable storage for CUDA, and the
        // ordinal was selected successfully immediately above.
        let error = unsafe { cuda_device_get_uuid(&mut actual_uuid, self.device_ordinal) };
        check_cuda(error, "cudaDeviceGetUuid")?;
        if actual_uuid.bytes != self.cuda_uuid_bytes {
            return Err(GpuBackendError::unavailable(format!(
                "CUDA ordinal {} changed identity from {} to {}",
                self.device_ordinal,
                self.cuda_uuid,
                canonical_cuda_uuid(&actual_uuid),
            )));
        }
        Ok(())
    }

    #[must_use]
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    #[must_use]
    pub fn cuda_uuid(&self) -> &str {
        &self.cuda_uuid
    }

    fn add(&mut self, inputs: &[GpuTensor]) -> Result<GpuTensor, GpuBackendError> {
        if inputs.len() != 2 || inputs[0].shape != inputs[1].shape {
            return Err(GpuBackendError::invalid(
                "gpu_add_f32 expects two equal-shaped tensors",
            ));
        }
        let left = DeviceBuffer::from_f32(&inputs[0].data)?;
        let right = DeviceBuffer::from_f32(&inputs[1].data)?;
        let count = c_int_count(inputs[0].data.len())?;
        let alpha = 1.0f32;
        // SAFETY: both pointers are live device allocations with `count` f32
        // elements, `alpha` is a valid host scalar, and the cuBLAS handle is
        // initialized for the selected device.
        let status = unsafe {
            cublas_saxpy(
                self.handle,
                count,
                &alpha,
                right.pointer,
                1,
                left.pointer,
                1,
            )
        };
        check_cublas(status, "cublasSaxpy_v2")?;
        synchronize()?;
        GpuTensor::new(inputs[0].shape.clone(), left.to_f32()?)
    }

    fn scale(&mut self, inputs: &[GpuTensor]) -> Result<GpuTensor, GpuBackendError> {
        if inputs.len() != 2 || !inputs[1].shape.is_empty() || inputs[1].data.len() != 1 {
            return Err(GpuBackendError::invalid(
                "gpu_scale_f32 expects a tensor and scalar",
            ));
        }
        let value = DeviceBuffer::from_f32(&inputs[0].data)?;
        let count = c_int_count(inputs[0].data.len())?;
        let scalar = inputs[1].data[0];
        // SAFETY: `value.pointer` is a live device allocation with `count` f32
        // elements, `scalar` is a valid host f32, and the cuBLAS handle is
        // initialized for the selected device.
        let status = unsafe { cublas_sscal(self.handle, count, &scalar, value.pointer, 1) };
        check_cublas(status, "cublasSscal_v2")?;
        synchronize()?;
        GpuTensor::new(inputs[0].shape.clone(), value.to_f32()?)
    }

    fn matmul(&mut self, inputs: &[GpuTensor]) -> Result<GpuTensor, GpuBackendError> {
        if inputs.len() != 2 {
            return Err(GpuBackendError::invalid(
                "gpu_matmul_f32 expects two tensors",
            ));
        }
        let [rows, inner] = inputs[0].shape.as_slice() else {
            return Err(GpuBackendError::invalid(
                "gpu_matmul_f32 expects rank-2 left input",
            ));
        };
        let [right_inner, columns] = inputs[1].shape.as_slice() else {
            return Err(GpuBackendError::invalid(
                "gpu_matmul_f32 expects rank-2 right input",
            ));
        };
        if inner != right_inner {
            return Err(GpuBackendError::invalid(
                "gpu_matmul_f32 inner dimensions do not match",
            ));
        }
        let output_len = rows
            .checked_mul(*columns)
            .ok_or_else(|| GpuBackendError::invalid("CUDA matmul output is too large"))?;
        let output_bytes = output_len
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| GpuBackendError::invalid("CUDA matmul output size overflow"))?;
        if output_bytes > super::GPU_MAX_TENSOR_BYTES {
            return Err(GpuBackendError::invalid(
                "CUDA matmul output exceeds the GPU-v1 byte limit",
            ));
        }
        let left = DeviceBuffer::from_f32(&inputs[0].data)?;
        let right = DeviceBuffer::from_f32(&inputs[1].data)?;
        let output = DeviceBuffer::from_f32(&vec![0.0f32; output_len])?;
        let m = c_int_count(*columns)?;
        let n = c_int_count(*rows)?;
        let k = c_int_count(*inner)?;
        let lda = m;
        let ldb = k;
        let ldc = m;
        let alpha = 1.0f32;
        let beta = 0.0f32;
        // cuBLAS uses column-major storage.  Passing right as A and left as B
        // computes (left * right)^T in column-major memory, which is exactly
        // the desired row-major byte order.
        // SAFETY: all matrix pointers are live device allocations sized for the
        // declared dimensions, the leading dimensions are valid for the
        // column-major operands, and the scalar pointers are valid host f32s.
        let status = unsafe {
            cublas_sgemm(
                self.handle,
                CUBLAS_OP_N,
                CUBLAS_OP_N,
                m,
                n,
                k,
                &alpha,
                right.pointer,
                lda,
                left.pointer,
                ldb,
                &beta,
                output.pointer,
                ldc,
            )
        };
        check_cublas(status, "cublasSgemm_v2")?;
        synchronize()?;
        GpuTensor::new(vec![*rows, *columns], output.to_f32()?)
    }
}

impl Drop for CudaGpuBackend {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: `handle` was returned by cublasCreate_v2 for this backend
            // and no cuBLAS operation is attempted after Drop begins.
            unsafe {
                let _ = cublas_destroy(self.handle);
            }
        }
    }
}

fn c_int_count(value: usize) -> Result<c_int, GpuBackendError> {
    c_int::try_from(value).map_err(|_| GpuBackendError::invalid("CUDA tensor is too large"))
}

fn synchronize() -> Result<(), GpuBackendError> {
    // SAFETY: this only synchronizes the current CUDA device selected by the
    // backend before each invocation.
    let error = unsafe { cuda_device_synchronize() };
    check_cuda(error, "cudaDeviceSynchronize")
}

fn parse_cuda_uuid(value: &str) -> Option<[u8; 16]> {
    let uuid = value.strip_prefix("GPU-")?;
    let bytes = uuid.as_bytes();
    if bytes.len() != 36
        || ![8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
    {
        return None;
    }
    let mut parsed = [0u8; 16];
    let mut output_index = 0usize;
    let mut input_index = 0usize;
    while input_index < bytes.len() {
        if [8, 13, 18, 23].contains(&input_index) {
            input_index += 1;
            continue;
        }
        let high = hex_value(bytes[input_index])?;
        let low = hex_value(bytes[input_index + 1])?;
        parsed[output_index] = (high << 4) | low;
        output_index += 1;
        input_index += 2;
    }
    (output_index == parsed.len()).then_some(parsed)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn valid_cuda_uuid(value: &str) -> bool {
    parse_cuda_uuid(value).is_some()
}

fn canonical_cuda_uuid(uuid: &CudaUuid) -> String {
    canonical_cuda_uuid_bytes(&uuid.bytes)
}

fn canonical_cuda_uuid_bytes(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(40);
    value.push_str("GPU-");
    for (index, byte) in bytes.iter().copied().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            value.push('-');
        }
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{
        CudaUuid, canonical_cuda_uuid, canonical_cuda_uuid_bytes, parse_cuda_uuid, valid_cuda_uuid,
    };

    #[test]
    fn canonical_cuda_uuid_uses_the_trusted_gpu_token_format() {
        let uuid = CudaUuid { bytes: [0xab; 16] };
        assert_eq!(
            canonical_cuda_uuid(&uuid),
            "GPU-abababab-abab-abab-abab-abababababab"
        );
    }

    #[test]
    fn cuda_uuid_validation_rejects_noncanonical_values() {
        assert!(valid_cuda_uuid("GPU-01234567-89ab-cdef-0123-456789abcdef"));
        assert!(!valid_cuda_uuid("GPU-0123456789abcdef0123456789abcdef"));
        assert!(!valid_cuda_uuid("GPU-test"));
        assert!(!valid_cuda_uuid("GPU-0123"));
        assert!(!valid_cuda_uuid(
            "uuid-01234567-89ab-cdef-0123-456789abcdef"
        ));
    }

    #[test]
    fn cuda_uuid_parser_normalizes_hex_case() {
        let upper = parse_cuda_uuid("GPU-01234567-89AB-CDEF-0123-456789ABCDEF").unwrap();
        assert_eq!(
            canonical_cuda_uuid_bytes(&upper),
            "GPU-01234567-89ab-cdef-0123-456789abcdef"
        );
    }
}

fn check_cuda_initialization(error: c_int, operation: &str) -> Result<(), GpuBackendError> {
    if error == CUDA_SUCCESS {
        Ok(())
    } else {
        Err(GpuBackendError::unavailable(cuda_error_message(
            error, operation,
        )))
    }
}

fn check_cuda(error: c_int, operation: &str) -> Result<(), GpuBackendError> {
    if error == CUDA_SUCCESS {
        return Ok(());
    }
    Err(GpuBackendError::execution(cuda_error_message(
        error, operation,
    )))
}

fn cuda_error_message(error: c_int, operation: &str) -> String {
    // SAFETY: CUDA returns a process-lifetime NUL-terminated diagnostic string
    // for a valid error code; a null pointer is handled without dereferencing.
    unsafe {
        let pointer = cuda_get_error_string(error);
        if pointer.is_null() {
            format!("{operation} failed with CUDA error {error}")
        } else {
            format!(
                "{operation} failed with CUDA error {error}: {}",
                CStr::from_ptr(pointer).to_string_lossy()
            )
        }
    }
}

fn check_cublas_initialization(status: c_int, operation: &str) -> Result<(), GpuBackendError> {
    if status == CUBLAS_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(GpuBackendError::unavailable(format!(
            "{operation} failed with cuBLAS status {status}"
        )))
    }
}

fn check_cublas(status: c_int, operation: &str) -> Result<(), GpuBackendError> {
    if status == CUBLAS_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(GpuBackendError::execution(format!(
            "{operation} failed with cuBLAS status {status}"
        )))
    }
}
