//! Sparse, deterministic brush strokes over immutable document tiles.
use op_core::doc::{DocError, DocState, LayerId, SelectionState, ToolCompositeMode};
use op_core::pixel::{Rgba8, round_unit_to_u8};
use op_core::selection::RasterMode;
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::composite::tool_composite;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Paintbrush,
    Eraser,
    Pencil,
}
impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paintbrush => "Paintbrush",
            Self::Eraser => "Eraser",
            Self::Pencil => "Pencil",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureMode {
    None,
    Size,
    Opacity,
    SizeAndOpacity,
}
#[derive(Clone, Debug)]
pub struct BrushSettings {
    pub tool: Tool,
    pub width: f64,
    pub color: [u8; 4],
    pub antialias: bool,
    pub spacing: f64,
    pub pressure: PressureMode,
    pub blend: ToolCompositeMode,
}
impl BrushSettings {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.width.is_finite() || !(1.0..=2000.0).contains(&self.width) {
            return Err("brush width must be 1..2000".into());
        }
        if !self.spacing.is_finite() || self.spacing <= 0.0 || self.spacing > 1.0 {
            return Err("spacing must be in (0,1]".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeSample {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub timestamp_ms: u64,
    pub tilt_x: f64,
    pub tilt_y: f64,
}
impl StrokeSample {
    pub fn mouse(x: f64, y: f64, timestamp_ms: u64) -> Self {
        Self {
            x,
            y,
            pressure: 1.0,
            timestamp_ms,
            tilt_x: 0.0,
            tilt_y: 0.0,
        }
    }
}
#[derive(Debug)]
pub enum StrokeError {
    BadInput(String),
    Document(DocError),
}
impl std::fmt::Display for StrokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(s) => write!(f, "{s}"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for StrokeError {}
impl From<DocError> for StrokeError {
    fn from(e: DocError) -> Self {
        Self::Document(e)
    }
}
pub type Result<T> = std::result::Result<T, StrokeError>;
/// One stroke owns its base pixels and sparse max-combined mask. Only dirty tiles
/// are recomposited when samples arrive.
pub struct StrokeSession {
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub settings: BrushSettings,
    pub seed: u64,
    base: DocState,
    preview: DocState,
    mask: BTreeMap<(i32, i32), Box<[f32]>>,
    last: Option<StrokeSample>,
    since_dab: f64,
    selection: Option<Arc<op_core::selection::MaskGrid>>,
    dirty: BTreeSet<(i32, i32)>,
    pencil_visited: HashSet<(i32, i32)>,
}
impl StrokeSession {
    pub fn new(base: DocState, layer: usize, settings: BrushSettings, seed: u64) -> Result<Self> {
        settings.validate().map_err(StrokeError::BadInput)?;
        let target_layer = base
            .layers
            .get(layer)
            .ok_or_else(|| StrokeError::BadInput("invalid layer".into()))?
            .id;
        let selection = if matches!(base.selection, SelectionState::None) {
            None
        } else {
            Some(base.rasterize_selection(RasterMode::Antialiased))
        };
        Ok(Self {
            base_revision: base.revision,
            target_layer,
            settings,
            seed,
            preview: base.clone(),
            base,
            mask: BTreeMap::new(),
            last: None,
            since_dab: 0.0,
            selection,
            dirty: BTreeSet::new(),
            pencil_visited: HashSet::new(),
        })
    }
    pub fn append(&mut self, samples: &[StrokeSample]) -> Result<&BTreeSet<(i32, i32)>> {
        let mut previous = self.last;
        for &s in samples {
            if !s.x.is_finite()
                || !s.y.is_finite()
                || !s.pressure.is_finite()
                || !(0.0..=1.0).contains(&s.pressure)
                || !s.tilt_x.is_finite()
                || !s.tilt_y.is_finite()
            {
                return Err(StrokeError::BadInput("invalid stroke sample".into()));
            }
            if let Some(prev) = previous {
                if s.timestamp_ms < prev.timestamp_ms {
                    return Err(StrokeError::BadInput("timestamp moved backward".into()));
                }
                if !((s.x - prev.x).hypot(s.y - prev.y)).is_finite() {
                    return Err(StrokeError::BadInput("stroke distance overflow".into()));
                }
            }
            previous = Some(s);
        }
        self.dirty.clear();
        for &s in samples {
            if let Some(prev) = self.last {
                if self.settings.tool == Tool::Pencil {
                    self.pencil_segment(prev, s);
                } else {
                    let dx = s.x - prev.x;
                    let dy = s.y - prev.y;
                    let len = dx.hypot(dy);
                    if len > 0.0 {
                        let step = (self.settings.width * self.settings.spacing).max(0.5);
                        let mut traveled = step - self.since_dab;
                        while traveled <= len + 1e-9 {
                            let t = (traveled / len).min(1.0);
                            self.dab(
                                prev.x + dx * t,
                                prev.y + dy * t,
                                prev.pressure + (s.pressure - prev.pressure) * t,
                            );
                            traveled += step;
                        }
                        self.since_dab = (self.since_dab + len) % step;
                    }
                }
            } else if self.settings.tool == Tool::Pencil {
                self.pencil_pixel(s.x.floor() as i32, s.y.floor() as i32, s.pressure);
            } else {
                self.dab(s.x, s.y, s.pressure);
            }
            self.last = Some(s);
        }
        for &(tx, ty) in &self.dirty {
            let coord = TileCoord { x: tx, y: ty };
            let mut tile = self.base.read_tile(self.layer_index()?, coord)?;
            let mask = &self.mask[&(tx, ty)];
            let base_x = coord.x as usize * TILE_SIZE;
            let base_y = coord.y as usize * TILE_SIZE;
            for (i, &coverage) in mask.iter().enumerate() {
                if coverage <= 0.0 {
                    continue;
                }
                let x = base_x + i % TILE_SIZE;
                let y = base_y + i / TILE_SIZE;
                let sel = self
                    .selection
                    .as_ref()
                    .map_or(255, |m| m.bytes[y * m.width as usize + x]);
                let cov = round_unit_to_u8(coverage * sel as f32 / 255.0);
                if cov == 0 {
                    continue;
                }
                let k = i * 4;
                let old = Rgba8::from([tile[k], tile[k + 1], tile[k + 2], tile[k + 3]]);
                let new = if self.settings.tool == Tool::Eraser {
                    let mut p = old;
                    p.a = round_unit_to_u8(
                        old.a as f32 / 255.0
                            * (1.0 - cov as f32 / 255.0 * self.settings.color[3] as f32 / 255.0),
                    );
                    p
                } else {
                    tool_composite(
                        old,
                        Rgba8::from(self.settings.color),
                        self.settings.blend,
                        cov,
                    )
                };
                tile[k..k + 4].copy_from_slice(&<[u8; 4]>::from(new));
            }
            self.preview = self.preview.write_tile(
                self.layer_index()?,
                coord,
                TileData::try_from(tile).map_err(DocError::from)?,
            )?;
        }
        Ok(&self.dirty)
    }
    fn layer_index(&self) -> Result<usize> {
        self.base
            .layers
            .iter()
            .position(|l| l.id == self.target_layer)
            .ok_or_else(|| StrokeError::BadInput("target layer gone".into()))
    }
    pub fn preview(&self) -> &DocState {
        &self.preview
    }
    pub fn finish(self) -> Result<DocState> {
        Ok(self.preview)
    }
    pub fn touched_tiles(&self) -> usize {
        self.mask.len()
    }
    fn pencil_segment(&mut self, a: StrokeSample, b: StrokeSample) {
        let (mut x, mut y) = (a.x.floor() as i32, a.y.floor() as i32);
        let (x1, y1) = (b.x.floor() as i32, b.y.floor() as i32);
        let dx = (x1 - x).abs();
        let sx = if x < x1 { 1 } else { -1 };
        let dy = -(y1 - y).abs();
        let sy = if y < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let steps = dx.max(-dy).max(1);
        let mut n = 0;
        loop {
            self.pencil_pixel(
                x,
                y,
                a.pressure + (b.pressure - a.pressure) * n as f64 / steps as f64,
            );
            if x == x1 && y == y1 {
                break;
            }
            let e = 2 * err;
            if e >= dy {
                err += dy;
                x += sx;
            }
            if e <= dx {
                err += dx;
                y += sy;
            }
            n += 1;
        }
    }
    fn pencil_pixel(&mut self, x: i32, y: i32, pressure: f64) {
        if !self.pencil_visited.insert((x, y)) {
            return;
        }
        let c = self.pressure_coverage(pressure);
        self.mark(x, y, c);
    }
    fn pressure_coverage(&self, p: f64) -> f32 {
        if matches!(
            self.settings.pressure,
            PressureMode::Opacity | PressureMode::SizeAndOpacity
        ) {
            p as f32
        } else {
            1.0
        }
    }
    fn dab(&mut self, x: f64, y: f64, pressure: f64) {
        let width = if matches!(
            self.settings.pressure,
            PressureMode::Size | PressureMode::SizeAndOpacity
        ) {
            self.settings.width * pressure
        } else {
            self.settings.width
        };
        if width <= 0.0 {
            return;
        }
        let radius = width / 2.0;
        let opacity = self.pressure_coverage(pressure);
        let min_x = ((x - radius - 1.0).floor() as i64)
            .max(0)
            .min(self.base.width as i64) as i32;
        let max_x = ((x + radius + 1.0).ceil() as i64)
            .max(0)
            .min(self.base.width as i64) as i32;
        let min_y = ((y - radius - 1.0).floor() as i64)
            .max(0)
            .min(self.base.height as i64) as i32;
        let max_y = ((y + radius + 1.0).ceil() as i64)
            .max(0)
            .min(self.base.height as i64) as i32;
        for py in min_y..max_y {
            for px in min_x..max_x {
                let area = if self.settings.antialias {
                    circle_pixel(x, y, radius, px as f64, py as f64)
                } else {
                    f64::from(((px as f64 + 0.5 - x).hypot(py as f64 + 0.5 - y) <= radius) as u8)
                };
                if area > 0.0 {
                    self.mark(px, py, (area as f32) * opacity);
                }
            }
        }
    }
    fn mark(&mut self, x: i32, y: i32, value: f32) {
        if value <= 0.0
            || x < 0
            || y < 0
            || x >= self.base.width as i32
            || y >= self.base.height as i32
        {
            return;
        }
        let coord = TileCoord {
            x: x / TILE_SIZE as i32,
            y: y / TILE_SIZE as i32,
        };
        let i = (y as usize % TILE_SIZE) * TILE_SIZE + x as usize % TILE_SIZE;
        let tile = self
            .mask
            .entry((coord.x, coord.y))
            .or_insert_with(|| vec![0.0; TILE_SIZE * TILE_SIZE].into_boxed_slice());
        if value > tile[i] {
            tile[i] = value;
            self.dirty.insert((coord.x, coord.y));
        }
    }
}
fn primitive(x: f64, r: f64) -> f64 {
    let x = x.clamp(0.0, r);
    0.5 * (x * (r * r - x * x).max(0.0).sqrt() + r * r * (x / r).asin())
}
fn quadrant(x: f64, y: f64, r: f64) -> f64 {
    let x = x.min(r);
    let y = y.min(r);
    if x <= 0.0 || y <= 0.0 {
        return 0.0;
    }
    let split = (r * r - y * y).max(0.0).sqrt().min(x);
    y * split + primitive(x, r) - primitive(split, r)
}
fn signed_area(x: f64, y: f64, r: f64) -> f64 {
    x.signum() * y.signum() * quadrant(x.abs(), y.abs(), r)
}
fn circle_pixel(cx: f64, cy: f64, r: f64, x: f64, y: f64) -> f64 {
    let a = x - cx;
    let b = y - cy;
    (signed_area(a + 1.0, b + 1.0, r) - signed_area(a, b + 1.0, r) - signed_area(a + 1.0, b, r)
        + signed_area(a, b, r))
    .clamp(0.0, 1.0)
}
