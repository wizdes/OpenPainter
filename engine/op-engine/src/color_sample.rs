//! The Color Picker's sample (M36): the average color of a square window around a
//! pixel, from the active layer or from the composite image. paint.net's Color
//! Picker page (https://www.getpaint.net/doc/latest/ColorPicker.html) says the
//! colors under the sample square are averaged into one color. The window is
//! clipped at the canvas edges and only pixels inside the canvas count.
use super::{Result, bad, internal};
use op_core::doc::DocState;
use op_core::tiles::{TILE_SIZE, TileCoord};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Sample sizes the tool offers (odd squares, in pixels).
pub const SAMPLE_SIZES: [u32; 8] = [1, 3, 5, 7, 9, 11, 31, 51];

/// The clipped window `[x0, x1) x [y0, y1)` for a `size` square centered on `(x, y)`.
pub fn sample_window(
    width: u32,
    height: u32,
    x: i64,
    y: i64,
    size: u32,
) -> Option<(u32, u32, u32, u32)> {
    let half = i64::from(size / 2);
    let x0 = (x - half).max(0);
    let y0 = (y - half).max(0);
    let x1 = (x + half + 1).min(i64::from(width));
    let y1 = (y + half + 1).min(i64::from(height));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

/// Alpha-weighted average of straight RGBA pixels, rounded to nearest. Fully
/// transparent pixels add nothing to the color, so a half-empty window keeps
/// the hue of its painted pixels and averages its alpha. All transparent gives
/// `[0, 0, 0, 0]`.
pub fn average(pixels: impl IntoIterator<Item = [u8; 4]>) -> ([u8; 4], u64) {
    let (mut n, mut sa) = (0u64, 0u64);
    let mut sc = [0u64; 3];
    for p in pixels {
        n += 1;
        let a = u64::from(p[3]);
        sa += a;
        for c in 0..3 {
            sc[c] += u64::from(p[c]) * a;
        }
    }
    if n == 0 || sa == 0 {
        return ([0; 4], n);
    }
    let div = |num: u64, den: u64| ((2 * num + den) / (2 * den)) as u8;
    (
        [div(sc[0], sa), div(sc[1], sa), div(sc[2], sa), div(sa, n)],
        n,
    )
}

/// The `doc.sample_color` query: `{x, y, size, sampling: "layer" | "image"}` to
/// `{color: "rrggbbaa", count, window: [x, y, w, h]}`.
pub fn sample_color(s: &DocState, p: &Value) -> Result<Value> {
    let x = p
        .get("x")
        .and_then(Value::as_i64)
        .ok_or_else(|| bad("x is required"))?;
    let y = p
        .get("y")
        .and_then(Value::as_i64)
        .ok_or_else(|| bad("y is required"))?;
    let size = p.get("size").and_then(Value::as_u64).unwrap_or(1);
    let size = u32::try_from(size).map_err(bad)?;
    if !SAMPLE_SIZES.contains(&size) {
        return Err(bad(format!("size must be one of {SAMPLE_SIZES:?}")));
    }
    let image = match p.get("sampling").and_then(Value::as_str).unwrap_or("layer") {
        "layer" => false,
        "image" => true,
        other => return Err(bad(format!("unknown sampling {other}"))),
    };
    if x < 0 || y < 0 || x >= i64::from(s.width) || y >= i64::from(s.height) {
        return Err(bad("sample point is outside the canvas"));
    }
    let (x0, y0, x1, y1) = sample_window(s.width, s.height, x, y, size)
        .ok_or_else(|| bad("sample window is empty"))?;
    let ts = TILE_SIZE as u32;
    let mut tiles: HashMap<(u32, u32), Vec<u8>> = HashMap::new();
    for ty in y0 / ts..=(y1 - 1) / ts {
        for tx in x0 / ts..=(x1 - 1) / ts {
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let bytes = if image {
                op_ops::composite::composite_tile(s, coord)
                    .map_err(internal)?
                    .as_bytes()
                    .to_vec()
            } else {
                s.read_tile(s.active_layer, coord).map_err(internal)?
            };
            tiles.insert((tx, ty), bytes);
        }
    }
    let pixels = (y0..y1).flat_map(|py| {
        let tiles = &tiles;
        (x0..x1).map(move |px| {
            let t = &tiles[&(px / ts, py / ts)];
            let i = (((py % ts) * ts + px % ts) * 4) as usize;
            [t[i], t[i + 1], t[i + 2], t[i + 3]]
        })
    });
    let (c, count) = average(pixels);
    Ok(json!({
        "color": format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]),
        "count": count,
        "window": [x0, y0, x1 - x0, y1 - y0],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_clips_at_every_edge_and_corner() {
        // 10 x 8 canvas, 5 x 5 window.
        assert_eq!(sample_window(10, 8, 5, 4, 5), Some((3, 2, 8, 7)));
        assert_eq!(sample_window(10, 8, 0, 4, 5), Some((0, 2, 3, 7)));
        assert_eq!(sample_window(10, 8, 9, 4, 5), Some((7, 2, 10, 7)));
        assert_eq!(sample_window(10, 8, 5, 0, 5), Some((3, 0, 8, 3)));
        assert_eq!(sample_window(10, 8, 5, 7, 5), Some((3, 5, 8, 8)));
        assert_eq!(sample_window(10, 8, 0, 0, 5), Some((0, 0, 3, 3)));
        assert_eq!(sample_window(10, 8, 9, 0, 5), Some((7, 0, 10, 3)));
        assert_eq!(sample_window(10, 8, 0, 7, 5), Some((0, 5, 3, 8)));
        assert_eq!(sample_window(10, 8, 9, 7, 5), Some((7, 5, 10, 8)));
        // A 51 x 51 window on a 3 x 2 canvas is the whole canvas.
        assert_eq!(sample_window(3, 2, 1, 1, 51), Some((0, 0, 3, 2)));
        assert_eq!(sample_window(3, 2, 1, 1, 1), Some((1, 1, 2, 2)));
    }

    #[test]
    fn average_weights_by_alpha_and_rounds() {
        assert_eq!(average([[10, 20, 30, 255]]), ([10, 20, 30, 255], 1));
        assert_eq!(
            average([[0, 0, 0, 255], [255, 255, 255, 255]]),
            ([128, 128, 128, 255], 2)
        );
        // A transparent pixel lowers alpha but not the color.
        assert_eq!(
            average([[200, 100, 50, 255], [0, 0, 0, 0]]),
            ([200, 100, 50, 128], 2)
        );
        assert_eq!(average([[9, 9, 9, 0], [1, 2, 3, 0]]), ([0, 0, 0, 0], 2));
        assert_eq!(average(std::iter::empty()), ([0, 0, 0, 0], 0));
    }
}
