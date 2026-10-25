//! Live brush strokes (M38): the brush options the Mac sends with `stroke.begin`,
//! and the tile diff a canvas view uses to upload only what a stroke changed.
use super::{Result, bad, internal};
use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::{TILE_SIZE, TileCoord};
use op_tools::{BrushSettings, PressureMode, Tool};
use serde_json::Value;
use std::sync::Arc;

/// The brush options of a live stroke, as JSON:
/// `{"tool":"paintbrush"|"eraser"|"pencil","width":1..2000,"color":"rrggbbaa",
///   "opacity":0..255,"antialias":bool,"blend":"normal"|"overwrite",
///   "hardness":0..1,"pressure_size":bool,"pressure_opacity":bool}`.
/// Missing keys take the `tool.stroke` defaults; hardness defaults to 1.
pub fn brush_from_json(p: &Value) -> Result<(BrushSettings, f64)> {
    let tool = match p["tool"].as_str().unwrap_or("paintbrush") {
        "paintbrush" => Tool::Paintbrush,
        "eraser" => Tool::Eraser,
        "pencil" => Tool::Pencil,
        other => return Err(bad(format!("unknown brush tool {other}"))),
    };
    let number = |key: &str, default: f64, min: f64, max: f64| -> Result<f64> {
        let v = match &p[key] {
            Value::Null => default,
            v => v
                .as_f64()
                .ok_or_else(|| bad(format!("{key} must be a number")))?,
        };
        if !v.is_finite() || v < min || v > max {
            return Err(bad(format!("{key} must be {min}..{max}")));
        }
        Ok(v)
    };
    let flag = |key: &str, default: bool| -> Result<bool> {
        match &p[key] {
            Value::Null => Ok(default),
            v => v
                .as_bool()
                .ok_or_else(|| bad(format!("{key} must be true or false"))),
        }
    };
    let hex = p["color"].as_str().unwrap_or("000000ff");
    if hex.len() != 8 || !hex.is_ascii() {
        return Err(bad("color must be eight RGBA hex digits"));
    }
    let mut color = [0u8; 4];
    for (i, c) in color.iter_mut().enumerate() {
        *c = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(bad)?;
    }
    let opacity = number("opacity", 255.0, 0.0, 255.0)?;
    color[3] = (f64::from(color[3]) * opacity / 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    let pressure = match (
        flag("pressure_size", false)?,
        flag("pressure_opacity", false)?,
    ) {
        (false, false) => PressureMode::None,
        (true, false) => PressureMode::Size,
        (false, true) => PressureMode::Opacity,
        (true, true) => PressureMode::SizeAndOpacity,
    };
    let blend = match p["blend"].as_str().unwrap_or("normal") {
        "normal" => ToolCompositeMode::Normal,
        "overwrite" => ToolCompositeMode::Overwrite,
        other => return Err(bad(format!("unknown blend {other}"))),
    };
    let settings = BrushSettings {
        tool,
        width: number("width", 2.0, 1.0, 2000.0)?,
        color,
        antialias: flag("antialias", true)?,
        spacing: 0.1,
        pressure,
        blend,
    };
    Ok((settings, number("hardness", 1.0, 0.0, 1.0)?))
}

/// The tiles whose composite can differ between `before` and `after`, or None
/// when the whole image must be redrawn (size, profile, layer list or layer
/// properties changed). Tiles are compared by identity, so this is cheap.
pub fn changed_tiles(before: &DocState, after: &DocState) -> Option<Vec<TileCoord>> {
    if before.width != after.width
        || before.height != after.height
        || before.profile != after.profile
        || before.layers.len() != after.layers.len()
    {
        return None;
    }
    let mut changed: Vec<usize> = Vec::new();
    for (i, (a, b)) in before.layers.iter().zip(&after.layers).enumerate() {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if a.id != b.id || a.visible != b.visible || a.opacity != b.opacity || a.blend != b.blend {
            return None;
        }
        changed.push(i);
    }
    let cols = after.width.div_ceil(TILE_SIZE as u32) as i32;
    let rows = after.height.div_ceil(TILE_SIZE as u32) as i32;
    let mut out = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            let c = TileCoord { x, y };
            if changed
                .iter()
                .any(|&i| before.layers[i].tiles.get(c) != after.layers[i].tiles.get(c))
            {
                out.push(c);
            }
        }
    }
    Some(out)
}

/// One tile of the flattened image, cropped to the image: `(x, y, w, h, rgba)`
/// in document pixels, straight RGBA8 rows of `w * 4` bytes.
pub fn tile_rgba(state: &DocState, coord: TileCoord) -> Result<(u32, u32, u32, u32, Vec<u8>)> {
    let tile = op_ops::composite::composite_tile(state, coord).map_err(internal)?;
    let x = coord.x as u32 * TILE_SIZE as u32;
    let y = coord.y as u32 * TILE_SIZE as u32;
    if x >= state.width || y >= state.height {
        return Err(bad("tile outside the image"));
    }
    let w = (state.width - x).min(TILE_SIZE as u32);
    let h = (state.height - y).min(TILE_SIZE as u32);
    let bytes = tile.as_bytes();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h as usize {
        let start = row * TILE_SIZE * 4;
        out.extend_from_slice(&bytes[start..start + w as usize * 4]);
    }
    Ok((x, y, w, h, out))
}
