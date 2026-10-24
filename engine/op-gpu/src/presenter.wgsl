// View renderer: draws an rgba8uint document into a color target at a zoom and pan,
// with an optional document -> display 3D lookup table. No hardware filtering:
// every document read is a textureLoad, and the table is interpolated by hand.
//
// Level 0 (`doc`) holds straight, encoded RGBA. The mip chain (`mips`, level k of
// it is pyramid level k + 1) holds premultiplied 2x2 box averages, built lazily
// by `mip_fs` below. Below 100% the renderer reads pyramid level `level` with a
// box of `taps` x `taps` texels; the filter is done in premultiplied alpha.

struct Params {
    origin: vec2<f32>,     // target pixel position of document pixel (0, 0)
    zoom: f32,             // target pixels per document pixel
    taps: u32,             // box filter width per axis on `level` (1 = nearest)
    doc_size: vec2<u32>,
    use_lut: u32,
    lut_n: u32,
    background: vec4<f32>, // outside the document, already in target encoding
    level: u32,            // pyramid level to read: 0 = doc, k >= 1 = mips level k - 1
    checker_px: u32,       // checkerboard square side in target pixels
    grid: u32,             // 1 = draw the pixel grid (display space, after the table)
    _pad: u32,
}

@group(0) @binding(0) var doc: texture_2d<u32>;
@group(0) @binding(1) var<storage, read> lut: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var mips: texture_2d<u32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn lut_at(r: u32, g: u32, b: u32) -> vec3<f32> {
    let n = p.lut_n;
    return lut[r + g * n + b * n * n].rgb;
}

fn apply_lut(c: vec3<f32>) -> vec3<f32> {
    let n = p.lut_n;
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n - 1u);
    let i0 = min(vec3<u32>(floor(x)), vec3<u32>(n - 2u));
    let f = x - vec3<f32>(i0);
    let c000 = lut_at(i0.x, i0.y, i0.z);
    let c100 = lut_at(i0.x + 1u, i0.y, i0.z);
    let c010 = lut_at(i0.x, i0.y + 1u, i0.z);
    let c110 = lut_at(i0.x + 1u, i0.y + 1u, i0.z);
    let c001 = lut_at(i0.x, i0.y, i0.z + 1u);
    let c101 = lut_at(i0.x + 1u, i0.y, i0.z + 1u);
    let c011 = lut_at(i0.x, i0.y + 1u, i0.z + 1u);
    let c111 = lut_at(i0.x + 1u, i0.y + 1u, i0.z + 1u);
    let c00 = mix(c000, c100, f.x);
    let c10 = mix(c010, c110, f.x);
    let c01 = mix(c001, c101, f.x);
    let c11 = mix(c011, c111, f.x);
    return mix(mix(c00, c10, f.y), mix(c01, c11, f.y), f.z);
}

// One texel of pyramid level `p.level`, premultiplied, 0..1.
fn texel(q: vec2<i32>) -> vec4<f32> {
    if (p.level == 0u) {
        let c = vec4<f32>(textureLoad(doc, vec2<u32>(q), 0)) / 255.0;
        return vec4<f32>(c.rgb * c.a, c.a);
    }
    return vec4<f32>(textureLoad(mips, vec2<u32>(q), i32(p.level) - 1)) / 255.0;
}

fn level_size() -> vec2<i32> {
    if (p.level == 0u) {
        return vec2<i32>(p.doc_size);
    }
    return vec2<i32>(textureDimensions(mips, i32(p.level) - 1));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = (pos.xy - p.origin) / p.zoom;
    let size = vec2<f32>(p.doc_size);
    if (d.x < 0.0 || d.y < 0.0 || d.x >= size.x || d.y >= size.y) {
        return p.background;
    }
    // Target pixels per texel of the level we read.
    let lzoom = p.zoom * f32(1u << p.level);
    let maxp = level_size() - vec2<i32>(1);
    var c: vec4<f32>;
    if (p.taps <= 1u) {
        c = texel(min(vec2<i32>(floor(d / f32(1u << p.level))), maxp));
    } else {
        // Box filter over the footprint of this target pixel on the chosen level.
        let start = vec2<i32>(floor((pos.xy - vec2<f32>(0.5) - p.origin) / lzoom));
        var sum = vec4<f32>(0.0);
        for (var y = 0u; y < p.taps; y++) {
            for (var x = 0u; x < p.taps; x++) {
                let q = clamp(start + vec2<i32>(i32(x), i32(y)), vec2<i32>(0), maxp);
                sum += texel(q);
            }
        }
        c = sum / f32(p.taps * p.taps);
    }
    // Checkerboard under the image, fixed in target space, in document encoding.
    let cell = (u32(pos.x) / p.checker_px + u32(pos.y) / p.checker_px) & 1u;
    let check = select(1.0, 0.8, cell == 1u);
    var rgb = c.rgb + vec3<f32>(check) * (1.0 - c.a);
    if (p.use_lut != 0u) {
        rgb = apply_lut(rgb);
    }
    if (p.grid != 0u) {
        // One target pixel on the left and top edge of each document pixel.
        let f = fract(d) * p.zoom;
        if (f.x < 1.0 || f.y < 1.0) {
            let lum = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
            rgb = mix(rgb, select(vec3<f32>(1.0), vec3<f32>(0.0), lum > 0.5), 0.45);
        }
    }
    return vec4<f32>(rgb, 1.0);
}
