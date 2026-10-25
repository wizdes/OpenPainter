use crate::{Result, bad};
use op_core::doc::ToolCompositeMode;
use op_tools::line::{Dash, Join, Point};
use op_tools::shape::{DrawMode, ShapeOptions};
use serde_json::Value;
fn point(v: &Value) -> Result<Point> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("shape point must be an array"))?;
    if a.len() != 2 {
        return Err(bad("shape point needs two numbers"));
    }
    Ok(Point::new(
        a[0].as_f64().ok_or_else(|| bad("invalid shape x"))?,
        a[1].as_f64().ok_or_else(|| bad("invalid shape y"))?,
    ))
}
fn corners(v: &Value) -> Result<[Point; 2]> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("shape box must be an array"))?;
    if a.len() != 2 {
        return Err(bad("shape box needs two corners"));
    }
    Ok([point(&a[0])?, point(&a[1])?])
}
fn color(v: &Value) -> Result<[u8; 4]> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("shape color must be four bytes"))?;
    if a.len() != 4 {
        return Err(bad("shape color must be four bytes"));
    }
    let mut out = [0; 4];
    for (i, x) in a.iter().enumerate() {
        out[i] = u8::try_from(x.as_u64().ok_or_else(|| bad("invalid shape color"))?)
            .map_err(|_| bad("invalid shape color"))?;
    }
    Ok(out)
}
pub fn begin(v: &Value) -> Result<(String, [Point; 2], f64, ShapeOptions)> {
    let shape = v
        .get("shape")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("missing shape"))?
        .to_owned();
    let b = corners(v.get("box").ok_or_else(|| bad("missing shape box"))?)?;
    let primary = color(v.get("primary").ok_or_else(|| bad("missing primary"))?)?;
    let secondary = color(v.get("secondary").ok_or_else(|| bad("missing secondary"))?)?;
    let o = ShapeOptions {
        mode: DrawMode::Outline,
        width: 2.,
        antialias: true,
        dash: Dash::Solid,
        join: Join::Miter,
        miter_limit: 4.,
        primary,
        secondary,
        blend: ToolCompositeMode::Normal,
    };
    let r = rotation(v)?.unwrap_or(0.);
    Ok((shape, b, r, style(v, o)?))
}
pub fn box_update(v: &Value) -> Result<Option<[Point; 2]>> {
    v.get("box").map(corners).transpose()
}
pub fn rotation(v: &Value) -> Result<Option<f64>> {
    v.get("rotation")
        .map(|x| x.as_f64().ok_or_else(|| bad("invalid shape rotation")))
        .transpose()
}
pub fn shape(v: &Value) -> Result<Option<&str>> {
    v.get("shape")
        .map(|x| x.as_str().ok_or_else(|| bad("shape must be a string")))
        .transpose()
}
pub fn has_style(v: &Value) -> bool {
    v.as_object().is_some_and(|o| {
        o.keys().any(|k| {
            matches!(
                k.as_str(),
                "mode"
                    | "width"
                    | "antialias"
                    | "dash"
                    | "join"
                    | "miter_limit"
                    | "primary"
                    | "secondary"
                    | "blend"
            )
        })
    })
}
pub fn style(v: &Value, mut o: ShapeOptions) -> Result<ShapeOptions> {
    if let Some(x) = v.get("mode") {
        o.mode = match x.as_str() {
            Some("outline") => DrawMode::Outline,
            Some("fill") => DrawMode::Fill,
            Some("both") => DrawMode::FillAndOutline,
            _ => return Err(bad("unknown shape mode")),
        };
    }
    if let Some(x) = v.get("width") {
        o.width = x.as_f64().ok_or_else(|| bad("invalid shape width"))?;
    }
    if let Some(x) = v.get("antialias") {
        o.antialias = x.as_bool().ok_or_else(|| bad("invalid shape antialias"))?;
    }
    if let Some(x) = v.get("dash") {
        o.dash = match x.as_str() {
            Some("solid") => Dash::Solid,
            Some("dash") => Dash::Dash,
            Some("dot") => Dash::Dot,
            Some("dash_dot") => Dash::DashDot,
            Some("dash_dot_dot") => Dash::DashDotDot,
            _ => return Err(bad("unknown shape dash")),
        };
    }
    if let Some(x) = v.get("join") {
        o.join = match x.as_str() {
            Some("miter") => Join::Miter,
            Some("round") => Join::Round,
            Some("bevel") => Join::Bevel,
            _ => return Err(bad("unknown shape join")),
        };
    }
    if let Some(x) = v.get("miter_limit") {
        o.miter_limit = x.as_f64().ok_or_else(|| bad("invalid shape miter limit"))?;
    }
    if let Some(x) = v.get("primary") {
        o.primary = color(x)?;
    }
    if let Some(x) = v.get("secondary") {
        o.secondary = color(x)?;
    }
    if let Some(x) = v.get("blend") {
        o.blend = match x.as_str() {
            Some("normal") => ToolCompositeMode::Normal,
            Some("overwrite") => ToolCompositeMode::Overwrite,
            _ => return Err(bad("unknown shape blend")),
        };
    }
    Ok(o)
}
