use crate::{Result, bad};
use op_core::doc::ToolCompositeMode;
use op_tools::gradient::{GradientMode, GradientOptions, GradientType, Handle};
use serde_json::Value;

type Point = (f64, f64);

fn point(v: &Value) -> Result<Point> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("gradient point must be an array"))?;
    if a.len() != 2 {
        return Err(bad("gradient point needs two numbers"));
    }
    Ok((
        a[0].as_f64().ok_or_else(|| bad("invalid gradient x"))?,
        a[1].as_f64().ok_or_else(|| bad("invalid gradient y"))?,
    ))
}
fn color(v: &Value) -> Result<[u8; 4]> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("gradient color must be an array"))?;
    if a.len() != 4 {
        return Err(bad("gradient color needs four bytes"));
    }
    let mut out = [0; 4];
    for (i, c) in a.iter().enumerate() {
        out[i] = u8::try_from(c.as_u64().ok_or_else(|| bad("invalid gradient color"))?)
            .map_err(|_| bad("invalid gradient color"))?;
    }
    Ok(out)
}
pub fn begin(v: &Value) -> Result<(GradientOptions, Point, Point)> {
    let shape = match v.get("shape").and_then(Value::as_str).unwrap_or("linear") {
        "linear" => GradientType::Linear,
        "linear_reflected" => GradientType::LinearReflected,
        "diamond" => GradientType::Diamond,
        "radial" => GradientType::Radial,
        "conical" => GradientType::Conical,
        _ => return Err(bad("unknown gradient shape")),
    };
    let mode = match v.get("mode").and_then(Value::as_str).unwrap_or("color") {
        "color" => GradientMode::Color,
        "transparency" => GradientMode::Transparency,
        _ => return Err(bad("unknown gradient mode")),
    };
    let blend = match v.get("blend").and_then(Value::as_str).unwrap_or("normal") {
        "normal" => ToolCompositeMode::Normal,
        "overwrite" => ToolCompositeMode::Overwrite,
        _ => return Err(bad("unknown gradient blend")),
    };
    let antialias = v
        .get("antialias")
        .map(Value::as_bool)
        .unwrap_or(Some(true))
        .ok_or_else(|| bad("antialias must be a boolean"))?;
    let primary = color(
        v.get("primary")
            .ok_or_else(|| bad("missing primary color"))?,
    )?;
    let secondary = color(
        v.get("secondary")
            .ok_or_else(|| bad("missing secondary color"))?,
    )?;
    let start = point(v.get("start").ok_or_else(|| bad("missing start"))?)?;
    let end = point(v.get("end").ok_or_else(|| bad("missing end"))?)?;
    Ok((
        GradientOptions {
            shape,
            mode,
            primary,
            secondary,
            antialias,
            blend,
        },
        start,
        end,
    ))
}
pub fn update(v: &Value) -> Result<(Handle, Point)> {
    let handle = match v.get("handle").and_then(Value::as_str) {
        Some("start") => Handle::Start,
        Some("end") => Handle::End,
        _ => return Err(bad("unknown gradient handle")),
    };
    Ok((
        handle,
        point(v.get("point").ok_or_else(|| bad("missing point"))?)?,
    ))
}
