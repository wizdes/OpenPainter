struct Params { width: u32, height: u32, mode: u32, opacity: u32, masked: u32 }
@group(0) @binding(0) var input_tex: texture_2d<u32>;
@group(0) @binding(1) var<storage, read_write> acc: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var output_tex: texture_storage_2d<rgba8uint, write>;
@group(0) @binding(4) var coverage_tex: texture_2d<u32>;

fn stylize_3x3(pos: vec2<i32>, kind: u32) -> vec4<u32> {
  var rgb = vec3<f32>(0.0);
  var gray = 0.0;
  for (var fy=0; fy<3; fy++) {
    for (var fx=0; fx<3; fx++) {
      let xy = pos + vec2<i32>(fx-1, fy-1);
      if xy.x < 0 || xy.y < 0 || xy.x >= i32(p.width) || xy.y >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, xy, 0);
      let w = acc[fy*3+fx].x;
      if kind == 1u {
        let intensity = (7471u*c.b + 38470u*c.g + 19595u*c.r) >> 16u;
        gray += w * f32(intensity);
      } else { rgb += w * vec3<f32>(c.rgb); }
    }
  }
  if kind == 1u {
    let value = u32(clamp(i32(gray)+128,0,255));
    return vec4<u32>(value,value,value,255u);
  }
  return vec4<u32>(vec3<u32>(clamp(vec3<i32>(rgb),vec3<i32>(0),vec3<i32>(255))),255u);
}
@compute @workgroup_size(16, 16)
fn effect_edge_detect(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  textureStore(output_tex,vec2<i32>(id.xy),stylize_3x3(vec2<i32>(id.xy),0u));
}
@compute @workgroup_size(16, 16)
fn effect_emboss(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  textureStore(output_tex,vec2<i32>(id.xy),stylize_3x3(vec2<i32>(id.xy),1u));
}
@compute @workgroup_size(16, 16)
fn effect_relief(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  textureStore(output_tex,vec2<i32>(id.xy),stylize_3x3(vec2<i32>(id.xy),2u));
}
@compute @workgroup_size(16, 16)
fn effect_oil_painting(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  var counts: array<u32,256>;
  var sums: array<vec4<u32>,256>;
  let radius = i32(p.mode);
  let coarse = p.opacity;
  for (var dy = -radius; dy <= radius; dy++) {
    let sy = i32(id.y) + dy;
    if sy < 0 || sy >= i32(p.height) { continue; }
    for (var dx = -radius; dx <= radius; dx++) {
      let sx = i32(id.x) + dx;
      if sx < 0 || sx >= i32(p.width) { continue; }
      let color = textureLoad(input_tex,vec2<i32>(sx,sy),0);
      let intensity = (7471u*color.b + 38470u*color.g + 19595u*color.r) >> 16u;
      let n = intensity * coarse + 0x80u;
      let bin = ((n >> 8u) + n) >> 8u;
      counts[bin] += 1u;
      sums[bin] += color;
    }
  }
  var chosen = 0u;
  var best = 0u;
  for (var bin=0u; bin<=coarse; bin++) {
    if counts[bin] > best { chosen=bin; best=counts[bin]; }
  }
  textureStore(output_tex,vec2<i32>(id.xy),sums[chosen]/max(best,1u));
}
@compute @workgroup_size(16, 16)
fn effect_outline(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  var hist: array<array<u32,256>,4>;
  let radius = i32(p.mode);
  let intensity = i32(p.opacity);
  let cutoff = ((radius*2+1)*(radius*2+1)+2)/4;
  var area = 0;
  for (var dy=-radius; dy<=radius; dy++) {
    let sy = i32(id.y)+dy;
    if sy<0 || sy>=i32(p.height) { continue; }
    for (var dx=-radius; dx<=radius; dx++) {
      let sx = i32(id.x)+dx;
      if sx<0 || sx>=i32(p.width) || dx*dx+dy*dy>cutoff { continue; }
      let color = textureLoad(input_tex,vec2<i32>(sx,sy),0);
      hist[0][color.r] += 1u;
      hist[1][color.g] += 1u;
      hist[2][color.b] += 1u;
      hist[3][color.a] += 1u;
      area += 1;
    }
  }
  let low = area*(100-intensity)/200;
  let high = area*(100+intensity)/200;
  var out = vec4<u32>(0u);
  for (var c=0; c<4; c++) {
    var first = 0;
    let skip_channel = select(c,2,c==3);
    while first<255 && hist[skip_channel][first]==0u { first+=1; }
    var count = 0;
    while first<255 && count<low { count += i32(hist[c][first]); first+=1; }
    var last=first;
    while last<255 && count<high { count += i32(hist[c][last]); last+=1; }
    out[c] = select(u32(255-(last-first)),u32(last),c==3);
  }
  textureStore(output_tex,vec2<i32>(id.xy),out);
}
fn artistic_intensity(color: vec4<u32>) -> u32 {
  return (7471u*color.b + 38470u*color.g + 19595u*color.r) >> 16u;
}
fn legacy_scale(a: u32, b: u32) -> u32 {
  let n = a*b + 0x80u;
  return ((n >> 8u) + n) >> 8u;
}
fn legacy_blend(lhs: vec4<u32>, rhs: vec4<u32>, mode: u32) -> vec4<u32> {
  let y = legacy_scale(lhs.a, 255u-rhs.a);
  let total = y+rhs.a;
  if total == 0u { return vec4<u32>(0u); }
  let x = legacy_scale(lhs.a,rhs.a);
  let z = rhs.a-x;
  var out = vec4<u32>(0u);
  for (var c=0; c<3; c++) {
    let a = lhs[c];
    let b = rhs[c];
    var blend = min(a,b);
    if mode == 0u { blend = a+b-legacy_scale(a,b); }
    if mode == 2u {
      if b == 255u { blend=255u; } else { blend=min(255u,a*255u/(255u-b)); }
    }
    out[c] = (a*y+b*z+blend*x)/total;
  }
  out.a=total;
  return out;
}
// 64-bit whole numbers as (high, low) u32 pairs; same helpers as
// gaussian_horizontal.wgsl, which is a separate shader module.
fn wide_mul(a: u32, b: u32) -> vec2<u32> {
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
fn wide_add(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
  let lo = a.y + b.y;
  return vec2<u32>(a.x + b.x + u32(lo < a.y), lo);
}
fn wide_ge(a: vec2<u32>, b: vec2<u32>) -> bool {
  return a.x > b.x || (a.x == b.x && a.y >= b.y);
}
fn wide_shl(a: vec2<u32>, n: u32) -> vec2<u32> {
  if n == 0u { return a; }
  return vec2<u32>((a.x << n) | (a.y >> (32u - n)), a.y << n);
}
// n / d truncated and clamped to 0..255, with d > 0.
fn wide_div_byte(n: vec2<u32>, d: vec2<u32>) -> u32 {
  if wide_ge(n, wide_shl(d, 8u)) { return 255u; }
  var rem = n;
  var q = 0u;
  for (var bit = 7; bit >= 0; bit--) {
    let part = wide_shl(d, u32(bit));
    if wide_ge(rem, part) {
      rem = vec2<u32>(rem.x - part.x - u32(rem.y < part.y), rem.y - part.y);
      q |= 1u << u32(bit);
    }
  }
  return q;
}
// The CPU's i64 Gaussian (op-ops gaussian_blur.rs without prepared columns).
// Column sums fit u32 (under 255 * 16 * 201^2); the row sums need 64 bits
// from tip size 13 up.
fn legacy_gaussian(pos: vec2<i32>, radius: i32) -> vec4<u32> {
  var wa_sum = vec2<u32>(0u);
  var wc_sum = vec2<u32>(0u);
  var sums = array<vec2<u32>, 4>(vec2<u32>(0u), vec2<u32>(0u), vec2<u32>(0u), vec2<u32>(0u));
  for (var dx=-radius; dx<=radius; dx++) {
    let xx=pos.x+dx;
    if xx<0 || xx>=i32(p.width) { continue; }
    let wx=u32(16*(radius+1-abs(dx)));
    var wa_col=0u;
    var wc_col=0u;
    var col=vec4<u32>(0u);
    for (var dy=-radius; dy<=radius; dy++) {
      let yy=pos.y+dy;
      if yy<0 || yy>=i32(p.height) { continue; }
      let color=textureLoad(input_tex,vec2<i32>(xx,yy),0);
      let wy=u32(16*(radius+1-abs(dy)));
      wa_col+=wy;
      let alpha_weight=wy*(color.a+(color.a>>7u));
      wc_col+=alpha_weight;
      col+=(alpha_weight>>8u)*color;
    }
    wa_sum=wide_add(wa_sum,wide_mul(wx,wa_col));
    wc_sum=wide_add(wc_sum,wide_mul(wx,wc_col));
    for (var c=0; c<4; c++) { sums[c]=wide_add(sums[c],wide_mul(wx,col[c])); }
  }
  let wc=vec2<u32>(wc_sum.x>>8u,(wc_sum.y>>8u)|(wc_sum.x<<24u));
  if (wa_sum.x==0u && wa_sum.y==0u) || (wc.x==0u && wc.y==0u) { return vec4<u32>(0u); }
  return vec4<u32>(
    wide_div_byte(sums[0],wc),
    wide_div_byte(sums[1],wc),
    wide_div_byte(sums[2],wc),
    wide_div_byte(sums[3],wa_sum));
}
fn legacy_bc(pixel: vec4<u32>, brightness: i32, contrast: i32) -> vec4<u32> {
  let i=i32(artistic_intensity(pixel));
  var multiply=1;
  var divide=1;
  if contrast<0 { multiply=contrast+100; divide=100; }
  if contrast>0 { multiply=100; divide=100-contrast; }
  var out=pixel;
  if divide==0 {
    let value=select(0u,255u,i+brightness>=128);
    out=vec4<u32>(vec3<u32>(value),pixel.a);
  } else {
    var shift=0;
    if divide==100 { shift=(i-127)*multiply/divide+127-i+brightness; }
    else { shift=(i-127+brightness)*multiply/divide+127-i; }
    out=vec4<u32>(vec3<u32>(clamp(vec3<i32>(pixel.rgb)+vec3<i32>(shift),vec3<i32>(0),vec3<i32>(255))),pixel.a);
  }
  return out;
}
@compute @workgroup_size(16, 16)
fn effect_pencil_sketch(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x>=p.width || id.y>=p.height { return; }
  let pos=vec2<i32>(id.xy);
  let original=textureLoad(input_tex,pos,0);
  let blurred=legacy_gaussian(pos,i32(p.mode));
  let range=bitcast<i32>(p.opacity);
  let adjusted=legacy_bc(blurred,range,-range);
  let inverted=vec4<u32>(vec3<u32>(255u)-adjusted.rgb,adjusted.a);
  let gray=artistic_intensity(inverted);
  let source_gray=artistic_intensity(original);
  let result=legacy_blend(vec4<u32>(vec3<u32>(source_gray),original.a),
    vec4<u32>(vec3<u32>(gray),adjusted.a),2u);
  textureStore(output_tex,pos,result);
}
@compute @workgroup_size(16, 16)
fn effect_ink_sketch(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x>=p.width || id.y>=p.height { return; }
  let pos=vec2<i32>(id.xy);
  let original=textureLoad(input_tex,pos,0);
  let blurred=legacy_gaussian(pos,6);
  let coloring=i32(p.opacity);
  let adjust=-(coloring-50)*2;
  let adjusted=legacy_bc(blurred,adjust,adjust);
  let background=legacy_blend(original,adjusted,0u);
  var sums=vec3<i32>(0);
  for (var fy=0; fy<5; fy++) {
    let sy=pos.y+fy-2;
    if sy<0 || sy>=i32(p.height) { continue; }
    for (var fx=0; fx<5; fx++) {
      let sx=pos.x+fx-2;
      if sx<0 || sx>=i32(p.width) { continue; }
      var weight=-1;
      if fx==2 && fy==2 { weight=30; }
      if fx==2 && fy==4 { weight=-5; }
      sums+=vec3<i32>(textureLoad(input_tex,vec2<i32>(sx,sy),0).rgb)*weight;
    }
  }
  let rgb=vec3<u32>(clamp(sums,vec3<i32>(0),vec3<i32>(255)));
  let line=select(0u,255u,i32(artistic_intensity(vec4<u32>(rgb,255u)))>i32(p.mode)*255/100);
  let result=legacy_blend(vec4<u32>(vec3<u32>(line),255u),background,1u);
  textureStore(output_tex,pos,result);
}
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
fn effect_red_eye_removal(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let hi = max(src.r, max(src.g, src.b));
  let lo = min(src.r, min(src.g, src.b));
  let saturation = select(0u, (hi - lo) * 255u / max(hi, 1u), hi > 0u);
  var red = src.r;
  if i32(src.r) - i32(max(src.g, src.b)) > i32(p.mode) && saturation > 100u {
    let scaled = (299u * src.r + 587u * src.g + 114u * src.b) * p.opacity;
    red = scaled / 100000u;
    // The CPU truncates an f64 product that can sit just under a whole number;
    // acc holds those colours for this saturation, sorted (see lib.rs).
    if scaled % 100000u == 0u && red > 0u {
      let key = f32((src.r << 16u) | (src.g << 8u) | src.b);
      var lo = 0u;
      var hi = arrayLength(&acc);
      while lo < hi {
        let mid = (lo + hi) / 2u;
        if acc[mid].x < key { lo = mid + 1u; } else { hi = mid; }
      }
      if acc[lo].x == key { red = red - 1u; }
    }
  }
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(red, src.g, src.b, src.a));
}
@compute @workgroup_size(16, 16)
fn effect_add_noise(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let delta = vec3<i32>(acc[id.y * p.width + id.x].rgb);
  let rgb = vec3<u32>(clamp(vec3<i32>(src.rgb) + delta, vec3<i32>(0), vec3<i32>(255)));
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, src.a));
}
@compute @workgroup_size(16, 16)
fn effect_clouds(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let gray = u32(acc[id.y * p.width + id.x].x);
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(gray, gray, gray, 255u));
}
fn turbulence_hash(x: i32, y: i32, channel: u32, seed: u32) -> i32 {
  var n = u32(x) * 374761393u ^ u32(y) * 668265263u ^ channel * 1442695041u ^ seed;
  n = (n ^ (n >> 13u)) * 1274126177u;
  return i32((n ^ (n >> 16u)) >> 17u) - 16384;
}
fn turbulence_smooth(fraction: i32) -> i32 {
  let f = u32(fraction);
  let square = (f * f + 32768u) >> 16u;
  let cube = (square * f + 32768u) >> 16u;
  return i32(3u * square - 2u * cube);
}
fn turbulence_lerp(a: i32, b: i32, fraction: i32) -> i32 {
  return a + ((b - a) * fraction + 32768) / 65536;
}
fn turbulence_noise(x: i32, y: i32, frequency: i32, period: i32, channel: u32, seed: u32) -> i32 {
  let sx = x * frequency;
  let sy = y * frequency;
  let gx = sx / period;
  let gy = sy / period;
  let fx = turbulence_smooth((sx % period) * 65536 / period);
  let fy = turbulence_smooth((sy % period) * 65536 / period);
  let upper = turbulence_lerp(turbulence_hash(gx, gy, channel, seed), turbulence_hash(gx+1, gy, channel, seed), fx);
  let lower = turbulence_lerp(turbulence_hash(gx, gy+1, channel, seed), turbulence_hash(gx+1, gy+1, channel, seed), fx);
  return turbulence_lerp(upper, lower, fy);
}
@compute @workgroup_size(16, 16)
fn effect_turbulence(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let octaves = i32(p.mode & 15u);
  let period = i32((p.mode >> 4u) & 1023u);
  let size = i32((p.mode >> 14u) & 127u);
  let turbulence = (p.mode & (1u << 21u)) != 0u;
  let blend = (p.mode & (1u << 22u)) != 0u;
  var out = vec4<u32>(0u, 0u, 0u, 255u);
  for (var channel = 0u; channel < 3u; channel++) {
    var sum = 0;
    var weight = 0;
    var amplitude = 256;
    var frequency = 1;
    for (var octave = 0; octave < octaves; octave++) {
      let value = turbulence_noise(i32(id.x), i32(id.y), frequency, period, channel, p.opacity);
      sum += select(value, abs(value), turbulence) * amplitude;
      weight += amplitude;
      frequency *= 2;
      amplitude = amplitude * size / 100;
    }
    let avg = sum / weight;
    var value = 0;
    if turbulence { value = (avg * 255 + 8192) / 16384; }
    else { value = ((avg + 16384) * 255 + 16384) / 32768; }
    out[channel] = u32(clamp(value, 0, 255));
  }
  if blend {
    let original = textureLoad(input_tex, vec2<i32>(id.xy), 0);
    out = vec4<u32>((out.rgb + original.rgb) / vec3<u32>(2u), original.a);
  }
  textureStore(output_tex, vec2<i32>(id.xy), out);
}
fn photo_scale(a: i32, b: i32) -> i32 {
  let value = a * b + 128;
  return (value + (value >> 8)) >> 8;
}
fn photo_blend(lhs: vec4<u32>, rhs: vec4<u32>, overlay: bool) -> vec4<u32> {
  let lhs_a = i32(lhs.a);
  let rhs_a = i32(rhs.a);
  let y = photo_scale(lhs_a, 255 - rhs_a);
  let total_a = y + rhs_a;
  if total_a == 0 { return vec4<u32>(0u); }
  let x = photo_scale(lhs_a, rhs_a);
  let z = rhs_a - x;
  var out = vec4<u32>(0u);
  for (var channel = 0u; channel < 3u; channel++) {
    let a = i32(lhs[channel]);
    let b = i32(rhs[channel]);
    var blended = a + b - photo_scale(a, b);
    if overlay {
      if a < 128 { blended = photo_scale(2 * a, b); }
      else { blended = 255 - photo_scale(2 * (255 - a), 255 - b); }
    }
    out[channel] = u32((a * y + b * z + blended * x) / total_a);
  }
  out.a = u32(total_a);
  return out;
}
@compute @workgroup_size(16, 16)
fn effect_photo_glow_blend(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let adjusted = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let original = vec4<u32>(acc[id.y * p.width + id.x]);
  textureStore(output_tex, vec2<i32>(id.xy), photo_blend(adjusted, original, false));
}
@compute @workgroup_size(16, 16)
fn effect_photo_soften_blend(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let adjusted = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let original = vec4<u32>(acc[id.y * p.width + id.x]);
  let gray = u32(legacy_intensity(original));
  // The CPU's f32 steps with exact rounding (sf_* below): Metal's divide is
  // approximate, so gray * 1.19 could truncate one level low.
  let warm = sf_div(sf(f32(p.mode)), sf(100.0));
  let warm_red = bitcast<f32>(sf_mul(sf(f32(gray)), sf_add(sf(1.0), warm)));
  let warm_blue = bitcast<f32>(sf_mul(sf(f32(gray)), sf_add(sf(1.0), sf_neg(warm))));
  let lit = vec4<u32>(u32(clamp(warm_red, 0.0, 255.0)), gray,
    u32(clamp(warm_blue, 0.0, 255.0)), original.a);
  textureStore(output_tex, vec2<i32>(id.xy), photo_blend(lit, adjusted, true));
}
@compute @workgroup_size(16, 16)
fn effect_vignette(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let strength = f32(p.masked & 65535u) / 100.0;
  if strength == 0.0 { textureStore(output_tex, vec2<i32>(id.xy), src); return; }
  let radius = f32((p.masked >> 16u) & 65535u) / 100.0;
  let cx = (f32(bitcast<i32>(p.mode)) / 1000.0 + 1.0) * 0.5;
  let cy = (f32(bitcast<i32>(p.opacity)) / 1000.0 + 1.0) * 0.5;
  let nx = ((f32(id.x) + 0.5) / f32(p.width) - cx) * 2.0;
  let ny = ((f32(id.y) + 0.5) / f32(p.height) - cy) * 2.0;
  let distance = sqrt(nx * nx + ny * ny);
  let edge = clamp((distance - radius) / max(1.0 - radius, 0.001), 0.0, 1.0);
  let falloff = edge * edge * (3.0 - 2.0 * edge);
  let factor = 1.0 - falloff * strength;
  let rgb = vec3<u32>(round(vec3<f32>(src.rgb) * factor));
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, src.a));
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

fn m48_div_u64(hi: u32, lo: u32, divisor: u32) -> u32 {
  var left = 0u;
  var right = 256u;
  while left + 1u < right {
    let guess = (left + right) / 2u;
    let low_part = (divisor & 65535u) * guess;
    let upper_part = (divisor >> 16u) * guess;
    let shifted = upper_part << 16u;
    let product_lo = shifted + low_part;
    let product_hi = (upper_part >> 16u) + select(0u, 1u, product_lo < shifted);
    if product_hi < hi || (product_hi == hi && product_lo <= lo) {
      left = guess;
    } else {
      right = guess;
    }
  }
  return left;
}

fn m48_mean(pos: vec2<i32>, circular: bool) -> vec4<u32> {
  let radius = i32(p.mode);
  if radius == 0 { return textureLoad(input_tex, pos, 0); }
  let boost = bitcast<i32>(p.opacity);
  let exponent = select(1.0 / (1.0 - f32(boost) / 50.0), 1.0 + f32(boost) / 50.0, boost >= 0);
  let stride = select(1, max(1, 11 - i32(p.masked)), circular);
  var sum = vec3<u32>(0u);
  var sum_hi = vec3<u32>(0u);
  var corrected = vec3<f32>(0.0);
  var alpha = 0u;
  var area = 0u;
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      if circular && (dx * dx + dy * dy > radius * radius || ((dx % stride + stride) % stride) != 0 || ((dy % stride + stride) % stride) != 0) { continue; }
      let xy = pos + vec2<i32>(dx, dy);
      if xy.x < 0 || xy.y < 0 || xy.x >= i32(p.width) || xy.y >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, xy, 0);
      area += 1u;
      alpha += c.a;
      let old = sum;
      sum += c.rgb * c.a;
      sum_hi += vec3<u32>(sum < old);
      if boost != 0 { corrected += pow(vec3<f32>(c.rgb) / 255.0, vec3<f32>(exponent)) * f32(c.a); }
    }
  }
  if area == 0u { return textureLoad(input_tex, pos, 0); }
  if alpha == 0u { return vec4<u32>(0u); }
  if boost == 0 {
    var rounded = sum + vec3<u32>(alpha / 2u);
    let rounded_hi = sum_hi + vec3<u32>(rounded < sum);
    return vec4<u32>(m48_div_u64(rounded_hi.r, rounded.r, alpha),
      m48_div_u64(rounded_hi.g, rounded.g, alpha),
      m48_div_u64(rounded_hi.b, rounded.b, alpha), alpha / area);
  }
  let color = pow(corrected / f32(alpha), vec3<f32>(1.0 / exponent)) * 255.0;
  return vec4<u32>(vec3<u32>(clamp(color + 0.5, vec3<f32>(0.0), vec3<f32>(255.0))), alpha / area);
}
@compute @workgroup_size(16, 16)
fn effect_bokeh(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  textureStore(output_tex, vec2<i32>(id.xy), m48_mean(vec2<i32>(id.xy), true));
}
@compute @workgroup_size(16, 16)
fn effect_square(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  textureStore(output_tex, vec2<i32>(id.xy), m48_mean(vec2<i32>(id.xy), false));
}
@compute @workgroup_size(16, 16)
fn effect_sketch(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let pos = vec2<i32>(id.xy);
  let radius = i32(p.mode);
  if radius == 0 { textureStore(output_tex, pos, textureLoad(input_tex, pos, 0)); return; }
  let stride = clamp((11 - i32(p.masked)) / 3, 1, 3);
  var hist: array<u32, 256>;
  var area = 0u;
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      let xy = pos + vec2<i32>(dx, dy);
      if dx * dx + dy * dy > radius * radius || ((dx % stride + stride) % stride) != 0 || ((dy % stride + stride) % stride) != 0 || xy.x < 0 || xy.y < 0 || xy.x >= i32(p.width) || xy.y >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, xy, 0);
      let lum = (19595u * c.r + 38470u * c.g + 7471u * c.b) >> 16u;
      hist[lum] += 1u;
      area += 1u;
    }
  }
  let rank = (area - 1u) * p.opacity / 100u;
  var cumulative = 0u;
  var chosen = 0u;
  for (var bin = 0u; bin < 256u; bin++) {
    if cumulative + hist[bin] > rank { chosen = bin; break; }
    cumulative += hist[bin];
  }
  var remaining = rank - cumulative;
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      let xy = pos + vec2<i32>(dx, dy);
      if dx * dx + dy * dy > radius * radius || ((dx % stride + stride) % stride) != 0 || ((dy % stride + stride) % stride) != 0 || xy.x < 0 || xy.y < 0 || xy.x >= i32(p.width) || xy.y >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, xy, 0);
      let lum = (19595u * c.r + 38470u * c.g + 7471u * c.b) >> 16u;
      if lum != chosen { continue; }
      if remaining == 0u { textureStore(output_tex, pos, c); return; }
      remaining -= 1u;
    }
  }
}
@compute @workgroup_size(16, 16)
fn effect_unfocus(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let radius = i32(p.mode);
  let cutoff = ((2 * radius + 1) * (2 * radius + 1) + 2) / 4;
  var sum = vec3<u32>(0u);
  var sum_hi = vec3<u32>(0u);
  var alpha = 0u;
  var area = 0u;
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      let xy = vec2<i32>(id.xy) + vec2<i32>(dx, dy);
      if dx * dx + dy * dy > cutoff || xy.x < 0 || xy.y < 0 || xy.x >= i32(p.width) || xy.y >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, xy, 0);
      area += 1u;
      alpha += c.a;
      let old = sum;
      sum += c.rgb * c.a;
      sum_hi += vec3<u32>(sum < old);
    }
  }
  let divisor = area * 255u;
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(
    m48_div_u64(sum_hi.r, sum.r, divisor), m48_div_u64(sum_hi.g, sum.g, divisor),
    m48_div_u64(sum_hi.b, sum.b, divisor), alpha / area));
}
@compute @workgroup_size(16, 16)
fn effect_surface(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let pos = vec2<i32>(id.xy);
  let center = textureLoad(input_tex, pos, 0);
  let radius = i32(p.mode);
  let threshold = i32(p.opacity);
  let cutoff = ((2 * radius + 1) * (2 * radius + 1) + 2) / 4;
  var sum = vec3<u32>(0u);
  var divisor = vec3<u32>(0u);
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      let xy = pos + vec2<i32>(dx, dy);
      if dx * dx + dy * dy > cutoff || xy.x < 0 || xy.y < 0 || xy.x >= i32(p.width) || xy.y >= i32(p.height) { continue; }
      let c = textureLoad(input_tex, xy, 0);
      let diff = abs(vec3<i32>(c.rgb) - vec3<i32>(center.rgb));
      let weight = vec3<u32>(max(vec3<i32>(0), vec3<i32>(255) - (diff * 96 + vec3<i32>(threshold / 2)) / threshold));
      sum += c.rgb * weight;
      divisor += weight;
    }
  }
  let color = (sum + divisor / vec3<u32>(2u)) / max(divisor, vec3<u32>(1u));
  textureStore(output_tex, pos, vec4<u32>(color, center.a));
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

// Median with any radius and percentile, as op-ops median_blur.rs. Its
// histogram walk returns min(255, k + 1), where k is the m-th smallest sample
// (m = area * percentile / 100), or the smallest sample when m is 0. Each
// channel's k is found one bit at a time by counting samples below a trial
// value, so no per-thread histogram is needed.
@compute @workgroup_size(16, 16)
fn effect_median(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let r = i32(p.mode);
  let x = i32(id.x);
  let y = i32(id.y);
  let cutoff = ((2 * r + 1) * (2 * r + 1) + 2) / 4;
  let y0 = max(-r, -y);
  let y1 = min(r, i32(p.height) - 1 - y);
  var area = 0u;
  for (var dy = y0; dy <= y1; dy++) {
    let half = median_half_width(cutoff - dy * dy, r);
    area += u32(max(min(half, i32(p.width) - 1 - x) - max(-half, -x) + 1, 0));
  }
  let m = area * p.opacity / 100u;
  let target_rank = max(m, 1u);
  var k = vec4<u32>(0u);
  for (var bit = 7; bit >= 0; bit--) {
    let trial = k | vec4<u32>(1u << u32(bit));
    var below = vec4<u32>(0u);
    for (var dy = y0; dy <= y1; dy++) {
      let half = median_half_width(cutoff - dy * dy, r);
      let x0 = max(-half, -x);
      let x1 = min(half, i32(p.width) - 1 - x);
      for (var dx = x0; dx <= x1; dx++) {
        let c = textureLoad(input_tex, vec2<i32>(x + dx, y + dy), 0);
        below += vec4<u32>(c < trial);
      }
    }
    k = select(k, trial, below < vec4<u32>(target_rank));
  }
  var result = k;
  if m > 0u { result = min(k + vec4<u32>(1u), vec4<u32>(255u)); }
  textureStore(output_tex, vec2<i32>(id.xy), result);
}
// Largest dx with dx * dx <= limit, at most r; -1 when limit < 0.
fn median_half_width(limit: i32, r: i32) -> i32 {
  if limit < 0 { return -1; }
  var half = min(i32(sqrt(f32(limit))), r);
  while half * half > limit { half -= 1; }
  while half < r && (half + 1) * (half + 1) <= limit { half += 1; }
  return half;
}
@compute @workgroup_size(16, 16)
fn effect_sharpen(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let radius = i32(p.mode);
  let diameter = 2 * radius + 1;
  let cutoff = (diameter * diameter + 2) / 4;
  var hist: array<vec4<u32>, 256>;
  var area = 0u;
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      let uv = vec2<i32>(id.xy) + vec2<i32>(dx,dy);
      if uv.x < 0 || uv.y < 0 || uv.x >= i32(p.width) || uv.y >= i32(p.height) || dx*dx+dy*dy > cutoff { continue; }
      let c = textureLoad(input_tex, uv, 0);
      hist[c.r].r += 1u;
      hist[c.g].g += 1u;
      hist[c.b].b += 1u;
      hist[c.a].a += 1u;
      area += 1u;
    }
  }
  let rank = area / 2u;
  var median = vec4<u32>(0u);
  for (var channel = 0u; channel < 4u; channel++) {
    var value = 0u;
    while value < 255u && hist[value][channel] == 0u { value += 1u; }
    var count = 0u;
    while value < 255u && count < rank { count += hist[value][channel]; value += 1u; }
    median[channel] = value;
  }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let result = vec4<u32>(clamp((3 * vec4<i32>(src) - vec4<i32>(median)) / 2, vec4<i32>(0), vec4<i32>(255)));
  textureStore(output_tex, vec2<i32>(id.xy), result);
}
@compute @workgroup_size(16, 16)
fn effect_reduce_noise(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let src = textureLoad(input_tex, vec2<i32>(id.xy), 0);
  let radius = i32(p.mode);
  let diameter = 2 * radius + 1;
  let cutoff = (diameter * diameter + 2) / 4;
  var counts = vec3<u32>(0u);
  var area = 0u;
  for (var dy = -radius; dy <= radius; dy++) {
    for (var dx = -radius; dx <= radius; dx++) {
      let uv = vec2<i32>(id.xy) + vec2<i32>(dx, dy);
      if uv.x < 0 || uv.y < 0 || uv.x >= i32(p.width) || uv.y >= i32(p.height) || dx*dx+dy*dy > cutoff { continue; }
      let neighbor = textureLoad(input_tex, uv, 0);
      counts += vec3<u32>(neighbor.rgb < src.rgb);
      area += 1u;
    }
  }
  let normalized = counts * 255u / area;
  let intensity = (0.299 * f32(src.r) + 0.587 * f32(src.g) + 0.114 * f32(src.b)) / 255.0;
  let lerp = -0.2 * (f32(p.opacity) / 1000.0) * (1.0 - 0.75 * intensity);
  let rgb = vec3<u32>(clamp(vec3<f32>(src.rgb) + (vec3<f32>(normalized) - vec3<f32>(src.rgb)) * lerp, vec3<f32>(0.0), vec3<f32>(255.0)));
  let alpha = u32(clamp(f32(src.a) + (255.0 - f32(src.a)) * lerp, 0.0, 255.0));
  textureStore(output_tex, vec2<i32>(id.xy), vec4<u32>(rgb, alpha));
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
// IEEE f32 arithmetic in integers, rounded to nearest even, for finite normal
// numbers and zero (no denormals, infinities or NaN). Metal compiles WGSL
// with fast math, which may fuse a * b + c and makes divide and sqrt
// approximate; these give the bits Rust's f32 operations give.
// Rounds sign * mant * 2^pow, where `sticky` says the true value is a little
// above mant (mant > 0).
fn sf_round(sign: u32, mant: u32, pow: i32, sticky: bool) -> u32 {
  var m = mant;
  var e = pow;
  var s = sticky;
  let top = 31 - i32(countLeadingZeros(m));
  if top > 30 {
    s = s || (m & 1u) != 0u;
    m = m >> 1u;
    e = e + 1;
  } else if top < 30 {
    m = m << u32(30 - top);
    e = e - (30 - top);
  }
  // m is in [2^30, 2^31): keep 24 bits, round on the 7 below.
  var kept = m >> 7u;
  let half = (m & 64u) != 0u;
  let rest = (m & 63u) != 0u || s;
  if half && (rest || (kept & 1u) != 0u) { kept = kept + 1u; }
  e = e + 7;
  if kept == 0x1000000u {
    kept = 0x800000u;
    e = e + 1;
  }
  return (sign << 31u) | (u32(e + 150) << 23u) | (kept & 0x7fffffu);
}
fn sf_exp(a: u32) -> i32 { return i32((a >> 23u) & 255u); }
fn sf_mant(a: u32) -> u32 { return (a & 0x7fffffu) | 0x800000u; }
fn sf_mul(a: u32, b: u32) -> u32 {
  if sf_exp(a) == 0 || sf_exp(b) == 0 { return 0u; }
  let ma = sf_mant(a);
  let mb = sf_mant(b);
  // 48-bit product from 16-bit halves.
  let a0 = ma & 0xffffu;
  let a1 = ma >> 16u;
  let b0 = mb & 0xffffu;
  let b1 = mb >> 16u;
  let low = a0 * b0;
  let mid = a1 * b0 + a0 * b1;
  let lo = low + (mid << 16u);
  let carry = u32(lo < low);
  let hi = a1 * b1 + (mid >> 16u) + carry;
  let mant = (hi << 16u) | (lo >> 16u);
  let pow = sf_exp(a) - 150 + sf_exp(b) - 150 + 16;
  return sf_round((a ^ b) >> 31u, mant, pow, (lo & 0xffffu) != 0u);
}
fn sf_add(a: u32, b: u32) -> u32 {
  if sf_exp(a) == 0 { return b; }
  if sf_exp(b) == 0 { return a; }
  // x has the larger magnitude.
  var x = a;
  var y = b;
  if (b & 0x7fffffffu) > (a & 0x7fffffffu) {
    x = b;
    y = a;
  }
  let mx = sf_mant(x) << 7u;
  let shift = u32(sf_exp(x) - sf_exp(y));
  let full = sf_mant(y) << 7u;
  var my = 0u;
  var sticky = full != 0u;
  if shift < 32u {
    my = full >> shift;
    sticky = shift > 0u && (full & ((1u << shift) - 1u)) != 0u;
  }
  let pow = sf_exp(x) - 157;
  if ((x ^ y) >> 31u) == 0u { return sf_round(x >> 31u, mx + my, pow, sticky); }
  // The true value is mx - my - fraction, so take one more off and keep it sticky.
  let diff = mx - my - u32(sticky);
  if diff == 0u { return 0u; }
  return sf_round(x >> 31u, diff, pow, sticky);
}
fn sf_neg(a: u32) -> u32 { return a ^ 0x80000000u; }
fn sf_div(a: u32, b: u32) -> u32 {
  if sf_exp(a) == 0 { return 0u; }
  var ma = sf_mant(a);
  let mb = sf_mant(b);
  var ea = sf_exp(a);
  if ma < mb {
    ma = ma << 1u;
    ea = ea - 1;
  }
  // 31 quotient bits of ma / mb, which is in [1, 2).
  var rem = ma;
  var q = 0u;
  for (var i = 0; i < 31; i = i + 1) {
    q = q << 1u;
    if rem >= mb {
      rem = rem - mb;
      q = q | 1u;
    }
    rem = rem << 1u;
  }
  return sf_round((a ^ b) >> 31u, q, ea - sf_exp(b) - 30, rem != 0u);
}
fn sf_sqrt(a: u32) -> u32 {
  if sf_exp(a) == 0 { return 0u; }
  var m = sf_mant(a);
  var pow = sf_exp(a) - 150;
  if (pow & 1) != 0 {
    m = m << 1u;
    pow = pow - 1;
  }
  // Square root of m * 2^28, two radicand bits at a time.
  var rem = 0u;
  var root = 0u;
  for (var i = 26; i >= 0; i = i - 1) {
    var pair = 0u;
    if i >= 14 { pair = (m >> u32(2 * i - 28)) & 3u; }
    rem = (rem << 2u) | pair;
    let trial = (root << 2u) | 1u;
    root = root << 1u;
    if rem >= trial {
      rem = rem - trial;
      root = root | 1u;
    }
  }
  return sf_round(0u, root, pow / 2 - 14, rem != 0u);
}
fn sf(x: f32) -> u32 { return bitcast<u32>(x); }
@compute @workgroup_size(16, 16)
fn effect_bulge(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  // Same f32 steps as op-ops bulge.rs, each rounded as the CPU rounds it.
  let cx = f32(p.width) * 0.5;
  let cy = f32(p.height) * 0.5;
  let radius = min(cx, cy);
  if radius <= 0.0 { textureStore(output_tex, vec2<i32>(id.xy), textureLoad(input_tex, vec2<i32>(id.xy), 0)); return; }
  let u = sf_add(sf(f32(id.x)), sf_neg(sf(cx)));
  let v = sf_add(sf(f32(id.y)), sf_neg(sf(cy)));
  let d = sf_sqrt(sf_add(sf_mul(u, u), sf_mul(v, v)));
  let t = sf_add(sf(1.0), sf_neg(sf_div(d, sf(radius))));
  if bitcast<f32>(t) <= 0.0 { textureStore(output_tex, vec2<i32>(id.xy), textureLoad(input_tex, vec2<i32>(id.xy), 0)); return; }
  let amount = sf_div(sf(f32(bitcast<i32>(p.mode))), sf(100.0));
  let scale = sf_add(sf(1.0), sf_neg(sf_mul(sf_mul(amount, t), t)));
  let x = bitcast<f32>(sf_add(sf_mul(u, scale), sf(cx)));
  let y = bitcast<f32>(sf_add(sf_mul(v, scale), sf(cy)));
  textureStore(output_tex, vec2<i32>(id.xy), bilinear_clamp(x, y));
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

// Provisional 5.1 Morphology: channel-wise extrema in an integer rectangle.
@compute @workgroup_size(16, 16)
fn effect_morphology(@builtin(global_invocation_id) id: vec3<u32>) {
  if id.x >= p.width || id.y >= p.height { return; }
  let w = i32(p.mode);
  let h = i32(p.opacity);
  var best = select(vec4<u32>(255u),vec4<u32>(0u),p.masked == 1u);
  for (var dy = -(h/2); dy <= (h-1)/2; dy++) {
    for (var dx = -(w/2); dx <= (w-1)/2; dx++) {
      let sx = clamp(i32(id.x)+dx,0,i32(p.width)-1);
      let sy = clamp(i32(id.y)+dy,0,i32(p.height)-1);
      let color = textureLoad(input_tex,vec2<i32>(sx,sy),0);
      if p.masked == 1u { best = max(best,color); }
      else { best = min(best,color); }
    }
  }
  textureStore(output_tex,vec2<i32>(id.xy),best);
}
