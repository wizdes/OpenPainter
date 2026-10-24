// View renderer: draws an rgba8uint document into a color target at a zoom and pan,
// with an optional document -> display 3D lookup table. No hardware filtering:
// every document read is a textureLoad, and the table is interpolated by hand.

struct Params {
    origin: vec2<f32>,     // target pixel position of document pixel (0, 0)
    zoom: f32,             // target pixels per document pixel
    taps: u32,             // box filter width per axis when zoom < 1 (1 = nearest)
    doc_size: vec2<u32>,
    use_lut: u32,
    lut_n: u32,
    background: vec4<f32>, // outside the document, already in target encoding
}

@group(0) @binding(0) var doc: texture_2d<u32>;
@group(0) @binding(1) var<storage, read> lut: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: Params;

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

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = (pos.xy - p.origin) / p.zoom;
    let size = vec2<f32>(p.doc_size);
    if (d.x < 0.0 || d.y < 0.0 || d.x >= size.x || d.y >= size.y) {
        return p.background;
    }
    var c: vec4<f32>;
    if (p.taps <= 1u) {
        c = vec4<f32>(textureLoad(doc, vec2<u32>(floor(d)), 0)) / 255.0;
    } else {
        // Box filter over the footprint of this target pixel (no mips in the spike).
        let start = vec2<i32>(floor((pos.xy - vec2<f32>(0.5) - p.origin) / p.zoom));
        let maxp = vec2<i32>(p.doc_size) - vec2<i32>(1);
        var sum = vec4<f32>(0.0);
        for (var y = 0u; y < p.taps; y++) {
            for (var x = 0u; x < p.taps; x++) {
                let q = clamp(start + vec2<i32>(i32(x), i32(y)), vec2<i32>(0), maxp);
                sum += vec4<f32>(textureLoad(doc, vec2<u32>(q), 0));
            }
        }
        c = sum / (255.0 * f32(p.taps * p.taps));
    }
    // Checkerboard under the image, 8 target pixels per square, in document encoding.
    let cell = (u32(pos.x) / 8u + u32(pos.y) / 8u) & 1u;
    let check = select(1.0, 0.8, cell == 1u);
    var rgb = c.rgb * c.a + vec3<f32>(check) * (1.0 - c.a);
    if (p.use_lut != 0u) {
        rgb = apply_lut(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
