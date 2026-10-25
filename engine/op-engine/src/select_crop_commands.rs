//! M23: Rectangle Select (`selection.rectangle`), the Crop tool's crop with a
//! straighten angle (`image.crop_rect`), and the queries the canvas needs for
//! the marching ants (`doc.selection_path`) and the Resize dialog's print size
//! (`doc.resolution`).
//!
//! Rectangle Select follows paint.net's Rectangle Select and Selection Tools
//! pages (https://www.getpaint.net/doc/latest/RectangleSelectionTool.html,
//! https://www.getpaint.net/doc/latest/SelectionTools.html): five combine
//! modes, and the rectangle is clipped to the canvas. With no selection, every
//! mode combines with an empty selection (Add and Invert give the rectangle;
//! Subtract and Intersect give an empty selection).
//!
//! `image.crop_rect` with an angle rotates every layer with the Rotate / Zoom
//! effect (bilinear, transparent edges, about the image center) and then crops
//! the rectangle, in one history step. So it equals `effect.layers.rotate_zoom`
//! on each layer followed by an unrotated `image.crop_rect`.
use super::{
    CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, always, bad,
    image_commands, internal, run_effect,
};
use op_core::doc::{DocState, PolygonSet, SelectionSource, SelectionState};
use op_core::history::History;
use op_core::selection::{CombineMode, RasterMode, Selection};
use serde_json::{Value, json};

fn field(key: &'static str, kind: ParamKind, default: Option<Value>) -> Param {
    Param {
        key,
        kind,
        min: None,
        max: None,
        default,
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "selection.rectangle",
            params: ParamSchema {
                fields: vec![
                    field("x", ParamKind::Float, None),
                    field("y", ParamKind::Float, None),
                    field("width", ParamKind::Float, None),
                    field("height", ParamKind::Float, None),
                    field(
                        "combine",
                        ParamKind::Choice(&["replace", "add", "subtract", "intersect", "invert"]),
                        Some(json!("replace")),
                    ),
                    // True: the rectangle of the current Rectangle Select step
                    // changed (a handle drag). The step is replaced, not added.
                    field("amend", ParamKind::Bool, Some(json!(false))),
                ],
            },
            enabled: always,
            run: rectangle,
            previewable: false,
        },
        CommandSpec {
            id: "image.crop_rect",
            params: ParamSchema {
                fields: vec![
                    field("x", ParamKind::Int, None),
                    field("y", ParamKind::Int, None),
                    field("width", ParamKind::Int, None),
                    field("height", ParamKind::Int, None),
                    Param {
                        key: "angle",
                        kind: ParamKind::Float,
                        min: Some(-45.0),
                        max: Some(45.0),
                        default: Some(json!(0.0)),
                    },
                ],
            },
            enabled: always,
            run: crop_rect,
            previewable: false,
        },
    ]
}

pub(super) fn history_label(id: &str) -> Option<&'static str> {
    match id {
        "selection.rectangle" => Some("Rectangle Select"),
        "image.crop_rect" => Some("Crop"),
        _ => None,
    }
}

/// For an amend of the current Rectangle Select (or, M29, Ellipse Select)
/// step: the state to combine against (the current state with the selection
/// from before that step). When the current step is not a step of the same
/// command, `amend` becomes false and the command adds a new step. Release
/// review item 12b: so does an amend of the saved step (right after a save),
/// so the image becomes edited instead of changing the saved step in place.
pub(super) fn amend_base(id: &str, mut params: Value, h: &History) -> (Option<DocState>, Value) {
    let amendable = id == "selection.rectangle" || id == "selection.ellipse";
    if !amendable || params["amend"].as_bool() != Some(true) {
        return (None, params);
    }
    let i = h.current_index();
    if i > 0
        && h.is_dirty()
        && h.command_id(i) == Some(id)
        && let Some(before) = h.state(i - 1)
    {
        return (
            Some(h.current_state().set_selection(before.selection.clone())),
            params,
        );
    }
    params["amend"] = json!(false);
    (None, params)
}

pub(super) fn combine_mode(name: &str) -> CombineMode {
    match name {
        "add" => CombineMode::Add,
        "subtract" => CombineMode::Subtract,
        "intersect" => CombineMode::Intersect,
        "invert" => CombineMode::Invert,
        _ => CombineMode::Replace,
    }
}

fn rectangle(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let num = |k: &str| p[k].as_f64().unwrap();
    let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
    if ![x, y, w, h].iter().all(|v| v.is_finite()) || w < 0.0 || h < 0.0 {
        return Err(bad("rectangle must be finite with a positive size"));
    }
    let (cw, ch) = (f64::from(s.width), f64::from(s.height));
    let (x0, y0) = (x.clamp(0.0, cw), y.clamp(0.0, ch));
    let (x1, y1) = ((x + w).clamp(0.0, cw), (y + h).clamp(0.0, ch));
    let rect = Selection::active_geometry(PolygonSet::rectangle(x0, y0, x1 - x0, y1 - y0));
    let base = match s.selection {
        SelectionState::None => Selection::empty(),
        _ => Selection::new(s.selection.clone()),
    };
    let mode = combine_mode(p["combine"].as_str().unwrap());
    let next = s.set_selection(base.combine(&rect, mode, s.width, s.height).state().clone());
    ctx.check()?;
    Ok(if p["amend"].as_bool().unwrap() {
        Outcome::Replace(next)
    } else {
        Outcome::State(next)
    })
}

/// The pixels of every layer of `s`, rotated by `angle` degrees with the
/// Rotate / Zoom effect (no selection, so the whole layer turns).
fn rotated_layers(ctx: &RunCtx, s: &DocState, angle: f64) -> Result<Vec<Vec<u8>>> {
    let plain = s.set_selection(SelectionState::None);
    let mut out = Vec::with_capacity(s.layers.len());
    for i in 0..s.layers.len() {
        ctx.check()?;
        let one = plain.set_active_layer(i).map_err(internal)?;
        let Outcome::State(turned) = run_effect(
            ctx,
            &one,
            "effect.layers.rotate_zoom",
            &json!({ "angle": angle }),
        )?
        else {
            return Err(internal("rotate returned no state"));
        };
        out.push(image_commands::read(&turned, i)?);
    }
    Ok(out)
}

fn crop_rect(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let int = |k: &str| p[k].as_i64().unwrap();
    let (x, y, w, h) = (int("x"), int("y"), int("width"), int("height"));
    let (cw, ch) = (i64::from(s.width), i64::from(s.height));
    if w <= 0 || h <= 0 || x < 0 || y < 0 || x + w > cw || y + h > ch {
        return Err(bad("crop rectangle must lie inside the canvas"));
    }
    let angle = p["angle"].as_f64().unwrap();
    let sources = if angle == 0.0 {
        let mut v = Vec::with_capacity(s.layers.len());
        for i in 0..s.layers.len() {
            ctx.check()?;
            v.push(image_commands::read(s, i)?);
        }
        v
    } else {
        rotated_layers(ctx, s, angle)?
    };
    let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
    let sw = s.width as usize;
    let mut next = s.empty_image_like(w as u32, h as u32).map_err(internal)?;
    for (i, src) in sources.iter().enumerate() {
        ctx.check()?;
        let mut out = vec![0; w * h * 4];
        for row in 0..h {
            let a = ((y + row) * sw + x) * 4;
            out[row * w * 4..(row + 1) * w * 4].copy_from_slice(&src[a..a + w * 4]);
        }
        next = image_commands::write(next, i, &out)?;
        ctx.progress(i + 1, sources.len());
    }
    // An unrotated crop moves the selection with the pixels; a straightened
    // crop drops it (its outline no longer lines up with the pixel grid).
    let selection = match &s.selection {
        SelectionState::None => SelectionState::None,
        _ if angle != 0.0 => SelectionState::None,
        sel => Selection::new(sel.clone())
            .translate(-(x as f64), -(y as f64), w as u32, h as u32)
            .state()
            .clone(),
    };
    ctx.check()?;
    Ok(Outcome::State(next.set_selection(selection)))
}

/// `doc.selection_path`: the outline for the marching ants, in document
/// pixels. A shape selection gives its polygons (`paths`, each a flat
/// `[x0, y0, x1, y1, ...]` ring). A pixel-mask selection (Magic Wand) gives
/// the edges between covered (128 and up) and uncovered pixels as
/// `segments`, a flat `[x0, y0, x1, y1, ...]` list of straight runs.
pub(super) fn selection_path(s: &DocState) -> Value {
    let revision = s.selection_revision;
    match &s.selection {
        SelectionState::None => {
            json!({"active":false,"revision":revision,"paths":[],"segments":[]})
        }
        SelectionState::Active(SelectionSource::Geometry(poly)) => {
            let paths: Vec<Vec<f64>> = poly
                .0
                .iter()
                .filter(|r| r.len() >= 2)
                .map(|r| r.iter().flat_map(|&(x, y)| [x, y]).collect())
                .collect();
            json!({"active":true,"revision":revision,"paths":paths,"segments":[]})
        }
        SelectionState::Active(SelectionSource::Coverage(_)) => {
            let mask = s.rasterize_selection(RasterMode::Antialiased);
            json!({"active":true,"revision":revision,"paths":[],
                   "segments":mask_edges(&mask.bytes, s.width as usize, s.height as usize)})
        }
    }
}

/// The straight edge runs between pixels at 128 and above and the rest.
pub(super) fn mask_edges(bytes: &[u8], w: usize, h: usize) -> Vec<u32> {
    let inside = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && bytes[y as usize * w + x as usize] >= 128
    };
    let mut out = Vec::new();
    // Horizontal edges on line y, between rows y - 1 and y.
    for y in 0..=h as isize {
        let mut start: Option<usize> = None;
        for x in 0..=w {
            let edge = x < w && inside(x as isize, y - 1) != inside(x as isize, y);
            match (edge, start) {
                (true, None) => start = Some(x),
                (false, Some(x0)) => {
                    out.extend([x0 as u32, y as u32, x as u32, y as u32]);
                    start = None;
                }
                _ => {}
            }
        }
    }
    // Vertical edges on line x, between columns x - 1 and x.
    for x in 0..=w as isize {
        let mut start: Option<usize> = None;
        for y in 0..=h {
            let edge = y < h && inside(x - 1, y as isize) != inside(x, y as isize);
            match (edge, start) {
                (true, None) => start = Some(y),
                (false, Some(y0)) => {
                    out.extend([x as u32, y0 as u32, x as u32, y as u32]);
                    start = None;
                }
                _ => {}
            }
        }
    }
    out
}

/// `doc.resolution`: the document's pixels per inch.
pub(super) fn resolution(s: &DocState) -> Value {
    json!({"dpi": s.dpi})
}
