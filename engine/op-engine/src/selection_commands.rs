//! M29: Ellipse Select (`selection.ellipse`), Lasso Select
//! (`selection.lasso`), the Edit menu's selection commands
//! (`selection.invert`, `selection.erase`, `selection.fill`,
//! `selection.expand`, `selection.paste`) and the `doc.selection_polygons`
//! query that Copy Selection puts on the clipboard.
//!
//! paint.net pages used:
//! - https://www.getpaint.net/doc/latest/SelectionTools.html: the five combine
//!   modes; Lasso Select closes the shape with a straight line back to the
//!   start; Ellipse Select drags a bounding box.
//! - https://www.getpaint.net/doc/latest/LassoSelectionTool.html and
//!   EllipseSelectionTool.html: one freeform lasso (the pages name no Polygon
//!   mode); Shift keeps the ellipse a circle (a Mac-side rule).
//! - https://www.getpaint.net/doc/latest/EditMenu.html: Erase Selection makes
//!   the selected pixels transparent black and deselects; Fill Selection fills
//!   with the primary color and keeps the selection; Invert Selection; Copy
//!   Selection writes `{"polygonList": ["x,y,x,y,..."]}` JSON text, and Paste
//!   Selection reads it back with a combine mode.
//!
//! Like `selection.rectangle`, every mode with no selection combines with an
//! empty selection, and new shapes are clipped to the canvas. Erase and Fill
//! use the antialiased coverage: a pixel covered by a fraction t mixes by t.
//! paint.net has no Expand item; `selection.expand` (a positive radius grows,
//! a negative one shrinks, square corners) is OpenPainter's.
use super::{
    CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, bad, internal,
    select_crop_commands::combine_mode,
};
use op_core::doc::{DocState, PolygonSet, SelectionSource, SelectionState};
use op_core::selection::{RasterMode, Selection};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData};
use serde_json::{Value, json};

const MODES: &[&str] = &["replace", "add", "subtract", "intersect", "invert"];

fn field(key: &'static str, kind: ParamKind, default: Option<Value>) -> Param {
    Param {
        key,
        kind,
        min: None,
        max: None,
        default,
    }
}

fn combine_field() -> Param {
    field("combine", ParamKind::Choice(MODES), Some(json!("replace")))
}

fn has_selection(s: &DocState) -> bool {
    matches!(s.selection, SelectionState::Active(_))
}

/// True when the selection covers at least part of one pixel.
fn covers_pixels(s: &DocState) -> bool {
    match &s.selection {
        SelectionState::None => false,
        SelectionState::Active(SelectionSource::Geometry(p)) if p.0.iter().all(|r| r.len() < 3) => {
            false
        }
        SelectionState::Active(_) => s
            .rasterize_selection(RasterMode::Antialiased)
            .bytes
            .iter()
            .any(|&c| c != 0),
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    let simple = |id, enabled, run| CommandSpec {
        id,
        params: ParamSchema::default(),
        enabled,
        run,
        previewable: false,
    };
    vec![
        CommandSpec {
            id: "selection.ellipse",
            params: ParamSchema {
                fields: vec![
                    field("x", ParamKind::Float, None),
                    field("y", ParamKind::Float, None),
                    field("width", ParamKind::Float, None),
                    field("height", ParamKind::Float, None),
                    combine_field(),
                    // True: a handle drag changed the current Ellipse Select
                    // step's box. The step is replaced, not added.
                    field("amend", ParamKind::Bool, Some(json!(false))),
                ],
            },
            enabled: |_| true,
            run: ellipse,
            previewable: false,
        },
        CommandSpec {
            id: "selection.lasso",
            params: ParamSchema {
                fields: vec![
                    // "x y x y ...": at least three points, closed back to the first.
                    field("points", ParamKind::String, None),
                    combine_field(),
                ],
            },
            enabled: |_| true,
            run: lasso,
            previewable: false,
        },
        simple("selection.invert", has_selection, invert),
        simple("selection.erase", covers_pixels, erase),
        CommandSpec {
            id: "selection.fill",
            params: ParamSchema {
                fields: vec![field("color", ParamKind::String, None)],
            },
            enabled: covers_pixels,
            run: fill,
            previewable: false,
        },
        CommandSpec {
            id: "selection.expand",
            params: ParamSchema {
                fields: vec![Param {
                    key: "radius",
                    kind: ParamKind::Int,
                    min: Some(-1000.0),
                    max: Some(1000.0),
                    default: None,
                }],
            },
            enabled: covers_pixels,
            run: expand,
            previewable: false,
        },
        CommandSpec {
            id: "selection.paste",
            params: ParamSchema {
                fields: vec![
                    // paint.net's Copy Selection JSON text.
                    field("polygons", ParamKind::String, None),
                    combine_field(),
                ],
            },
            enabled: |_| true,
            run: paste,
            previewable: false,
        },
    ]
}

pub(super) fn history_label(id: &str, params: &Value) -> Option<String> {
    Some(
        match id {
            "selection.ellipse" => "Ellipse Select",
            "selection.lasso" => "Lasso Select",
            "selection.invert" => "Invert Selection",
            "selection.erase" => "Erase Selection",
            "selection.fill" => "Fill Selection",
            "selection.expand" if params["radius"].as_i64().unwrap_or(0) < 0 => {
                "Contract Selection"
            }
            "selection.expand" => "Expand Selection",
            "selection.paste" => "Paste Selection",
            _ => return None,
        }
        .into(),
    )
}

/// `s` with `shape` (clipped to the canvas) combined into its selection.
/// With no selection, every mode combines with an empty selection.
fn combine_shape(s: &DocState, shape: PolygonSet, mode: &str) -> DocState {
    let canvas = PolygonSet::rectangle(0.0, 0.0, f64::from(s.width), f64::from(s.height));
    let clipped = canvas.boolean(&shape, op_core::selection::CombineMode::Intersect);
    let base = match s.selection {
        SelectionState::None => Selection::empty(),
        _ => Selection::new(s.selection.clone()),
    };
    let next = base.combine(
        &Selection::active_geometry(clipped),
        combine_mode(mode),
        s.width,
        s.height,
    );
    s.set_selection(next.state().clone())
}

fn ellipse(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let num = |k: &str| p[k].as_f64().unwrap();
    let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
    if ![x, y, w, h].iter().all(|v| v.is_finite()) || w < 0.0 || h < 0.0 {
        return Err(bad("ellipse must be finite with a positive size"));
    }
    let next = combine_shape(
        s,
        PolygonSet::ellipse(x, y, w, h),
        p["combine"].as_str().unwrap(),
    );
    ctx.check()?;
    Ok(if p["amend"].as_bool().unwrap() {
        Outcome::Replace(next)
    } else {
        Outcome::State(next)
    })
}

/// Parses "x y x y ..." (whitespace) or "x,y,x,y,..." (commas) into points.
fn parse_ring(text: &str) -> Result<Vec<(f64, f64)>> {
    let numbers: Vec<f64> = text
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|n| !n.is_empty())
        .map(|n| n.parse::<f64>().map_err(bad))
        .collect::<Result<_>>()?;
    if !numbers.len().is_multiple_of(2) || numbers.iter().any(|n| !n.is_finite()) {
        return Err(bad("points must be finite x y pairs"));
    }
    let ring: Vec<(f64, f64)> = numbers.chunks_exact(2).map(|c| (c[0], c[1])).collect();
    if ring.len() < 3 {
        return Err(bad("a selection outline needs at least three points"));
    }
    Ok(ring)
}

fn lasso(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let ring = parse_ring(p["points"].as_str().unwrap())?;
    let next = combine_shape(s, PolygonSet::lasso(ring), p["combine"].as_str().unwrap());
    ctx.check()?;
    Ok(Outcome::State(next))
}

fn invert(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    ctx.check()?;
    let inverted = Selection::new(s.selection.clone()).invert(s.width, s.height);
    Ok(Outcome::State(s.set_selection(inverted.state().clone())))
}

/// Changes every covered pixel of the active layer with `f(pixel, coverage)`.
fn per_covered_pixel(
    ctx: &RunCtx,
    s: &DocState,
    f: impl Fn([u8; 4], u8) -> [u8; 4],
) -> Result<DocState> {
    let mask = s.rasterize_selection(RasterMode::Antialiased);
    let (w, h) = (s.width as usize, s.height as usize);
    let mut next = s.clone();
    let (tw, th) = (w.div_ceil(TILE_SIZE), h.div_ceil(TILE_SIZE));
    for ty in 0..th {
        for tx in 0..tw {
            ctx.check()?;
            let (x0, y0) = (tx * TILE_SIZE, ty * TILE_SIZE);
            let (x1, y1) = ((x0 + TILE_SIZE).min(w), (y0 + TILE_SIZE).min(h));
            let covered =
                (y0..y1).any(|y| mask.bytes[y * w + x0..y * w + x1].iter().any(|&c| c != 0));
            if !covered {
                continue;
            }
            let coord = TileCoord {
                x: tx as i32,
                y: ty as i32,
            };
            let mut tile = s.read_tile(s.active_layer, coord).map_err(internal)?;
            for y in y0..y1 {
                for x in x0..x1 {
                    let c = mask.bytes[y * w + x];
                    if c == 0 {
                        continue;
                    }
                    let i = ((y - y0) * TILE_SIZE + (x - x0)) * 4;
                    let px = [tile[i], tile[i + 1], tile[i + 2], tile[i + 3]];
                    tile[i..i + 4].copy_from_slice(&f(px, c));
                }
            }
            next = next
                .write_tile(
                    s.active_layer,
                    coord,
                    TileData::try_from(tile).map_err(internal)?,
                )
                .map_err(internal)?;
        }
    }
    Ok(next)
}

/// Straight-alpha mix of `src` toward `color` by coverage `c` / 255: alpha
/// mixes linearly and color by alpha weight (the premultiplied mix).
fn mix(src: [u8; 4], color: [u8; 4], c: u8) -> [u8; 4] {
    if c == 255 {
        return color;
    }
    let t = f64::from(c) / 255.0;
    let (sa, ca) = (f64::from(src[3]), f64::from(color[3]));
    let a = sa * (1.0 - t) + ca * t;
    let mut out = [0u8; 4];
    if a > 0.0 {
        for k in 0..3 {
            out[k] = ((f64::from(src[k]) * sa * (1.0 - t) + f64::from(color[k]) * ca * t) / a)
                .round() as u8;
        }
    }
    out[3] = a.round() as u8;
    out
}

fn erase(ctx: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let next = per_covered_pixel(ctx, s, |px, c| {
        if c == 255 {
            return [0, 0, 0, 0];
        }
        let a = (f64::from(px[3]) * (1.0 - f64::from(c) / 255.0)).round() as u8;
        if a == 0 {
            [0, 0, 0, 0]
        } else {
            [px[0], px[1], px[2], a]
        }
    })?;
    ctx.check()?;
    Ok(Outcome::State(next.set_selection(SelectionState::None)))
}

fn parse_color(hex: &str) -> Result<[u8; 4]> {
    if hex.len() != 8 || !hex.is_ascii() {
        return Err(bad("color must be eight RGBA hex digits"));
    }
    let mut color = [0; 4];
    for (i, c) in color.iter_mut().enumerate() {
        *c = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(bad)?;
    }
    Ok(color)
}

fn fill(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let color = parse_color(p["color"].as_str().unwrap())?;
    let next = per_covered_pixel(ctx, s, |px, c| mix(px, color, c))?;
    ctx.check()?;
    Ok(Outcome::State(next))
}

fn expand(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let r = p["radius"].as_i64().unwrap();
    if r == 0 {
        return Err(bad("radius must not be zero"));
    }
    let sel = Selection::new(s.selection.clone());
    let next = if r > 0 {
        sel.expand(r as u32, s.width, s.height)
    } else {
        sel.contract(r.unsigned_abs() as u32, s.width, s.height)
    };
    ctx.check()?;
    Ok(Outcome::State(s.set_selection(next.state().clone())))
}

/// Parses paint.net's Copy Selection text into rings.
fn parse_polygons(text: &str) -> Result<Vec<Vec<(f64, f64)>>> {
    let v: Value = serde_json::from_str(text).map_err(|_| bad("not selection text"))?;
    let list = v["polygonList"]
        .as_array()
        .ok_or_else(|| bad("selection text has no polygonList"))?;
    if list.is_empty() {
        return Err(bad("selection text has no polygons"));
    }
    list.iter()
        .map(|r| {
            r.as_str()
                .ok_or_else(|| bad("each polygon must be a string"))
                .and_then(parse_ring)
        })
        .collect()
}

fn paste(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let rings = parse_polygons(p["polygons"].as_str().unwrap())?;
    let next = combine_shape(s, PolygonSet(rings), p["combine"].as_str().unwrap());
    ctx.check()?;
    Ok(Outcome::State(next))
}

fn number(v: f64) -> String {
    format!("{v}")
}

/// `doc.selection_polygons`: the selection as paint.net's Copy Selection
/// JSON, `{"polygonList": ["x,y,x,y,...,x0,y0"]}`, each ring closed by
/// repeating its first point. A shape selection gives its rings; a pixel mask
/// (Magic Wand) gives one rectangle per row run of pixels at 128 and up. No
/// selection gives an empty list.
pub(super) fn selection_polygons(s: &DocState) -> Value {
    let mut out: Vec<String> = Vec::new();
    match &s.selection {
        SelectionState::None => {}
        SelectionState::Active(SelectionSource::Geometry(poly)) => {
            for ring in poly.0.iter().filter(|r| r.len() >= 3) {
                let mut nums: Vec<String> = Vec::with_capacity(ring.len() * 2 + 2);
                for &(x, y) in ring.iter().chain(std::iter::once(&ring[0])) {
                    nums.push(number(x));
                    nums.push(number(y));
                }
                out.push(nums.join(","));
            }
        }
        SelectionState::Active(SelectionSource::Coverage(_)) => {
            let mask = s.rasterize_selection(RasterMode::Antialiased);
            let (w, h) = (s.width as usize, s.height as usize);
            for y in 0..h {
                let mut x = 0;
                while x < w {
                    if mask.bytes[y * w + x] < 128 {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < w && mask.bytes[y * w + x] >= 128 {
                        x += 1;
                    }
                    let (x0, x1, y0, y1) = (start, x, y, y + 1);
                    out.push(format!("{x0},{y0},{x1},{y0},{x1},{y1},{x0},{y1},{x0},{y0}"));
                }
            }
        }
    }
    json!({ "polygonList": out })
}
