//! Provisional 5.1 Morphology from https://paint.net/doc/latest/EffectsDistortMenu.html.
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let w = v["width"].as_i64().unwrap() as i32;
    let h = if v["linked"].as_bool().unwrap() {
        w
    } else {
        v["height"].as_i64().unwrap() as i32
    };
    if w == 1 && h == 1 {
        return s(x, y);
    }
    let erode = v["mode"].as_str().unwrap() == "erode";
    let mut best = if erode { [255u8; 4] } else { [0u8; 4] };
    for dy in -(h / 2)..=(h - 1) / 2 {
        for dx in -(w / 2)..=(w - 1) / 2 {
            let pix = s(
                (x + dx).clamp(0, p.width - 1),
                (y + dy).clamp(0, p.height - 1),
            );
            for c in 0..4 {
                best[c] = if erode {
                    best[c].min(pix[c])
                } else {
                    best[c].max(pix[c])
                };
            }
        }
    }
    best
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.distort.morphology",
        category: "Distort",
        label_key: "effect.morphology",
        schema: ParamSchema {
            fields: vec![
                d::choice("mode", &["erode", "dilate"], "erode"),
                d::field("width", ParamKind::Int, 1.0, 101.0, json!(3)),
                d::field("height", ParamKind::Int, 1.0, 101.0, json!(3)),
                d::field("linked", ParamKind::Bool, 0.0, 1.0, json!(true)),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "own"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: Some(GpuKernel::Morphology),
    }
}
