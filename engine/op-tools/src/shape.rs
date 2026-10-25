//! Paint.NET 5.1 catalog paths in f64 unit boxes. This geometry is our own reading
//! of https://www.getpaint.net/doc/latest/ShapesTool.html and is provisional until
//! Paint.NET 5.1 captures exist. OpenPDN ShapeTool.cs informs edit behavior only.
use crate::line::{Cap, Dash, Join, LineOptions, Point, rasterize_closed};
use op_core::doc::{DocError, DocState, LayerId, SelectionState, ToolCompositeMode};
use op_core::pixel::Rgba8;
use op_core::selection::{PolygonSet, RasterMode, Selection};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use op_ops::composite::tool_composite;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct ShapeInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub vertex_count: usize,
}
const NAMES: [(&str, &str, &str); 29] = [
    ("rectangle", "Rectangle", "Basic"),
    ("rounded_rectangle", "Rounded Rectangle", "Basic"),
    ("ellipse", "Ellipse", "Basic"),
    ("diamond", "Diamond", "Basic"),
    ("trapezoid", "Trapezoid", "Basic"),
    ("parallelogram", "Parallelogram", "Basic"),
    ("triangle", "Triangle", "Basic"),
    ("right_triangle", "Right Angle Triangle", "Basic"),
    ("pentagon", "Pentagon", "Polygons & Stars"),
    ("hexagon", "Hexagon", "Polygons & Stars"),
    ("heptagon", "Heptagon", "Polygons & Stars"),
    ("octagon", "Octagon", "Polygons & Stars"),
    ("star_3", "Three-point Star", "Polygons & Stars"),
    ("star_4", "Four-point Star", "Polygons & Stars"),
    ("star_5", "Five-point Star", "Polygons & Stars"),
    ("star_6", "Six-point Star", "Polygons & Stars"),
    ("arrow", "Arrow", "Arrows"),
    ("notched_arrow", "Notched Arrow", "Arrows"),
    ("pentagon_arrow", "Pentagon Arrow", "Arrows"),
    ("chevron_arrow", "Chevron Arrow", "Arrows"),
    ("rectangular_callout", "Rectangular Callout", "Callouts"),
    ("rounded_callout", "Rounded Rectangle Callout", "Callouts"),
    ("ellipse_callout", "Ellipse Callout", "Callouts"),
    ("cloud_callout", "Cloud Callout", "Callouts"),
    ("lightning_bolt", "Lightning Bolt", "Symbols"),
    ("check_mark", "Check Mark", "Symbols"),
    ("multiply", "Multiply", "Symbols"),
    ("gear", "Gear", "Symbols"),
    ("heart", "Heart", "Symbols"),
];
pub fn catalog() -> Vec<ShapeInfo> {
    NAMES
        .iter()
        .map(|&(id, name, group)| ShapeInfo {
            id,
            name,
            group,
            vertex_count: geometry(id).len() - 1,
        })
        .collect()
}
fn points(p: &[(f64, f64)]) -> Vec<Point> {
    p.iter().map(|&(x, y)| Point::new(x, y)).collect()
}
fn radial(n: usize, inner: f64, outer: f64) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64 - std::f64::consts::FRAC_PI_2;
            let r = if i % 2 == 0 { outer } else { inner };
            Point::new(0.5 + r * a.cos(), 0.5 + r * a.sin())
        })
        .collect()
}
fn regular(n: usize) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64 - std::f64::consts::FRAC_PI_2;
            Point::new(0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
        })
        .collect()
}
fn ellipse(n: usize) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Point::new(0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
        })
        .collect()
}
fn rounded() -> Vec<Point> {
    let mut p = Vec::new();
    for (cx, cy, start) in [
        (0.82, 0.18, -90.0),
        (0.82, 0.82, 0.0),
        (0.18, 0.82, 90.0),
        (0.18, 0.18, 180.0),
    ] {
        for j in 0..=8 {
            let a = (start + j as f64 * 90.0 / 8.0).to_radians();
            p.push(Point::new(cx + 0.18 * a.cos(), cy + 0.18 * a.sin()));
        }
    }
    p
}
/// Returns a closed unit-box boundary. Unknown ids return an empty path.
pub fn geometry(id: &str) -> Vec<Point> {
    let mut p = match id {
        "rectangle" => points(&[(0., 0.), (1., 0.), (1., 1.), (0., 1.)]),
        "rounded_rectangle" => rounded(),
        "ellipse" => ellipse(64),
        "diamond" => points(&[(0.5, 0.), (1., 0.5), (0.5, 1.), (0., 0.5)]),
        "trapezoid" => points(&[(0.25, 0.), (0.75, 0.), (1., 1.), (0., 1.)]),
        "parallelogram" => points(&[(0.25, 0.), (1., 0.), (0.75, 1.), (0., 1.)]),
        "triangle" => points(&[(0.5, 0.), (1., 1.), (0., 1.)]),
        "right_triangle" => points(&[(0., 0.), (1., 1.), (0., 1.)]),
        "pentagon" => regular(5),
        "hexagon" => regular(6),
        "heptagon" => regular(7),
        "octagon" => regular(8),
        "star_3" => radial(6, 0.18, 0.5),
        "star_4" => radial(8, 0.22, 0.5),
        "star_5" => radial(10, 0.21, 0.5),
        "star_6" => radial(12, 0.25, 0.5),
        "arrow" => points(&[
            (0., 0.33),
            (0.62, 0.33),
            (0.62, 0.),
            (1., 0.5),
            (0.62, 1.),
            (0.62, 0.67),
            (0., 0.67),
        ]),
        "notched_arrow" => points(&[
            (0., 0.33),
            (0.62, 0.33),
            (0.62, 0.),
            (1., 0.5),
            (0.62, 1.),
            (0.62, 0.67),
            (0., 0.67),
            (0.17, 0.5),
        ]),
        "pentagon_arrow" => points(&[(0., 0.), (0.65, 0.), (1., 0.5), (0.65, 1.), (0., 1.)]),
        "chevron_arrow" => points(&[
            (0., 0.),
            (0.55, 0.),
            (1., 0.5),
            (0.55, 1.),
            (0., 1.),
            (0.45, 0.5),
        ]),
        "rectangular_callout" => points(&[
            (0., 0.),
            (1., 0.),
            (1., 0.73),
            (0.42, 0.73),
            (0.19, 1.),
            (0.24, 0.73),
            (0., 0.73),
        ]),
        "rounded_callout" => points(&[
            (0.15, 0.),
            (0.85, 0.),
            (1., 0.15),
            (1., 0.6),
            (0.85, 0.75),
            (0.42, 0.75),
            (0.19, 1.),
            (0.24, 0.75),
            (0.15, 0.75),
            (0., 0.6),
            (0., 0.15),
        ]),
        "ellipse_callout" => points(&[
            (0.5, 0.),
            (0.85, 0.1),
            (1., 0.35),
            (0.88, 0.64),
            (0.55, 0.75),
            (0.19, 1.),
            (0.28, 0.73),
            (0., 0.5),
            (0.05, 0.2),
        ]),
        "cloud_callout" => {
            let mut a = Vec::new();
            for i in 0..32 {
                let t = std::f64::consts::TAU * i as f64 / 32.;
                let r = 0.42 + 0.05 * (8. * t).cos();
                a.push(Point::new(0.5 + r * t.cos(), 0.41 + r * 0.7 * t.sin()));
                if i == 11 {
                    a.push(Point::new(0.2, 1.));
                }
            }
            a
        }
        "lightning_bolt" => points(&[
            (0.57, 0.),
            (0.17, 0.55),
            (0.46, 0.55),
            (0.33, 1.),
            (0.85, 0.4),
            (0.55, 0.4),
            (0.83, 0.),
        ]),
        "check_mark" => points(&[
            (0., 0.53),
            (0.16, 0.36),
            (0.38, 0.6),
            (0.84, 0.),
            (1., 0.14),
            (0.4, 1.),
        ]),
        "multiply" => points(&[
            (0.18, 0.),
            (0.5, 0.32),
            (0.82, 0.),
            (1., 0.18),
            (0.68, 0.5),
            (1., 0.82),
            (0.82, 1.),
            (0.5, 0.68),
            (0.18, 1.),
            (0., 0.82),
            (0.32, 0.5),
            (0., 0.18),
        ]),
        "gear" => radial(32, 0.37, 0.5),
        "heart" => (0..64)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 64.;
                let x = 16. * t.sin().powi(3);
                let y = 13. * t.cos() - 5. * (2. * t).cos() - 2. * (3. * t).cos() - (4. * t).cos();
                Point::new(0.5 + x / 36., 0.52 - y / 36.)
            })
            .collect(),
        _ => return Vec::new(),
    };
    if let Some(first) = p.first().copied() {
        p.push(first);
    }
    p
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawMode {
    Outline,
    Fill,
    FillAndOutline,
}
#[derive(Clone, Copy, Debug)]
pub struct ShapeOptions {
    pub mode: DrawMode,
    pub width: f64,
    pub antialias: bool,
    pub dash: Dash,
    pub join: Join,
    pub miter_limit: f64,
    pub primary: [u8; 4],
    pub secondary: [u8; 4],
    pub blend: ToolCompositeMode,
}
#[derive(Debug)]
pub enum ShapeError {
    BadInput(&'static str),
    Document(DocError),
}
impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(s) => write!(f, "{s}"),
            Self::Document(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ShapeError {}
impl From<DocError> for ShapeError {
    fn from(e: DocError) -> Self {
        Self::Document(e)
    }
}
type Result<T> = std::result::Result<T, ShapeError>;
fn valid(shape: &str, b: [Point; 2], r: f64, o: ShapeOptions) -> Result<()> {
    if geometry(shape).is_empty() {
        return Err(ShapeError::BadInput("unknown shape"));
    }
    if !b
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9)
        || !r.is_finite()
    {
        return Err(ShapeError::BadInput("invalid shape geometry"));
    }
    if !o.width.is_finite()
        || !(0.1..=2000.).contains(&o.width)
        || !o.miter_limit.is_finite()
        || !(1.0..=100.).contains(&o.miter_limit)
    {
        return Err(ShapeError::BadInput("invalid shape style"));
    }
    Ok(())
}
pub struct ShapeSession {
    base: DocState,
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub shape: String,
    pub corners: [Point; 2],
    pub rotation: f64,
    pub options: ShapeOptions,
    selection: Option<Arc<op_core::selection::MaskGrid>>,
}
impl ShapeSession {
    pub fn new(
        base: DocState,
        shape: &str,
        corners: [Point; 2],
        rotation: f64,
        options: ShapeOptions,
    ) -> Result<Self> {
        valid(shape, corners, rotation, options)?;
        let target_layer = base
            .layers
            .get(base.active_layer)
            .ok_or(ShapeError::BadInput("no active layer"))?
            .id;
        let selection = if matches!(base.selection, SelectionState::None) {
            None
        } else {
            Some(base.rasterize_selection(RasterMode::Antialiased))
        };
        Ok(Self {
            base_revision: base.revision,
            target_layer,
            base,
            shape: shape.into(),
            corners,
            rotation,
            options,
            selection,
        })
    }
    pub fn update(
        &mut self,
        shape: Option<&str>,
        corners: Option<[Point; 2]>,
        rotation: Option<f64>,
        options: Option<ShapeOptions>,
    ) -> Result<()> {
        let shape = shape.unwrap_or(&self.shape);
        let corners = corners.unwrap_or(self.corners);
        let rotation = rotation.unwrap_or(self.rotation);
        let options = options.unwrap_or(self.options);
        valid(shape, corners, rotation, options)?;
        self.shape = shape.into();
        self.corners = corners;
        self.rotation = rotation;
        self.options = options;
        Ok(())
    }
    pub fn preview(&self) -> Result<DocState> {
        let mut next = self.base.clone();
        let [a, b] = self.corners;
        if a.x == b.x || a.y == b.y {
            return Ok(next);
        }
        let cx = (a.x + b.x) * 0.5;
        let cy = (a.y + b.y) * 0.5;
        let (sn, cs) = self.rotation.sin_cos();
        let path: Vec<Point> = geometry(&self.shape)
            .iter()
            .map(|p| {
                let x = a.x + (b.x - a.x) * p.x - cx;
                let y = a.y + (b.y - a.y) * p.y - cy;
                Point::new(cx + x * cs - y * sn, cy + x * sn + y * cs)
            })
            .collect();
        let (w, h) = (self.base.width, self.base.height);
        let fill = if self.options.mode != DrawMode::Outline {
            let poly = PolygonSet(vec![
                path[..path.len() - 1].iter().map(|p| (p.x, p.y)).collect(),
            ]);
            Selection::active_geometry(poly)
                .rasterize(
                    w,
                    h,
                    if self.options.antialias {
                        RasterMode::Antialiased
                    } else {
                        RasterMode::Pixelated
                    },
                )
                .bytes
                .to_vec()
        } else {
            vec![0; w as usize * h as usize]
        };
        let line = LineOptions {
            kind: crate::line::LineKind::Straight,
            width: self.options.width,
            antialias: self.options.antialias,
            dash: self.options.dash,
            start_cap: Cap::Flat,
            end_cap: Cap::Flat,
            join: self.options.join,
            miter_limit: self.options.miter_limit,
            primary: self.options.primary,
            blend: self.options.blend,
        };
        let outline = if self.options.mode != DrawMode::Fill {
            rasterize_closed(&path, line, w, h)
        } else {
            vec![0; w as usize * h as usize]
        };
        for ty in 0..h.div_ceil(TILE_SIZE as u32) {
            for tx in 0..w.div_ceil(TILE_SIZE as u32) {
                let coord = TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                };
                let old = self.base.read_tile(self.base.active_layer, coord)?;
                let mut tile = old.clone();
                for y in 0..(h as usize - ty as usize * TILE_SIZE).min(TILE_SIZE) {
                    for x in 0..(w as usize - tx as usize * TILE_SIZE).min(TILE_SIZE) {
                        let n = (ty as usize * TILE_SIZE + y) * w as usize
                            + tx as usize * TILE_SIZE
                            + x;
                        let sel = self.selection.as_ref().map_or(255, |s| s.bytes[n]);
                        let k = (y * TILE_SIZE + x) * 4;
                        let mut dst =
                            Rgba8::from(<[u8; 4]>::try_from(&old[k..k + 4]).expect("pixel"));
                        for (mask, color) in [
                            (
                                &fill,
                                if self.options.mode == DrawMode::Fill {
                                    self.options.primary
                                } else {
                                    self.options.secondary
                                },
                            ),
                            (&outline, self.options.primary),
                        ] {
                            let cov = ((u16::from(mask[n]) * u16::from(sel) + 127) / 255) as u8;
                            if cov > 0 {
                                dst = tool_composite(
                                    dst,
                                    Rgba8::from(color),
                                    self.options.blend,
                                    cov,
                                );
                            }
                        }
                        tile[k..k + 4].copy_from_slice(&<[u8; 4]>::from(dst));
                    }
                }
                if tile != old {
                    next = next.write_tile(
                        self.base.active_layer,
                        coord,
                        TileData::try_from(tile).map_err(DocError::from)?,
                    )?;
                }
            }
        }
        Ok(next)
    }
    pub fn finish(self) -> Result<DocState> {
        self.preview()
    }
}
