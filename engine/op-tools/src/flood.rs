//! Flood tools port the integer color test and four-connected scanline search from
//! OpenPDN 3.36, `src/tools/FloodToolBase.cs`, commit cca476b (MIT).
//! Copyright dotPDN LLC, Rick Brewster, Tom Jackson, and contributors.
//! The premultiplied mode and antialias edge rule are provisional until paint.net 5.1 captures exist.

use op_core::doc::{DocError, DocState, SelectionState, ToolCompositeMode};
use op_core::pixel::Rgba8;
use op_core::selection::{CombineMode, MaskGrid, RasterMode, Selection};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::composite::{composite_tile, tool_composite};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Straight,
    Premultiplied,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    Layer,
    Image,
}
#[derive(Clone, Copy, Debug)]
pub struct FloodOptions {
    pub contiguous: bool,
    pub tolerance: f64,
    pub alpha: AlphaMode,
    pub sampling: Sampling,
    pub antialias: bool,
}
impl FloodOptions {
    fn threshold(self) -> Result<i32, String> {
        if !self.tolerance.is_finite() || !(0.0..=1.0).contains(&self.tolerance) {
            return Err("tolerance must be 0..1".into());
        }
        Ok((self.tolerance * self.tolerance * 256.0).trunc() as i32)
    }
}
fn premultiply(c: [u8; 4]) -> [u8; 4] {
    [
        ((c[0] as u16 * c[3] as u16) / 255) as u8,
        ((c[1] as u16 * c[3] as u16) / 255) as u8,
        ((c[2] as u16 * c[3] as u16) / 255) as u8,
        c[3],
    ]
}
/// The three RGB divisions each truncate separately, matching C# integer arithmetic.
pub fn tolerance_distance(seed: [u8; 4], pixel: [u8; 4], alpha: AlphaMode) -> i32 {
    let (a, b) = if alpha == AlphaMode::Premultiplied {
        (premultiply(seed), premultiply(pixel))
    } else {
        (seed, pixel)
    };
    let mut sum = 0;
    for c in 0..3 {
        let diff = a[c] as i32 - b[c] as i32;
        sum += (1 + diff * diff) * a[3] as i32 / 256;
    }
    let diff = a[3] as i32 - b[3] as i32;
    sum + diff * diff
}
fn source_tiles(
    doc: &DocState,
    layer: usize,
    sampling: Sampling,
) -> Result<BTreeMap<(usize, usize), Vec<u8>>, DocError> {
    let mut tiles = BTreeMap::new();
    for ty in 0..(doc.height as usize).div_ceil(TILE_SIZE) {
        for tx in 0..(doc.width as usize).div_ceil(TILE_SIZE) {
            let c = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let bytes = match sampling {
                Sampling::Layer => doc.read_tile(layer, c)?,
                Sampling::Image => composite_tile(doc, c)?.as_bytes().to_vec(),
            };
            tiles.insert((tx, ty), bytes);
        }
    }
    Ok(tiles)
}
fn at(tiles: &BTreeMap<(usize, usize), Vec<u8>>, x: usize, y: usize) -> [u8; 4] {
    let t = &tiles[&(x / TILE_SIZE, y / TILE_SIZE)];
    let k = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4;
    t[k..k + 4].try_into().expect("pixel")
}
fn bit_get(bits: &[u8], i: usize) -> bool {
    bits[i / 8] & (1 << (i % 8)) != 0
}
fn bit_set(bits: &mut [u8], i: usize) {
    bits[i / 8] |= 1 << (i % 8);
}

/// Detects against one frozen source. Contiguous search compares every candidate with
/// the seed; only shared edges connect pixels, as in the OpenPDN source.
pub fn detect_region(
    doc: &DocState,
    layer: usize,
    seed: (u32, u32),
    options: FloodOptions,
) -> Result<MaskGrid, String> {
    let t = options.threshold()?;
    if layer >= doc.layers.len() || seed.0 >= doc.width || seed.1 >= doc.height {
        return Err("invalid layer or seed".into());
    }
    let (w, h) = (doc.width as usize, doc.height as usize);
    let tiles = source_tiles(doc, layer, options.sampling).map_err(|e| e.to_string())?;
    let seed_color = at(&tiles, seed.0 as usize, seed.1 as usize);
    let limit = 4 * t * t;
    let matches = |x: usize, y: usize| {
        tolerance_distance(seed_color, at(&tiles, x, y), options.alpha) <= limit
    };
    let mut bytes = vec![0; w * h];
    if options.contiguous {
        let mut visited = vec![0; (w * h).div_ceil(8)];
        let mut queue = VecDeque::new();
        queue.push_back((seed.0 as usize, seed.1 as usize));
        while let Some((x, y)) = queue.pop_front() {
            let k = y * w + x;
            if bit_get(&visited, k) || !matches(x, y) {
                continue;
            }
            let mut left = x;
            while left > 0 && !bit_get(&visited, y * w + left - 1) && matches(left - 1, y) {
                left -= 1;
            }
            let mut right = x;
            while right + 1 < w && !bit_get(&visited, y * w + right + 1) && matches(right + 1, y) {
                right += 1;
            }
            for col in left..=right {
                let i = y * w + col;
                bit_set(&mut visited, i);
                bytes[i] = 255;
            }
            for ny in [y.checked_sub(1), (y + 1 < h).then_some(y + 1)]
                .into_iter()
                .flatten()
            {
                let mut in_run = false;
                for col in left..=right {
                    let valid = !bit_get(&visited, ny * w + col) && matches(col, ny);
                    if valid && !in_run {
                        queue.push_back((col, ny));
                    }
                    in_run = valid;
                }
            }
        }
    } else {
        for y in 0..h {
            for x in 0..w {
                if matches(x, y) {
                    bytes[y * w + x] = 255;
                }
            }
        }
    }
    if options.antialias {
        // Provisional rule: average each binary pixel with its four edge neighbors.
        // Missing neighbors count as zero. Detection itself always uses the binary mask.
        let binary = bytes.clone();
        for y in 0..h {
            for x in 0..w {
                let mut sum = binary[y * w + x] as u16;
                for (nx, ny) in [
                    (x.checked_sub(1), Some(y)),
                    ((x + 1 < w).then_some(x + 1), Some(y)),
                    (Some(x), y.checked_sub(1)),
                    (Some(x), (y + 1 < h).then_some(y + 1)),
                ] {
                    if let (Some(nx), Some(ny)) = (nx, ny) {
                        sum += binary[ny * w + nx] as u16;
                    }
                }
                bytes[y * w + x] = (sum / 5) as u8;
            }
        }
    }
    MaskGrid::new(doc.width, doc.height, bytes).ok_or_else(|| "invalid mask".into())
}

pub fn magic_wand(
    doc: &DocState,
    layer: usize,
    seed: (u32, u32),
    options: FloodOptions,
    mode: CombineMode,
) -> Result<DocState, String> {
    let mask = detect_region(doc, layer, seed, options)?;
    // With no selection, every mode combines with an empty selection (as
    // `selection.rectangle` does): Add and Invert give the region, Subtract
    // and Intersect give nothing. `Selection::none()` rasterizes as all
    // covered, which made Add select the whole canvas.
    let base = match doc.selection {
        SelectionState::None => Selection::empty(),
        _ => Selection::new(doc.selection.clone()),
    };
    let selected = base.combine(
        &Selection::active_coverage(mask),
        mode,
        doc.width,
        doc.height,
    );
    Ok(doc.set_selection(selected.state().clone()))
}

pub fn paint_bucket(
    doc: &DocState,
    layer: usize,
    seed: (u32, u32),
    options: FloodOptions,
    color: [u8; 4],
    blend: ToolCompositeMode,
) -> Result<DocState, String> {
    let region = detect_region(doc, layer, seed, options)?;
    let selection = if matches!(doc.selection, SelectionState::None) {
        None
    } else {
        Some(doc.rasterize_selection(RasterMode::Antialiased))
    };
    let mut next = doc.clone();
    for ty in 0..(doc.height as usize).div_ceil(TILE_SIZE) {
        for tx in 0..(doc.width as usize).div_ceil(TILE_SIZE) {
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let mut tile = doc.read_tile(layer, coord).map_err(|e| e.to_string())?;
            let mut changed = false;
            for ly in 0..TILE_SIZE {
                let y = ty * TILE_SIZE + ly;
                if y >= doc.height as usize {
                    break;
                }
                for lx in 0..TILE_SIZE {
                    let x = tx * TILE_SIZE + lx;
                    if x >= doc.width as usize {
                        break;
                    }
                    let i = y * doc.width as usize + x;
                    let coverage = region.bytes[i] as u16
                        * selection.as_ref().map_or(255, |s| s.bytes[i]) as u16
                        / 255;
                    if coverage == 0 {
                        continue;
                    }
                    let k = (ly * TILE_SIZE + lx) * 4;
                    let old = Rgba8::from(<[u8; 4]>::try_from(&tile[k..k + 4]).expect("pixel"));
                    let new = tool_composite(old, Rgba8::from(color), blend, coverage as u8);
                    tile[k..k + 4].copy_from_slice(&<[u8; 4]>::from(new));
                    changed = true;
                }
            }
            if changed {
                next = next
                    .write_tile(
                        layer,
                        coord,
                        TileData::try_from(tile).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(next)
}
