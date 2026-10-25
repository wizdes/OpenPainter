struct Params { width: u32, height: u32, radius: u32, pad: u32 }
@group(0) @binding(0) var source: texture_2d<u32>;
@group(0) @binding(1) var sums_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var wc_out: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var<uniform> p: Params;
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let r = i32(p.radius);
  var sums = vec4<f32>(0.0);
  var wc = 0.0;
  for (var dy = -r; dy <= r; dy++) {
    let yy = i32(id.y) + dy;
    if yy < 0 || yy >= i32(p.height) { continue; }
    let c = textureLoad(source, vec2<i32>(i32(id.x), yy), 0);
    let wy = u32(16 * (r + 1 - abs(dy)));
    let aw = wy * (c.a + (c.a >> 7u));
    wc += f32(aw);
    sums += f32(aw >> 8u) * vec4<f32>(c);
  }
  textureStore(sums_out, vec2<i32>(id.xy), sums);
  textureStore(wc_out, vec2<i32>(id.xy), vec4<f32>(wc, 0.0, 0.0, 0.0));
}
