struct Params { width: u32, height: u32, radius: u32, pad: u32 }
@group(0) @binding(0) var sums_in: texture_2d<f32>;
@group(0) @binding(1) var wc_in: texture_2d<f32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba8uint, write>;
@group(0) @binding(3) var<uniform> p: Params;
fn weight_sum(coord: i32, length: i32, r: i32) -> f32 {
  let whole = 16 * (r + 1) * (r + 1);
  let left = max(r - coord, 0);
  let right = max(r - (length - 1 - coord), 0);
  return f32(whole - 8 * left * (left + 1) - 8 * right * (right + 1));
}
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let r = i32(p.radius);
  var sums = vec4<f32>(0.0);
  var wc = 0.0;
  for (var dx = -r; dx <= r; dx++) {
    let xx = i32(id.x) + dx;
    if xx < 0 || xx >= i32(p.width) { continue; }
    let wx = f32(16 * (r + 1 - abs(dx)));
    sums += wx * textureLoad(sums_in, vec2<i32>(xx, i32(id.y)), 0);
    wc += wx * textureLoad(wc_in, vec2<i32>(xx, i32(id.y)), 0).r;
  }
  let wa = weight_sum(i32(id.x), i32(p.width), r) * weight_sum(i32(id.y), i32(p.height), r);
  let denominator = floor(wc / 256.0);
  var result = vec4<u32>(0u);
  if wa > 0.0 && denominator > 0.0 {
    result = vec4<u32>(vec4<f32>(floor(clamp(sums.rgb / denominator, vec3<f32>(0.0), vec3<f32>(255.0))),
                                 floor(clamp(sums.a / wa, 0.0, 255.0))));
  }
  textureStore(output, vec2<i32>(id.xy), result);
}
