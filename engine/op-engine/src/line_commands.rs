use crate::{Result, bad};
use op_core::doc::ToolCompositeMode;
use op_tools::line::{Cap, Dash, Join, LineKind, LineOptions, Point};
use serde_json::Value;

fn point(v: &Value) -> Result<Point> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("line point must be an array"))?;
    if a.len() != 2 {
        return Err(bad("line point needs two numbers"));
    }
    Ok(Point::new(
        a[0].as_f64().ok_or_else(|| bad("invalid line x"))?,
        a[1].as_f64().ok_or_else(|| bad("invalid line y"))?,
    ))
}
fn points(v: &Value) -> Result<Vec<Point>> {
    v.as_array()
        .ok_or_else(|| bad("line points must be an array"))?
        .iter()
        .map(point)
        .collect()
}
fn color(v: &Value) -> Result<[u8; 4]> {
    let a = v
        .as_array()
        .ok_or_else(|| bad("primary must be four bytes"))?;
    if a.len() != 4 {
        return Err(bad("primary must be four bytes"));
    }
    let mut out = [0; 4];
    for (i, v) in a.iter().enumerate() {
        out[i] = u8::try_from(v.as_u64().ok_or_else(|| bad("invalid primary"))?)
            .map_err(|_| bad("invalid primary"))?;
    }
    Ok(out)
}
fn kind(v: &str) -> Result<LineKind> {
    match v {
        "straight" => Ok(LineKind::Straight),
        "bezier" => Ok(LineKind::Bezier),
        "spline" => Ok(LineKind::Spline),
        _ => Err(bad("unknown line kind")),
    }
}
fn dash(v: &str) -> Result<Dash> {
    match v {
        "solid" => Ok(Dash::Solid),
        "dash" => Ok(Dash::Dash),
        "dot" => Ok(Dash::Dot),
        "dash_dot" => Ok(Dash::DashDot),
        "dash_dot_dot" => Ok(Dash::DashDotDot),
        _ => Err(bad("unknown line dash")),
    }
}
fn cap(v: &str) -> Result<Cap> {
    match v {
        "flat" => Ok(Cap::Flat),
        "square" => Ok(Cap::Square),
        "round" => Ok(Cap::Round),
        "arrow" => Ok(Cap::Arrow),
        _ => Err(bad("unknown line cap")),
    }
}
fn join(v: &str) -> Result<Join> {
    match v {
        "miter" => Ok(Join::Miter),
        "round" => Ok(Join::Round),
        "bevel" => Ok(Join::Bevel),
        _ => Err(bad("unknown line join")),
    }
}
pub fn style(v: &Value, mut o: LineOptions) -> Result<LineOptions> {
    if let Some(x) = v.get("kind") {
        o.kind = kind(x.as_str().ok_or_else(|| bad("kind must be a string"))?)?;
    }
    if let Some(x) = v.get("width") {
        o.width = x.as_f64().ok_or_else(|| bad("invalid line width"))?;
    }
    if let Some(x) = v.get("antialias") {
        o.antialias = x.as_bool().ok_or_else(|| bad("invalid antialias"))?;
    }
    if let Some(x) = v.get("dash") {
        o.dash = dash(x.as_str().ok_or_else(|| bad("dash must be a string"))?)?;
    }
    if let Some(x) = v.get("start_cap") {
        o.start_cap = cap(x.as_str().ok_or_else(|| bad("cap must be a string"))?)?;
    }
    if let Some(x) = v.get("end_cap") {
        o.end_cap = cap(x.as_str().ok_or_else(|| bad("cap must be a string"))?)?;
    }
    if let Some(x) = v.get("join") {
        o.join = join(x.as_str().ok_or_else(|| bad("join must be a string"))?)?;
    }
    if let Some(x) = v.get("miter_limit") {
        o.miter_limit = x.as_f64().ok_or_else(|| bad("invalid miter limit"))?;
    }
    if let Some(x) = v.get("primary") {
        o.primary = color(x)?;
    }
    if let Some(x) = v.get("blend") {
        o.blend = match x.as_str() {
            Some("normal") => ToolCompositeMode::Normal,
            Some("overwrite") => ToolCompositeMode::Overwrite,
            _ => return Err(bad("unknown line blend")),
        };
    }
    Ok(o)
}
pub fn begin(v: &Value) -> Result<(Vec<Point>, LineOptions)> {
    let pts = points(v.get("points").ok_or_else(|| bad("missing line points"))?)?;
    let primary = color(v.get("primary").ok_or_else(|| bad("missing primary"))?)?;
    let default = LineOptions {
        kind: LineKind::Straight,
        width: 2.0,
        antialias: true,
        dash: Dash::Solid,
        start_cap: Cap::Flat,
        end_cap: Cap::Flat,
        join: Join::Miter,
        miter_limit: 4.0,
        primary,
        blend: ToolCompositeMode::Normal,
    };
    Ok((pts, style(v, default)?))
}
pub struct LineUpdate {
    pub points: Option<Vec<Point>>,
    pub handle: Option<(usize, Point)>,
}
pub fn update(v: &Value) -> Result<LineUpdate> {
    let all = v.get("points").map(points).transpose()?;
    let handle = if let Some(x) = v.get("handle") {
        let index = usize::try_from(x.as_u64().ok_or_else(|| bad("invalid line handle"))?)
            .map_err(|_| bad("invalid line handle"))?;
        let p = point(v.get("point").ok_or_else(|| bad("missing line point"))?)?;
        Some((index, p))
    } else {
        None
    };
    if all.is_some() && handle.is_some() {
        return Err(bad("choose points or handle"));
    }
    Ok(LineUpdate {
        points: all,
        handle,
    })
}
