//! Parameters for `tool.text.begin` and `tool.text.update`.
//!
//! ```text
//! origin  [x, y]            whole document pixels; run positions are relative to it
//! runs    [{kind, x, y, width, height, data}]
//!         kind "coverage" (1 byte per pixel) or "color" (premultiplied RGBA),
//!         data is standard base64 (RFC 4648, with padding)
//! primary [r, g, b, a]      straight RGBA
//! blend   "normal" | "overwrite"
//! text    the editable text; caret is a UTF-8 byte offset into it
//! ```
//! `begin` needs origin, runs and primary. `update` takes any subset.
use crate::{Result, bad};
use op_core::doc::ToolCompositeMode;
use op_tools::text::{RunKind, TextModel, TextRun, TextStyle, TextUpdate};
use serde_json::Value;

fn int(v: &Value, what: &'static str) -> Result<i32> {
    let n = v.as_i64().ok_or_else(|| bad(what))?;
    i32::try_from(n).map_err(|_| bad(what))
}
fn origin(v: &Value) -> Result<(i32, i32)> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| bad("text origin needs two whole numbers"))?;
    Ok((
        int(&a[0], "invalid text origin x")?,
        int(&a[1], "invalid text origin y")?,
    ))
}
fn color(v: &Value) -> Result<[u8; 4]> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 4)
        .ok_or_else(|| bad("primary must be four bytes"))?;
    let mut out = [0; 4];
    for (i, c) in a.iter().enumerate() {
        out[i] = u8::try_from(c.as_u64().ok_or_else(|| bad("invalid primary"))?)
            .map_err(|_| bad("invalid primary"))?;
    }
    Ok(out)
}
fn blend(v: &Value) -> Result<ToolCompositeMode> {
    match v.as_str() {
        Some("normal") => Ok(ToolCompositeMode::Normal),
        Some("overwrite") => Ok(ToolCompositeMode::Overwrite),
        _ => Err(bad("unknown text blend")),
    }
}
fn run(v: &Value) -> Result<TextRun> {
    let kind = match v.get("kind").and_then(Value::as_str) {
        Some("coverage") => RunKind::Coverage,
        Some("color") => RunKind::Color,
        _ => return Err(bad("text run kind must be coverage or color")),
    };
    let size = |key: &str| -> Result<u32> {
        let n = v
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("text run size must be a whole number"))?;
        u32::try_from(n).map_err(|_| bad("text run size is too large"))
    };
    let data = v
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("text run needs base64 data"))?;
    Ok(TextRun {
        kind,
        x: int(v.get("x").unwrap_or(&Value::Null), "invalid text run x")?,
        y: int(v.get("y").unwrap_or(&Value::Null), "invalid text run y")?,
        width: size("width")?,
        height: size("height")?,
        pixels: base64_decode(data)?.into(),
    })
}
fn runs(v: &Value) -> Result<Vec<TextRun>> {
    v.as_array()
        .ok_or_else(|| bad("text runs must be an array"))?
        .iter()
        .map(run)
        .collect()
}
fn model(v: &Value) -> Result<Option<TextModel>> {
    let Some(text) = v.get("text") else {
        return Ok(None);
    };
    let text = text
        .as_str()
        .ok_or_else(|| bad("text must be a string"))?
        .to_owned();
    let caret = match v.get("caret") {
        None => text.len(),
        Some(c) => usize::try_from(
            c.as_u64()
                .ok_or_else(|| bad("caret must be a whole number"))?,
        )
        .map_err(|_| bad("caret is too large"))?,
    };
    Ok(Some(TextModel::new(text, caret).map_err(bad)?))
}

pub struct Begin {
    pub origin: (i32, i32),
    pub style: TextStyle,
    pub runs: Vec<TextRun>,
    pub model: TextModel,
}

pub fn begin(v: &Value) -> Result<Begin> {
    Ok(Begin {
        origin: origin(v.get("origin").ok_or_else(|| bad("missing text origin"))?)?,
        style: TextStyle {
            primary: color(
                v.get("primary")
                    .ok_or_else(|| bad("missing primary color"))?,
            )?,
            blend: blend(v.get("blend").unwrap_or(&Value::from("normal")))?,
        },
        runs: runs(v.get("runs").ok_or_else(|| bad("missing text runs"))?)?,
        model: model(v)?.unwrap_or_default(),
    })
}

pub fn update(v: &Value, current: TextStyle) -> Result<TextUpdate> {
    if !v.is_object() {
        return Err(bad("parameters must be an object"));
    }
    let style = if v.get("primary").is_some() || v.get("blend").is_some() {
        Some(TextStyle {
            primary: v.get("primary").map_or(Ok(current.primary), color)?,
            blend: v.get("blend").map_or(Ok(current.blend), blend)?,
        })
    } else {
        None
    };
    Ok(TextUpdate {
        runs: v.get("runs").map(runs).transpose()?,
        origin: v.get("origin").map(origin).transpose()?,
        style,
        model: model(v)?,
    })
}

/// Standard base64 with padding (RFC 4648 section 4). Whitespace is not allowed.
pub fn base64_decode(s: &str) -> Result<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(bad("base64 length must be a multiple of 4"));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunks = bytes.len() / 4;
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = i + 1 == chunks;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err(bad("bad base64 padding"));
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = n << 6 | value(c).ok_or_else(|| bad("bad base64 character"))?;
        }
        n <<= 6 * pad as u32;
        let three = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&three[..3 - pad]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::base64_decode;
    #[test]
    fn base64_round_trips_rfc_vectors() {
        // RFC 4648 section 10 test vectors.
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_decode(coded).unwrap(), plain.as_bytes());
        }
        assert_eq!(base64_decode("/+8=").unwrap(), [0xff, 0xef]);
        for bad in ["Zg=", "Z===", "Zg==Zg==", "Zm9!", "Zm 9v"] {
            assert!(base64_decode(bad).is_err(), "{bad}");
        }
    }
}
