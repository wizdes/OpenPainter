//! Provisional 5.1 Quantize: https://paint.net/doc/latest/EffectsColorMenu.html.
//! The palette is built from the whole active layer in a fixed scan order.
use crate::effect::*;
use crate::effects::distort_common as d;
use op_core::doc::{DocState, SelectionState};
use op_core::tiles::{TILE_SIZE, TileCoord};
use serde_json::{Value, json};
use std::sync::Arc;

fn palette(doc: &DocState, colors: usize, method: &str) -> Result<Vec<[u8; 3]>> {
    let mut pixels = Vec::new();
    for y in 0..doc.height {
        let mut last_tile_x = None;
        let mut tile = None;
        for x in 0..doc.width {
            let tile_x = (x as usize / TILE_SIZE) as i32;
            if last_tile_x != Some(tile_x) {
                let coord = TileCoord {
                    x: tile_x,
                    y: (y as usize / TILE_SIZE) as i32,
                };
                tile = Some(
                    doc.read_tile(doc.active_layer, coord)
                        .map_err(|e| EffectError::Tile(e.to_string()))?,
                );
                last_tile_x = Some(tile_x);
            }
            let tile = tile.as_ref().unwrap();
            let index = ((y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE) * 4;
            if tile[index + 3] > 0 {
                pixels.push([tile[index], tile[index + 1], tile[index + 2]]);
            }
        }
    }
    if pixels.is_empty() {
        return Ok(vec![[0, 0, 0]]);
    }
    if method == "octree" {
        // Fixed-depth octree buckets; ties are broken by the bucket key.
        let bits = (1..=8)
            .find(|&n| (1usize << (3 * n.min(3))) >= colors)
            .unwrap_or(3)
            .min(3);
        let mut buckets = std::collections::BTreeMap::<u32, [u64; 4]>::new();
        for p in pixels {
            let key = ((p[0] as u32 >> (8 - bits)) << (2 * bits))
                | ((p[1] as u32 >> (8 - bits)) << bits)
                | (p[2] as u32 >> (8 - bits));
            let b = buckets.entry(key).or_default();
            for c in 0..3 {
                b[c] += u64::from(p[c]);
            }
            b[3] += 1;
        }
        let mut ranked: Vec<_> = buckets.into_iter().collect();
        ranked.sort_unstable_by_key(|(key, bucket)| (std::cmp::Reverse(bucket[3]), *key));
        return Ok(ranked
            .into_iter()
            .take(colors)
            .map(|(_, b)| {
                [
                    (b[0] / b[3]) as u8,
                    (b[1] / b[3]) as u8,
                    (b[2] / b[3]) as u8,
                ]
            })
            .collect());
    }
    let mut boxes = vec![pixels];
    while boxes.len() < colors {
        let Some((idx, _)) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let range = (0..3)
                    .map(|c| {
                        b.iter().map(|p| p[c]).max().unwrap()
                            - b.iter().map(|p| p[c]).min().unwrap()
                    })
                    .max()
                    .unwrap();
                (i, range)
            })
            .max_by_key(|&(i, range)| (range, usize::MAX - i))
        else {
            break;
        };
        let mut current = boxes.remove(idx);
        let channel = (0..3)
            .max_by_key(|&c| {
                (
                    current.iter().map(|p| p[c]).max().unwrap()
                        - current.iter().map(|p| p[c]).min().unwrap(),
                    3 - c,
                )
            })
            .unwrap();
        current.sort_unstable_by_key(|p| (p[channel], p[0], p[1], p[2]));
        let right = current.split_off(current.len() / 2);
        boxes.insert(idx, current);
        boxes.insert(idx + 1, right);
    }
    Ok(boxes
        .into_iter()
        .map(|b| {
            let n = b.len() as u64;
            let mut sum = [0u64; 3];
            for p in b {
                for c in 0..3 {
                    sum[c] += u64::from(p[c]);
                }
            }
            [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
        })
        .collect())
}
fn prepare(doc: &DocState, _: &SelectionState, v: &Value) -> Result<Prepared> {
    Ok(Prepared {
        width: doc.width as i32,
        height: doc.height as i32,
        palette: Some(Arc::new(palette(
            doc,
            v["colors"].as_u64().unwrap() as usize,
            v["method"].as_str().unwrap(),
        )?)),
        ..Prepared::default()
    })
}
fn render(s: &PixelReader<'_>, x: i32, y: i32, p: &Prepared, v: &Value) -> [u8; 4] {
    let original = s(x, y);
    if original[3] == 0 {
        return original;
    }
    let fallback = [[0, 0, 0], [255, 255, 255]];
    let palette = p
        .palette
        .as_deref()
        .map(|v| v.as_slice())
        .unwrap_or(&fallback);
    let level = v["dithering"].as_u64().unwrap() as i32;
    let bayer = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let delta = (bayer[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize] - 8) * level / 4;
    let adjusted = [0, 1, 2].map(|c| (i32::from(original[c]) + delta).clamp(0, 255) as u8);
    let color = palette
        .iter()
        .min_by_key(|candidate| {
            (0..3)
                .map(|c| {
                    let diff = i32::from(adjusted[c]) - i32::from(candidate[c]);
                    diff * diff
                })
                .sum::<i32>()
        })
        .unwrap();
    [color[0], color[1], color[2], original[3]]
}
pub fn spec() -> EffectSpec {
    EffectSpec {
        id: "effect.color.quantize",
        category: "Color",
        label_key: "effect.quantize",
        schema: ParamSchema {
            fields: vec![
                d::choice("method", &["median_cut", "octree"], "median_cut"),
                d::field("colors", ParamKind::Int, 2.0, 256.0, json!(16)),
                d::field("dithering", ParamKind::Int, 0.0, 8.0, json!(0)),
            ],
        },
        contract: d::contract(BorderRule::Clamp, "own"),
        prepare,
        required_input: d::input_whole,
        passes: &[PassKind::Reduction, PassKind::Pointwise],
        render_cpu: render,
        gpu: None,
    }
}
