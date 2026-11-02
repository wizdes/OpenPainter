struct Uniform {
    view: vec4<f32>, // origin x, y, device pixels per document point, backing scale
    dash: vec4<f32>, // dash phase in screen points
    crop: array<vec4<f32>, 4>,
}
@group(0) @binding(0) var<uniform> u: Uniform;

struct LineIn {
    @location(0) anchor: vec2<f32>,
    @location(1) offset: vec2<f32>,
    @location(2) distance: f32,
    @location(3) kind: f32,
    @location(4) edge: f32,
    @location(5) width: f32,
}
struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) distance: f32,
    @location(1) kind: f32,
    @location(2) edge: f32,
    @location(3) width: f32,
}
@vertex
fn vs_line(v: LineIn) -> LineOut {
    var out: LineOut;
    // The target size is in dash.zw.
    let xy = u.view.xy + v.anchor * u.view.z + v.offset * u.view.w;
    out.pos = vec4<f32>(xy.x / u.dash.z * 2.0 - 1.0, 1.0 - xy.y / u.dash.w * 2.0, 0.0, 1.0);
    out.distance = v.distance * u.view.z / u.view.w + u.dash.x;
    out.kind = v.kind;
    out.edge = v.edge;
    out.width = v.width;
    return out;
}
@fragment
fn fs_line(v: LineOut) -> @location(0) vec4<f32> {
    var color = vec4<f32>(0.098, 0.451, 0.784, 1.0);
    if (v.kind < 0.5) {
        color = select(vec4<f32>(1.0), vec4<f32>(0.0,0.0,0.0,1.0), fract(v.distance / 8.0) < 0.5);
    } else if (v.kind > 7.5) {
        color = select(vec4<f32>(1.0), vec4<f32>(0.0,0.0,0.0,1.0), fract((v.distance-u.dash.x) / 8.0) < 0.5);
    } else if (v.kind < 1.5) {
        color = vec4<f32>(0.098, 0.451, 0.784, 1.0);
    } else if (v.kind < 2.5) {
        color = vec4<f32>(1.0);
    } else if (v.kind < 3.5) {
        color = vec4<f32>(1.0,1.0,1.0,0.55);
    } else if (v.kind < 5.5) {
        color = vec4<f32>(0.0,0.0,0.0,1.0);
    } else if (v.kind < 6.5) {
        color = vec4<f32>(1.0,1.0,1.0,0.9);
    } else {
        color = vec4<f32>(0.0,0.0,0.0,select(0.0,0.8,fract(v.distance / 6.0) < 0.5));
    }
    if (v.width > 0.0) {
        let distance = abs(v.edge) * (v.width * 0.5 + 1.0) * u.view.w;
        color.a *= clamp(v.width * 0.5 * u.view.w + 0.5 - distance, 0.0, 1.0);
    }
    return color;
}
@vertex
fn vs_dim(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x,y,0.0,1.0);
}
@fragment
fn fs_dim(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var inside = false;
    for (var i = 0u; i < 4u; i++) {
        let a = u.view.xy + u.crop[i].xy * u.view.z;
        let b = u.view.xy + u.crop[(i+1u)%4u].xy * u.view.z;
        if ((a.y > pos.y) != (b.y > pos.y)) {
            let x = a.x + (pos.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if (pos.x < x) { inside = !inside; }
        }
    }
    return select(vec4<f32>(0.0,0.0,0.0,0.45), vec4<f32>(0.0), inside);
}
