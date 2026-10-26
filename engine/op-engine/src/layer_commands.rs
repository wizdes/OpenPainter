//! Layers panel commands (M20): duplicate, merge down, move up and down, move
//! (drag reorder), visibility and Layer Properties (previewable), plus the
//! per-layer content key and the `layer.thumbnail` query.
use super::{CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, bad, internal};
use op_core::doc::{DocState, LayerBlendMode, LayerState};
use op_core::pixel::Rgba8;
use op_core::tiles::{TILE_SIZE, TileData};
use op_ops::composite::composite_pixel;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// The 14 layer blend modes in paint.net's order, with their parameter names.
pub const BLEND_MODES: [(&str, LayerBlendMode); 14] = [
    ("normal", LayerBlendMode::Normal),
    ("multiply", LayerBlendMode::Multiply),
    ("additive", LayerBlendMode::Additive),
    ("color_burn", LayerBlendMode::ColorBurn),
    ("color_dodge", LayerBlendMode::ColorDodge),
    ("reflect", LayerBlendMode::Reflect),
    ("glow", LayerBlendMode::Glow),
    ("overlay", LayerBlendMode::Overlay),
    ("difference", LayerBlendMode::Difference),
    ("negation", LayerBlendMode::Negation),
    ("lighten", LayerBlendMode::Lighten),
    ("darken", LayerBlendMode::Darken),
    ("screen", LayerBlendMode::Screen),
    ("xor", LayerBlendMode::Xor),
];
const BLEND_NAMES: [&str; 14] = [
    "normal",
    "multiply",
    "additive",
    "color_burn",
    "color_dodge",
    "reflect",
    "glow",
    "overlay",
    "difference",
    "negation",
    "lighten",
    "darken",
    "screen",
    "xor",
];

pub fn blend_name(mode: LayerBlendMode) -> &'static str {
    BLEND_MODES
        .iter()
        .find(|(_, m)| *m == mode)
        .map(|(n, _)| *n)
        .expect("every mode has a name")
}
fn blend_mode(name: &str) -> Result<LayerBlendMode> {
    BLEND_MODES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, m)| *m)
        .ok_or_else(|| bad(format!("unknown blend mode {name}")))
}

/// A key that changes whenever the layer's pixels change and comes back when
/// history returns to the same pixels. Tile ids are never reused, so equal keys
/// mean equal pixels (up to a 64-bit hash collision). 53 bits, so JSON readers
/// that use doubles keep it exact.
pub fn content_key(layer: &LayerState) -> u64 {
    let mut h = DefaultHasher::new();
    layer.id.0.hash(&mut h);
    for (coord, id) in layer.tiles.iter() {
        (coord.x, coord.y, id.0).hash(&mut h);
    }
    h.finish() & ((1 << 53) - 1)
}

fn int(key: &'static str, min: f64, max: Option<f64>) -> Param {
    Param {
        key,
        kind: ParamKind::Int,
        min: Some(min),
        max,
        default: None,
    }
}
fn index(p: &Value, key: &str, s: &DocState) -> Result<usize> {
    let i = usize::try_from(p[key].as_i64().unwrap()).map_err(bad)?;
    if i >= s.layers.len() {
        return Err(bad("layer index out of range"));
    }
    Ok(i)
}
fn can_merge_down(s: &DocState) -> bool {
    s.active_layer > 0
}
fn can_move_up(s: &DocState) -> bool {
    s.active_layer + 1 < s.layers.len()
}
fn always(_: &DocState) -> bool {
    true
}

fn duplicate(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let next = s.duplicate_layer(s.active_layer).map_err(internal)?;
    // The copy sits above the original and becomes the active layer.
    Ok(Outcome::State(
        next.set_active_layer(s.active_layer + 1)
            .map_err(internal)?,
    ))
}

/// Blends the active layer onto the layer below with its own mode and opacity,
/// then removes it. The lower layer keeps its properties and becomes active. A
/// hidden upper layer adds nothing.
fn merge_down(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let top = s.active_layer;
    if top == 0 {
        return Err(bad("no layer below"));
    }
    let bottom = top - 1;
    let upper = &s.layers[top];
    let mut next = s.clone();
    if upper.visible && upper.opacity > 0 {
        let coords: Vec<_> = upper.tiles.iter().map(|(c, _)| c).collect();
        for coord in coords {
            ctx.check()?;
            let below = s.read_tile(bottom, coord).map_err(internal)?;
            let above = s.read_tile(top, coord).map_err(internal)?;
            let mut out = Vec::with_capacity(below.len());
            for (b, t) in below.chunks_exact(4).zip(above.chunks_exact(4)) {
                let px = composite_pixel(
                    Rgba8::from([b[0], b[1], b[2], b[3]]),
                    Rgba8::from([t[0], t[1], t[2], t[3]]),
                    upper.blend,
                    upper.opacity,
                );
                out.extend_from_slice(&<[u8; 4]>::from(px));
            }
            next = next
                .write_tile(bottom, coord, TileData::try_from(out).map_err(internal)?)
                .map_err(internal)?;
        }
    }
    let next = next.delete_layer(top).map_err(internal)?;
    Ok(Outcome::State(
        next.set_active_layer(bottom).map_err(internal)?,
    ))
}

fn move_up(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let i = s.active_layer;
    if i + 1 >= s.layers.len() {
        return Err(bad("already the top layer"));
    }
    Ok(Outcome::State(s.move_layer(i, i + 1).map_err(internal)?))
}
fn move_down(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let i = s.active_layer;
    if i == 0 {
        return Err(bad("already the bottom layer"));
    }
    Ok(Outcome::State(s.move_layer(i, i - 1).map_err(internal)?))
}
fn move_to(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let (from, to) = (index(p, "from", s)?, index(p, "to", s)?);
    if from == to {
        return Err(bad("the layer is already there"));
    }
    Ok(Outcome::State(s.move_layer(from, to).map_err(internal)?))
}
fn set_visible(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let i = index(p, "index", s)?;
    let visible = p["visible"].as_bool().unwrap();
    if s.layers[i].visible == visible {
        return Err(bad("the layer already has that visibility"));
    }
    Ok(Outcome::State(
        s.set_layer_visible(i, visible).map_err(internal)?,
    ))
}
/// Every Layer Properties field at once. A call that changes nothing is
/// refused, so OK with nothing changed adds no history step.
fn properties(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let i = index(p, "index", s)?;
    let name = p["name"].as_str().unwrap();
    let visible = p["visible"].as_bool().unwrap();
    let blend = blend_mode(p["blend"].as_str().unwrap())?;
    let opacity = u8::try_from(p["opacity"].as_i64().unwrap()).map_err(bad)?;
    let l = &s.layers[i];
    if l.name == name && l.visible == visible && l.blend == blend && l.opacity == opacity {
        return Err(bad("nothing changed"));
    }
    let next = s
        .set_layer_name(i, name)
        .and_then(|n| n.set_layer_visible(i, visible))
        .and_then(|n| n.set_layer_blend(i, blend))
        .and_then(|n| n.set_layer_opacity(i, opacity))
        .map_err(internal)?;
    Ok(Outcome::State(next))
}

pub(super) fn specs() -> Vec<CommandSpec> {
    let none = ParamSchema::default;
    vec![
        CommandSpec {
            id: "layer.duplicate",
            params: none(),
            enabled: |s| s.can_add_layer(),
            run: duplicate,
            previewable: false,
        },
        CommandSpec {
            id: "layer.merge_down",
            params: none(),
            enabled: can_merge_down,
            run: merge_down,
            previewable: false,
        },
        CommandSpec {
            id: "layer.move_up",
            params: none(),
            enabled: can_move_up,
            run: move_up,
            previewable: false,
        },
        CommandSpec {
            id: "layer.move_down",
            params: none(),
            enabled: can_merge_down,
            run: move_down,
            previewable: false,
        },
        CommandSpec {
            id: "layer.move",
            params: ParamSchema {
                fields: vec![int("from", 0.0, None), int("to", 0.0, None)],
            },
            enabled: always,
            run: move_to,
            previewable: false,
        },
        CommandSpec {
            id: "layer.set_visible",
            params: ParamSchema {
                fields: vec![
                    int("index", 0.0, None),
                    Param {
                        key: "visible",
                        kind: ParamKind::Bool,
                        min: None,
                        max: None,
                        default: None,
                    },
                ],
            },
            enabled: always,
            run: set_visible,
            previewable: false,
        },
        CommandSpec {
            id: "layer.properties",
            params: ParamSchema {
                fields: vec![
                    int("index", 0.0, None),
                    Param {
                        key: "name",
                        kind: ParamKind::String,
                        min: None,
                        max: None,
                        default: None,
                    },
                    Param {
                        key: "visible",
                        kind: ParamKind::Bool,
                        min: None,
                        max: None,
                        default: None,
                    },
                    Param {
                        key: "blend",
                        kind: ParamKind::Choice(&BLEND_NAMES),
                        min: None,
                        max: None,
                        default: None,
                    },
                    int("opacity", 0.0, Some(255.0)),
                ],
            },
            enabled: always,
            run: properties,
            previewable: true,
        },
    ]
}

/// The History label for the commands of this file.
pub(super) fn history_label(id: &str, params: &Value) -> Option<String> {
    Some(
        match id {
            "layer.add" => "Add New Layer",
            "layer.delete" => "Delete Layer",
            "layer.duplicate" => "Duplicate Layer",
            "layer.merge_down" => "Merge Layer Down",
            "layer.move_up" => "Move Layer Up",
            "layer.move_down" => "Move Layer Down",
            "layer.move" => "Move Layer",
            "layer.set_visible" if params["visible"] == Value::Bool(true) => "Show Layer",
            "layer.set_visible" => "Hide Layer",
            "layer.properties" => "Layer Properties",
            _ => return None,
        }
        .into(),
    )
}

/// `layer.thumbnail` {"id", "max"}: the pixels of the layer with that id (no
/// opacity or visibility applied) box-filtered to fit `max` x `max`, never
/// upscaled. Returns width and height as little-endian u32, the layer's content
/// key as a little-endian u64, then straight-alpha RGBA rows.
pub fn thumbnail(s: &DocState, params: &Value) -> Result<Vec<u8>> {
    let id = params
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("id must be a layer id"))?;
    let max = params
        .get("max")
        .and_then(Value::as_u64)
        .filter(|m| (1..=1024).contains(m))
        .ok_or_else(|| bad("max must be 1 to 1024"))? as f64;
    let i = s
        .layers
        .iter()
        .position(|l| l.id.0 == id)
        .ok_or_else(|| bad("no layer with that id"))?;
    let layer = &s.layers[i];
    let (w, h) = (s.width as usize, s.height as usize);
    let scale = (max / w as f64).min(max / h as f64).min(1.0);
    let tw = ((w as f64 * scale).round() as usize).max(1);
    let th = ((h as f64 * scale).round() as usize).max(1);
    // Which thumbnail column or row each source column or row falls in, and how
    // many source pixels each box holds.
    let col: Vec<usize> = (0..w).map(|x| x * tw / w).collect();
    let row: Vec<usize> = (0..h).map(|y| y * th / h).collect();
    let mut col_n = vec![0u64; tw];
    let mut row_n = vec![0u64; th];
    for &c in &col {
        col_n[c] += 1;
    }
    for &r in &row {
        row_n[r] += 1;
    }
    // Premultiplied sums per box: r*a, g*a, b*a, a.
    let mut sums = vec![[0u64; 4]; tw * th];
    for (coord, _) in layer.tiles.iter() {
        let tile = s.read_tile(i, coord).map_err(internal)?;
        let (x0, y0) = (coord.x as usize * TILE_SIZE, coord.y as usize * TILE_SIZE);
        for ty in 0..TILE_SIZE.min(h.saturating_sub(y0)) {
            let r = row[y0 + ty];
            for tx in 0..TILE_SIZE.min(w.saturating_sub(x0)) {
                let px = &tile[(ty * TILE_SIZE + tx) * 4..][..4];
                let a = u64::from(px[3]);
                if a == 0 {
                    continue;
                }
                let sum = &mut sums[r * tw + col[x0 + tx]];
                for c in 0..3 {
                    sum[c] += u64::from(px[c]) * a;
                }
                sum[3] += a;
            }
        }
    }
    let mut out = Vec::with_capacity(16 + tw * th * 4);
    out.extend_from_slice(&(tw as u32).to_le_bytes());
    out.extend_from_slice(&(th as u32).to_le_bytes());
    out.extend_from_slice(&content_key(layer).to_le_bytes());
    for (k, sum) in sums.iter().enumerate() {
        let n = col_n[k % tw] * row_n[k / tw];
        let a = sum[3];
        if a == 0 {
            out.extend_from_slice(&[0; 4]);
            continue;
        }
        for c in sum.iter().take(3) {
            out.push(((c + a / 2) / a) as u8);
        }
        out.push(((a + n / 2) / n) as u8);
    }
    Ok(out)
}
