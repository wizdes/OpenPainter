//! Move tools derived from OpenPDN's MIT-licensed `MoveTool.cs`,
//! `MoveSelectionTool.cs`, and `MoveToolBase.cs` (rivy/OpenPDN, cca476b).
//! A lift keeps the original pixels through all handle drags. Mouse release ends a
//! drag; `finish` ends the session and writes one history step.
use op_core::doc::{DocError, DocState, LayerId};
use op_core::selection::{MaskGrid, RasterMode, Selection, SelectionSource, SelectionState};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::image::Resampling;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f64; 6]);
impl Affine {
    pub const fn identity() -> Self {
        Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }
    pub const fn translation(dx: f64, dy: f64) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, dx, dy])
    }
    pub fn components(
        dx: f64,
        dy: f64,
        radians: f64,
        sx: f64,
        sy: f64,
        pivot: (f64, f64),
        lock_aspect: bool,
    ) -> Self {
        let sy = if lock_aspect { sx.copysign(sy) } else { sy };
        let (sn, cs) = radians.sin_cos();
        let (a, b, c, d) = (cs * sx, sn * sx, -sn * sy, cs * sy);
        Self([
            a,
            b,
            c,
            d,
            dx + pivot.0 - a * pivot.0 - c * pivot.1,
            dy + pivot.1 - b * pivot.0 - d * pivot.1,
        ])
    }
    pub fn inverse(self) -> Option<Self> {
        let [a, b, c, d, tx, ty] = self.0;
        let det = a * d - b * c;
        if !self.0.iter().all(|v| v.is_finite()) || !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        Some(Self([
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * ty - d * tx) / det,
            (b * tx - a * ty) / det,
        ]))
    }
    pub fn map(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, tx, ty] = self.0;
        (a * x + c * y + tx, b * x + d * y + ty)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Pixels,
    Selection,
}
#[derive(Clone, Copy, Debug)]
pub struct MoveOptions {
    pub transform: Affine,
    pub resampling: Resampling,
    pub copy: bool,
}
#[derive(Debug)]
pub enum MoveError {
    SingularTransform,
    Document(DocError),
}
impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SingularTransform => write!(f, "singular move transform"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for MoveError {}
impl From<DocError> for MoveError {
    fn from(e: DocError) -> Self {
        Self::Document(e)
    }
}

/// Pasted pixels floating over the active layer (M34 Paste): straight RGBA8,
/// `width` x `height`, with the top-left corner at (`x`, `y`) in document
/// pixels. It may reach past the canvas; the part outside shows once the move
/// brings it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloatImage {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct MoveSession {
    base: DocState,
    /// Paste: the floating pixels. The layer keeps its own pixels (nothing is
    /// cut) and the pasted pixels go over them.
    float: Option<FloatImage>,
    /// The history label `finish` should use instead of the kind's (paste).
    label: Option<String>,
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub kind: MoveKind,
    options: MoveOptions,
    original: Vec<u8>,
    coverage: Vec<u8>,
    generation: u64,
}
impl MoveSession {
    pub fn new(base: DocState, kind: MoveKind, options: MoveOptions) -> Result<Self, MoveError> {
        options
            .transform
            .inverse()
            .ok_or(MoveError::SingularTransform)?;
        let layer = base.active_layer;
        let original = read_layer(&base, layer)?;
        let coverage = match base.selection {
            SelectionState::None => vec![255; base.width as usize * base.height as usize],
            _ => base
                .rasterize_selection(RasterMode::Antialiased)
                .bytes
                .to_vec(),
        };
        Ok(Self {
            base_revision: base.revision,
            target_layer: base.layers[layer].id,
            base,
            float: None,
            label: None,
            kind,
            options,
            original,
            coverage,
            generation: 0,
        })
    }
    /// A paste session (M34): `float` floats over the active layer of `base`
    /// and moves like selected pixels. `base` should select the pasted bounds.
    pub fn paste(
        base: DocState,
        float: FloatImage,
        options: MoveOptions,
    ) -> Result<Self, MoveError> {
        options
            .transform
            .inverse()
            .ok_or(MoveError::SingularTransform)?;
        if float.width == 0
            || float.height == 0
            || float.rgba.len() != float.width as usize * float.height as usize * 4
        {
            return Err(MoveError::Document(DocError::Dimensions));
        }
        let layer = base.active_layer;
        let original = read_layer(&base, layer)?;
        Ok(Self {
            base_revision: base.revision,
            target_layer: base.layers[layer].id,
            base,
            float: Some(float),
            label: None,
            kind: MoveKind::Pixels,
            options,
            original,
            coverage: Vec::new(),
            generation: 0,
        })
    }
    /// Sets the history label for `finish` (such as "Paste").
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// The label set with `with_label`.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
    /// True for a paste session.
    pub fn is_paste(&self) -> bool {
        self.float.is_some()
    }
    pub fn update(&mut self, options: MoveOptions) -> Result<(), MoveError> {
        options
            .transform
            .inverse()
            .ok_or(MoveError::SingularTransform)?;
        self.options = options;
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn preview(&self) -> Result<DocState, MoveError> {
        let w = self.base.width as usize;
        let h = self.base.height as usize;
        let mut next = self.base.clone();
        let selection = self.transform_selection();
        if self.kind == MoveKind::Selection {
            return Ok(next.set_selection(selection));
        }
        if self.float.is_none()
            && self.options.transform == Affine::identity()
            && !self.options.copy
        {
            return Ok(next.set_selection(selection));
        }
        let inverse = self
            .options
            .transform
            .inverse()
            .ok_or(MoveError::SingularTransform)?;
        let mut out = self.original.clone();
        if self.float.is_none() && !self.options.copy {
            // For coverage c and source alpha a, the cut keeps a(1-c) and the
            // floating part gets ac. Source-over of those parts at identity
            // gives a(1-c)+ac(1-a(1-c)), which is less than a. The identity
            // path above returns the original bytes instead of recompositing.
            for i in 0..w * h {
                let c = self.coverage[i] as f64 / 255.0;
                let j = i * 4;
                out[j + 3] = byte(self.original[j + 3] as f64 * (1.0 - c));
                if out[j + 3] == 0 {
                    out[j..j + 3].fill(0);
                }
            }
        }
        let exact = is_grid_transform(self.options.transform);
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = inverse.map(x as f64 + 0.5, y as f64 + 0.5);
                let sample = if exact {
                    self.exact_sample(sx, sy)
                } else {
                    self.filtered_sample(sx, sy)
                };
                let i = (y * w + x) * 4;
                if sample[3] == 0 {
                    continue;
                }
                let dest = [out[i], out[i + 1], out[i + 2], out[i + 3]];
                let mixed = over(sample, dest);
                out[i..i + 4].copy_from_slice(&mixed);
            }
        }
        for ty in 0..self.base.height.div_ceil(TILE_SIZE as u32) {
            for tx in 0..self.base.width.div_ceil(TILE_SIZE as u32) {
                let mut tile = vec![0; TILE_SIZE * TILE_SIZE * 4];
                for y in 0..(h - ty as usize * TILE_SIZE).min(TILE_SIZE) {
                    let count = (w - tx as usize * TILE_SIZE).min(TILE_SIZE) * 4;
                    let src = ((ty as usize * TILE_SIZE + y) * w + tx as usize * TILE_SIZE) * 4;
                    tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + count]
                        .copy_from_slice(&out[src..src + count]);
                }
                let coord = TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                };
                if tile != self.base.read_tile(self.base.active_layer, coord)? {
                    next = next.write_tile(
                        self.base.active_layer,
                        coord,
                        TileData::try_from(tile).map_err(DocError::from)?,
                    )?;
                }
            }
        }
        Ok(next.set_selection(selection))
    }
    pub fn finish(self) -> Result<DocState, MoveError> {
        self.preview()
    }
    fn transform_selection(&self) -> SelectionState {
        let source = Selection::new(self.base.selection.clone());
        match &self.base.selection {
            SelectionState::None => SelectionState::None,
            SelectionState::Active(SelectionSource::Geometry(_)) => source
                .affine(self.options.transform.0, self.base.width, self.base.height)
                .state()
                .clone(),
            SelectionState::Active(SelectionSource::Coverage(grid)) => {
                let inverse = self
                    .options
                    .transform
                    .inverse()
                    .expect("validated transform");
                let exact = is_grid_transform(self.options.transform);
                let mut out = vec![0; self.base.width as usize * self.base.height as usize];
                for y in 0..self.base.height as usize {
                    for x in 0..self.base.width as usize {
                        let (sx, sy) = inverse.map(x as f64 + 0.5, y as f64 + 0.5);
                        out[y * self.base.width as usize + x] = if exact {
                            let ix = (sx - 0.5).round() as isize;
                            let iy = (sy - 0.5).round() as isize;
                            if ix >= 0
                                && iy >= 0
                                && ix < grid.width as isize
                                && iy < grid.height as isize
                            {
                                grid.bytes[iy as usize * grid.width as usize + ix as usize]
                            } else {
                                0
                            }
                        } else {
                            sample_mask(grid, sx, sy, self.options.resampling)
                        };
                    }
                }
                SelectionState::Active(SelectionSource::Coverage(Arc::new(
                    MaskGrid::new(self.base.width, self.base.height, out).expect("matching mask"),
                )))
            }
        }
    }
    fn exact_sample(&self, sx: f64, sy: f64) -> [u8; 4] {
        self.source((sx - 0.5).round() as isize, (sy - 0.5).round() as isize)
    }
    /// The floating pixel at (x, y) before the transform; clear outside it.
    fn source(&self, x: isize, y: isize) -> [u8; 4] {
        if let Some(f) = &self.float {
            let (fx, fy) = (x as i64 - f.x, y as i64 - f.y);
            if fx < 0 || fy < 0 || fx >= f.width as i64 || fy >= f.height as i64 {
                return [0; 4];
            }
            let j = (fy as usize * f.width as usize + fx as usize) * 4;
            return [f.rgba[j], f.rgba[j + 1], f.rgba[j + 2], f.rgba[j + 3]];
        }
        if x < 0 || y < 0 || x >= self.base.width as isize || y >= self.base.height as isize {
            return [0; 4];
        }
        self.floating(y as usize * self.base.width as usize + x as usize)
    }
    /// The floating pixels' bounds before the transform: (x0, y0, x1, y1).
    fn source_bounds(&self) -> (f64, f64, f64, f64) {
        match &self.float {
            Some(f) => (
                f.x as f64,
                f.y as f64,
                f.x as f64 + f.width as f64,
                f.y as f64 + f.height as f64,
            ),
            None => (0.0, 0.0, self.base.width as f64, self.base.height as f64),
        }
    }
    fn floating(&self, p: usize) -> [u8; 4] {
        let j = p * 4;
        let mut v = [
            self.original[j],
            self.original[j + 1],
            self.original[j + 2],
            self.original[j + 3],
        ];
        v[3] = byte(v[3] as f64 * self.coverage[p] as f64 / 255.0);
        v
    }
    fn filtered_sample(&self, sx: f64, sy: f64) -> [u8; 4] {
        if self.options.resampling == Resampling::SuperSampling {
            let inverse = self
                .options
                .transform
                .inverse()
                .expect("validated transform");
            let nx = inverse.0[0].hypot(inverse.0[1]).ceil().clamp(1.0, 64.0) as usize;
            let ny = inverse.0[2].hypot(inverse.0[3]).ceil().clamp(1.0, 64.0) as usize;
            if nx > 1 || ny > 1 {
                let mut sum = [0.0; 4];
                for iy in 0..ny {
                    for ix in 0..nx {
                        let dx = (ix as f64 + 0.5) / nx as f64 - 0.5;
                        let dy = (iy as f64 + 0.5) / ny as f64 - 0.5;
                        let p = self.exact_sample(
                            sx + inverse.0[0] * dx + inverse.0[2] * dy,
                            sy + inverse.0[1] * dx + inverse.0[3] * dy,
                        );
                        let a = p[3] as f64 / 255.0;
                        sum[3] += a;
                        for c in 0..3 {
                            sum[c] += p[c] as f64 * a;
                        }
                    }
                }
                let count = (nx * ny) as f64;
                return if sum[3] > 0.0 {
                    [
                        byte(sum[0] / sum[3]),
                        byte(sum[1] / sum[3]),
                        byte(sum[2] / sum[3]),
                        byte(255.0 * sum[3] / count),
                    ]
                } else {
                    [0; 4]
                };
            }
        }
        let (x0, y0, x1, y1) = self.source_bounds();
        if sx < x0 - 3.0 || sy < y0 - 3.0 || sx > x1 + 3.0 || sy > y1 + 3.0 {
            return [0; 4];
        }
        let mode = effective_mode(self.options.resampling, self.options.transform);
        if mode == Resampling::Nearest {
            return self.exact_sample(sx, sy);
        }
        let radius = radius(mode);
        let mut sum = [0.0; 4];
        let mut weights = 0.0;
        let fx = sx - 0.5;
        let fy = sy - 0.5;
        for iy in (fy - radius).floor() as isize..=(fy + radius).ceil() as isize {
            for ix in (fx - radius).floor() as isize..=(fx + radius).ceil() as isize {
                let weight = kernel(mode, ix as f64 - fx) * kernel(mode, iy as f64 - fy);
                if weight == 0.0 {
                    continue;
                }
                weights += weight;
                let p = self.source(ix, iy);
                if p[3] == 0 {
                    continue;
                }
                let a = p[3] as f64 / 255.0;
                sum[3] += weight * a;
                for c in 0..3 {
                    sum[c] += weight * a * p[c] as f64 / 255.0;
                }
            }
        }
        if weights == 0.0 {
            return [0; 4];
        }
        let alpha = (sum[3] / weights).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return [0; 4];
        }
        [
            byte(255.0 * sum[0] / sum[3]),
            byte(255.0 * sum[1] / sum[3]),
            byte(255.0 * sum[2] / sum[3]),
            byte(255.0 * alpha),
        ]
    }
}
fn read_layer(s: &DocState, layer: usize) -> Result<Vec<u8>, DocError> {
    let mut out = vec![0; s.width as usize * s.height as usize * 4];
    for ty in 0..s.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..s.width.div_ceil(TILE_SIZE as u32) {
            let tile = s.read_tile(
                layer,
                TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                },
            )?;
            for y in 0..(s.height as usize - ty as usize * TILE_SIZE).min(TILE_SIZE) {
                let count = (s.width as usize - tx as usize * TILE_SIZE).min(TILE_SIZE) * 4;
                let dst = ((ty as usize * TILE_SIZE + y) * s.width as usize
                    + tx as usize * TILE_SIZE)
                    * 4;
                out[dst..dst + count]
                    .copy_from_slice(&tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + count]);
            }
        }
    }
    Ok(out)
}
fn byte(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}
fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    if src[3] == 255 || dst[3] == 0 {
        return src;
    }
    let sa = src[3] as f64 / 255.0;
    let da = dst[3] as f64 / 255.0;
    let a = sa + da * (1.0 - sa);
    if a == 0.0 {
        return [0; 4];
    }
    [
        byte((src[0] as f64 * sa + dst[0] as f64 * da * (1.0 - sa)) / a),
        byte((src[1] as f64 * sa + dst[1] as f64 * da * (1.0 - sa)) / a),
        byte((src[2] as f64 * sa + dst[2] as f64 * da * (1.0 - sa)) / a),
        byte(a * 255.0),
    ]
}
fn is_grid_transform(m: Affine) -> bool {
    let [a, b, c, d, tx, ty] = m.0;
    [a, b, c, d, tx, ty]
        .iter()
        .all(|v| (*v - v.round()).abs() < 1e-10)
        && (a * d - b * c).abs() == 1.0
        && (a * a + b * b - 1.0).abs() < 1e-10
        && (c * c + d * d - 1.0).abs() < 1e-10
}
fn effective_mode(mode: Resampling, m: Affine) -> Resampling {
    match mode {
        Resampling::BestQuality | Resampling::AdaptiveSharp => {
            let sx = m.0[0].hypot(m.0[1]);
            let sy = m.0[2].hypot(m.0[3]);
            if sx > 1.0 || sy > 1.0 {
                Resampling::Lanczos
            } else {
                Resampling::Bicubic
            }
        }
        Resampling::SuperSampling => Resampling::Bilinear,
        Resampling::Fant | Resampling::BilinearLowQuality => Resampling::Bilinear,
        x => x,
    }
}
fn radius(mode: Resampling) -> f64 {
    match mode {
        Resampling::Lanczos => 3.0,
        Resampling::Bicubic | Resampling::BicubicSmooth => 2.0,
        _ => 1.0,
    }
}
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let p = std::f64::consts::PI * x;
        p.sin() / p
    }
}
fn kernel(mode: Resampling, x: f64) -> f64 {
    let x = x.abs();
    match mode {
        Resampling::Bicubic => {
            if x < 1.0 {
                1.5 * x * x * x - 2.5 * x * x + 1.0
            } else if x < 2.0 {
                -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
            } else {
                0.0
            }
        }
        Resampling::BicubicSmooth => {
            if x < 1.0 {
                (4.0 - 6.0 * x * x + 3.0 * x * x * x) / 6.0
            } else if x < 2.0 {
                (2.0 - x).powi(3) / 6.0
            } else {
                0.0
            }
        }
        Resampling::Lanczos => {
            if x < 3.0 {
                sinc(x) * sinc(x / 3.0)
            } else {
                0.0
            }
        }
        _ => (1.0 - x).max(0.0),
    }
}
fn sample_mask(grid: &MaskGrid, sx: f64, sy: f64, mode: Resampling) -> u8 {
    let w = grid.width as isize;
    let h = grid.height as isize;
    if mode == Resampling::Nearest {
        let x = sx.floor() as isize;
        let y = sy.floor() as isize;
        if x < 0 || y < 0 || x >= w || y >= h {
            return 0;
        }
        return grid.bytes[y as usize * w as usize + x as usize];
    }
    let mode = effective_mode(mode, Affine::identity());
    let fx = sx - 0.5;
    let fy = sy - 0.5;
    let r = radius(mode);
    let mut sum = 0.0;
    for y in (fy - r).floor() as isize..=(fy + r).ceil() as isize {
        for x in (fx - r).floor() as isize..=(fx + r).ceil() as isize {
            if x >= 0 && y >= 0 && x < w && y < h {
                sum += kernel(mode, x as f64 - fx)
                    * kernel(mode, y as f64 - fy)
                    * grid.bytes[y as usize * w as usize + x as usize] as f64;
            }
        }
    }
    byte(sum)
}
