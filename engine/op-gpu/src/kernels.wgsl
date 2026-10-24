struct Params { width: u32, height: u32, mode: u32, opacity: u32, masked: u32 }
@group(0) @binding(0) var input_tex: texture_2d<u32>;
@group(0) @binding(1) var<storage, read_write> acc: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var output_tex: texture_storage_2d<rgba8uint, write>;
@group(0) @binding(4) var coverage_tex: texture_2d<u32>;
@compute @workgroup_size(16, 16)
fn effect_invert_colors(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(255u - src.r, 255u - src.g, 255u - src.b, src.a));
}
@compute @workgroup_size(16, 16)
fn effect_invert_alpha(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(src.r, src.g, src.b, 255u - src.a));
}
fn unit(x: u32) -> f32 { return f32(x) / 255.0; }
fn byte(x: f32) -> u32 { return u32(floor(clamp(x, 0.0, 1.0) * 255.0 + 0.5)); }
fn blend(mode: u32, b: f32, s: f32) -> f32 {
  switch mode {
    case 0u: { return s; }
    case 1u: { return b * s; }
    case 2u: { return min(b + s, 1.0); }
    case 3u: { if s == 0.0 { return 0.0; } return max(1.0 - (1.0 - b) / s, 0.0); }
    case 4u: { if s == 1.0 { return 1.0; } return min(b / (1.0 - s), 1.0); }
    case 5u: { if s == 1.0 { return 1.0; } return min(b * b / (1.0 - s), 1.0); }
    case 6u: { if b == 1.0 { return 1.0; } return min(s * s / (1.0 - b), 1.0); }
    case 7u: { if b < 0.5 { return 2.0 * b * s; } return 1.0 - 2.0 * (1.0 - b) * (1.0 - s); }
    case 8u: { return abs(b - s); }
    case 9u: { return 1.0 - abs(1.0 - b - s); }
    case 10u: { return max(b, s); }
    case 11u: { return min(b, s); }
    case 12u: { return b + s - b * s; }
    default: { return unit(byte(b) ^ byte(s)); }
  }
}
@compute @workgroup_size(16, 16)
fn layer(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let index = id.y * p.width + id.x;
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  var coverage = p.opacity;
  if p.masked != 0u { coverage = textureLoad(coverage_tex, vec2<i32>(id.xy), 0).r; }
  let sa = unit(src.a) * unit(coverage);
  if sa == 0.0 { return; }
  let old = acc[index];
  let ba = old.a;
  var next: vec4<f32>;
  for (var i = 0u; i < 3u; i++) {
    let s = unit(src[i]);
    var b = 0.0;
    if ba > 0.0 { b = clamp(old[i] / ba, 0.0, 1.0); }
    if b > 0.999999 { b = 1.0; }
    let t0 = sa * (1.0 - ba) * s;
    let t1 = (1.0 - sa) * old[i];
    let t2 = sa * ba * blend(p.mode, b, s);
    next[i] = t0 + t1 + t2;
  }
  next.a = sa + ba * (1.0 - sa);
  acc[index] = next;
}
@compute @workgroup_size(16, 16)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let old = acc[id.y * p.width + id.x];
  if old.a <= 0.0 { textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(0u)); return; }
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(byte(old.r / old.a), byte(old.g / old.a), byte(old.b / old.a), byte(old.a)));
}
@compute @workgroup_size(16, 16)
fn overwrite(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  var coverage = p.opacity;
  if p.masked != 0u { coverage = textureLoad(coverage_tex, vec2<i32>(id.xy), 0).r; }
  let t = unit(coverage);
  let index = id.y * p.width + id.x;
  let old = acc[index];
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(
    byte((old.r * (1.0 - t) + f32(src.r) * t) / 255.0),
    byte((old.g * (1.0 - t) + f32(src.g) * t) / 255.0),
    byte((old.b * (1.0 - t) + f32(src.b) * t) / 255.0),
    byte((old.a * (1.0 - t) + f32(src.a) * t) / 255.0)));
}
@compute @workgroup_size(16, 16)
fn box_blur(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  var sum = vec4<u32>(0u);
  for (var dy = -1; dy <= 1; dy++) {
    for (var dx = -1; dx <= 1; dx++) {
      let x = clamp(i32(id.x) + dx, 0, i32(p.width) - 1);
      let y = clamp(i32(id.y) + dy, 0, i32(p.height) - 1);
      sum += textureLoad(input_tex, vec2<i32>(x, y), 0);
    }
  }
  textureStore(output_tex, vec2<i32>(id.xy), (sum + vec4<u32>(4u)) / vec4<u32>(9u));
}
