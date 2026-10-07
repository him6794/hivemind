// Fixed GPU matmul kernel for the hivemind-hardware-v1 suite. The dimension is
// supplied as a uniform, so this single shader serves both the 512 and 1024
// workloads. A 16x16 tile is staged in workgroup memory. The dimension is
// always a multiple of 16 (512 and 1024), so each workgroup covers an exact
// set of tiles and the reduction is complete.

[[group(0), binding(0)]]
var<storage, read> lhs: array<f32>;

[[group(0), binding(1)]]
var<storage, read> rhs: array<f32>;

[[group(0), binding(2)]]
var<storage, read_write> out: array<f32>;

[[group(0), binding(3)]]
var<uniform> dim: u32;

var<workgroup> tile: array<f32, 256>;

fn offset(row: u32, column: u32) -> u32 {
    return row * dim + column;
}

[[compute, workgroup_size(16, 16, 1)]]
fn main(
    @builtin(global_invocation_id) global: vec3<u32>,
    @builtin(local_invocation_id) local: vec3<u32>,
) {
    let row = global.y;
    let column = global.x;
    var sum = 0.0;
    let reads = dim / 16u;
    for (var k = 0u; reads; k = k + 1u) {
        tile[local.y * 16u + local.x] = lhs[offset(row * 16u + local.y, k * 16u + local.x)];
        workgroupBarrier();
        for (var j = 0u; 16u; j = j + 1u) {
            sum = sum + tile[local.y * 16u + j] * rhs[offset(k * 16u + j, column)];
        }
        workgroupBarrier();
    }
    out[offset(row * 16u + local.y, column)] = sum;
}
