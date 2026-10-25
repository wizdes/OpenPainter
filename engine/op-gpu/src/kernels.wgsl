struct Params { width: u32, height: u32, mode: u32, opacity: u32, masked: u32 }
@group(0) @binding(0) var input_tex: texture_2d<u32>;
@group(0) @binding(1) var<storage, read_write> acc: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var output_tex: texture_storage_2d<rgba8uint, write>;
@group(0) @binding(4) var coverage_tex: texture_2d<u32>;
@compute @workgroup_size(16, 16)
fn resample_nearest(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let source = textureDimensions(input_tex);
  let sx = min(id.x * source.x / p.width, source.x - 1u);
  let sy = min(id.y * source.y / p.height, source.y - 1u);
  textureStore(output_tex, vec2<i32>(id.xy), textureLoad(input_tex, vec2<i32>(i32(sx), i32(sy)), 0));
}
fn cubic_smooth(x: f32) -> f32 {
  let a = abs(x);
  if a < 1.0 { return (4.0 - 6.0*a*a + 3.0*a*a*a) / 6.0; }
  if a < 2.0 { let t = 2.0-a; return t*t*t/6.0; }
  return 0.0;
}
@compute @workgroup_size(16, 16)
fn resample_bilinear(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let source = textureDimensions(input_tex);
  let fx = f32(id.x * (source.x - 1u)) / f32(p.width - 1u);
  let fy = f32(id.y * (source.y - 1u)) / f32(p.height - 1u);
  let ix = u32(floor(fx)); let iy = u32(floor(fy));
  let xf = u32(256.0 * (fx - f32(ix)));
  let yf = u32(256.0 * (fy - f32(iy)));
  let weights = vec4<u32>((256u-xf)*(256u-yf),xf*(256u-yf),(256u-xf)*yf,xf*yf);
  let s0 = textureLoad(input_tex, vec2<i32>(i32(ix),i32(iy)),0);
  let s1 = textureLoad(input_tex, vec2<i32>(i32(min(ix+1u,source.x-1u)),i32(iy)),0);
  let s2 = textureLoad(input_tex, vec2<i32>(i32(ix),i32(min(iy+1u,source.y-1u))),0);
  let s3 = textureLoad(input_tex, vec2<i32>(i32(min(ix+1u,source.x-1u)),i32(min(iy+1u,source.y-1u))),0);
  let af = s0.a*weights.x+s1.a*weights.y+s2.a*weights.z+s3.a*weights.w;
  let a = (af+32768u)>>16u;
  var rgb = vec3<u32>(0u);
  if a > 0u {
    let weighted = vec3<f32>(s0.rgb)*f32(s0.a*weights.x)+vec3<f32>(s1.rgb)*f32(s1.a*weights.y)+vec3<f32>(s2.rgb)*f32(s2.a*weights.z)+vec3<f32>(s3.rgb)*f32(s3.a*weights.w);
    rgb = vec3<u32>(floor(weighted/f32(af)));
  }
  textureStore(output_tex,vec2<i32>(id.xy),vec4<u32>(rgb,a));
}
@compute @workgroup_size(16, 16)
fn resample_super_sampling(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let source = textureDimensions(input_tex);
  let left = f32(id.x)*f32(source.x)/f32(p.width);
  let right = f32(id.x+1u)*f32(source.x)/f32(p.width);
  let top = f32(id.y)*f32(source.y)/f32(p.height);
  let bottom = f32(id.y+1u)*f32(source.y)/f32(p.height);
  var rgb = vec3<f32>(0.0); var alpha = 0.0;
  for (var sy = u32(floor(top)); sy < min(u32(ceil(bottom)), source.y); sy++) {
    let wy = max(min(bottom,f32(sy+1u))-max(top,f32(sy)),0.0);
    for (var sx = u32(floor(left)); sx < min(u32(ceil(right)),source.x); sx++) {
      let wx = max(min(right,f32(sx+1u))-max(left,f32(sx)),0.0);
      let sample = textureLoad(input_tex, vec2<i32>(i32(sx),i32(sy)),0);
      let weight = wx*wy*f32(sample.a);
      alpha += weight;
      rgb += vec3<f32>(sample.rgb)*weight;
    }
  }
  var out_rgb = vec3<u32>(0u);
  if alpha > 0.0 {out_rgb = vec3<u32>(round(clamp(rgb/alpha,vec3<f32>(0.0),vec3<f32>(255.0))));}
  let out_alpha = u32(round(clamp(alpha/((right-left)*(bottom-top)),0.0,255.0)));
  textureStore(output_tex,vec2<i32>(id.xy),vec4<u32>(out_rgb,out_alpha));
}
@compute @workgroup_size(16, 16)
fn resample_bicubic(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let source = textureDimensions(input_tex);
  if source.x < 2u || source.y < 2u || p.width < 2u || p.height < 2u { return; }
  let fx = f32(id.x) * f32(source.x - 1u) / f32(p.width - 1u);
  let fy = f32(id.y) * f32(source.y - 1u) / f32(p.height - 1u);
  let ix = i32(floor(fx)); let iy = i32(floor(fy));
  var rgb = vec3<f32>(0.0); var alpha = 0.0; var total = 0.0;
  for (var n = -1; n <= 2; n++) {
    let sy = iy + n;
    if sy < 0 || sy >= i32(source.y) { continue; }
    let wy = cubic_smooth(fy - f32(iy + n));
    for (var m = -1; m <= 2; m++) {
      let sx = ix + m;
      if sx < 0 || sx >= i32(source.x) { continue; }
      let w = wy * cubic_smooth(fx - f32(ix + m));
      let sample = textureLoad(input_tex, vec2<i32>(sx,sy), 0);
      let a = f32(sample.a);
      rgb += vec3<f32>(sample.rgb) * (a*w);
      alpha += a*w;
      total += w;
    }
  }
  var out_alpha = 0u;
  var out_rgb = vec3<u32>(0u);
  if total > 0.0 {
    out_alpha = u32(round(clamp(alpha / total, 0.0, 255.0)));
    if alpha > 0.0 { out_rgb = vec3<u32>(round(clamp(rgb / alpha, vec3<f32>(0.0), vec3<f32>(255.0)))); }
  }
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(out_rgb,out_alpha));
}
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
@compute @workgroup_size(16, 16)
fn effect_black_and_white(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let value = (7471u * src.b + 38470u * src.g + 19595u * src.r) >> 16u;
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(value, value, value, src.a));
}
fn legacy_intensity(src: vec4<u32>) -> i32 {
  return i32((7471u * src.b + 38470u * src.g + 19595u * src.r) >> 16u);
}
@compute @workgroup_size(16, 16)
fn effect_brightness_contrast(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let brightness = bitcast<i32>(p.mode);
  let contrast = bitcast<i32>(p.opacity);
  let i = legacy_intensity(src);
  var multiply = 1i;
  var divide = 1i;
  if contrast < 0i { multiply = contrast + 100i; divide = 100i; }
  if contrast > 0i { multiply = 100i; divide = 100i - contrast; }
  var rgb: vec3<u32>;
  if divide == 0i {
    let value = select(255u, 0u, i + brightness < 128i);
    rgb = vec3<u32>(value);
  } else {
    var shift: i32;
    if divide == 100i { shift = (i - 127i) * multiply / divide + 127i - i + brightness; }
    else { shift = (i - 127i + brightness) * multiply / divide + 127i - i; }
    rgb = vec3<u32>(clamp(vec3<i32>(src.rgb) + vec3<i32>(shift), vec3<i32>(0i), vec3<i32>(255i)));
  }
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, src.a));
}
fn posterize_channel(value: u32, count: u32) -> u32 {
  if value == 0u { return 0u; }
  let index = (value * count - 1u) / 255u;
  return 255u * index / (count - 1u);
}
@compute @workgroup_size(16, 16)
fn effect_posterize(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(
    posterize_channel(src.r, p.mode), posterize_channel(src.g, p.opacity),
    posterize_channel(src.b, p.masked), src.a));
}
fn sepia_level(value: u32, gamma: f32) -> u32 {
  if value == 0u || value == 255u { return value; }
  return u32(255.0 * pow(f32(value) / 255.0, gamma));
}
@compute @workgroup_size(16, 16)
fn effect_sepia(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let gray = legacy_intensity(src);
  let strength = f32(p.mode) / 100.0;
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(
    sepia_level(u32(gray), 1.0 - 0.4 * strength), u32(gray),
    sepia_level(u32(gray), 1.0 + 0.4 * strength), src.a));
}
@compute @workgroup_size(16, 16)
fn effect_temperature_tint(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let temperature = bitcast<i32>(p.mode);
  let tint = bitcast<i32>(p.opacity);
  let shift = vec3<i32>(temperature / 2i + tint / 2i,
    temperature / 2i - tint, -temperature + tint / 2i);
  let rgb = vec3<u32>(clamp(vec3<i32>(src.rgb) + shift, vec3<i32>(0i), vec3<i32>(255i)));
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, src.a));
}
@compute @workgroup_size(16, 16)
fn effect_lut_rgb(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(
    u32(acc[src.r].r), u32(acc[src.g].g), u32(acc[src.b].b), src.a));
}
@compute @workgroup_size(16, 16)
fn effect_lut_luminosity(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let old = legacy_intensity(src);
  let diff = i32(acc[u32(old)].r) - old;
  let rgb = vec3<u32>(clamp(vec3<i32>(src.rgb) + vec3<i32>(diff), vec3<i32>(0i), vec3<i32>(255i)));
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, src.a));
}
@compute @workgroup_size(16, 16)
fn effect_highlights_shadows(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let l = legacy_intensity(src);
  let highlights = bitcast<i32>(p.mode);
  let shadows = bitcast<i32>(p.opacity);
  let shift = (shadows * (255 - l) * (255 - l) + highlights * l * l) / (100 * 255);
  let rgb = vec3<u32>(clamp(vec3<i32>(src.rgb) + vec3<i32>(shift), vec3<i32>(0i), vec3<i32>(255i)));
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, src.a));
}
@compute @workgroup_size(16, 16)
fn effect_gaussian(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let radius = i32(p.mode);
  var wa_sum = 0u;
  var wc_sum = 0u;
  var sums = vec4<u32>(0u);
  for (var dx = -radius; dx <= radius; dx++) {
    let xx = i32(id.x) + dx;
    if xx < 0 || xx >= i32(p.width) { continue; }
    let wx = 16u * u32(radius + 1 - abs(dx));
    var wa_col = 0u;
    var wc_col = 0u;
    var col = vec4<u32>(0u);
    for (var dy = -radius; dy <= radius; dy++) {
      let yy = i32(id.y) + dy;
      if yy < 0 || yy >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, vec2<i32>(xx, yy), 0);
      let wy = 16u * u32(radius + 1 - abs(dy));
      wa_col += wy;
      let alpha_weight = wy * (c.a + (c.a >> 7u));
      wc_col += alpha_weight;
      col += (alpha_weight >> 8u) * c;
    }
    wa_sum += wx * wa_col;
    wc_sum += wx * wc_col;
    sums += wx * col;
  }
  wc_sum = wc_sum >> 8u;
  var result = vec4<u32>(0u);
  if wa_sum > 0u && wc_sum > 0u {
    result = vec4<u32>(sums.r / wc_sum, sums.g / wc_sum, sums.b / wc_sum, sums.a / wa_sum);
  }
  textureStore(output_tex, vec2<i32>(id.xy), result);
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

@compute @workgroup_size(16, 16)
fn effect_fragment(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let offsets = array<vec2<i32>, 4>(vec2<i32>(8,0), vec2<i32>(0,-8), vec2<i32>(-8,0), vec2<i32>(0,8));
  var count = 0u;
  var alpha = 0u;
  var rgb = vec3<u32>(0u);
  for (var j = 0u; j < 4u; j++) {
    let uv = vec2<i32>(id.xy) - offsets[j];
    if uv.x < 0 || uv.y < 0 || uv.x >= i32(p.width) || uv.y >= i32(p.height) { continue; }
    let c = textureLoad(input_tex, uv, 0);
    count += 1u;
    alpha += c.a;
    rgb += c.rgb * c.a;
  }
  var result = vec4<u32>(0u);
  if count == 0u { result = vec4<u32>(255u,255u,255u,0u); }
  else if alpha > 0u { result = vec4<u32>(rgb / alpha, alpha / count); }
  textureStore(output_tex, vec2<i32>(id.xy), result);
}

@compute @workgroup_size(16, 16)
fn effect_zoom(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let cx = i32(p.width) << 15;
  let cy = i32(p.height) << 15;
  var fx = (i32(id.x) << 16) - cx;
  var fy = (i32(id.y) << 16) - cy;
  var count = 1u;
  let origin = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  var alpha = origin.a;
  var rgb = origin.rgb * origin.a;
  for (var j = 0; j < 64; j++) {
    fx -= ((fx >> 4) * 10) >> 10;
    fy -= ((fy >> 4) * 10) >> 10;
    let u = (fx + cx + 32768) >> 16;
    let v = (fy + cy + 32768) >> 16;
    if u < 0 || v < 0 || u >= i32(p.width) || v >= i32(p.height) { continue; }
    let c = textureLoad(input_tex, vec2<i32>(u,v), 0);
    count += 1u;
    alpha += c.a;
    rgb += c.rgb * c.a;
  }
  var result = vec4<u32>(0u);
  if alpha > 0u { result = vec4<u32>(rgb / alpha, alpha / count); }
  textureStore(output_tex, vec2<i32>(id.xy), result);
}

fn radial_rotate(point: vec2<i32>, fr: i32) -> vec2<i32> {
  let quadratic = (fr * fr) >> 11;
  let nx = point.x - (((point.y >> 8) * fr) >> 8) - (((point.x >> 14) * quadratic) >> 8);
  let ny = point.y + (((point.x >> 8) * fr) >> 8) - (((point.y >> 14) * quadratic) >> 8);
  return vec2<i32>(nx, ny);
}
@compute @workgroup_size(16, 16)
fn effect_radial(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let cx = i32(p.width) << 15;
  let cy = i32(p.height) << 15;
  let initial = vec2<i32>((i32(id.x) << 16) - cx, (i32(id.y) << 16) - cy);
  var plus = initial;
  var minus = initial;
  let origin = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  var alpha = origin.a;
  var rgb = origin.rgb * origin.a;
  var count = 1u;
  for (var j = 0; j < 136; j++) {
    plus = radial_rotate(plus, 16);
    minus = radial_rotate(minus, -16);
    for (var side = 0; side < 2; side++) {
      let point = select(minus, plus, side == 0);
      let u = (point.x + cx + 32768) >> 16;
      let v = (point.y + cy + 32768) >> 16;
      if u <= 0 || v <= 0 || u >= i32(p.width) || v >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, vec2<i32>(u,v), 0);
      count += 1u;
      alpha += c.a;
      rgb += c.rgb * c.a;
    }
  }
  var result = vec4<u32>(0u);
  if alpha > 0u { result = vec4<u32>(rgb / alpha, alpha / count); }
  textureStore(output_tex, vec2<i32>(id.xy), result);
}

@compute @workgroup_size(16, 16)
fn effect_median(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  var hist: array<vec4<u32>, 256>;
  var area = 0u;
  for (var dy = -10; dy <= 10; dy++) {
    for (var dx = -10; dx <= 10; dx++) {
      let uv = vec2<i32>(id.xy) + vec2<i32>(dx,dy);
      if uv.x < 0 || uv.y < 0 || uv.x >= i32(p.width) || uv.y >= i32(p.height) || dx*dx+dy*dy > 110 { continue; }
      let c = textureLoad(input_tex, uv, 0);
      hist[c.r].r += 1u;
      hist[c.g].g += 1u;
      hist[c.b].b += 1u;
      hist[c.a].a += 1u;
      area += 1u;
    }
  }
  let rank = area / 2u;
  var result = vec4<u32>(0u);
  for (var channel = 0u; channel < 4u; channel++) {
    var value = 0u;
    while value < 255u && hist[value][channel] == 0u { value += 1u; }
    var count = 0u;
    while value < 255u && count < rank { count += hist[value][channel]; value += 1u; }
    result[channel] = value;
  }
  textureStore(output_tex, vec2<i32>(id.xy), result);
}

fn motion_bilinear(pt: vec2<f32>) -> vec4<u32> {
  let ix = i32(floor(pt.x));
  let iy = i32(floor(pt.y));
  let fx = u32(256.0 * (pt.x - f32(ix)));
  let fy = u32(256.0 * (pt.y - f32(iy)));
  let xx = min(ix + 1, i32(p.width) - 1);
  let yy = min(iy + 1, i32(p.height) - 1);
  let c1 = textureLoad(input_tex, vec2<i32>(ix,iy), 0);
  let c2 = textureLoad(input_tex, vec2<i32>(xx,iy), 0);
  let c3 = textureLoad(input_tex, vec2<i32>(ix,yy), 0);
  let c4 = textureLoad(input_tex, vec2<i32>(xx,yy), 0);
  let w1 = (256u-fx)*(256u-fy);
  let w2 = fx*(256u-fy);
  let w3 = (256u-fx)*fy;
  let w4 = fx*fy;
  let af = c1.a*w1 + c2.a*w2 + c3.a*w3 + c4.a*w4;
  if af == 0u { return vec4<u32>(0u); }
  let rgb = (c1.rgb*c1.a*w1 + c2.rgb*c2.a*w2 + c3.rgb*c3.a*w3 + c4.rgb*c4.a*w4) / af;
  return vec4<u32>(rgb, (af+32768u)>>16u);
}
@compute @workgroup_size(16, 16)
fn effect_motion(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let start = vec2<f32>(4.531538963317871, -2.113091230392456);
  let end = -start;
  var count = 0u;
  var alpha = 0u;
  var rgb = vec3<u32>(0u);
  for (var j = 0; j < 16; j++) {
    let fraction = f32(j) / 15.0;
    let pt = vec2<f32>(id.xy) + start + fraction * (end - start);
    if pt.x < 0.0 || pt.y < 0.0 || pt.x > f32(p.width - 1u) || pt.y > f32(p.height - 1u) { continue; }
    let c = motion_bilinear(pt);
    count += 1u;
    alpha += c.a;
    rgb += c.rgb*c.a;
  }
  var result = vec4<u32>(0u);
  if count == 0u { result = vec4<u32>(255u,255u,255u,0u); }
  else if alpha > 0u { result = vec4<u32>(rgb/alpha,alpha/count); }
  textureStore(output_tex, vec2<i32>(id.xy), result);
}
@compute @workgroup_size(16, 16)
fn effect_pixelate(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let size = max(1u, p.mode);
  let left = (id.x / size) * size;
  let top = (id.y / size) * size;
  let right = min(left + size - 1u, p.width - 1u);
  let bottom = min(top + size - 1u, p.height - 1u);
  let a = textureLoad(input_tex, vec2<i32>(i32(left), i32(top)), 0);
  let b = textureLoad(input_tex, vec2<i32>(i32(right), i32(top)), 0);
  let c = textureLoad(input_tex, vec2<i32>(i32(left), i32(bottom)), 0);
  let d = textureLoad(input_tex, vec2<i32>(i32(right), i32(bottom)), 0);
  let alpha = a.a + b.a + c.a + d.a;
  var out = vec4<u32>(0u);
  if alpha > 0u {
    out = vec4<u32>((a.rgb * a.a + b.rgb * b.a + c.rgb * c.a + d.rgb * d.a) / alpha, alpha / 4u);
  }
  textureStore(output_tex, vec2<i32>(id.xy), out);
}
fn bilinear_clamp(x: f32, y: f32) -> vec4<u32> {
  let sx = clamp(x, 0.0, f32(p.width - 1u));
  let sy = clamp(y, 0.0, f32(p.height - 1u));
  let ix = i32(floor(sx));
  let iy = i32(floor(sy));
  let fx = u32(256.0 * (sx - f32(ix)));
  let fy = u32(256.0 * (sy - f32(iy)));
  let x1 = min(ix + 1, i32(p.width) - 1);
  let y1 = min(iy + 1, i32(p.height) - 1);
  let a = textureLoad(input_tex, vec2<i32>(ix, iy), 0);
  let b = textureLoad(input_tex, vec2<i32>(x1, iy), 0);
  let c = textureLoad(input_tex, vec2<i32>(ix, y1), 0);
  let d = textureLoad(input_tex, vec2<i32>(x1, y1), 0);
  let wa = (256u - fx) * (256u - fy);
  let wb = fx * (256u - fy);
  let wc = (256u - fx) * fy;
  let wd = fx * fy;
  let alpha = a.a * wa + b.a * wb + c.a * wc + d.a * wd;
  if (alpha + 32768u) >> 16u == 0u { return vec4<u32>(0u); }
  let rgb = (a.rgb * a.a * wa + b.rgb * b.a * wb + c.rgb * c.a * wc + d.rgb * d.a * wd) / alpha;
  return vec4<u32>(rgb, (alpha + 32768u) >> 16u);
}
@compute @workgroup_size(16, 16)
fn effect_bulge(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let cx = f32(p.width) * 0.5;
  let cy = f32(p.height) * 0.5;
  let radius = min(cx, cy);
  if radius <= 0.0 { textureStore(output_tex, vec2<i32>(id.xy), textureLoad(input_tex, vec2<i32>(id.xy), 0)); return; }
  let u = f32(id.x) - cx;
  let v = f32(id.y) - cy;
  let t = 1.0 - sqrt(u * u + v * v) / radius;
  if t <= 0.0 { textureStore(output_tex, vec2<i32>(id.xy), textureLoad(input_tex, vec2<i32>(id.xy), 0)); return; }
  let scale = 1.0 - (f32(bitcast<i32>(p.mode)) / 100.0) * t * t;
  textureStore(output_tex, vec2<i32>(id.xy), bilinear_clamp(u * scale + cx, v * scale + cy));
}
fn blend4(a: vec4<u32>, b: vec4<u32>, c: vec4<u32>, d: vec4<u32>) -> vec4<u32> {
  let alpha = a.a + b.a + c.a + d.a;
  if alpha == 0u { return vec4<u32>(0u); }
  return vec4<u32>((a.rgb * a.a + b.rgb * b.a + c.rgb * c.a + d.rgb * d.a) / alpha, alpha / 4u);
}
@compute @workgroup_size(16, 16)
fn effect_twist(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let cx = f32(p.width) * 0.5;
  let cy = f32(p.height) * 0.5;
  let inv_radius = 2.0 / min(f32(p.width), f32(p.height));
  var samples: array<vec4<u32>, 4>;
  for (var i = 0; i < 4; i++) {
    let oy = f32(i + 1) / 5.0 - 0.5;
    let ox0 = f32(i + 1) * 2.0 / 5.0;
    let ox = ox0 - floor(ox0) - 0.5;
    let u = f32(id.x) - cx + ox;
    let v = f32(id.y) - cy + oy;
    let r = sqrt(u * u + v * v);
    var theta = atan2(v, u);
    let t = max(0.0, 1.0 - r * inv_radius);
    theta += -9.0 * t * t * t;
    samples[i] = bilinear_clamp(cx + r * cos(theta), cy + r * sin(theta));
  }
  textureStore(output_tex, vec2<i32>(id.xy), blend4(samples[0], samples[1], samples[2], samples[3]));
}
