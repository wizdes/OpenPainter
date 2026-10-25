struct Params { width: u32, height: u32, radius: u32, pad: u32 }
@group(0) @binding(0) var sums_in: texture_2d<u32>;
@group(0) @binding(1) var wc_in: texture_2d<u32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba8uint, write>;
@group(0) @binding(3) var<uniform> p: Params;
// The row sums pass 2^32 (about 2^39 at radius 200), so they are 64-bit
// whole numbers as on the CPU, kept as (high, low) u32 pairs.
fn mul64(a: u32, b: u32) -> vec2<u32> {
  let a0 = a & 0xffffu;
  let a1 = a >> 16u;
  let b0 = b & 0xffffu;
  let b1 = b >> 16u;
  let low = a0 * b0;
  let cross1 = a1 * b0;
  let cross2 = a0 * b1;
  var hi = a1 * b1 + (cross1 >> 16u) + (cross2 >> 16u);
  var lo = low + (cross1 << 16u);
  hi += u32(lo < low);
  let before = lo;
  lo += cross2 << 16u;
  hi += u32(lo < before);
  return vec2<u32>(hi, lo);
}
fn add64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
  let lo = a.y + b.y;
  return vec2<u32>(a.x + b.x + u32(lo < a.y), lo);
}
fn sub64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
  return vec2<u32>(a.x - b.x - u32(a.y < b.y), a.y - b.y);
}
fn ge64(a: vec2<u32>, b: vec2<u32>) -> bool {
  return a.x > b.x || (a.x == b.x && a.y >= b.y);
}
fn shl64(a: vec2<u32>, n: u32) -> vec2<u32> {
  if n == 0u { return a; }
  return vec2<u32>((a.x << n) | (a.y >> (32u - n)), a.y << n);
}
fn is_zero64(a: vec2<u32>) -> bool { return a.x == 0u && a.y == 0u; }
// n / d truncated and clamped to 0..255, with d > 0.
fn div64_byte(n: vec2<u32>, d: vec2<u32>) -> u32 {
  if ge64(n, shl64(d, 8u)) { return 255u; }
  var rem = n;
  var q = 0u;
  for (var bit = 7; bit >= 0; bit--) {
    let part = shl64(d, u32(bit));
    if ge64(rem, part) {
      rem = sub64(rem, part);
      q |= 1u << u32(bit);
    }
  }
  return q;
}
fn weight_sum(coord: i32, length: i32, r: i32) -> u32 {
  let whole = 16 * (r + 1) * (r + 1);
  let left = max(r - coord, 0);
  let right = max(r - (length - 1 - coord), 0);
  return u32(whole - 8 * left * (left + 1) - 8 * right * (right + 1));
}
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let r = i32(p.radius);
  var sums = array<vec2<u32>, 4>(vec2<u32>(0u), vec2<u32>(0u), vec2<u32>(0u), vec2<u32>(0u));
  var wc = vec2<u32>(0u);
  for (var dx = -r; dx <= r; dx++) {
    let xx = i32(id.x) + dx;
    if xx < 0 || xx >= i32(p.width) { continue; }
    let wx = u32(16 * (r + 1 - abs(dx)));
    let column = textureLoad(sums_in, vec2<i32>(xx, i32(id.y)), 0);
    for (var c = 0; c < 4; c++) {
      sums[c] = add64(sums[c], mul64(wx, column[c]));
    }
    wc = add64(wc, mul64(wx, textureLoad(wc_in, vec2<i32>(xx, i32(id.y)), 0).r));
  }
  let wa = mul64(weight_sum(i32(id.x), i32(p.width), r), weight_sum(i32(id.y), i32(p.height), r));
  // wc_sum >>= 8, as the CPU does before dividing.
  let denominator = vec2<u32>(wc.x >> 8u, (wc.y >> 8u) | (wc.x << 24u));
  var result = vec4<u32>(0u);
  if !is_zero64(wa) && !is_zero64(denominator) {
    result = vec4<u32>(
      div64_byte(sums[0], denominator),
      div64_byte(sums[1], denominator),
      div64_byte(sums[2], denominator),
      div64_byte(sums[3], wa),
    );
  }
  textureStore(output, vec2<i32>(id.xy), result);
}
