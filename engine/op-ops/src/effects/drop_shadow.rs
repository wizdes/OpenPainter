//! Provisional 5.1 Drop Shadow: https://paint.net/doc/latest/EffectsObjectMenu.html.
use crate::effect::*;
use crate::effects::distort_common as d;
use serde_json::{Value, json};
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let radius = v["shadow_radius"].as_i64().unwrap() as i32;
    let distance = v["distance"].as_f64().unwrap();
    let angle = v["angle"].as_f64().unwrap().to_radians();
    let ox = (distance * angle.cos()).round() as i32;
    let oy = (distance * angle.sin()).round() as i32;
    let mut sum = 0u32;
    let mut count = 0u32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                let sx = x - ox + dx;
                let sy = y - oy + dy;
                if sx >= 0 && sy >= 0 && sx < p.width && sy < p.height {
                    sum += u32::from(s(sx, sy)[3]);
                }
                count += 1;
            }
        }
    }
    let opacity = v["opacity"].as_u64().unwrap() as u32;
    let shadow_alpha = ((sum * opacity + count * 50) / (count * 100)).min(255) as u8;
    let color = v["color"].as_array().unwrap();
    let shadow = [
        color[0].as_u64().unwrap() as u8,
        color[1].as_u64().unwrap() as u8,
        color[2].as_u64().unwrap() as u8,
        shadow_alpha,
    ];
    if v["only_draw_shadow"].as_bool().unwrap() {
        return shadow;
    }
    let source = s(x, y);
    let out_a =
        u32::from(source[3]) + (u32::from(shadow_alpha) * (255 - u32::from(source[3])) + 127) / 255;
    if out_a == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for c in 0..3 {
        let front = u32::from(source[c]) * u32::from(source[3]) * 255;
        let back = u32::from(shadow[c]) * u32::from(shadow_alpha) * (255 - u32::from(source[3]));
        out[c] = ((front + back + out_a * 127) / (out_a * 255)).min(255) as u8;
    }
    out[3] = out_a as u8;
    out
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.object.drop_shadow",
        category: "Object",
        label_key: "effect.drop_shadow",
        schema: ParamSchema {
            fields: vec![
                d::field("shadow_radius", ParamKind::Int, 0.0, 100.0, json!(5)),
                d::field("distance", ParamKind::Float, 0.0, 200.0, json!(5.0)),
                d::field("angle", ParamKind::Float, -180.0, 180.0, json!(45.0)),
                d::field("opacity", ParamKind::Int, 0.0, 100.0, json!(75)),
                Param {
                    key: "color",
                    kind: ParamKind::RgbBytes,
                    min: None,
                    max: None,
                    default: Some(json!([0, 0, 0])),
                },
                d::field("only_draw_shadow", ParamKind::Bool, 0.0, 1.0, json!(false)),
            ],
        },
        contract: d::contract(BorderRule::Transparent, "own"),
        prepare: prepare_noop,
        required_input: d::input_whole,
        passes: &[PassKind::Neighborhood],
        render_cpu: render,
        gpu: None,
    }
}
