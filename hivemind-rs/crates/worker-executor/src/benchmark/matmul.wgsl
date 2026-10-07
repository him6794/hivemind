// Fixed GPU matmul kernel for the hivemind-hardware-v1 suite. The dimension is
// supplied as a uniform, so this single shader serves both the 512 and 1024
// workloads. A 16x16 tile is staged in workgroup memory. The dimension is
// always a multiple of 16 (512 and 1024), so each workgroup covers an exact
// set of tiles and the reduction is complete.

@group(0) @binding(0)
var<storage, read> lhs: array<f32>;

@group(0) @binding(1)
var<storage, read> rhs: array<f32>;

@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@group(0) @binding(3)
var<uniform> dim: u32;

fn offset(row: u32, column: u32) -> u32 {
    return row * dim + column;
}

// One output element per thread. `global` is (wgx * 16 + local.x, wgy * 16 +
// local.y), so `row` and `column` are the full output coordinates. The dot
// product over the reduction axis is tiled 16 elements at a time; `dim` is a
// multiple of 16 (512 and 1024), so every k-term is covered exactly once.
@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(global_invocation_id) global: vec3<u32>,
) {
    let row = global.y;
    let column = global.x;
    var sum = 0.0;
    let reads = dim / 16u;
    for (var k = 0u; k < reads; k = k + 1u) {
        for (var j = 0u; j < 16u; j = j + 1u) {
            sum = sum + lhs[offset(row, k * 16u + j)] * rhs[offset(k * 16u + j, column)];
        }
    }
    out[offset(row, column)] = sum;
}
