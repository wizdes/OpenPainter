//! Selection geometry, coverage, and cached display outlines.
//!
//! The continuation/commit and interim transform model follows OpenPDN's MIT-licensed
//! `src/Core/Selection.cs` (rivy/OpenPDN, cca476b). Its GPC clipping code is not used.
//! All rectangles use half-open bounds. Paths use the even-odd fill rule: nested paths
//! make holes, and self-crossing lassos are valid. Coverage uses one byte per pixel.
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::outline::offset::OutlineOffset;
use i_overlay::mesh::style::{LineJoin, OutlineStyle};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::sync::Arc;

type Point = (f64, f64);
type Path = Vec<Point>;
type Shapes = Vec<Vec<Path>>;
type RasterCache = Arc<Mutex<HashMap<(u32, u32, RasterMode), Arc<MaskGrid>>>>;
type ContourCache = Arc<Mutex<HashMap<(u32, u32), Arc<PolygonSet>>>>;

#[derive(Clone, Debug, PartialEq)]
pub struct PolygonSet(pub Vec<Path>);

impl PolygonSet {
    pub fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Self {
        if width <= 0.0 || height <= 0.0 || ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return Self(vec![]);
        }
        Self(vec![vec![
            (x, y),
            (x + width, y),
            (x + width, y + height),
            (x, y + height),
        ]])
    }

    /// Ellipses are flattened with at most 0.05 px radial error.
    pub fn ellipse(x: f64, y: f64, width: f64, height: f64) -> Self {
        if width <= 0.0 || height <= 0.0 || ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return Self(vec![]);
        }
        let radius = width.max(height) * 0.5;
        let n = if radius <= 0.05 {
            16
        } else {
            ((std::f64::consts::PI / (1.0 - 0.05 / radius).acos()).ceil() as usize)
                .clamp(16, 65_536)
        };
        Self(vec![
            (0..n)
                .map(|i| {
                    let t = TAU * i as f64 / n as f64;
                    (
                        x + width * 0.5 * (1.0 + t.cos()),
                        y + height * 0.5 * (1.0 + t.sin()),
                    )
                })
                .collect(),
        ])
    }

    pub fn lasso(points: Vec<Point>) -> Self {
        Self(vec![points])
    }

    fn to_paths(&self) -> Vec<Vec<[f64; 2]>> {
        self.0
            .iter()
            .filter(|path| path.len() >= 3)
            .map(|path| path.iter().map(|&(x, y)| [x, y]).collect())
            .collect()
    }

    fn from_shapes(shapes: Shapes) -> Self {
        Self(shapes.into_iter().flatten().collect())
    }

    fn normalized(&self) -> Shapes {
        let paths = self.to_paths();
        let empty: Vec<Vec<[f64; 2]>> = vec![];
        paths
            .overlay(&empty, OverlayRule::Subject, FillRule::EvenOdd)
            .into_iter()
            .map(|shape| {
                shape
                    .into_iter()
                    .map(|ring| ring.into_iter().map(|p| (p[0], p[1])).collect())
                    .collect()
            })
            .collect()
    }

    pub fn boolean(&self, other: &Self, mode: CombineMode) -> Self {
        if mode == CombineMode::Replace {
            return other.clone();
        }
        let rule = match mode {
            CombineMode::Add => OverlayRule::Union,
            CombineMode::Subtract => OverlayRule::Difference,
            CombineMode::Intersect => OverlayRule::Intersect,
            CombineMode::Invert => OverlayRule::Xor,
            CombineMode::Replace => unreachable!(),
        };
        let result = self
            .to_paths()
            .overlay(&other.to_paths(), rule, FillRule::EvenOdd);
        Self::from_shapes(
            result
                .into_iter()
                .map(|shape| {
                    shape
                        .into_iter()
                        .map(|ring| ring.into_iter().map(|p| (p[0], p[1])).collect())
                        .collect()
                })
                .collect(),
        )
    }

    fn map(&self, f: impl Fn(Point) -> Point) -> Self {
        Self(
            self.0
                .iter()
                .map(|path| path.iter().copied().map(&f).collect())
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskGrid {
    pub width: u32,
    pub height: u32,
    pub bytes: Arc<[u8]>,
}
impl MaskGrid {
    pub fn new(width: u32, height: u32, bytes: Vec<u8>) -> Option<Self> {
        (bytes.len() == width as usize * height as usize).then(|| Self {
            width,
            height,
            bytes: bytes.into(),
        })
    }
    fn at(&self, x: u32, y: u32) -> u8 {
        if x < self.width && y < self.height {
            self.bytes
                .get(y as usize * self.width as usize + x as usize)
                .copied()
                .unwrap_or(0)
        } else {
            0
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectionSource {
    Geometry(Arc<PolygonSet>),
    Coverage(Arc<MaskGrid>),
}
#[derive(Clone, Debug, PartialEq)]
pub enum SelectionState {
    None,
    Active(SelectionSource),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombineMode {
    Replace,
    Add,
    Subtract,
    Intersect,
    Invert,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RasterMode {
    Antialiased,
    Pixelated,
}

/// A pending shape and transform. Preview can be called many times; commit returns one
/// finished selection for the caller to put in one document history step.
/// This mirrors OpenPDN's base, continuation, and interim-transform behavior.
#[derive(Clone, Debug)]
pub struct SelectionContinuation {
    base: Selection,
    continuation: Selection,
    mode: CombineMode,
    interim_transform: [f64; 6],
}
impl SelectionContinuation {
    pub fn new(base: Selection, mode: CombineMode, continuation: Selection) -> Self {
        Self {
            base,
            continuation,
            mode,
            interim_transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
    pub fn set_continuation(&mut self, mode: CombineMode, continuation: Selection) {
        self.mode = mode;
        self.continuation = continuation;
        self.interim_transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    pub fn with_interim_transform(mut self, transform: [f64; 6]) -> Self {
        self.interim_transform = transform;
        self
    }
    pub fn preview(&self, width: u32, height: u32) -> Selection {
        self.base
            .combine(&self.continuation, self.mode, width, height)
            .affine(self.interim_transform, width, height)
    }
    pub fn commit(self, width: u32, height: u32) -> Selection {
        self.preview(width, height)
    }
}

/// A selection plus caches for its current revision. Derived contours never become source data.
#[derive(Debug)]
pub struct Selection {
    state: SelectionState,
    revision: u64,
    raster_cache: RasterCache,
    contour_cache: ContourCache,
}
impl Clone for Selection {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            revision: self.revision,
            raster_cache: self.raster_cache.clone(),
            contour_cache: self.contour_cache.clone(),
        }
    }
}
impl Selection {
    pub(crate) fn from_revision(state: SelectionState, revision: u64) -> Self {
        let mut selection = Self::new(state);
        selection.revision = revision;
        selection
    }
    pub fn state(&self) -> &SelectionState {
        &self.state
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn new(state: SelectionState) -> Self {
        Self {
            state,
            revision: 0,
            raster_cache: Arc::default(),
            contour_cache: Arc::default(),
        }
    }
    pub fn none() -> Self {
        Self::new(SelectionState::None)
    }
    pub fn empty() -> Self {
        Self::active_geometry(PolygonSet(vec![]))
    }
    pub fn active_geometry(shape: PolygonSet) -> Self {
        Self::new(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            shape,
        ))))
    }
    pub fn active_coverage(mask: MaskGrid) -> Self {
        Self::new(SelectionState::Active(SelectionSource::Coverage(Arc::new(
            mask,
        ))))
    }
    pub fn select_all(width: u32, height: u32) -> Self {
        Self::active_geometry(PolygonSet::rectangle(0.0, 0.0, width as f64, height as f64))
    }
    pub fn deselect(&self) -> Self {
        self.changed(SelectionState::None)
    }
    fn changed(&self, state: SelectionState) -> Self {
        let mut result = Self::new(state);
        result.revision = self.revision.saturating_add(1);
        result
    }

    pub fn rasterize(&self, width: u32, height: u32, mode: RasterMode) -> Arc<MaskGrid> {
        let key = (width, height, mode);
        if let Some(grid) = self.raster_cache.lock().get(&key) {
            return grid.clone();
        }
        let mut bytes = vec![0; width as usize * height as usize];
        match &self.state {
            SelectionState::None => bytes.fill(255),
            SelectionState::Active(SelectionSource::Coverage(grid)) => {
                for y in 0..height {
                    for x in 0..width {
                        bytes[y as usize * width as usize + x as usize] = grid.at(x, y);
                    }
                }
            }
            SelectionState::Active(SelectionSource::Geometry(poly)) => {
                let shapes = poly.normalized();
                for y in 0..height {
                    for x in 0..width {
                        let coverage = if mode == RasterMode::Pixelated {
                            if contains(&shapes, (x as f64 + 0.5, y as f64 + 0.5)) {
                                1.0
                            } else {
                                0.0
                            }
                        } else {
                            area_in_pixel(&shapes, x as f64, y as f64)
                        };
                        bytes[y as usize * width as usize + x as usize] = coverage_byte(coverage);
                    }
                }
            }
        }
        let grid = Arc::new(MaskGrid {
            width,
            height,
            bytes: bytes.into(),
        });
        self.raster_cache.lock().insert(key, grid.clone());
        grid
    }

    pub fn contours(&self, width: u32, height: u32) -> Arc<PolygonSet> {
        if let Some(contours) = self.contour_cache.lock().get(&(width, height)) {
            return contours.clone();
        }
        let grid = self.rasterize(width, height, RasterMode::Antialiased);
        let mut result = PolygonSet(vec![]);
        for y in 0..height {
            for x in 0..width {
                if grid.at(x, y) >= 128 {
                    result = result.boolean(
                        &PolygonSet::rectangle(x as f64, y as f64, 1.0, 1.0),
                        CombineMode::Add,
                    );
                }
            }
        }
        let result = Arc::new(result);
        self.contour_cache
            .lock()
            .insert((width, height), result.clone());
        result
    }

    pub fn combine(&self, other: &Self, mode: CombineMode, width: u32, height: u32) -> Self {
        if mode == CombineMode::Replace {
            return self.changed(other.state.clone());
        }
        if let (
            SelectionState::Active(SelectionSource::Geometry(a)),
            SelectionState::Active(SelectionSource::Geometry(b)),
        ) = (&self.state, &other.state)
        {
            return self.changed(SelectionState::Active(SelectionSource::Geometry(Arc::new(
                a.boolean(b, mode),
            ))));
        }
        let a = self.rasterize(width, height, RasterMode::Antialiased);
        let b = other.rasterize(width, height, RasterMode::Antialiased);
        let bytes = a
            .bytes
            .iter()
            .zip(b.bytes.iter())
            .map(|(&a, &b)| match mode {
                CombineMode::Add => a.max(b),
                CombineMode::Intersect => a.min(b),
                CombineMode::Subtract => a.saturating_sub(b),
                CombineMode::Invert => a.abs_diff(b),
                CombineMode::Replace => unreachable!(),
            })
            .collect();
        self.changed(SelectionState::Active(SelectionSource::Coverage(Arc::new(
            MaskGrid {
                width,
                height,
                bytes,
            },
        ))))
    }
    pub fn invert(&self, width: u32, height: u32) -> Self {
        Self::select_all(width, height).combine(self, CombineMode::Subtract, width, height)
    }

    pub fn offset(&self, dx: f64, dy: f64, width: u32, height: u32) -> Self {
        self.translate(dx, dy, width, height)
    }
    pub fn translate(&self, dx: f64, dy: f64, width: u32, height: u32) -> Self {
        self.affine([1.0, 0.0, 0.0, 1.0, dx, dy], width, height)
    }
    pub fn scale(&self, sx: f64, sy: f64, pivot: Point, width: u32, height: u32) -> Self {
        self.affine(
            [sx, 0.0, 0.0, sy, pivot.0 * (1.0 - sx), pivot.1 * (1.0 - sy)],
            width,
            height,
        )
    }
    pub fn rotate(&self, radians: f64, pivot: Point, width: u32, height: u32) -> Self {
        let (s, c) = radians.sin_cos();
        self.affine(
            [
                c,
                s,
                -s,
                c,
                pivot.0 - c * pivot.0 + s * pivot.1,
                pivot.1 - s * pivot.0 - c * pivot.1,
            ],
            width,
            height,
        )
    }
    /// Matrix maps source points to destination: x'=ax+cy+tx, y'=bx+dy+ty.
    pub fn affine(&self, m: [f64; 6], width: u32, height: u32) -> Self {
        if !m.iter().all(|v| v.is_finite()) {
            return self.clone();
        }
        match &self.state {
            SelectionState::None => self.clone(),
            SelectionState::Active(SelectionSource::Geometry(poly)) => {
                self.changed(SelectionState::Active(SelectionSource::Geometry(Arc::new(
                    poly.map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])),
                ))))
            }
            SelectionState::Active(SelectionSource::Coverage(grid)) => {
                let det = m[0] * m[3] - m[1] * m[2];
                if det.abs() < f64::EPSILON {
                    return self.changed(SelectionState::Active(SelectionSource::Coverage(
                        Arc::new(MaskGrid {
                            width,
                            height,
                            bytes: vec![0; width as usize * height as usize].into(),
                        }),
                    )));
                }
                let mut bytes = vec![0; width as usize * height as usize];
                for y in 0..height {
                    for x in 0..width {
                        let px = x as f64 + 0.5 - m[4];
                        let py = y as f64 + 0.5 - m[5];
                        let sx = (m[3] * px - m[2] * py) / det;
                        let sy = (-m[1] * px + m[0] * py) / det;
                        if sx >= 0.0 && sy >= 0.0 {
                            bytes[y as usize * width as usize + x as usize] =
                                grid.at(sx.floor() as u32, sy.floor() as u32);
                        }
                    }
                }
                self.changed(SelectionState::Active(SelectionSource::Coverage(Arc::new(
                    MaskGrid {
                        width,
                        height,
                        bytes: bytes.into(),
                    },
                ))))
            }
        }
    }

    /// Morphology uses a square (Chebyshev) radius on coverage bytes. Dilation takes
    /// the maximum; erosion takes the minimum, treating outside the canvas as zero.
    pub fn expand(&self, radius: u32, width: u32, height: u32) -> Self {
        self.morph(radius, width, height, true)
    }
    pub fn contract(&self, radius: u32, width: u32, height: u32) -> Self {
        self.morph(radius, width, height, false)
    }
    fn morph(&self, radius: u32, width: u32, height: u32, grow: bool) -> Self {
        if radius == 0 {
            return self.clone();
        }
        if let SelectionState::Active(SelectionSource::Geometry(poly)) = &self.state {
            let shapes: Vec<Vec<Vec<[f64; 2]>>> = poly
                .normalized()
                .into_iter()
                .map(|shape| {
                    shape
                        .into_iter()
                        .map(|path| path.into_iter().map(|(x, y)| [x, y]).collect())
                        .collect()
                })
                .collect();
            let offset = if grow {
                radius as f64
            } else {
                -(radius as f64)
            };
            let style = OutlineStyle::new(offset).line_join(LineJoin::Miter(0.01));
            let outlined = shapes.outline(&style);
            let result = PolygonSet::from_shapes(
                outlined
                    .into_iter()
                    .map(|shape| {
                        shape
                            .into_iter()
                            .map(|ring| ring.into_iter().map(|p| (p[0], p[1])).collect())
                            .collect()
                    })
                    .collect(),
            );
            return self.changed(SelectionState::Active(SelectionSource::Geometry(Arc::new(
                result,
            ))));
        }
        let src = self.rasterize(width, height, RasterMode::Antialiased);
        let mut bytes = vec![0; width as usize * height as usize];
        for y in 0..height {
            for x in 0..width {
                let mut value = if grow { 0 } else { 255 };
                for dy in -(radius as i64)..=(radius as i64) {
                    for dx in -(radius as i64)..=(radius as i64) {
                        let px = x as i64 + dx;
                        let py = y as i64 + dy;
                        let v = if px >= 0 && py >= 0 {
                            src.at(px as u32, py as u32)
                        } else {
                            0
                        };
                        value = if grow { value.max(v) } else { value.min(v) };
                    }
                }
                bytes[y as usize * width as usize + x as usize] = value;
            }
        }
        self.changed(SelectionState::Active(SelectionSource::Coverage(Arc::new(
            MaskGrid {
                width,
                height,
                bytes: bytes.into(),
            },
        ))))
    }
}

/// Shared rounding rule: clamp to [0,1], multiply by 255, then round half up.
fn coverage_byte(area: f64) -> u8 {
    (area.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

fn contains(shapes: &Shapes, p: Point) -> bool {
    shapes.iter().any(|shape| {
        shape
            .iter()
            .fold(false, |inside, path| inside ^ path_contains(path, p))
    })
}
fn path_contains(path: &Path, p: Point) -> bool {
    let mut inside = false;
    for i in 0..path.len() {
        let (ax, ay) = path[i];
        let (bx, by) = path[(i + 1) % path.len()];
        if (ay > p.1) != (by > p.1) && p.0 < (bx - ax) * (p.1 - ay) / (by - ay) + ax {
            inside = !inside;
        }
    }
    inside
}
fn area_in_pixel(shapes: &Shapes, x: f64, y: f64) -> f64 {
    let mut area = 0.0;
    for shape in shapes {
        for (index, path) in shape.iter().enumerate() {
            let clipped = clip_square(path, x, y);
            let signed = polygon_area(&clipped).abs();
            area += if index == 0 { signed } else { -signed };
        }
    }
    area.clamp(0.0, 1.0)
}
fn polygon_area(path: &Path) -> f64 {
    if path.len() < 3 {
        return 0.0;
    }
    // Translate to the first point to avoid cancellation at large canvas offsets.
    let origin = path[0];
    path[1..]
        .windows(2)
        .map(|edge| {
            let a = (edge[0].0 - origin.0, edge[0].1 - origin.1);
            let b = (edge[1].0 - origin.0, edge[1].1 - origin.1);
            a.0 * b.1 - a.1 * b.0
        })
        .sum::<f64>()
        * 0.5
}
fn clip_square(path: &Path, x: f64, y: f64) -> Path {
    let mut out = path.clone();
    for edge in 0..4 {
        let input = out;
        out = Vec::new();
        if input.is_empty() {
            break;
        }
        let mut a = *input.last().unwrap();
        for &b in &input {
            let da = distance(a, edge, x, y);
            let db = distance(b, edge, x, y);
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                out.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
            }
            if db >= 0.0 {
                out.push(b);
            }
            a = b;
        }
    }
    out
}
fn distance(p: Point, edge: usize, x: f64, y: f64) -> f64 {
    match edge {
        0 => p.0 - x,
        1 => x + 1.0 - p.0,
        2 => p.1 - y,
        _ => y + 1.0 - p.1,
    }
}
