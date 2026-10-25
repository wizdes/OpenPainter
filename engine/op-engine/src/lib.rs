//! Ordered document commands, immutable published views, and cancellable preview jobs.
use arc_swap::ArcSwap;
use crossbeam_channel::{Receiver, Sender, unbounded};
use op_core::doc::{
    DocState, LayerId, PolygonSet, SelectionSource, SelectionState, ToolCompositeMode,
};
use op_core::history::History;
use op_core::selection::CombineMode;
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use op_ops::effect::{self, Rect};
pub use op_ops::effect::{Param, ParamKind, ParamSchema};
use op_tools::flood::{self, AlphaMode, FloodOptions, Sampling};
use op_tools::gradient::GradientSession;
use op_tools::line::LineSession;
use op_tools::r#move::{MoveKind, MoveOptions, MoveSession};
use op_tools::shape::ShapeSession;
use op_tools::text::TextSession;
use op_tools::{BrushSettings, CloneSource, RecolorOptions, StrokeSample, StrokeSession};
use parking_lot::Mutex;
use rayon::ThreadPool;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use thiserror::Error;
mod color_sample;
mod document_file;
mod flood_commands;
mod gpu_effects;
mod gradient_commands;
mod image_commands;
mod layer_commands;
mod layer_menu_commands;
mod line_commands;
pub mod live_stroke;
mod profile_query;
pub mod save_export;
mod select_crop_commands;
mod selection_commands;
mod shape_commands;
mod shell_commands;
mod text_commands;
pub use document_file::SaveFormat;
pub use gpu_effects::EngineOptions;
pub use layer_commands::{BLEND_MODES, blend_name, content_key};
pub use shell_commands::{effect_label, effects_list};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("bad argument: {0}")]
    BadArgument(String),
    #[error("no such document")]
    NoSuchDoc,
    #[error("document is read only after a panic")]
    ReadOnly,
    #[error("command or job panicked")]
    Panic,
    #[error("operation is busy")]
    Busy,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("job was cancelled")]
    Cancelled,
}
impl Error {
    /// The short name used as `status` in JobFinished payloads.
    pub fn status_name(&self) -> &'static str {
        match self {
            Error::BadArgument(_) => "bad_argument",
            Error::NoSuchDoc => "no_such_doc",
            Error::ReadOnly => "read_only",
            Error::Panic => "panic",
            Error::Busy => "busy",
            Error::Internal(_) => "internal",
            Error::Cancelled => "cancelled",
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn internal(e: impl std::fmt::Display) -> Error {
    Error::Internal(e.to_string())
}
fn bad(e: impl std::fmt::Display) -> Error {
    Error::BadArgument(e.to_string())
}
/// The JobFinished payload for a finished operation: `{"status":"ok"}` or
/// `{"status":<error name>,"error":<text>}`.
pub fn finished_json<T>(result: &Result<T>) -> Value {
    match result {
        Ok(_) => json!({"status":"ok"}),
        Err(e) => json!({"status":e.status_name(),"error":e.to_string()}),
    }
}

/// A caller's handle on one dispatched operation. Cancelling it stops queued work
/// before it starts and running work at its next check (between tiles and inside
/// long loops). Cancelled work never commits.
#[derive(Clone, Debug)]
pub struct JobTicket {
    id: u64,
    cancel: Arc<AtomicBool>,
}
impl JobTicket {
    /// Ticket 0 means "no caller ticket"; JobFinished is still sent for preview renders.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
}
/// What a running command can see besides the document: its cancel flag and a
/// progress reporter.
pub struct RunCtx {
    ticket: JobTicket,
    doc: u64,
    events: Sender<Event>,
    gpu: Arc<gpu_effects::GpuShared>,
}
impl RunCtx {
    /// Returns `Err(Cancelled)` once the ticket is cancelled.
    pub fn check(&self) -> Result<()> {
        if self.ticket.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    /// Sleeps in 5 ms steps and stops early on cancel.
    pub fn sleep(&self, ms: u64) -> Result<()> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            self.check()?;
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            std::thread::sleep(left.min(Duration::from_millis(5)));
        }
    }
    /// Sends a Progress event for caller tickets.
    pub fn progress(&self, done: usize, total: usize) {
        if self.ticket.id != 0 {
            let _ = self.events.send(Event {
                kind: EventKind::Progress,
                doc: self.doc,
                ticket: self.ticket.id,
                json: json!({"done":done,"total":total}),
            });
        }
    }
}

pub struct CommandSpec {
    pub id: &'static str,
    pub params: ParamSchema,
    pub enabled: fn(&DocState) -> bool,
    pub run: fn(&RunCtx, &DocState, &Value) -> Result<Outcome>,
    pub previewable: bool,
}
pub enum Outcome {
    State(DocState),
    HistoryUndo,
    HistoryRedo,
    HistoryJump(usize),
    /// Replaces the current step's state with no new history step and no
    /// change to the dirty flag (choosing the active layer).
    Replace(DocState),
}
fn always(_: &DocState) -> bool {
    true
}
fn can_delete(s: &DocState) -> bool {
    s.layers.len() > 1
}
fn add(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    Ok(Outcome::State(
        s.add_layer(s.layers.len(), p["name"].as_str().unwrap())
            .map_err(internal)?,
    ))
}
fn delete(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    Ok(Outcome::State(
        s.delete_layer(s.active_layer).map_err(internal)?,
    ))
}
/// Inverts RGB and keeps alpha on every stored tile of the active layer, checking
/// for cancel before each tile. `delay_ms` slows each tile for tests.
fn invert_tiles(ctx: &RunCtx, s: &DocState, delay_ms: u64) -> Result<Outcome> {
    let mut next = s.clone();
    let layer = &s.layers[s.active_layer];
    let coords: Vec<TileCoord> = layer.tiles.iter().map(|(c, _)| c).collect();
    for (done, coord) in coords.iter().enumerate() {
        ctx.check()?;
        ctx.sleep(delay_ms)?;
        let mut tile = s.read_tile(s.active_layer, *coord).map_err(internal)?;
        for px in tile.chunks_exact_mut(4) {
            for c in &mut px[..3] {
                *c = 255 - *c;
            }
        }
        next = next
            .write_tile(
                s.active_layer,
                *coord,
                TileData::try_from(tile).map_err(internal)?,
            )
            .map_err(internal)?;
        ctx.progress(done + 1, coords.len());
    }
    ctx.check()?;
    Ok(Outcome::State(next))
}
fn run_effect(ctx: &RunCtx, s: &DocState, id: &str, params: &Value) -> Result<Outcome> {
    let effect = effect::find(id).ok_or_else(|| bad(format!("unknown effect {id}")))?;
    if let Some(next) = gpu_effects::try_render(ctx, s, &effect, params)? {
        ctx.check()?;
        return Ok(Outcome::State(next));
    }
    let rect = Rect::new(0, 0, s.width, s.height);
    let coords = effect::tile_coords(rect);
    let next = effect::render_tiles_checked(
        s,
        &effect,
        rect,
        &coords,
        Some(params.clone()),
        || ctx.check().map_err(|_| effect::EffectError::Cancelled),
        |done, total| ctx.progress(done, total),
    )
    .map_err(|error| match error {
        effect::EffectError::Cancelled => Error::Cancelled,
        other => bad(other),
    })?;
    ctx.check()?;
    Ok(Outcome::State(next))
}
fn run_invert_colors(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    run_effect(ctx, s, "effect.adjust.invert_colors", p)
}
fn run_invert_alpha(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    run_effect(ctx, s, "effect.adjust.invert_alpha", p)
}
fn slow_invert(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    invert_tiles(ctx, s, p["delay_ms"].as_u64().unwrap())
}
fn preview_fill(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.sleep(p["delay_ms"].as_u64().unwrap())?;
    let mut tile = s
        .read_tile(s.active_layer, TileCoord { x: 0, y: 0 })
        .map_err(internal)?;
    tile[0] = p["value"].as_u64().unwrap() as u8;
    let next = s
        .write_tile(
            s.active_layer,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).map_err(internal)?,
        )
        .map_err(internal)?;
    Ok(Outcome::State(next))
}
fn flood_options(p: &Value) -> FloodOptions {
    FloodOptions {
        contiguous: p["contiguous"].as_bool().unwrap(),
        tolerance: p["tolerance"].as_f64().unwrap(),
        alpha: if p["alpha"].as_str().unwrap() == "premultiplied" {
            AlphaMode::Premultiplied
        } else {
            AlphaMode::Straight
        },
        sampling: if p["sampling"].as_str().unwrap() == "image" {
            Sampling::Image
        } else {
            Sampling::Layer
        },
        antialias: p["antialias"].as_bool().unwrap(),
    }
}
fn flood_seed(p: &Value) -> Result<(u32, u32)> {
    Ok((
        u32::try_from(p["x"].as_i64().unwrap()).map_err(bad)?,
        u32::try_from(p["y"].as_i64().unwrap()).map_err(bad)?,
    ))
}
fn run_wand(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let mode = match p["combine"].as_str().unwrap() {
        "replace" => CombineMode::Replace,
        "add" => CombineMode::Add,
        "subtract" => CombineMode::Subtract,
        "intersect" => CombineMode::Intersect,
        _ => CombineMode::Invert,
    };
    let next = flood::magic_wand(s, s.active_layer, flood_seed(p)?, flood_options(p), mode)
        .map_err(bad)?;
    ctx.check()?;
    Ok(Outcome::State(next))
}
fn run_bucket(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let hex = p["color"].as_str().unwrap();
    if hex.len() != 8 || !hex.is_ascii() {
        return Err(bad("color must be eight RGBA hex digits"));
    }
    let mut color = [0; 4];
    for (i, c) in color.iter_mut().enumerate() {
        *c = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(bad)?;
    }
    let blend = if p["blend"].as_str().unwrap() == "overwrite" {
        ToolCompositeMode::Overwrite
    } else {
        ToolCompositeMode::Normal
    };
    let next = flood::paint_bucket(
        s,
        s.active_layer,
        flood_seed(p)?,
        flood_options(p),
        color,
        blend,
    )
    .map_err(bad)?;
    ctx.check()?;
    Ok(Outcome::State(next))
}
fn flood_params(bucket: bool) -> ParamSchema {
    let mut fields = vec![
        Param {
            key: "x",
            kind: ParamKind::Int,
            min: Some(0.0),
            max: None,
            default: None,
        },
        Param {
            key: "y",
            kind: ParamKind::Int,
            min: Some(0.0),
            max: None,
            default: None,
        },
        Param {
            key: "tolerance",
            kind: ParamKind::Float,
            min: Some(0.0),
            max: Some(1.0),
            default: Some(json!(0.0)),
        },
        Param {
            key: "contiguous",
            kind: ParamKind::Bool,
            min: None,
            max: None,
            default: Some(json!(true)),
        },
        Param {
            key: "alpha",
            kind: ParamKind::Choice(&["straight", "premultiplied"]),
            min: None,
            max: None,
            default: Some(json!("straight")),
        },
        Param {
            key: "sampling",
            kind: ParamKind::Choice(&["layer", "image"]),
            min: None,
            max: None,
            default: Some(json!("layer")),
        },
        Param {
            key: "antialias",
            kind: ParamKind::Bool,
            min: None,
            max: None,
            default: Some(json!(false)),
        },
    ];
    if bucket {
        fields.extend([
            Param {
                key: "color",
                kind: ParamKind::String,
                min: None,
                max: None,
                default: None,
            },
            Param {
                key: "blend",
                kind: ParamKind::Choice(&["normal", "overwrite"]),
                min: None,
                max: None,
                default: Some(json!("normal")),
            },
        ]);
    } else {
        fields.push(Param {
            key: "combine",
            kind: ParamKind::Choice(&["replace", "add", "subtract", "intersect", "invert"]),
            min: None,
            max: None,
            default: Some(json!("replace")),
        });
    }
    ParamSchema { fields }
}
/// Test-only: toggles between no selection and a small triangle.
fn toggle_selection(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let next = match s.selection {
        SelectionState::None => {
            SelectionState::Active(SelectionSource::Geometry(Arc::new(PolygonSet(vec![vec![
                (0.0, 0.0),
                (1.0, 0.0),
                (1.0, 1.0),
            ]]))))
        }
        SelectionState::Active(_) => SelectionState::None,
    };
    Ok(Outcome::State(s.set_selection(next)))
}
/// Test-only: an antialiased ellipse (geometry) or a soft gradient mask
/// (coverage) that leaves the left and bottom edges unselected.
fn set_selection_shape(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let (w, h) = (f64::from(s.width), f64::from(s.height));
    let source = match p["shape"].as_str().unwrap() {
        "ellipse" => {
            let (cx, cy, rx, ry) = (w * 0.47 + 0.3, h * 0.52 + 0.2, w * 0.33, h * 0.36);
            let points = (0..96)
                .map(|i| {
                    let t = f64::from(i) * std::f64::consts::TAU / 96.0;
                    (cx + rx * t.cos(), cy + ry * t.sin())
                })
                .collect();
            SelectionSource::Geometry(Arc::new(PolygonSet(vec![points])))
        }
        _ => {
            let mut bytes = Vec::with_capacity(s.width as usize * s.height as usize);
            for y in 0..s.height {
                for x in 0..s.width {
                    let off = x < s.width / 6 || y > s.height * 5 / 6;
                    bytes.push(if off {
                        0
                    } else {
                        ((x * 7 + y * 3) % 300).min(255) as u8
                    });
                }
            }
            let grid = op_core::doc::MaskGrid::new(s.width, s.height, bytes)
                .ok_or_else(|| internal("mask size"))?;
            SelectionSource::Coverage(Arc::new(grid))
        }
    };
    Ok(Outcome::State(
        s.set_selection(SelectionState::Active(source)),
    ))
}
fn panic_preview(_: &RunCtx, _: &DocState, _: &Value) -> Result<Outcome> {
    panic!("test worker panic")
}
fn undo(_: &RunCtx, _: &DocState, _: &Value) -> Result<Outcome> {
    Ok(Outcome::HistoryUndo)
}
fn redo(_: &RunCtx, _: &DocState, _: &Value) -> Result<Outcome> {
    Ok(Outcome::HistoryRedo)
}
fn jump(_: &RunCtx, _: &DocState, p: &Value) -> Result<Outcome> {
    Ok(Outcome::HistoryJump(p["index"].as_u64().unwrap() as usize))
}
fn panic_run(_: &RunCtx, _: &DocState, _: &Value) -> Result<Outcome> {
    panic!("test command panic")
}
fn requested_profile(value: &Value) -> Result<op_color::Profile> {
    let s = value
        .as_str()
        .ok_or_else(|| bad("profile must be a string"))?;
    if let Ok(p) = op_color::Profile::builtin(s) {
        return Ok(p);
    }
    let data = decode_base64(s.strip_prefix("base64:").unwrap_or(s))?;
    op_color::Profile::from_icc(&data).map_err(bad)
}
fn decode_base64(s: &str) -> Result<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) || bytes.len() > 24 * 1024 * 1024 {
        return Err(bad("invalid base64 ICC"));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let mut v = [0u8; 4];
        for j in 0..4 {
            v[j] = match chunk[j] {
                b'A'..=b'Z' => chunk[j] - b'A',
                b'a'..=b'z' => chunk[j] - b'a' + 26,
                b'0'..=b'9' => chunk[j] - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if i == bytes.len() / 4 - 1 && j >= 2 => 0,
                _ => return Err(bad("invalid base64 ICC")),
            };
        }
        if chunk[2] == b'=' && chunk[3] != b'=' {
            return Err(bad("invalid base64 padding"));
        }
        let n = if chunk[2] == b'=' {
            1
        } else if chunk[3] == b'=' {
            2
        } else {
            3
        };
        if n < 3 && ((n == 1 && v[1] & 15 != 0) || (n == 2 && v[2] & 3 != 0)) {
            return Err(bad("invalid base64 padding"));
        }
        let bits =
            ((v[0] as u32) << 18) | ((v[1] as u32) << 12) | ((v[2] as u32) << 6) | v[3] as u32;
        out.push((bits >> 16) as u8);
        if n > 1 {
            out.push((bits >> 8) as u8);
        }
        if n > 2 {
            out.push(bits as u8);
        }
    }
    Ok(out)
}
fn assign_profile(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let profile = requested_profile(&p["profile"])?;
    if profile.color_space() != lcms2::ColorSpaceSignature::RgbData {
        return Err(bad("document profile must be RGB"));
    }
    Ok(Outcome::State(s.set_profile(Some(profile.bytes()))))
}
fn convert_profile(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let dst = requested_profile(&p["profile"])?;
    let src = match &s.profile {
        Some(bytes) => op_color::Profile::from_icc(bytes).map_err(bad)?,
        None => op_color::Profile::builtin("srgb").map_err(bad)?,
    };
    let intent = op_color::RenderingIntent::parse(p["intent"].as_str().unwrap()).map_err(bad)?;
    let mut next = s.clone();
    for layer in 0..s.layers.len() {
        let coords: Vec<_> = s.layers[layer]
            .tiles
            .iter()
            .map(|(coord, _)| coord)
            .collect();
        for coord in coords {
            ctx.check()?;
            let mut tile = s.read_tile(layer, coord).map_err(internal)?;
            op_color::convert_rgba(&src, &dst, intent, p["bpc"].as_bool().unwrap(), &mut tile)
                .map_err(bad)?;
            next = next
                .write_tile(layer, coord, TileData::try_from(tile).map_err(internal)?)
                .map_err(internal)?;
        }
    }
    ctx.check()?;
    Ok(Outcome::State(next.set_profile(Some(dst.bytes()))))
}
fn specs() -> Vec<CommandSpec> {
    let empty = || ParamSchema::default();
    let mut entries = vec![
        CommandSpec {
            id: "tool.magic_wand",
            params: flood_params(false),
            enabled: always,
            run: run_wand,
            previewable: true,
        },
        CommandSpec {
            id: "tool.paint_bucket",
            params: flood_params(true),
            enabled: always,
            run: run_bucket,
            previewable: true,
        },
        CommandSpec {
            id: "image.assign_profile",
            params: ParamSchema {
                fields: vec![Param {
                    key: "profile",
                    kind: ParamKind::String,
                    min: None,
                    max: None,
                    default: None,
                }],
            },
            enabled: always,
            run: assign_profile,
            previewable: false,
        },
        CommandSpec {
            id: "image.convert_profile",
            params: ParamSchema {
                fields: vec![
                    Param {
                        key: "profile",
                        kind: ParamKind::String,
                        min: None,
                        max: None,
                        default: None,
                    },
                    Param {
                        key: "intent",
                        kind: ParamKind::Choice(&[
                            "perceptual",
                            "relative",
                            "saturation",
                            "absolute",
                        ]),
                        min: None,
                        max: None,
                        default: Some(json!("perceptual")),
                    },
                    Param {
                        key: "bpc",
                        kind: ParamKind::Bool,
                        min: None,
                        max: None,
                        default: Some(json!(false)),
                    },
                ],
            },
            enabled: always,
            run: convert_profile,
            previewable: false,
        },
        CommandSpec {
            id: "layer.add",
            params: ParamSchema {
                fields: vec![Param {
                    key: "name",
                    kind: ParamKind::String,
                    min: None,
                    max: None,
                    default: Some(json!("Layer")),
                }],
            },
            enabled: always,
            run: add,
            previewable: false,
        },
        CommandSpec {
            id: "layer.delete",
            params: empty(),
            enabled: can_delete,
            run: delete,
            previewable: false,
        },
        CommandSpec {
            id: "image.invert_colors",
            params: empty(),
            enabled: always,
            run: run_invert_colors,
            previewable: true,
        },
        CommandSpec {
            id: "edit.undo",
            params: empty(),
            enabled: always,
            run: undo,
            previewable: false,
        },
        CommandSpec {
            id: "edit.redo",
            params: empty(),
            enabled: always,
            run: redo,
            previewable: false,
        },
        CommandSpec {
            id: "history.jump",
            params: ParamSchema {
                fields: vec![Param {
                    key: "index",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: None,
                    default: None,
                }],
            },
            enabled: always,
            run: jump,
            previewable: false,
        },
        CommandSpec {
            id: "test.panic",
            params: empty(),
            enabled: always,
            run: panic_run,
            previewable: false,
        },
        CommandSpec {
            id: "test.panic_preview",
            params: empty(),
            enabled: always,
            run: panic_preview,
            previewable: true,
        },
        CommandSpec {
            id: "test.preview_fill",
            params: ParamSchema {
                fields: vec![
                    Param {
                        key: "value",
                        kind: ParamKind::Int,
                        min: Some(0.0),
                        max: Some(255.0),
                        default: None,
                    },
                    Param {
                        key: "delay_ms",
                        kind: ParamKind::Int,
                        min: Some(0.0),
                        max: Some(1000.0),
                        default: Some(json!(0)),
                    },
                ],
            },
            enabled: always,
            run: preview_fill,
            previewable: true,
        },
        CommandSpec {
            id: "test.slow_invert",
            params: ParamSchema {
                fields: vec![Param {
                    key: "delay_ms",
                    kind: ParamKind::Int,
                    min: Some(0.0),
                    max: Some(1000.0),
                    default: Some(json!(0)),
                }],
            },
            enabled: always,
            run: slow_invert,
            previewable: true,
        },
        CommandSpec {
            id: "test.set_selection",
            params: empty(),
            enabled: always,
            run: toggle_selection,
            previewable: false,
        },
        CommandSpec {
            id: "test.set_selection_shape",
            params: ParamSchema {
                fields: vec![Param {
                    key: "shape",
                    kind: ParamKind::Choice(&["ellipse", "soft"]),
                    min: None,
                    max: None,
                    default: None,
                }],
            },
            enabled: always,
            run: set_selection_shape,
            previewable: false,
        },
    ];
    entries.extend(image_commands::specs());
    entries.extend(shell_commands::specs());
    entries.extend(layer_commands::specs());
    entries.extend(layer_menu_commands::specs());
    entries.extend(select_crop_commands::specs());
    entries.extend(selection_commands::specs());
    for (index, effect) in effect::registered().into_iter().enumerate() {
        let run = match effect.id {
            "effect.adjust.invert_colors" => run_invert_colors,
            "effect.adjust.invert_alpha" => run_invert_alpha,
            _ => shell_commands::effect_runner(index).unwrap_or_else(|| {
                panic!(
                    "effect {} at registry index {index} has no runner slot; \
                     grow RUNNERS in shell_commands.rs",
                    effect.id
                )
            }),
        };
        entries.push(CommandSpec {
            id: effect.id,
            params: effect.schema,
            enabled: always,
            run,
            previewable: true,
        });
    }
    entries
}
fn spec(id: &str) -> Result<CommandSpec> {
    specs()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| bad(format!("unknown command {id}")))
}
#[derive(Clone)]
pub struct HistorySummary {
    pub labels: Vec<String>,
    /// The command id of each step, for the History panel's icons.
    pub commands: Vec<String>,
    pub index: usize,
    pub dirty: bool,
}
#[derive(Clone)]
pub struct Published {
    pub state: DocState,
    pub history: HistorySummary,
    pub revision: u64,
    pub preview: Option<DocState>,
    /// Changes whenever a different preview is shown; 0 with no preview. Views
    /// redraw when `(revision, preview_serial)` changes.
    pub preview_serial: u64,
}
impl Published {
    pub fn info(&self) -> Value {
        let profile_description = match &self.state.profile {
            Some(bytes) => op_color::Profile::from_icc(bytes)
                .map(|p| p.description().to_owned())
                .unwrap_or_else(|e| format!("Invalid ICC profile: {e}")),
            None => "sRGB".into(),
        };
        json!({"width":self.state.width,"height":self.state.height,"layers":self.state.layers.iter().map(|l|json!({"id":l.id.0,"name":l.name,"visible":l.visible,"opacity":l.opacity,"blend":blend_name(l.blend),"revision":content_key(l)})).collect::<Vec<_>>(),"active_layer":self.state.active_layer,"history_labels":self.history.labels,"history_commands":self.history.commands,"history_index":self.history.index,"dirty":self.history.dirty,"revision":self.revision,"profile_description":profile_description,"profile_embedded":self.state.profile.is_some()})
    }
    pub fn pixels(&self, preview: bool) -> Result<Vec<u8>> {
        let state = if preview {
            self.preview.as_ref().unwrap_or(&self.state)
        } else {
            &self.state
        };
        flatten(state)
    }
}
/// The flattened composite of `state` as straight RGBA8, row by row.
pub(crate) fn flatten(state: &DocState) -> Result<Vec<u8>> {
    let len = (state.width as usize)
        .checked_mul(state.height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| bad("image too large"))?;
    if len > 1024 * 1024 * 1024 {
        return Err(bad("image too large"));
    }
    let mut out = vec![0; len];
    for ty in 0..state.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..state.width.div_ceil(TILE_SIZE as u32) {
            let tile = op_ops::composite::composite_tile(
                state,
                TileCoord {
                    x: tx as i32,
                    y: ty as i32,
                },
            )
            .map_err(internal)?;
            for y in 0..(state.height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize {
                let n = (state.width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize * 4;
                let dst = ((ty as usize * TILE_SIZE + y) * state.width as usize
                    + tx as usize * TILE_SIZE)
                    * 4;
                out[dst..dst + n]
                    .copy_from_slice(&tile.as_bytes()[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + n]);
            }
        }
    }
    Ok(out)
}
#[derive(Clone, Debug)]
pub enum EventKind {
    DocChanged,
    HistoryChanged,
    LayersChanged,
    JobFinished,
    Progress,
}
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: EventKind,
    pub doc: u64,
    pub ticket: u64,
    pub json: Value,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobIdentity {
    pub doc: u64,
    pub base_revision: u64,
    pub target_layer: LayerId,
    pub selection_revision: u64,
    pub preview_session: u64,
    pub generation: u64,
}
impl JobIdentity {
    /// Names of the fields that differ from `current`, in declaration order.
    pub fn mismatches(&self, current: &JobIdentity) -> Vec<&'static str> {
        [
            ("doc", self.doc == current.doc),
            ("base_revision", self.base_revision == current.base_revision),
            ("target_layer", self.target_layer == current.target_layer),
            (
                "selection_revision",
                self.selection_revision == current.selection_revision,
            ),
            (
                "preview_session",
                self.preview_session == current.preview_session,
            ),
            ("generation", self.generation == current.generation),
        ]
        .into_iter()
        .filter(|(_, same)| !same)
        .map(|(name, _)| name)
        .collect()
    }
}
/// One mutating operation. `Engine::submit` queues it on the document's actor.
#[derive(Clone, Debug)]
pub enum Op {
    Command(String, Value),
    PreviewBegin(String, Value),
    PreviewUpdate(Value),
    PreviewCommit,
    PreviewCancel,
}
struct PreviewSession {
    id: u64,
    command: &'static str,
    base_revision: u64,
    target: LayerId,
    selection_revision: u64,
    params: Value,
    generation: u64,
}
struct Document {
    tx: Sender<Message>,
    published: Arc<ArcSwap<Published>>,
    read_only: Arc<AtomicBool>,
    join: Mutex<Option<JoinHandle<()>>>,
    store: Arc<TileStore>,
}
enum Message {
    Run(Op, JobTicket, Option<Sender<Result<Option<u64>>>>),
    Job(JobIdentity, JobTicket, Result<DocState>),
    Inject(JobIdentity, JobTicket, DocState),
    Probe(Sender<(DocState, usize, usize, u64)>),
    View(Sender<Published>),
    Identity(Sender<JobIdentity>),
    StrokeBegin(BrushSettings, u64, Sender<Result<u64>>),
    /// A brush stroke with a hardness (M38 live strokes).
    StrokeBeginHard(BrushSettings, f64, u64, Sender<Result<u64>>),
    StrokeBeginClone(
        BrushSettings,
        u64,
        CloneSource,
        (i32, i32),
        Option<(i32, i32)>,
        Sender<Result<u64>>,
    ),
    StrokeBeginRecolor(BrushSettings, u64, RecolorOptions, Sender<Result<u64>>),
    StrokeBeginHeal(
        BrushSettings,
        u64,
        CloneSource,
        (i32, i32),
        Option<(i32, i32)>,
        Sender<Result<u64>>,
    ),
    StrokeBeginSpotHeal(BrushSettings, u64, Sender<Result<u64>>),
    StrokeCloneOffset(Sender<Result<Option<(i32, i32)>>>),
    StrokeAppend(Vec<StrokeSample>, Sender<Result<()>>),
    StrokeCommit(Sender<Result<()>>),
    StrokeCancel(Sender<Result<()>>),
    MoveBegin(MoveKind, MoveOptions, Sender<Result<u64>>),
    MoveUpdate(MoveOptions, Sender<Result<()>>),
    MoveFinish(Sender<Result<()>>),
    MoveCancel(Sender<Result<()>>),
    /// M19: save, mark saved, replace content (document_file.rs).
    File(document_file::FileOp, Sender<Result<()>>),
    Stop(Sender<()>),
}
struct ActiveStroke {
    session: StrokeSession,
    revision: u64,
    selection_revision: u64,
}
struct Actor {
    id: u64,
    history: History,
    revision: u64,
    /// Counts changes of the current selection, including undo and redo.
    selection_revision: u64,
    preview: Option<PreviewSession>,
    stroke: Option<ActiveStroke>,
    move_session: Option<(u64, MoveSession)>,
    gradient_session: Option<(u64, GradientSession)>,
    line_session: Option<(u64, LineSession)>,
    shape_session: Option<(u64, ShapeSession)>,
    text_session: Option<(u64, TextSession)>,
    published: Arc<ArcSwap<Published>>,
    read_only: Arc<AtomicBool>,
    events: Sender<Event>,
    tx: Sender<Message>,
    pool: Arc<ThreadPool>,
    next_session: Arc<AtomicU64>,
    pending_jobs: usize,
    /// The last `Published::preview_serial` handed out.
    preview_serial: AtomicU64,
    gpu: Arc<gpu_effects::GpuShared>,
}
impl Actor {
    fn view(&self, preview: Option<DocState>) -> Published {
        let h = &self.history;
        Published {
            state: h.current_state().clone(),
            history: HistorySummary {
                labels: (0..h.len())
                    .map(|i| h.label(i).unwrap().to_owned())
                    .collect(),
                commands: (0..h.len())
                    .map(|i| h.command_id(i).unwrap().to_owned())
                    .collect(),
                index: h.current_index(),
                dirty: h.is_dirty(),
            },
            revision: self.revision,
            preview_serial: if preview.is_some() {
                self.preview_serial.fetch_add(1, Ordering::Relaxed) + 1
            } else {
                0
            },
            preview,
        }
    }
    fn publish(&self, preview: Option<DocState>) {
        self.published.store(Arc::new(self.view(preview)));
    }
    fn event(&self, kind: EventKind, ticket: u64, json: Value) {
        let _ = self.events.send(Event {
            kind,
            doc: self.id,
            ticket,
            json,
        });
    }
    fn ctx(&self, ticket: &JobTicket) -> RunCtx {
        RunCtx {
            ticket: ticket.clone(),
            doc: self.id,
            events: self.events.clone(),
            gpu: self.gpu.clone(),
        }
    }
    fn active_id(&self) -> LayerId {
        let s = self.history.current_state();
        s.layers[s.active_layer].id
    }
    /// The identity a job started now would carry.
    fn identity_now(&self) -> JobIdentity {
        JobIdentity {
            doc: self.id,
            base_revision: self.revision,
            target_layer: self.active_id(),
            selection_revision: self.selection_revision,
            preview_session: self.preview.as_ref().map_or(0, |p| p.id),
            generation: self.preview.as_ref().map_or(0, |p| p.generation),
        }
    }
    fn mark_read_only(&mut self) {
        self.read_only.store(true, Ordering::Release);
        self.preview = None;
        self.stroke = None;
        self.move_session = None;
        self.gradient_session = None;
        self.line_session = None;
        self.shape_session = None;
        self.text_session = None;
        self.publish(None);
    }
    fn command(&mut self, id: &str, params: Value, ticket: &JobTicket) -> Result<()> {
        // M31: an open Magic Wand or Paint Bucket session (flood_commands.rs).
        if self
            .preview
            .as_ref()
            .is_some_and(|p| flood_commands::is_flood(p.command))
        {
            if id == "edit.undo" {
                self.preview = None;
                self.publish(None);
                return Ok(());
            }
            self.commit(ticket)?;
        }
        if id.starts_with("tool.text.") {
            if self.line_session.is_some() {
                self.line_finish()?;
            }
            if self.shape_session.is_some() {
                self.shape_finish()?;
            }
            return self.text_command(id, &params);
        }
        if self.text_session.is_some() {
            // Same rules as Line/Curve: Undo drops the open text, another tool finishes it.
            if id == "edit.undo" {
                self.text_session = None;
                self.publish(None);
                return Ok(());
            }
            if id == "layer.delete" {
                self.text_session = None;
                self.publish(None);
            } else if id == "tool.select" || id.starts_with("tool.") {
                self.text_finish()?;
                if id == "tool.select" {
                    return Ok(());
                }
            } else {
                return Err(Error::Busy);
            }
        }
        if id.starts_with("tool.shape.") {
            return self.shape_command(id, &params);
        }
        if self.shape_session.is_some() {
            if id == "edit.undo" {
                self.shape_session = None;
                self.publish(None);
                return Ok(());
            }
            if id == "layer.delete" {
                self.shape_session = None;
                self.publish(None);
            } else if id == "tool.select" || id.starts_with("tool.") {
                self.shape_finish()?;
                if id == "tool.select" {
                    return Ok(());
                }
            } else {
                return Err(Error::Busy);
            }
        }
        if id.starts_with("tool.line.") {
            return self.line_command(id, &params);
        }
        if self.line_session.is_some() {
            if id == "edit.undo" {
                self.line_session = None;
                self.publish(None);
                return Ok(());
            }
            if id == "layer.delete" {
                self.line_session = None;
                self.publish(None);
            } else if id == "tool.select" || id.starts_with("tool.") {
                self.line_finish()?;
                if id == "tool.select" {
                    return Ok(());
                }
            } else {
                return Err(Error::Busy);
            }
        }
        if id == "tool.select" {
            return Ok(());
        }
        if id.starts_with("tool.gradient.") {
            return self.gradient_command(id, &params);
        }
        if self.gradient_session.is_some() {
            return Err(Error::Busy);
        }
        if self.move_session.is_some() {
            self.move_finish()?;
        }
        if self.stroke.is_some() {
            return Err(Error::Busy);
        }
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        let spec = spec(id)?;
        let params = spec.params.validate(params).map_err(bad)?;
        if !(spec.enabled)(self.history.current_state()) {
            return Err(bad("command disabled"));
        }
        let (amend, params) = select_crop_commands::amend_base(id, params, &self.history);
        let base = amend.as_ref().unwrap_or(self.history.current_state());
        let outcome = (spec.run)(&self.ctx(ticket), base, &params)?;
        // A cancel that lands after the last check still wins: nothing commits.
        self.ctx(ticket).check()?;
        let selection = self.history.current_state().selection.clone();
        let changed = match outcome {
            Outcome::State(state) => {
                let label = match id {
                    "effect.adjust.invert_colors" | "image.invert_colors" => "Invert Colors".into(),
                    "effect.adjust.invert_alpha" => "Invert Alpha".into(),
                    _ => shell_commands::history_label(id, &params)
                        .or_else(|| layer_commands::history_label(id, &params))
                        .or_else(|| layer_menu_commands::history_label(id).map(str::to_owned))
                        .or_else(|| select_crop_commands::history_label(id).map(str::to_owned))
                        .or_else(|| selection_commands::history_label(id, &params))
                        .or_else(|| image_commands::history_label(id).map(str::to_owned))
                        .or_else(|| flood_commands::history_label(id).map(str::to_owned))
                        .unwrap_or_else(|| id.into()),
                };
                self.history.commit(label, id, state);
                true
            }
            Outcome::HistoryUndo => self.history.undo(),
            Outcome::HistoryRedo => self.history.redo(),
            Outcome::HistoryJump(index) => {
                self.history.jump(index).map_err(bad)?;
                true
            }
            Outcome::Replace(state) => {
                self.history.replace_current(state);
                true
            }
        };
        if changed {
            if self.history.current_state().selection != selection {
                self.selection_revision += 1;
            }
            self.revision += 1;
            self.preview = None;
            self.publish(None);
            self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
            self.event(EventKind::HistoryChanged, 0, json!({}));
            if id.starts_with("layer.") {
                self.event(EventKind::LayersChanged, 0, json!({}));
            }
        }
        Ok(())
    }
    fn begin(&mut self, id: &str, params: Value, ticket: JobTicket) -> Result<u64> {
        if self.stroke.is_some()
            || self.move_session.is_some()
            || self.gradient_session.is_some()
            || self.line_session.is_some()
            || self.shape_session.is_some()
            || self.text_session.is_some()
        {
            return Err(Error::Busy);
        }
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        let spec = specs()
            .into_iter()
            .find(|s| s.id == id && s.previewable)
            .ok_or_else(|| bad("command cannot preview"))?;
        let params = spec.params.validate(params).map_err(bad)?;
        let session = self.next_session.fetch_add(1, Ordering::Relaxed);
        self.preview = Some(PreviewSession {
            id: session,
            command: spec.id,
            base_revision: self.revision,
            target: self.active_id(),
            selection_revision: self.selection_revision,
            params,
            generation: 0,
        });
        self.publish(None);
        self.render_preview(ticket);
        Ok(session)
    }
    /// Starts a render of the current session on the pool. Its result comes back as
    /// `Message::Job` and is shown only if its identity still matches.
    fn render_preview(&mut self, ticket: JobTicket) {
        let Some(p) = &mut self.preview else { return };
        p.generation += 1;
        let identity = JobIdentity {
            doc: self.id,
            base_revision: p.base_revision,
            target_layer: p.target,
            selection_revision: p.selection_revision,
            preview_session: p.id,
            generation: p.generation,
        };
        let state = self.history.current_state().clone();
        let params = p.params.clone();
        let id = p.command;
        let tx = self.tx.clone();
        let ctx = self.ctx(&ticket);
        self.pending_jobs += 1;
        self.pool.spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                ctx.check()?;
                match (spec(id)?.run)(&ctx, &state, &params)? {
                    Outcome::State(s) => Ok(s),
                    _ => Err(bad("preview command did not return pixels")),
                }
            }))
            .unwrap_or(Err(Error::Panic));
            let _ = tx.send(Message::Job(identity, ticket, result));
        });
    }
    fn update(&mut self, params: Value, ticket: JobTicket) -> Result<()> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        let p = self.preview.as_mut().ok_or_else(|| bad("no preview"))?;
        let params = if p.command.starts_with("tool.") {
            let mut merged = p.params.as_object().cloned().expect("validated parameters");
            for (key, value) in params
                .as_object()
                .ok_or_else(|| bad("parameters must be an object"))?
            {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        } else {
            params
        };
        p.params = spec(p.command)?.params.validate(params).map_err(bad)?;
        self.publish(None);
        self.render_preview(ticket);
        Ok(())
    }
    fn commit(&mut self, ticket: &JobTicket) -> Result<()> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        let p = self.preview.take().ok_or_else(|| bad("no preview"))?;
        if p.base_revision != self.revision
            || p.target != self.active_id()
            || p.selection_revision != self.selection_revision
        {
            return Err(Error::Busy);
        }
        let result = self.command(p.command, p.params, ticket);
        if result.is_err() {
            // Nothing committed: show the plain document again.
            self.publish(None);
        }
        result
    }
    fn gradient_command(&mut self, id: &str, params: &Value) -> Result<()> {
        match id {
            "tool.gradient.begin" => {
                if self.read_only.load(Ordering::Acquire) {
                    return Err(Error::ReadOnly);
                }
                if self.preview.is_some()
                    || self.stroke.is_some()
                    || self.move_session.is_some()
                    || self.gradient_session.is_some()
                    || self.text_session.is_some()
                {
                    return Err(Error::Busy);
                }
                let (options, start, end) = gradient_commands::begin(params)?;
                let session =
                    GradientSession::new(self.history.current_state().clone(), options, start, end)
                        .map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                let id = self.next_session.fetch_add(1, Ordering::Relaxed);
                self.gradient_session = Some((id, session));
                self.publish(Some(state));
                Ok(())
            }
            "tool.gradient.update" => {
                let (handle, point) = gradient_commands::update(params)?;
                let (_, session) = self
                    .gradient_session
                    .as_mut()
                    .ok_or_else(|| bad("no gradient session"))?;
                session.update(handle, point).map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                self.publish(Some(state));
                Ok(())
            }
            "tool.gradient.finish" => {
                if self.read_only.load(Ordering::Acquire) {
                    return Err(Error::ReadOnly);
                }
                let (_, session) = self
                    .gradient_session
                    .take()
                    .ok_or_else(|| bad("no gradient session"))?;
                if session.base_revision != self.history.current_state().revision
                    || session.target_layer != self.active_id()
                {
                    self.publish(None);
                    return Err(Error::Busy);
                }
                let state = session.finish().map_err(bad)?;
                self.history
                    .commit("Gradient", "tool.gradient.finish", state);
                self.revision += 1;
                self.publish(None);
                self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
                self.event(EventKind::HistoryChanged, 0, json!({}));
                Ok(())
            }
            "tool.gradient.cancel" => {
                if self.gradient_session.take().is_none() {
                    return Err(bad("no gradient session"));
                }
                self.publish(None);
                Ok(())
            }
            _ => Err(bad("unknown gradient command")),
        }
    }
    fn shape_finish(&mut self) -> Result<()> {
        let Some((_, session)) = self.shape_session.take() else {
            return Ok(());
        };
        if session.base_revision != self.history.current_state().revision
            || session.target_layer != self.active_id()
        {
            self.publish(None);
            return Err(Error::Busy);
        }
        let state = session.finish().map_err(bad)?;
        self.history.commit("Shape", "tool.shape.finish", state);
        self.revision += 1;
        self.publish(None);
        self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
        self.event(EventKind::HistoryChanged, 0, json!({}));
        Ok(())
    }
    fn shape_command(&mut self, id: &str, params: &Value) -> Result<()> {
        match id {
            "tool.shape.begin" => {
                if self.read_only.load(Ordering::Acquire) {
                    return Err(Error::ReadOnly);
                }
                if self.preview.is_some()
                    || self.stroke.is_some()
                    || self.move_session.is_some()
                    || self.gradient_session.is_some()
                    || self.line_session.is_some()
                    || self.shape_session.is_some()
                    || self.text_session.is_some()
                {
                    return Err(Error::Busy);
                }
                let (shape, corners, rotation, options) = shape_commands::begin(params)?;
                let session = ShapeSession::new(
                    self.history.current_state().clone(),
                    &shape,
                    corners,
                    rotation,
                    options,
                )
                .map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                let sid = self.next_session.fetch_add(1, Ordering::Relaxed);
                self.shape_session = Some((sid, session));
                self.publish(Some(state));
                Ok(())
            }
            "tool.shape.update" => {
                let shape = shape_commands::shape(params)?;
                let corners = shape_commands::box_update(params)?;
                let rotation = shape_commands::rotation(params)?;
                let (_, session) = self
                    .shape_session
                    .as_mut()
                    .ok_or_else(|| bad("no shape session"))?;
                let style = if shape_commands::has_style(params) {
                    Some(shape_commands::style(params, session.options)?)
                } else {
                    None
                };
                session
                    .update(shape, corners, rotation, style)
                    .map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                self.publish(Some(state));
                Ok(())
            }
            "tool.shape.finish" => self.shape_finish(),
            "tool.shape.cancel" => {
                self.shape_session = None;
                self.publish(None);
                Ok(())
            }
            _ => Err(bad("unknown shape command")),
        }
    }
    fn text_finish(&mut self) -> Result<()> {
        let Some((_, session)) = self.text_session.take() else {
            return Ok(());
        };
        if session.base_revision != self.history.current_state().revision
            || session.target_layer != self.active_id()
        {
            self.publish(None);
            return Err(Error::Busy);
        }
        let state = session.finish().map_err(bad)?;
        self.history.commit("Text", "tool.text.finish", state);
        self.revision += 1;
        self.publish(None);
        self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
        self.event(EventKind::HistoryChanged, 0, json!({}));
        Ok(())
    }
    fn text_command(&mut self, id: &str, params: &Value) -> Result<()> {
        match id {
            "tool.text.begin" => {
                if self.read_only.load(Ordering::Acquire) {
                    return Err(Error::ReadOnly);
                }
                if self.preview.is_some()
                    || self.stroke.is_some()
                    || self.move_session.is_some()
                    || self.gradient_session.is_some()
                    || self.line_session.is_some()
                    || self.shape_session.is_some()
                    || self.text_session.is_some()
                {
                    return Err(Error::Busy);
                }
                let b = text_commands::begin(params)?;
                let session = TextSession::new(
                    self.history.current_state().clone(),
                    b.origin,
                    b.style,
                    b.runs,
                    b.model,
                )
                .map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                let sid = self.next_session.fetch_add(1, Ordering::Relaxed);
                self.text_session = Some((sid, session));
                self.publish(Some(state));
                Ok(())
            }
            "tool.text.update" => {
                let (_, session) = self
                    .text_session
                    .as_mut()
                    .ok_or_else(|| bad("no text session"))?;
                let update = text_commands::update(params, session.style)?;
                session.update(update).map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                self.publish(Some(state));
                Ok(())
            }
            "tool.text.finish" => {
                if self.read_only.load(Ordering::Acquire) {
                    return Err(Error::ReadOnly);
                }
                if self.text_session.is_none() {
                    return Err(bad("no text session"));
                }
                self.text_finish()
            }
            "tool.text.cancel" => {
                if self.text_session.take().is_none() {
                    return Err(bad("no text session"));
                }
                self.publish(None);
                Ok(())
            }
            _ => Err(bad("unknown text command")),
        }
    }
    fn line_finish(&mut self) -> Result<()> {
        let Some((_, session)) = self.line_session.take() else {
            return Ok(());
        };
        if session.base_revision != self.history.current_state().revision
            || session.target_layer != self.active_id()
        {
            self.publish(None);
            return Err(Error::Busy);
        }
        let state = session.finish().map_err(bad)?;
        self.history.commit("Line/Curve", "tool.line.finish", state);
        self.revision += 1;
        self.publish(None);
        self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
        self.event(EventKind::HistoryChanged, 0, json!({}));
        Ok(())
    }
    fn line_command(&mut self, id: &str, params: &Value) -> Result<()> {
        match id {
            "tool.line.begin" => {
                if self.read_only.load(Ordering::Acquire) {
                    return Err(Error::ReadOnly);
                }
                if self.preview.is_some()
                    || self.stroke.is_some()
                    || self.move_session.is_some()
                    || self.gradient_session.is_some()
                    || self.line_session.is_some()
                    || self.shape_session.is_some()
                    || self.text_session.is_some()
                {
                    return Err(Error::Busy);
                }
                let (points, options) = line_commands::begin(params)?;
                let session =
                    LineSession::new(self.history.current_state().clone(), points, options)
                        .map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                let sid = self.next_session.fetch_add(1, Ordering::Relaxed);
                self.line_session = Some((sid, session));
                self.publish(Some(state));
                Ok(())
            }
            "tool.line.update" => {
                let update = line_commands::update(params)?;
                let (_, session) = self
                    .line_session
                    .as_mut()
                    .ok_or_else(|| bad("no line session"))?;
                let style = if params.as_object().is_some_and(|o| {
                    o.keys().any(|k| {
                        matches!(
                            k.as_str(),
                            "kind"
                                | "width"
                                | "antialias"
                                | "dash"
                                | "start_cap"
                                | "end_cap"
                                | "join"
                                | "miter_limit"
                                | "primary"
                                | "blend"
                        )
                    })
                }) {
                    Some(line_commands::style(params, session.options)?)
                } else {
                    None
                };
                session
                    .update_all(update.points, update.handle, style)
                    .map_err(bad)?;
                let state = session.preview().map_err(bad)?;
                self.publish(Some(state));
                Ok(())
            }
            "tool.line.finish" => self.line_finish(),
            "tool.line.cancel" => {
                self.line_session = None;
                self.publish(None);
                Ok(())
            }
            _ => Err(bad("unknown line command")),
        }
    }
    fn stroke_begin(&mut self, settings: BrushSettings, seed: u64) -> Result<u64> {
        self.check_stroke_begin()?;
        let session = StrokeSession::new(
            self.history.current_state().clone(),
            self.history.current_state().active_layer,
            settings,
            seed,
        )
        .map_err(bad)?;
        self.begin_stroke_session(session)
    }
    fn stroke_begin_hard(
        &mut self,
        settings: BrushSettings,
        hardness: f64,
        seed: u64,
    ) -> Result<u64> {
        self.check_stroke_begin()?;
        let mut session = StrokeSession::new(
            self.history.current_state().clone(),
            self.history.current_state().active_layer,
            settings,
            seed,
        )
        .map_err(bad)?;
        session.set_hardness(hardness).map_err(bad)?;
        self.begin_stroke_session(session)
    }
    fn stroke_begin_clone(
        &mut self,
        settings: BrushSettings,
        seed: u64,
        source: CloneSource,
        point: (i32, i32),
        offset: Option<(i32, i32)>,
    ) -> Result<u64> {
        self.check_stroke_begin()?;
        let session = StrokeSession::new_clone(
            self.history.current_state().clone(),
            self.history.current_state().active_layer,
            settings,
            seed,
            source,
            point,
            offset,
        )
        .map_err(bad)?;
        self.begin_stroke_session(session)
    }
    fn stroke_begin_heal(
        &mut self,
        settings: BrushSettings,
        seed: u64,
        source: CloneSource,
        point: (i32, i32),
        offset: Option<(i32, i32)>,
    ) -> Result<u64> {
        self.check_stroke_begin()?;
        let session = StrokeSession::new_heal(
            self.history.current_state().clone(),
            self.history.current_state().active_layer,
            settings,
            seed,
            source,
            point,
            offset,
        )
        .map_err(bad)?;
        self.begin_stroke_session(session)
    }
    fn stroke_begin_spot_heal(&mut self, settings: BrushSettings, seed: u64) -> Result<u64> {
        self.check_stroke_begin()?;
        let session = StrokeSession::new_spot_heal(
            self.history.current_state().clone(),
            self.history.current_state().active_layer,
            settings,
            seed,
        )
        .map_err(bad)?;
        self.begin_stroke_session(session)
    }
    fn stroke_begin_recolor(
        &mut self,
        settings: BrushSettings,
        seed: u64,
        options: RecolorOptions,
    ) -> Result<u64> {
        self.check_stroke_begin()?;
        let session = StrokeSession::new_recolor(
            self.history.current_state().clone(),
            self.history.current_state().active_layer,
            settings,
            seed,
            options,
        )
        .map_err(bad)?;
        self.begin_stroke_session(session)
    }
    fn check_stroke_begin(&self) -> Result<()> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        if self.preview.is_some()
            || self.stroke.is_some()
            || self.move_session.is_some()
            || self.gradient_session.is_some()
            || self.text_session.is_some()
        {
            return Err(Error::Busy);
        }
        Ok(())
    }
    fn begin_stroke_session(&mut self, session: StrokeSession) -> Result<u64> {
        let id = self.next_session.fetch_add(1, Ordering::Relaxed);
        self.stroke = Some(ActiveStroke {
            session,
            revision: self.revision,
            selection_revision: self.selection_revision,
        });
        Ok(id)
    }
    fn move_begin(&mut self, kind: MoveKind, options: MoveOptions) -> Result<u64> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        if self.preview.is_some()
            || self.stroke.is_some()
            || self.move_session.is_some()
            || self.gradient_session.is_some()
        {
            return Err(Error::Busy);
        }
        let session =
            MoveSession::new(self.history.current_state().clone(), kind, options).map_err(bad)?;
        let state = session.preview().map_err(bad)?;
        let id = self.next_session.fetch_add(1, Ordering::Relaxed);
        self.move_session = Some((id, session));
        self.publish(Some(state));
        Ok(id)
    }
    fn move_update(&mut self, options: MoveOptions) -> Result<()> {
        let (_, session) = self
            .move_session
            .as_mut()
            .ok_or_else(|| bad("no move session"))?;
        session.update(options).map_err(bad)?;
        let state = session.preview().map_err(bad)?;
        self.publish(Some(state));
        Ok(())
    }
    fn move_finish(&mut self) -> Result<()> {
        let (_, session) = self
            .move_session
            .take()
            .ok_or_else(|| bad("no move session"))?;
        if session.target_layer != self.active_id() {
            self.publish(None);
            return Err(Error::Busy);
        }
        let kind = session.kind;
        let state = session.finish().map_err(bad)?;
        let label = match kind {
            MoveKind::Pixels => "Move Selected Pixels",
            MoveKind::Selection => "Move Selection",
        };
        self.history.commit(label, label, state);
        if kind == MoveKind::Selection
            || self.history.current_state().selection
                != self
                    .history
                    .state(self.history.current_index() - 1)
                    .unwrap()
                    .selection
        {
            self.selection_revision += 1;
        }
        self.revision += 1;
        self.publish(None);
        self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
        self.event(EventKind::HistoryChanged, 0, json!({}));
        Ok(())
    }
    fn stroke_append(&mut self, samples: &[StrokeSample]) -> Result<()> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        let stroke = self.stroke.as_mut().ok_or_else(|| bad("no stroke"))?;
        if stroke.revision != self.revision || stroke.selection_revision != self.selection_revision
        {
            return Err(Error::Busy);
        }
        stroke.session.append(samples).map_err(bad)?;
        let preview = stroke.session.preview().clone();
        self.publish(Some(preview));
        Ok(())
    }
    fn stroke_commit(&mut self) -> Result<()> {
        if self.read_only.load(Ordering::Acquire) {
            return Err(Error::ReadOnly);
        }
        let stroke = self.stroke.take().ok_or_else(|| bad("no stroke"))?;
        if stroke.revision != self.revision
            || stroke.selection_revision != self.selection_revision
            || stroke.session.target_layer != self.active_id()
        {
            self.publish(None);
            return Err(Error::Busy);
        }
        let label = stroke.session.settings.tool.label();
        let state = stroke.session.finish().map_err(bad)?;
        self.history.commit(label, label, state);
        self.revision += 1;
        self.publish(None);
        self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
        self.event(EventKind::HistoryChanged, 0, json!({}));
        Ok(())
    }
    /// Handles a finished render. Every render ends with exactly one JobFinished.
    fn job(&mut self, identity: JobIdentity, ticket: JobTicket, result: Result<DocState>) {
        self.pending_jobs -= 1;
        let state = match result {
            Ok(state) => state,
            Err(e) => {
                if e == Error::Panic {
                    self.mark_read_only();
                }
                self.event(
                    EventKind::JobFinished,
                    ticket.id,
                    finished_json::<()>(&Err(e)),
                );
                return;
            }
        };
        if ticket.is_cancelled() {
            let cancelled = finished_json::<()>(&Err(Error::Cancelled));
            self.event(EventKind::JobFinished, ticket.id, cancelled);
            return;
        }
        let mismatch = identity.mismatches(&self.identity_now());
        if !mismatch.is_empty() {
            self.event(
                EventKind::JobFinished,
                ticket.id,
                json!({"status":"dropped","mismatch":mismatch}),
            );
            return;
        }
        self.publish(Some(state));
        self.event(
            EventKind::Progress,
            ticket.id,
            json!({"generation":identity.generation,"fraction":1.0}),
        );
        self.event(EventKind::JobFinished, ticket.id, json!({"status":"ok"}));
    }
    /// Runs one operation. Returns the result and whether a render now owns the
    /// ticket's JobFinished.
    fn run_op(&mut self, op: Op, ticket: &JobTicket) -> (Result<Option<u64>>, bool) {
        if ticket.is_cancelled() {
            return (Err(Error::Cancelled), false);
        }
        let render = matches!(op, Op::PreviewBegin(..) | Op::PreviewUpdate(_));
        let result = catch_unwind(AssertUnwindSafe(|| match op {
            Op::Command(id, p) => self.command(&id, p, ticket).map(|_| None),
            Op::PreviewBegin(id, p) => self.begin(&id, p, ticket.clone()).map(Some),
            Op::PreviewUpdate(p) => self.update(p, ticket.clone()).map(|_| None),
            Op::PreviewCommit => self.commit(ticket).map(|_| None),
            Op::PreviewCancel => {
                self.preview = None;
                self.publish(None);
                Ok(None)
            }
        }))
        .unwrap_or(Err(Error::Panic));
        if result == Err(Error::Panic) {
            self.mark_read_only();
        }
        let rendering = render && result.is_ok();
        (result, rendering)
    }
    fn run(mut self, rx: Receiver<Message>) {
        let mut stopping: Option<Sender<()>> = None;
        while let Ok(msg) = rx.recv() {
            match msg {
                Message::Stop(reply) => {
                    self.preview = None;
                    self.stroke = None;
                    self.gradient_session = None;
                    self.line_session = None;
                    self.shape_session = None;
                    self.text_session = None;
                    self.publish(None);
                    stopping = Some(reply);
                }
                Message::Run(_, ticket, reply) if stopping.is_some() => {
                    self.finish_unrun(ticket, reply);
                }
                Message::StrokeBegin(_, _, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::StrokeBeginHard(_, _, _, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::StrokeBeginHeal(_, _, _, _, _, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::StrokeBeginSpotHeal(_, _, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::StrokeAppend(_, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::StrokeCommit(r) | Message::StrokeCancel(r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::MoveBegin(_, _, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::File(_, r) if stopping.is_some() => {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::File(op, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| self.file_op(op)))
                        .unwrap_or(Err(Error::Panic));
                    let _ = r.send(result);
                }
                Message::MoveUpdate(_, r) | Message::MoveFinish(r) | Message::MoveCancel(r)
                    if stopping.is_some() =>
                {
                    let _ = r.send(Err(Error::NoSuchDoc));
                }
                Message::Run(op, ticket, reply) => {
                    let (result, rendering) = self.run_op(op, &ticket);
                    if !rendering && ticket.id != 0 {
                        self.event(EventKind::JobFinished, ticket.id, finished_json(&result));
                    }
                    if let Some(r) = reply {
                        let _ = r.send(result);
                    }
                }
                Message::Job(id, ticket, res) => {
                    if catch_unwind(AssertUnwindSafe(|| self.job(id, ticket, res))).is_err() {
                        self.mark_read_only();
                    }
                }
                Message::Inject(id, ticket, state) => {
                    self.pending_jobs += 1;
                    self.job(id, ticket, Ok(state));
                }
                Message::Probe(r) => {
                    let _ = r.send((
                        self.history.current_state().clone(),
                        self.history.current_index(),
                        self.history.len(),
                        self.revision,
                    ));
                }
                Message::View(r) => {
                    let _ = r.send(self.view(None));
                }
                Message::Identity(r) => {
                    let _ = r.send(self.identity_now());
                }
                Message::StrokeBegin(settings, seed, r) => {
                    let result =
                        catch_unwind(AssertUnwindSafe(|| self.stroke_begin(settings, seed)))
                            .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeBeginHard(settings, hardness, seed, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.stroke_begin_hard(settings, hardness, seed)
                    }))
                    .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeBeginClone(settings, seed, source, point, offset, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.stroke_begin_clone(settings, seed, source, point, offset)
                    }))
                    .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeBeginHeal(settings, seed, source, point, offset, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.stroke_begin_heal(settings, seed, source, point, offset)
                    }))
                    .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeBeginSpotHeal(settings, seed, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.stroke_begin_spot_heal(settings, seed)
                    }))
                    .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeBeginRecolor(settings, seed, options, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.stroke_begin_recolor(settings, seed, options)
                    }))
                    .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeCloneOffset(r) => {
                    let _ = r.send(
                        self.stroke
                            .as_ref()
                            .map(|s| s.session.clone_offset())
                            .ok_or_else(|| bad("no stroke")),
                    );
                }
                Message::StrokeAppend(samples, r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| self.stroke_append(&samples)))
                        .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeCommit(r) => {
                    let result = catch_unwind(AssertUnwindSafe(|| self.stroke_commit()))
                        .unwrap_or(Err(Error::Panic));
                    if result == Err(Error::Panic) {
                        self.mark_read_only();
                    }
                    let _ = r.send(result);
                }
                Message::StrokeCancel(r) => {
                    if self.stroke.take().is_some() {
                        self.publish(None);
                        let _ = r.send(Ok(()));
                    } else {
                        let _ = r.send(Err(bad("no stroke")));
                    }
                }
                Message::MoveBegin(kind, options, r) => {
                    let _ = r.send(self.move_begin(kind, options));
                }
                Message::MoveUpdate(options, r) => {
                    let _ = r.send(self.move_update(options));
                }
                Message::MoveFinish(r) => {
                    let _ = r.send(self.move_finish());
                }
                Message::MoveCancel(r) => {
                    let result = if self.move_session.take().is_some() {
                        self.publish(None);
                        Ok(())
                    } else {
                        Err(bad("no move session"))
                    };
                    let _ = r.send(result);
                }
            }
            if stopping.is_some() && self.pending_jobs == 0 {
                break;
            }
        }
        // Operations that raced with close still get their one JobFinished.
        while let Ok(msg) = rx.try_recv() {
            if let Message::Run(_, ticket, reply) = msg {
                self.finish_unrun(ticket, reply);
            }
        }
        if let Some(r) = stopping {
            let _ = r.send(());
        }
    }
    fn finish_unrun(&self, ticket: JobTicket, reply: Option<Sender<Result<Option<u64>>>>) {
        let result: Result<Option<u64>> = Err(Error::NoSuchDoc);
        if ticket.id != 0 {
            self.event(EventKind::JobFinished, ticket.id, finished_json(&result));
        }
        if let Some(r) = reply {
            let _ = r.send(result);
        }
    }
}
pub struct Engine {
    docs: Mutex<HashMap<u64, Arc<Document>>>,
    next_doc: AtomicU64,
    next_session: Arc<AtomicU64>,
    pool: Arc<ThreadPool>,
    events_tx: Sender<Event>,
    event_join: Mutex<Option<JoinHandle<()>>>,
    gpu: Arc<gpu_effects::GpuShared>,
}
impl Engine {
    /// An engine with options from the environment (`OP_FORCE_CPU=1` turns the GPU off).
    pub fn new(callback: Option<Arc<dyn Fn(Event) + Send + Sync>>) -> Result<Self> {
        Self::with_options(callback, EngineOptions::from_env())
    }
    /// The GPU is not created here; it starts on the first effect, on a worker.
    pub fn with_options(
        callback: Option<Arc<dyn Fn(Event) + Send + Sync>>,
        options: EngineOptions,
    ) -> Result<Self> {
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .map_err(internal)?,
        );
        let (tx, rx) = unbounded();
        let join = std::thread::spawn(move || {
            while let Ok(e) = rx.recv() {
                if let Some(cb) = &callback {
                    let _ = catch_unwind(AssertUnwindSafe(|| cb(e)));
                }
            }
        });
        Ok(Self {
            docs: Mutex::new(HashMap::new()),
            next_doc: AtomicU64::new(1),
            next_session: Arc::new(AtomicU64::new(1)),
            pool,
            events_tx: tx,
            event_join: Mutex::new(Some(join)),
            gpu: gpu_effects::GpuShared::new(options),
        })
    }
    pub fn import_pixels(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        icc: Option<&[u8]>,
    ) -> Result<u64> {
        self.import_pixels_at(width, height, rgba, icc, 96.0)
    }
    /// `import_pixels` at `dpi` pixels per inch (M50: the file's resolution).
    pub fn import_pixels_at(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        icc: Option<&[u8]>,
        dpi: f64,
    ) -> Result<u64> {
        if !dpi.is_finite() || !(1.0..=100_000.0).contains(&dpi) {
            return Err(bad("resolution out of range"));
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| bad("dimensions overflow"))?;
        if width == 0 || height == 0 || rgba.len() != len || len > 1024 * 1024 * 1024 {
            return Err(bad("invalid pixel dimensions or length"));
        }
        if let Some(icc) = icc {
            let p = op_color::Profile::from_icc(icc).map_err(bad)?;
            if p.color_space() != lcms2::ColorSpaceSignature::RgbData {
                return Err(bad("RGBA import requires an RGB profile"));
            }
        }
        let store =
            Arc::new(TileStore::new(std::env::temp_dir(), 64 * 1024 * 1024).map_err(internal)?);
        let mut state = DocState::new(store.clone(), width, height, dpi).map_err(internal)?;
        for ty in 0..height.div_ceil(TILE_SIZE as u32) {
            for tx in 0..width.div_ceil(TILE_SIZE as u32) {
                let mut tile = vec![0; TILE_BYTES];
                for y in 0..(height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize {
                    let n = (width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize * 4;
                    let src = ((ty as usize * TILE_SIZE + y) * width as usize
                        + tx as usize * TILE_SIZE)
                        * 4;
                    tile[y * TILE_SIZE * 4..y * TILE_SIZE * 4 + n]
                        .copy_from_slice(&rgba[src..src + n]);
                }
                state = state
                    .write_tile(
                        0,
                        TileCoord {
                            x: tx as i32,
                            y: ty as i32,
                        },
                        TileData::try_from(tile).map_err(internal)?,
                    )
                    .map_err(internal)?;
            }
        }
        if let Some(icc) = icc {
            state = state.set_profile(Some(Arc::from(icc)));
        }
        self.spawn_document(state, store)
    }
    /// Starts the actor for a new document whose step 0 is `state`.
    fn spawn_document(&self, state: DocState, store: Arc<TileStore>) -> Result<u64> {
        let history = History::new(state.clone());
        let id = self.next_doc.fetch_add(1, Ordering::Relaxed);
        let published = Arc::new(ArcSwap::from_pointee(Published {
            state,
            history: HistorySummary {
                labels: vec!["Open".into()],
                commands: vec!["document.open".into()],
                index: 0,
                dirty: false,
            },
            revision: 0,
            preview: None,
            preview_serial: 0,
        }));
        let read_only = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();
        let actor = Actor {
            id,
            history,
            revision: 0,
            selection_revision: 0,
            preview: None,
            stroke: None,
            move_session: None,
            gradient_session: None,
            text_session: None,
            line_session: None,
            shape_session: None,
            published: published.clone(),
            read_only: read_only.clone(),
            events: self.events_tx.clone(),
            tx: tx.clone(),
            pool: self.pool.clone(),
            next_session: self.next_session.clone(),
            pending_jobs: 0,
            preview_serial: AtomicU64::new(0),
            gpu: self.gpu.clone(),
        };
        let join = std::thread::spawn(move || actor.run(rx));
        self.docs.lock().insert(
            id,
            Arc::new(Document {
                tx,
                published,
                read_only,
                join: Mutex::new(Some(join)),
                store,
            }),
        );
        Ok(id)
    }
    pub fn open_png(&self, bytes: &[u8]) -> Result<u64> {
        let d = op_formats::png::decode_png(bytes).map_err(bad)?;
        // M50: a pHYs resolution in range becomes the document's.
        let dpi = op_formats::png::png_dpi(bytes)
            .filter(|d| (1.0..=100_000.0).contains(d))
            .unwrap_or(96.0);
        self.import_pixels_at(d.width, d.height, &d.pixels, d.icc.as_deref(), dpi)
    }
    pub fn import_gray_pixels(
        &self,
        width: u32,
        height: u32,
        gray: &[u8],
        alpha: Option<&[u8]>,
        icc: Option<&[u8]>,
    ) -> Result<u64> {
        let count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| bad("dimensions overflow"))?;
        if gray.len() != count {
            return Err(bad("gray pixel count mismatch"));
        }
        let src = match icc {
            Some(bytes) => op_color::Profile::from_icc(bytes).map_err(bad)?,
            None => op_color::Profile::builtin("gray-2.2").map_err(bad)?,
        };
        let dst = op_color::Profile::builtin("srgb").map_err(bad)?;
        let rgba = op_color::expand_gray(gray, alpha, &src, &dst).map_err(bad)?;
        self.import_pixels(width, height, &rgba, Some(&dst.bytes()))
    }
    pub fn import_cmyk_pixels(
        &self,
        width: u32,
        height: u32,
        cmyk: &[[u8; 4]],
        icc: &[u8],
    ) -> Result<(u64, String)> {
        let count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| bad("dimensions overflow"))?;
        if cmyk.len() != count {
            return Err(bad("CMYK pixel count mismatch"));
        }
        let src = op_color::Profile::from_icc(icc).map_err(bad)?;
        let (rgba, notice) = op_color::import_cmyk(cmyk, &src).map_err(bad)?;
        let dst = op_color::Profile::builtin("srgb").map_err(bad)?;
        let doc = self.import_pixels(width, height, &rgba, Some(&dst.bytes()))?;
        Ok((doc, notice))
    }
    fn doc(&self, id: u64) -> Result<Arc<Document>> {
        self.docs.lock().get(&id).cloned().ok_or(Error::NoSuchDoc)
    }
    pub fn snapshot(&self, id: u64) -> Result<Arc<Published>> {
        Ok(self.doc(id)?.published.load_full())
    }
    pub fn is_read_only(&self, id: u64) -> Result<bool> {
        Ok(self.doc(id)?.read_only.load(Ordering::Acquire))
    }
    fn call<T>(&self, id: u64, make: impl FnOnce(Sender<T>) -> Message) -> Result<T> {
        let d = self.doc(id)?;
        let (tx, rx) = crossbeam_channel::bounded(1);
        d.tx.send(make(tx)).map_err(internal)?;
        rx.recv().map_err(internal)
    }
    /// Checks what can be checked without the actor: the document, the command id
    /// and, for commands and preview.begin, the parameters.
    fn precheck(&self, op: &Op) -> Result<()> {
        match op {
            Op::Command(id, p) => {
                if id.starts_with("tool.gradient.")
                    || id.starts_with("tool.line.")
                    || id.starts_with("tool.shape.")
                    || id.starts_with("tool.text.")
                    || id == "tool.select"
                {
                    return Ok(());
                }
                spec(id)?.params.validate(p.clone()).map_err(bad)?;
            }
            Op::PreviewBegin(id, p) => {
                let s = spec(id)?;
                if !s.previewable {
                    return Err(bad("command cannot preview"));
                }
                s.params.validate(p.clone()).map_err(bad)?;
            }
            _ => {}
        }
        Ok(())
    }
    /// Queues `op` on the document's actor and returns at once. Operations on one
    /// document run in submit order. The ticket gets exactly one JobFinished event:
    /// from the actor, or from the render that preview.begin / preview.update starts.
    pub fn submit(&self, id: u64, op: Op, ticket: JobTicket) -> Result<()> {
        let d = self.doc(id)?;
        self.precheck(&op)?;
        d.tx.send(Message::Run(op, ticket, None))
            .map_err(|_| Error::NoSuchDoc)
    }
    /// Runs `op` and waits for the actor's answer (not for a preview render).
    pub fn run_op(&self, id: u64, op: Op, ticket: &JobTicket) -> Result<Option<u64>> {
        self.call(id, |r| Message::Run(op, ticket.clone(), Some(r)))?
    }
    pub fn dispatch_with(
        &self,
        id: u64,
        command: &str,
        params: Value,
        ticket: &JobTicket,
    ) -> Result<()> {
        self.run_op(id, Op::Command(command.to_owned(), params), ticket)
            .map(|_| ())
    }
    pub fn dispatch(&self, id: u64, command: &str, params: Value) -> Result<()> {
        self.dispatch_with(id, command, params, &JobTicket::new(0))
    }
    pub fn preview_begin(&self, id: u64, command: &str, params: Value) -> Result<u64> {
        self.run_op(
            id,
            Op::PreviewBegin(command.to_owned(), params),
            &JobTicket::new(0),
        )?
        .ok_or_else(|| internal("preview.begin returned no session"))
    }
    pub fn preview_update(&self, id: u64, params: Value) -> Result<()> {
        self.run_op(id, Op::PreviewUpdate(params), &JobTicket::new(0))
            .map(|_| ())
    }
    pub fn preview_commit(&self, id: u64) -> Result<()> {
        self.run_op(id, Op::PreviewCommit, &JobTicket::new(0))
            .map(|_| ())
    }
    pub fn preview_cancel(&self, id: u64) -> Result<()> {
        self.run_op(id, Op::PreviewCancel, &JobTicket::new(0))
            .map(|_| ())
    }
    /// Begins a brush preview on the active layer.
    pub fn stroke_begin(&self, id: u64, settings: BrushSettings, seed: u64) -> Result<u64> {
        self.call(id, |r| Message::StrokeBegin(settings, seed, r))?
    }
    /// Begins a Paintbrush, Eraser or Pencil stroke with a brush hardness (0..=1).
    pub fn stroke_begin_hard(
        &self,
        id: u64,
        settings: BrushSettings,
        hardness: f64,
        seed: u64,
    ) -> Result<u64> {
        self.call(id, |r| {
            Message::StrokeBeginHard(settings, hardness, seed, r)
        })?
    }
    /// Begin cloning. Pass the previous stroke's offset for aligned mode, or None to reset at the first sample.
    pub fn stroke_begin_clone(
        &self,
        id: u64,
        settings: BrushSettings,
        seed: u64,
        source: CloneSource,
        point: (i32, i32),
        offset: Option<(i32, i32)>,
    ) -> Result<u64> {
        self.call(id, |r| {
            Message::StrokeBeginClone(settings, seed, source, point, offset, r)
        })?
    }
    pub fn stroke_begin_heal(
        &self,
        id: u64,
        settings: BrushSettings,
        seed: u64,
        source: CloneSource,
        point: (i32, i32),
        offset: Option<(i32, i32)>,
    ) -> Result<u64> {
        self.call(id, |r| {
            Message::StrokeBeginHeal(settings, seed, source, point, offset, r)
        })?
    }
    pub fn stroke_begin_spot_heal(
        &self,
        id: u64,
        settings: BrushSettings,
        seed: u64,
    ) -> Result<u64> {
        self.call(id, |r| Message::StrokeBeginSpotHeal(settings, seed, r))?
    }
    pub fn stroke_begin_recolor(
        &self,
        id: u64,
        settings: BrushSettings,
        seed: u64,
        options: RecolorOptions,
    ) -> Result<u64> {
        self.call(id, |r| {
            Message::StrokeBeginRecolor(settings, seed, options, r)
        })?
    }
    pub fn stroke_clone_offset(&self, id: u64) -> Result<Option<(i32, i32)>> {
        self.call(id, Message::StrokeCloneOffset)?
    }
    /// Appends ordered input samples and publishes only tiles changed by the batch.
    pub fn stroke_append(&self, id: u64, samples: &[StrokeSample]) -> Result<()> {
        self.call(id, |r| Message::StrokeAppend(samples.to_vec(), r))?
    }
    pub fn stroke_commit(&self, id: u64) -> Result<()> {
        self.call(id, Message::StrokeCommit)?
    }
    pub fn stroke_cancel(&self, id: u64) -> Result<()> {
        self.call(id, Message::StrokeCancel)?
    }
    /// Starts a move preview. Releasing and dragging another handle calls `move_update`;
    /// only `move_finish` writes history.
    pub fn move_begin(&self, id: u64, kind: MoveKind, options: MoveOptions) -> Result<u64> {
        self.call(id, |r| Message::MoveBegin(kind, options, r))?
    }
    pub fn move_update(&self, id: u64, options: MoveOptions) -> Result<()> {
        self.call(id, |r| Message::MoveUpdate(options, r))?
    }
    pub fn move_finish(&self, id: u64) -> Result<()> {
        self.call(id, Message::MoveFinish)?
    }
    pub fn move_cancel(&self, id: u64) -> Result<()> {
        self.call(id, Message::MoveCancel)?
    }
    pub fn query(&self, id: u64, query: &str, params: Value) -> Result<Vec<u8>> {
        // Catalog queries need no document (id 0 is fine).
        if query == "tool.shape.catalog" {
            let entries: Vec<_> = op_tools::shape::catalog()
                .iter()
                .map(|s| json!({"id":s.id,"display_name":s.name,"group":s.group}))
                .collect();
            return serde_json::to_vec(&entries).map_err(internal);
        }
        if query == "effects.list" {
            return serde_json::to_vec(&effects_list()).map_err(internal);
        }
        let d = self.doc(id)?;
        let s = d.published.load_full();
        // `{"preview": true}`: the selection queries read the open preview's
        // state (the live Magic Wand selection, M31), else the document's.
        let shown = if params.get("preview").and_then(Value::as_bool) == Some(true) {
            s.preview.as_ref().unwrap_or(&s.state)
        } else {
            &s.state
        };
        match query {
            "doc.info" => {
                let mut info = s.info();
                info["read_only"] = json!(d.read_only.load(Ordering::Acquire));
                if let (Some(info), Value::Object(gpu)) = (info.as_object_mut(), self.gpu.info()) {
                    info.extend(gpu);
                }
                serde_json::to_vec(&info).map_err(internal)
            }
            "doc.pixels" => s.pixels(
                params
                    .get("preview")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            "doc.profile" => {
                serde_json::to_vec(&profile_query::profile_info(&s.state)).map_err(internal)
            }
            "doc.profile_icc" => Ok(profile_query::profile_icc(&s.state)),
            "doc.selection" => {
                serde_json::to_vec(&shell_commands::selection_info(shown)).map_err(internal)
            }
            "doc.sample_color" => {
                serde_json::to_vec(&color_sample::sample_color(&s.state, &params)?)
                    .map_err(internal)
            }
            "layer.thumbnail" => layer_commands::thumbnail(&s.state, &params),
            "doc.selection_path" => {
                serde_json::to_vec(&select_crop_commands::selection_path(shown)).map_err(internal)
            }
            "doc.selection_mask" => Ok(shown
                .rasterize_selection(op_core::selection::RasterMode::Pixelated)
                .bytes
                .to_vec()),
            "doc.selection_polygons" => {
                serde_json::to_vec(&selection_commands::selection_polygons(&s.state))
                    .map_err(internal)
            }
            "doc.resolution" => {
                serde_json::to_vec(&select_crop_commands::resolution(&s.state)).map_err(internal)
            }
            "commands.list" => {
                let ids: Vec<_> = specs()
                    .iter()
                    .filter(|c| !c.id.starts_with("test."))
                    .map(|c| (c.id, (c.enabled)(&s.state)))
                    .collect();
                serde_json::to_vec(&shell_commands::commands_list(&ids)).map_err(internal)
            }
            _ => Err(bad("unknown query")),
        }
    }
    pub fn close(&self, id: u64) -> Result<()> {
        let d = self.docs.lock().remove(&id).ok_or(Error::NoSuchDoc)?;
        let (tx, rx) = crossbeam_channel::bounded(1);
        d.tx.send(Message::Stop(tx)).map_err(internal)?;
        rx.recv().map_err(internal)?;
        if let Some(join) = d.join.lock().take() {
            join.join().map_err(|_| Error::Panic)?;
        }
        Ok(())
    }
    pub fn tile_count(&self, id: u64) -> Result<usize> {
        Ok(self.doc(id)?.store.stats().tile_count)
    }
    #[doc(hidden)]
    pub fn actor_probe(&self, id: u64) -> Result<(DocState, usize, usize, u64)> {
        self.call(id, Message::Probe)
    }
    /// Test hook: the actor's own state, built the same way as a published snapshot.
    #[doc(hidden)]
    pub fn actor_view(&self, id: u64) -> Result<Published> {
        self.call(id, Message::View)
    }
    /// Test hook: the identity a job started now would carry.
    #[doc(hidden)]
    pub fn current_identity(&self, id: u64) -> Result<JobIdentity> {
        self.call(id, Message::Identity)
    }
    /// Test hook: hands the actor a finished render result with any identity.
    #[doc(hidden)]
    pub fn inject_job_result(
        &self,
        id: u64,
        identity: JobIdentity,
        state: DocState,
        ticket: JobTicket,
    ) -> Result<()> {
        self.doc(id)?
            .tx
            .send(Message::Inject(identity, ticket, state))
            .map_err(internal)
    }
    pub fn tile_store(&self, id: u64) -> Result<Arc<TileStore>> {
        Ok(self.doc(id)?.store.clone())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let ids: Vec<_> = self.docs.lock().keys().copied().collect();
        for id in ids {
            let _ = self.close(id);
        }
        let (replacement, _) = unbounded();
        let tx = std::mem::replace(&mut self.events_tx, replacement);
        drop(tx);
        if let Some(join) = self.event_join.lock().take() {
            let _ = join.join();
        }
    }
}
