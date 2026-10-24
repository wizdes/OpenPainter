// Mip builder: one 2x2 premultiplied box average of the level below.
// `src_premul` is 0 when the source is level 0 (straight alpha).
struct MipParams {
    src_premul: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
}
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@group(0) @binding(0) var mip_src: texture_2d<u32>;
@group(0) @binding(1) var<uniform> mp: MipParams;

@fragment
fn mip_fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let o = vec2<i32>(floor(pos.xy)) * 2;
    let maxp = vec2<i32>(textureDimensions(mip_src)) - vec2<i32>(1);
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let c = vec4<f32>(textureLoad(mip_src, min(o + vec2<i32>(x, y), maxp), 0));
            if (mp.src_premul == 0u) {
                sum += vec4<f32>(c.rgb * c.a / 255.0, c.a);
            } else {
                sum += c;
            }
        }
    }
    return vec4<u32>(floor(sum / 4.0 + vec4<f32>(0.5)));
}
