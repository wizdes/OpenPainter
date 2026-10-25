//! Commands and queries the Mac main window needs (M15): Select All, Deselect,
//! choosing the active layer, one-call brush strokes, a command for every
//! registered effect, and the effect and command catalogs.
use super::{
    CommandSpec, Outcome, Param, ParamKind, ParamSchema, Result, RunCtx, always, bad, internal,
    run_effect,
};
use op_core::doc::{DocState, PolygonSet, SelectionSource, SelectionState, ToolCompositeMode};
use op_core::selection::RasterMode;
use op_ops::effect;
use op_tools::{BrushSettings, PressureMode, StrokeSample, StrokeSession, Tool};
use serde_json::{Value, json};
use std::sync::Arc;

fn field(key: &'static str, kind: ParamKind, default: Option<Value>) -> Param {
    Param {
        key,
        kind,
        min: None,
        max: None,
        default,
    }
}
fn has_selection(s: &DocState) -> bool {
    matches!(s.selection, SelectionState::Active(_))
}
fn select_all(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    let all = PolygonSet::rectangle(0.0, 0.0, s.width as f64, s.height as f64);
    Ok(Outcome::State(s.set_selection(SelectionState::Active(
        SelectionSource::Geometry(Arc::new(all)),
    ))))
}
fn deselect(_: &RunCtx, s: &DocState, _: &Value) -> Result<Outcome> {
    Ok(Outcome::State(s.set_selection(SelectionState::None)))
}
fn set_active(_: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let index = usize::try_from(p["index"].as_i64().unwrap()).map_err(bad)?;
    if index >= s.layers.len() {
        return Err(bad("layer index out of range"));
    }
    // Not an edit: no history step, and the dirty flag stays.
    Ok(Outcome::Replace(s.set_active_layer(index).map_err(bad)?))
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
/// `points` is "x y pressure" triples separated by spaces, in document pixels.
fn parse_points(text: &str) -> Result<Vec<StrokeSample>> {
    let numbers: Vec<f64> = text
        .split_ascii_whitespace()
        .map(|n| n.parse::<f64>().map_err(bad))
        .collect::<Result<_>>()?;
    if numbers.is_empty() || !numbers.len().is_multiple_of(3) {
        return Err(bad("points must be x y pressure triples"));
    }
    if numbers.iter().any(|n| !n.is_finite()) {
        return Err(bad("points must be finite"));
    }
    Ok(numbers
        .chunks_exact(3)
        .enumerate()
        .map(|(i, c)| StrokeSample {
            x: c[0],
            y: c[1],
            pressure: c[2].clamp(0.0, 1.0),
            timestamp_ms: i as u64,
            tilt_x: 0.0,
            tilt_y: 0.0,
        })
        .collect())
}
fn stroke_tool(name: &str) -> Tool {
    match name {
        "eraser" => Tool::Eraser,
        "pencil" => Tool::Pencil,
        _ => Tool::Paintbrush,
    }
}
/// A whole brush stroke in one command: the samples are drawn on the active
/// layer and committed as one history step named after the tool.
fn stroke(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    ctx.check()?;
    let mut color = parse_color(p["color"].as_str().unwrap())?;
    let opacity = p["opacity"].as_f64().unwrap();
    color[3] = (f64::from(color[3]) * opacity / 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    let settings = BrushSettings {
        tool: stroke_tool(p["tool"].as_str().unwrap()),
        width: p["width"].as_f64().unwrap(),
        color,
        antialias: p["antialias"].as_bool().unwrap(),
        spacing: 0.1,
        pressure: PressureMode::None,
        blend: if p["blend"].as_str().unwrap() == "overwrite" {
            ToolCompositeMode::Overwrite
        } else {
            ToolCompositeMode::Normal
        },
    };
    let samples = parse_points(p["points"].as_str().unwrap())?;
    let mut session = StrokeSession::new(s.clone(), s.active_layer, settings, 0).map_err(bad)?;
    for chunk in samples.chunks(64) {
        ctx.check()?;
        session.append(chunk).map_err(bad)?;
    }
    ctx.check()?;
    Ok(Outcome::State(session.finish().map_err(bad)?))
}

/// Runs registered effect number `I` with its parameters.
fn run_registered<const I: usize>(ctx: &RunCtx, s: &DocState, p: &Value) -> Result<Outcome> {
    let id = effect::registered()
        .get(I)
        .map(|e| e.id)
        .ok_or_else(|| internal("effect registry changed"))?;
    run_effect(ctx, s, id, p)
}
type RunFn = fn(&RunCtx, &DocState, &Value) -> Result<Outcome>;
/// One runner per registry slot. Registries longer than this table leave the
/// extra effects without a command (the test `every_effect_has_a_command` fails).
const RUNNERS: [RunFn; 128] = [
    run_registered::<0>,
    run_registered::<1>,
    run_registered::<2>,
    run_registered::<3>,
    run_registered::<4>,
    run_registered::<5>,
    run_registered::<6>,
    run_registered::<7>,
    run_registered::<8>,
    run_registered::<9>,
    run_registered::<10>,
    run_registered::<11>,
    run_registered::<12>,
    run_registered::<13>,
    run_registered::<14>,
    run_registered::<15>,
    run_registered::<16>,
    run_registered::<17>,
    run_registered::<18>,
    run_registered::<19>,
    run_registered::<20>,
    run_registered::<21>,
    run_registered::<22>,
    run_registered::<23>,
    run_registered::<24>,
    run_registered::<25>,
    run_registered::<26>,
    run_registered::<27>,
    run_registered::<28>,
    run_registered::<29>,
    run_registered::<30>,
    run_registered::<31>,
    run_registered::<32>,
    run_registered::<33>,
    run_registered::<34>,
    run_registered::<35>,
    run_registered::<36>,
    run_registered::<37>,
    run_registered::<38>,
    run_registered::<39>,
    run_registered::<40>,
    run_registered::<41>,
    run_registered::<42>,
    run_registered::<43>,
    run_registered::<44>,
    run_registered::<45>,
    run_registered::<46>,
    run_registered::<47>,
    run_registered::<48>,
    run_registered::<49>,
    run_registered::<50>,
    run_registered::<51>,
    run_registered::<52>,
    run_registered::<53>,
    run_registered::<54>,
    run_registered::<55>,
    run_registered::<56>,
    run_registered::<57>,
    run_registered::<58>,
    run_registered::<59>,
    run_registered::<60>,
    run_registered::<61>,
    run_registered::<62>,
    run_registered::<63>,
    run_registered::<64>,
    run_registered::<65>,
    run_registered::<66>,
    run_registered::<67>,
    run_registered::<68>,
    run_registered::<69>,
    run_registered::<70>,
    run_registered::<71>,
    run_registered::<72>,
    run_registered::<73>,
    run_registered::<74>,
    run_registered::<75>,
    run_registered::<76>,
    run_registered::<77>,
    run_registered::<78>,
    run_registered::<79>,
    run_registered::<80>,
    run_registered::<81>,
    run_registered::<82>,
    run_registered::<83>,
    run_registered::<84>,
    run_registered::<85>,
    run_registered::<86>,
    run_registered::<87>,
    run_registered::<88>,
    run_registered::<89>,
    run_registered::<90>,
    run_registered::<91>,
    run_registered::<92>,
    run_registered::<93>,
    run_registered::<94>,
    run_registered::<95>,
    run_registered::<96>,
    run_registered::<97>,
    run_registered::<98>,
    run_registered::<99>,
    run_registered::<100>,
    run_registered::<101>,
    run_registered::<102>,
    run_registered::<103>,
    run_registered::<104>,
    run_registered::<105>,
    run_registered::<106>,
    run_registered::<107>,
    run_registered::<108>,
    run_registered::<109>,
    run_registered::<110>,
    run_registered::<111>,
    run_registered::<112>,
    run_registered::<113>,
    run_registered::<114>,
    run_registered::<115>,
    run_registered::<116>,
    run_registered::<117>,
    run_registered::<118>,
    run_registered::<119>,
    run_registered::<120>,
    run_registered::<121>,
    run_registered::<122>,
    run_registered::<123>,
    run_registered::<124>,
    run_registered::<125>,
    run_registered::<126>,
    run_registered::<127>,
];
/// The runner for the effect at `index` in `effect::registered()`.
pub(super) fn effect_runner(index: usize) -> Option<RunFn> {
    RUNNERS.get(index).copied()
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "selection.select_all",
            params: ParamSchema::default(),
            enabled: always,
            run: select_all,
            previewable: false,
        },
        CommandSpec {
            id: "selection.deselect",
            params: ParamSchema::default(),
            enabled: has_selection,
            run: deselect,
            previewable: false,
        },
        CommandSpec {
            id: "layer.set_active",
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
            run: set_active,
            previewable: false,
        },
        CommandSpec {
            id: "tool.stroke",
            params: ParamSchema {
                fields: vec![
                    field(
                        "tool",
                        ParamKind::Choice(&["paintbrush", "eraser", "pencil"]),
                        Some(json!("paintbrush")),
                    ),
                    Param {
                        key: "width",
                        kind: ParamKind::Float,
                        min: Some(1.0),
                        max: Some(2000.0),
                        default: Some(json!(2.0)),
                    },
                    field("color", ParamKind::String, Some(json!("000000ff"))),
                    Param {
                        key: "opacity",
                        kind: ParamKind::Float,
                        min: Some(0.0),
                        max: Some(255.0),
                        default: Some(json!(255.0)),
                    },
                    field("antialias", ParamKind::Bool, Some(json!(true))),
                    field(
                        "blend",
                        ParamKind::Choice(&["normal", "overwrite"]),
                        Some(json!("normal")),
                    ),
                    field("points", ParamKind::String, None),
                ],
            },
            enabled: always,
            run: stroke,
            previewable: false,
        },
    ]
}

/// The History label for commands of this file and for registered effects.
pub(super) fn history_label(id: &str, params: &Value) -> Option<String> {
    match id {
        "selection.select_all" => Some("Select All".into()),
        "selection.deselect" => Some("Deselect".into()),
        "tool.stroke" => Some(
            stroke_tool(params["tool"].as_str().unwrap_or("paintbrush"))
                .label()
                .into(),
        ),
        _ => effect::find(id).map(|e| effect_label(&e)),
    }
}

/// A menu label for an effect, as paint.net names it.
pub fn effect_label(e: &effect::EffectSpec) -> String {
    let known = match e.id {
        "effect.adjust.auto_level" => "Auto-Level",
        "effect.adjust.black_and_white" => "Black and White",
        "effect.adjust.brightness_contrast" => "Brightness / Contrast",
        "effect.adjust.curves" => "Curves",
        "effect.adjust.exposure" => "Exposure",
        "effect.adjust.highlights_shadows" => "Highlights / Shadows",
        "effect.adjust.levels" => "Levels",
        "effect.blur.bokeh" => "Bokeh Blur",
        "effect.blur.fragment" => "Fragment Blur",
        "effect.blur.sketch" => "Sketch Blur",
        "effect.blur.square" => "Square Blur",
        "effect.blur.surface" => "Surface Blur",
        "effect.blur.gaussian" => "Gaussian Blur",
        "effect.blur.median" => "Median",
        "effect.noise.add" => "Add Noise",
        "effect.noise.reduce" => "Reduce Noise",
        "effect.blur.motion" => "Motion Blur",
        "effect.blur.radial" => "Radial Blur",
        "effect.blur.zoom" => "Zoom Blur",
        "effect.adjust.hue_saturation" => "Hue / Saturation",
        "effect.adjust.invert_alpha" => "Invert Alpha",
        "effect.adjust.invert_colors" => "Invert Colors",
        "effect.adjust.posterize" => "Posterize",
        "effect.adjust.sepia" => "Sepia",
        "effect.adjust.temperature_tint" => "Temperature and Tint",
        "effect.fill.content_aware" => "Content-Aware Fill",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    // Fallback: "effect.blur.gaussian_blur" -> "Gaussian Blur".
    e.id.rsplit('.')
        .next()
        .unwrap_or(e.id)
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn schema_json(schema: &ParamSchema) -> Value {
    Value::Array(
        schema
            .fields
            .iter()
            .map(|f| {
                let kind = match &f.kind {
                    ParamKind::Int => json!("int"),
                    ParamKind::Float => json!("float"),
                    ParamKind::Bool => json!("bool"),
                    ParamKind::String => json!("string"),
                    ParamKind::Choice(c) => json!({"choice": c}),
                    ParamKind::Curve => json!("curve"),
                    ParamKind::RgbBytes => json!("rgb_bytes"),
                    ParamKind::RgbGamma => json!("rgb_gamma"),
                };
                json!({"key":f.key,"kind":kind,"min":f.min,"max":f.max,"default":f.default})
            })
            .collect(),
    )
}

/// `effects.list`: every registered effect in registry order.
pub fn effects_list() -> Value {
    Value::Array(
        effect::registered()
            .iter()
            .map(|e| {
                json!({"id":e.id,"label":effect_label(e),"category":e.category,
                       "schema":schema_json(&e.schema)})
            })
            .collect(),
    )
}

/// `commands.list`: every command id, and whether it is enabled for `state`.
pub fn commands_list(ids: &[(&'static str, bool)]) -> Value {
    Value::Array(
        ids.iter()
            .map(|(id, enabled)| json!({"id":id,"enabled":enabled}))
            .collect(),
    )
}

/// `doc.selection`: `{"active":bool,"bounds":[x,y,w,h]}` in document pixels.
/// An active empty selection has zero bounds.
pub fn selection_info(s: &DocState) -> Value {
    match &s.selection {
        SelectionState::None => json!({"active":false,"bounds":[0,0,0,0]}),
        SelectionState::Active(_) => {
            let mask = s.rasterize_selection(RasterMode::Pixelated);
            let (w, h) = (mask.width as usize, mask.height as usize);
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
            for y in 0..h {
                let row = &mask.bytes[y * w..(y + 1) * w];
                if let (Some(first), Some(last)) = (
                    row.iter().position(|&c| c != 0),
                    row.iter().rposition(|&c| c != 0),
                ) {
                    x0 = x0.min(first);
                    x1 = x1.max(last + 1);
                    y0 = y0.min(y);
                    y1 = y + 1;
                }
            }
            if x0 == usize::MAX {
                json!({"active":true,"bounds":[0,0,0,0]})
            } else {
                json!({"active":true,"bounds":[x0,y0,x1 - x0,y1 - y0]})
            }
        }
    }
}
