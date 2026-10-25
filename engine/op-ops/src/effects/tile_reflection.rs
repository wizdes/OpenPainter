//! Port of OpenPDN TileEffect.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let rotation = -v["angle"].as_f64().unwrap() * std::f64::consts::PI / 180.0;
    let (sin, cos) = rotation.sin_cos();
    let curve = v["curvature"].as_f64().unwrap();
    let intensity = (curve * curve / 10.0 * curve.signum()) as f32;
    let scale = (std::f64::consts::PI / v["tile_size"].as_f64().unwrap()) as f32;
    let q = v["quality"].as_i64().unwrap() as usize;
    let q = if q == 1 { 1 } else { q + 1 };
    let mut samples = Vec::with_capacity(q * q);
    let sin = sin as f32;
    let cos = cos as f32;
    for (ox, oy) in d::rgss(q) {
        let u = x as f32 - p.width as f32 * 0.5 + ox;
        let w = y as f32 - p.height as f32 * 0.5 - oy;
        let a = cos * u + sin * w;
        let b = -sin * u + cos * w;
        let a2 = a + intensity * tan(a * scale);
        let b2 = b + intensity * tan(b * scale);
        samples.push(d::bilinear(
            s,
            p.width as f32 * 0.5 + cos * a2 - sin * b2,
            p.height as f32 * 0.5 + sin * a2 + cos * b2,
            p,
            v["edge_behavior"].as_str().unwrap(),
        ));
    }
    d::blend(&samples)
}
/// The tangent the way the C# original computes it: `(float)Math.Tan(x)`, in
/// f64. `f32::tan` calls the system's `tanf`, and macOS and glibc round it
/// differently near the poles, where the warp magnifies one ulp into a whole
/// pixel, so Linux missed the legacy oracle by up to 11 channels.
fn tan(x: f32) -> f32 {
    f64::from(x).tan() as f32
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.tile_reflection",
        category: "Distort",
        label_key: "effect.tile_reflection",
        schema: ParamSchema {
            fields: vec![
                d::field("angle", ParamKind::Float, -180.0, 180.0, json!(30.0)),
                d::field("tile_size", ParamKind::Float, 1.0, 800.0, json!(40.0)),
                d::field("curvature", ParamKind::Float, -100.0, 100.0, json!(8.0)),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(2)),
                d::choice(
                    "edge_behavior",
                    &["clamp", "wrap", "reflect", "transparent"],
                    "wrap",
                ),
            ],
        },
        contract: d::contract(BorderRule::Wrap, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        // CPU-only until a Metal tangent and wrap kernel meets 1/255 parity at poles.
        gpu: None,
    }
}
