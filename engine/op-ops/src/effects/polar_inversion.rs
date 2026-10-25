//! Port of OpenPDN PolarInversionEffect.cs and WarpEffectBase.cs at cca476b (MIT, dotPDN LLC).
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let cx = p.width as f64 * (1.0 + v["offset_x"].as_f64().unwrap()) * 0.5;
    let cy = p.height as f64 * (1.0 + v["offset_y"].as_f64().unwrap()) * 0.5;
    let radius = (p.width.min(p.height) as f64 * 0.5).powi(2);
    let amount = v["scale"].as_f64().unwrap();
    let q = v["quality"].as_i64().unwrap() as usize;
    let mut samples = Vec::with_capacity(q * q);
    for (ox, oy) in d::rgss(q) {
        let dx = x as f64 - cx + ox as f64;
        let dy = y as f64 - cy - oy as f64;
        let r2 = dx * dx + dy * dy;
        if r2 == 0.0 {
            samples.push([0; 4]);
            continue;
        }
        let factor = 1.0 + (radius / r2 - 1.0) * amount;
        samples.push(d::bilinear(
            s,
            (dx * factor + cx) as f32,
            (dy * factor + cy) as f32,
            p,
            v["edge_behavior"].as_str().unwrap(),
        ));
    }
    d::blend(&samples)
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.polar_inversion",
        category: "Distort",
        label_key: "effect.polar_inversion",
        schema: ParamSchema {
            fields: vec![
                d::field("scale", ParamKind::Float, -4.0, 4.0, json!(1.0)),
                d::field("offset_x", ParamKind::Float, -2.0, 2.0, json!(0.0)),
                d::field("offset_y", ParamKind::Float, -2.0, 2.0, json!(0.0)),
                d::choice("edge_behavior", &["clamp", "reflect", "wrap"], "wrap"),
                d::field("quality", ParamKind::Int, 1.0, 5.0, json!(2)),
            ],
        },
        contract: d::contract(BorderRule::Wrap, "legacy-3.36"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Warp],
        render_cpu: render,
        // CPU-only: f32 Metal inversion around the pole exceeded 1/255 parity.
        gpu: None,
    }
}
