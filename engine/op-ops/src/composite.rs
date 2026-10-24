//! Encoded-space layer compositing. Inputs and output are straight RGBA8; the stack
//! stays premultiplied until its final rounding. Transparent output is black.
use op_core::doc::{DocError, DocState, LayerBlendMode, ToolCompositeMode};
use op_core::pixel::{Rgba8, round_unit_to_u8, round_unit_to_u8_f64};
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData};
use std::collections::{HashMap, HashSet};

// Formulas derived from OpenPDN's MIT-licensed UserBlendOps.Generated.cs,
// commit cca476b0df2a2f70996e6b9486ec45327631568c. Its lhs is backdrop,
// rhs is source, and its byte layout is BGRA rather than our RGBA.
fn blend64(mode: LayerBlendMode, b: f64, s: f64) -> f64 {
    use LayerBlendMode::*;
    match mode {
        Normal => s,
        Multiply => b * s,
        Additive => (b + s).min(1.0),
        ColorBurn => {
            if s == 0.0 {
                0.0
            } else {
                (1.0 - (1.0 - b) / s).max(0.0)
            }
        }
        ColorDodge => {
            if s == 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        Reflect => {
            if s == 1.0 {
                1.0
            } else {
                (b * b / (1.0 - s)).min(1.0)
            }
        }
        Glow => {
            if b == 1.0 {
                1.0
            } else {
                (s * s / (1.0 - b)).min(1.0)
            }
        }
        Overlay => {
            if b < 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        Difference => (b - s).abs(),
        Negation => 1.0 - (1.0 - b - s).abs(),
        Lighten => b.max(s),
        Darken => b.min(s),
        Screen => b + s - b * s,
        // 3.36 XOR is channel-byte XOR, not Porter-Duff XOR. Float inputs
        // are mapped to bytes with the shared rounding rule before XOR.
        Xor => (round_unit_to_u8_f64(b) ^ round_unit_to_u8_f64(s)) as f64 / 255.0,
    }
}
fn blend32(mode: LayerBlendMode, b: f32, s: f32) -> f32 {
    use LayerBlendMode::*;
    match mode {
        Normal => s,
        Multiply => b * s,
        Additive => (b + s).min(1.0),
        ColorBurn => {
            if s == 0.0 {
                0.0
            } else {
                (1.0 - (1.0 - b) / s).max(0.0)
            }
        }
        ColorDodge => {
            if s == 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        Reflect => {
            if s == 1.0 {
                1.0
            } else {
                (b * b / (1.0 - s)).min(1.0)
            }
        }
        Glow => {
            if b == 1.0 {
                1.0
            } else {
                (s * s / (1.0 - b)).min(1.0)
            }
        }
        Overlay => {
            if b < 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        Difference => (b - s).abs(),
        Negation => 1.0 - (1.0 - b - s).abs(),
        Lighten => b.max(s),
        Darken => b.min(s),
        Screen => b + s - b * s,
        Xor => (round_unit_to_u8(b) ^ round_unit_to_u8(s)) as f32 / 255.0,
    }
}

#[derive(Clone, Copy, Default)]
struct Acc32 {
    c: [f32; 3],
    a: f32,
}
impl Acc32 {
    fn over(&mut self, src: Rgba8, mode: LayerBlendMode, opacity: u8) {
        let sa = src.a as f32 / 255.0 * (opacity as f32 / 255.0);
        if sa == 0.0 {
            return;
        }
        let ba = self.a;
        let sc = [src.r, src.g, src.b];
        for (i, byte) in sc.iter().enumerate() {
            let s = *byte as f32 / 255.0;
            let b = if ba > 0.0 { self.c[i] / ba } else { 0.0 };
            self.c[i] =
                sa * (1.0 - ba) * s + (1.0 - sa) * self.c[i] + sa * ba * blend32(mode, b, s);
        }
        self.a = sa + ba * (1.0 - sa);
    }
    fn pixel(self) -> Rgba8 {
        if self.a <= 0.0 {
            return Rgba8::default();
        }
        [
            round_unit_to_u8(self.c[0] / self.a),
            round_unit_to_u8(self.c[1] / self.a),
            round_unit_to_u8(self.c[2] / self.a),
            round_unit_to_u8(self.a),
        ]
        .into()
    }
}

/// Scalar f64 oracle with no quantization between layers.
pub mod spec {
    use super::*;
    #[derive(Clone, Copy, Default)]
    pub struct Acc {
        pub c: [f64; 3],
        pub a: f64,
    }
    impl Acc {
        pub fn over(&mut self, src: Rgba8, mode: LayerBlendMode, opacity: u8) {
            let sa = src.a as f64 / 255.0 * (opacity as f64 / 255.0);
            if sa == 0.0 {
                return;
            }
            let ba = self.a;
            for (i, byte) in [src.r, src.g, src.b].iter().enumerate() {
                let s = *byte as f64 / 255.0;
                let b = if ba > 0.0 { self.c[i] / ba } else { 0.0 };
                self.c[i] =
                    sa * (1.0 - ba) * s + ba * (1.0 - sa) * b + sa * ba * blend64(mode, b, s);
            }
            self.a = sa + ba * (1.0 - sa);
        }
        pub fn pixel(self) -> Rgba8 {
            if self.a <= 0.0 {
                return Rgba8::default();
            }
            [
                round_unit_to_u8_f64(self.c[0] / self.a),
                round_unit_to_u8_f64(self.c[1] / self.a),
                round_unit_to_u8_f64(self.c[2] / self.a),
                round_unit_to_u8_f64(self.a),
            ]
            .into()
        }
    }
    pub fn composite_pixel(bottom: Rgba8, top: Rgba8, mode: LayerBlendMode, opacity: u8) -> Rgba8 {
        let mut acc = Acc::default();
        acc.over(bottom, LayerBlendMode::Normal, 255);
        acc.over(top, mode, opacity);
        acc.pixel()
    }
}

pub fn composite_pixel(bottom: Rgba8, top: Rgba8, mode: LayerBlendMode, opacity: u8) -> Rgba8 {
    let mut acc = Acc32::default();
    acc.over(bottom, LayerBlendMode::Normal, 255);
    acc.over(top, mode, opacity);
    acc.pixel()
}
/// Coverage is a byte mask. Overwrite replaces color and alpha inside that mask.
pub fn tool_composite(dst: Rgba8, src: Rgba8, mode: ToolCompositeMode, coverage: u8) -> Rgba8 {
    match mode {
        ToolCompositeMode::Normal => composite_pixel(dst, src, LayerBlendMode::Normal, coverage),
        ToolCompositeMode::Overwrite => {
            let t = coverage as f32 / 255.0;
            [dst.r, dst.g, dst.b, dst.a]
                .into_iter()
                .zip([src.r, src.g, src.b, src.a])
                .map(|(d, s)| round_unit_to_u8((d as f32 * (1.0 - t) + s as f32 * t) / 255.0))
                .collect::<Vec<_>>()
                .try_into()
                .map(|bytes: [u8; 4]| Rgba8::from(bytes))
                .unwrap()
        }
    }
}

pub fn composite_tile(doc: &DocState, coord: TileCoord) -> Result<TileData, DocError> {
    if coord.x < 0
        || coord.y < 0
        || coord.x as u64 >= (doc.width as u64).div_ceil(TILE_SIZE as u64)
        || coord.y as u64 >= (doc.height as u64).div_ceil(TILE_SIZE as u64)
    {
        return Err(DocError::TileBounds);
    }
    let mut state = vec![Acc32::default(); TILE_SIZE * TILE_SIZE];
    for (index, layer) in doc.layers.iter().enumerate() {
        if !layer.visible || layer.opacity == 0 || layer.tiles.get(coord).is_none() {
            continue;
        }
        let tile = doc.read_tile(index, coord)?;
        for (acc, rgba) in state.iter_mut().zip(tile.chunks_exact(4)) {
            acc.over(
                [rgba[0], rgba[1], rgba[2], rgba[3]].into(),
                layer.blend,
                layer.opacity,
            );
        }
    }
    let mut out = Vec::with_capacity(TILE_BYTES);
    for acc in state {
        out.extend_from_slice(&<[u8; 4]>::from(acc.pixel()));
    }
    Ok(TileData::try_from(out).expect("fixed tile size"))
}

#[derive(Default)]
pub struct CompositeCache {
    tiles: HashMap<TileCoord, TileData>,
    signatures: HashMap<TileCoord, Vec<Option<op_core::tiles::TileId>>>,
    properties: Vec<(op_core::doc::LayerId, String, bool, u8, LayerBlendMode)>,
    size: (u32, u32),
    dirty: HashSet<TileCoord>,
}
impl CompositeCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn invalidate_tiles(&mut self, coords: impl IntoIterator<Item = TileCoord>) {
        self.dirty.extend(coords);
    }
    pub fn invalidate_all(&mut self) {
        self.tiles.clear();
        self.signatures.clear();
        self.dirty.clear();
    }
    pub fn get(&mut self, doc: &DocState, coord: TileCoord) -> Result<&TileData, DocError> {
        let properties: Vec<_> = doc
            .layers
            .iter()
            .map(|l| (l.id, l.name.clone(), l.visible, l.opacity, l.blend))
            .collect();
        if self.size != (doc.width, doc.height) || self.properties != properties {
            self.invalidate_all();
            self.size = (doc.width, doc.height);
            self.properties = properties;
        }
        let signature: Vec<_> = doc.layers.iter().map(|l| l.tiles.get(coord)).collect();
        if self.dirty.remove(&coord) || self.signatures.get(&coord) != Some(&signature) {
            self.tiles.remove(&coord);
        }
        if !self.tiles.contains_key(&coord) {
            let tile = composite_tile(doc, coord)?;
            self.signatures.insert(coord, signature);
            self.tiles.insert(coord, tile);
        }
        Ok(self.tiles.get(&coord).expect("inserted"))
    }
}
