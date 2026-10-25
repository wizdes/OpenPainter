//! M53: Remove Background. The Mac side computes the foreground mask (Apple
//! Vision) and sends it here; the engine stays portable and only applies it.
//!
//! - `selection.from_mask` {"mask"}: replaces the selection with a Coverage
//!   selection holding the mask bytes as they are, so soft edges survive.
//!   A mask that is 0 everywhere is refused (it would select nothing).
//!   History: "Select Foreground".
//! - `layer.remove_background` {"mask"}: multiplies the active layer's
//!   straight alpha by mask / 255 (rounded); a pixel whose alpha becomes 0 is
//!   transparent black, as Erase Selection leaves it. The selection is not
//!   used and not changed. History: "Remove Background".
//! - Query `layer.pixels` {"index"?}: the layer's straight RGBA rows (the
//!   active layer when no index is given), the Vision input.
//!
//! `mask` is standard base64 (RFC 4648, with padding) of width x height bytes,
//! one per document pixel, rows top first.
use super::{
    CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, bad, internal,
    text_commands::base64_decode,
};
use op_core::doc::{DocState, MaskGrid, SelectionSource, SelectionState};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use serde_json::Value;
use std::sync::Arc;

fn mask_schema() -> ParamSchema {
    ParamSchema {
        fields: vec![Param {
            key: "mask",
            kind: ParamKind::String,
            min: None,
            max: None,
            default: None,
        }],
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "selection.from_mask",
            params: mask_schema(),
            enabled: |_| true,
            run: from_mask,
            previewable: false,
        },
        CommandSpec {
            id: "layer.remove_background",
            params: mask_schema(),
            enabled: |_| true,
            run: remove_background,
            previewable: false,
        },
    ]
}

pub(super) fn history_label(id: &str) -> Option<&'static str> {
    match id {
        "selection.from_mask" => Some("Select Foreground"),
        "layer.remove_background" => Some("Remove Background"),
        _ => None,
    }
}

/// The decoded mask, checked against the document size.
fn mask(s: &DocState, p: &Value) -> Result<Vec<u8>> {
    let text = p["mask"]
        .as_str()
        .ok_or_else(|| bad("mask must be base64 text"))?;
    if text.is_empty() {
        return Err(bad("mask is empty"));
    }
    let bytes = base64_decode(text)?;
    let want = s.width as usize * s.height as usize;
    if bytes.len() != want {
        return Err(bad(format!(
            "mask has {} bytes; the document needs {want} ({} x {})",
            bytes.len(),
            s.width,
            s.height
        )));
    }
    Ok(bytes)
}

fn from_mask(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let bytes = mask(s, p)?;
    if bytes.iter().all(|&c| c == 0) {
        return Err(bad("the mask selects nothing"));
    }
    let grid = MaskGrid::new(s.width, s.height, bytes).ok_or_else(|| internal("mask grid size"))?;
    ctx.check()?;
    Ok(Outcome::State(s.set_selection(SelectionState::Active(
        SelectionSource::Coverage(Arc::new(grid)),
    ))))
}

fn remove_background(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let m = mask(s, p)?;
    let (w, h) = (s.width as usize, s.height as usize);
    let layer = s.active_layer;
    let coords: Vec<TileCoord> = s.layers[layer]
        .tiles
        .iter()
        .map(|(coord, _)| coord)
        .collect();
    let mut next = s.clone();
    // Only stored tiles hold pixels; a missing tile is transparent already.
    for coord in coords {
        ctx.check()?;
        let (x0, y0) = (coord.x as usize * TILE_SIZE, coord.y as usize * TILE_SIZE);
        if x0 >= w || y0 >= h {
            continue;
        }
        let (x1, y1) = ((x0 + TILE_SIZE).min(w), (y0 + TILE_SIZE).min(h));
        let mut tile = s.read_tile(layer, coord).map_err(internal)?;
        let mut changed = false;
        for y in y0..y1 {
            for x in x0..x1 {
                let c = m[y * w + x];
                if c == 255 {
                    continue;
                }
                let i = ((y - y0) * TILE_SIZE + (x - x0)) * 4;
                let a = (f64::from(tile[i + 3]) * f64::from(c) / 255.0).round() as u8;
                let px = if a == 0 {
                    [0, 0, 0, 0]
                } else {
                    [tile[i], tile[i + 1], tile[i + 2], a]
                };
                if tile[i..i + 4] != px {
                    tile[i..i + 4].copy_from_slice(&px);
                    changed = true;
                }
            }
        }
        if changed {
            next = next
                .write_tile(layer, coord, TileData::try_from(tile).map_err(internal)?)
                .map_err(internal)?;
        }
    }
    ctx.check()?;
    Ok(Outcome::State(next))
}

/// `layer.pixels`: one layer's straight RGBA rows (no opacity, blend or
/// visibility applied). `{"index"}` picks a layer; the default is the active one.
pub(super) fn layer_pixels(s: &DocState, params: &Value) -> Result<Vec<u8>> {
    let index = match params.get("index") {
        None | Some(Value::Null) => s.active_layer,
        Some(v) => v
            .as_u64()
            .map(|i| i as usize)
            .filter(|&i| i < s.layers.len())
            .ok_or_else(|| bad("index must be a layer index"))?,
    };
    let (w, h) = (s.width as usize, s.height as usize);
    let len = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .filter(|&n| n <= 1024 * 1024 * 1024)
        .ok_or_else(|| bad("image too large"))?;
    let mut out = vec![0u8; len];
    for (coord, _) in s.layers[index].tiles.iter() {
        let (x0, y0) = (coord.x as usize * TILE_SIZE, coord.y as usize * TILE_SIZE);
        if x0 >= w || y0 >= h {
            continue;
        }
        let tile = s.read_tile(index, coord).map_err(internal)?;
        let n = (w - x0).min(TILE_SIZE) * 4;
        for y in 0..(h - y0).min(TILE_SIZE) {
            let dst = ((y0 + y) * w + x0) * 4;
            out[dst..dst + n].copy_from_slice(&tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + n]);
        }
    }
    Ok(out)
}
