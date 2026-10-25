//! `opcli scenario <script.json> -o <out-dir>`: runs a scenario script (M28)
//! on one document without the Mac app, and writes its outputs.
//!
//! A script is JSON:
//!
//! - `input`: `{"path", "dpi", "pixels"?, "decoder_max_diff"?}`. Paths are
//!   relative to the script. The file is decoded by `op_formats::decode` (EXIF
//!   orientation applied, ICC profile kept) and imported at `dpi` pixels per
//!   inch. With `pixels`, a PNG holding another decoder's result for the same
//!   file (the Mac app decodes JPEG with ImageIO), the document starts from
//!   that PNG's pixels and profile instead; the engine's own decode of `path`
//!   must then have the same size and differ by at most `decoder_max_diff`
//!   levels in any channel (default 0), and the summary reports the
//!   difference.
//! - `steps`: `[{"command", "params", "ui"?}]`, each run with
//!   `Engine::dispatch` in order. `ui` says what a person does in the app for
//!   that step; the runner ignores it.
//! - `expect` (optional): `width`, `height`, `history_labels`,
//!   `history_index`. A mismatch fails the run.
//! - `outputs`: `[{"path", "format", ...}]`, paths relative to the out
//!   folder. `opaint` and `png` are written by the engine's own save
//!   (`Engine::save_document`); `jpeg` is the flattened image on white (the
//!   app's rule, `DocumentSaver.onWhite`) through `save_export::encode_image`
//!   with the step's `jpeg_quality`, `jpeg_subsampling` and the document
//!   resolution, as the app's JPEG save does.
//!
//! The run prints one JSON line: size, history and the files written.
use op_engine::{Engine, SaveFormat};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub fn run(script: &Path, out_dir: &Path) -> Res<Value> {
    let text = std::fs::read_to_string(script)
        .map_err(|e| format!("cannot read {}: {e}", script.display()))?;
    let s: Value = serde_json::from_str(&text)?;
    let base = script.parent().unwrap_or(Path::new("."));
    let input = s.get("input").ok_or("script has no input")?;
    let path = base.join(
        input
            .get("path")
            .and_then(Value::as_str)
            .ok_or("input.path missing")?,
    );
    let dpi = input.get("dpi").and_then(Value::as_f64).unwrap_or(96.0);
    let read = |p: &Path| -> Res<op_formats::Decoded> {
        let bytes = std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        Ok(op_formats::decode(&bytes).map_err(|e| format!("{}: {e}", p.display()))?)
    };
    let own = read(&path)?;
    let mut decoder_check = Value::Null;
    let decoded = match input.get("pixels").and_then(Value::as_str) {
        None => own,
        Some(rel) => {
            let given = read(&base.join(rel))?;
            if (given.width, given.height) != (own.width, own.height) {
                return Err(format!(
                    "input.pixels is {}x{}, the engine decodes {}x{}",
                    given.width, given.height, own.width, own.height
                )
                .into());
            }
            let allowed = input
                .get("decoder_max_diff")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let (max, changed) = difference(&own.rgba8, &given.rgba8);
            if max as u64 > allowed {
                return Err(format!(
                    "the engine's decode differs from input.pixels by {max} levels (allowed {allowed})"
                )
                .into());
            }
            decoder_check = json!({"max_diff": max, "samples_differing": changed});
            given
        }
    };
    let engine = Engine::with_options(
        None,
        op_engine::EngineOptions {
            force_cpu: std::env::var("OP_CLI_GPU").as_deref() != Ok("1"),
            ..Default::default()
        },
    )?;
    let doc = engine.import_pixels_at(
        decoded.width,
        decoded.height,
        &decoded.rgba8,
        decoded.icc.as_deref(),
        dpi,
    )?;
    let result = steps_and_outputs(&engine, doc, &s, out_dir);
    let closed = engine.close(doc);
    let mut summary = result?;
    closed?;
    summary["decoder_check"] = decoder_check;
    Ok(summary)
}

/// The largest channel difference and how many channel samples differ.
fn difference(a: &[u8], b: &[u8]) -> (u8, usize) {
    a.iter().zip(b).fold((0, 0), |(max, n), (x, y)| {
        let d = x.abs_diff(*y);
        (max.max(d), n + usize::from(d > 0))
    })
}

fn steps_and_outputs(engine: &Engine, doc: u64, s: &Value, out_dir: &Path) -> Res<Value> {
    let empty = vec![];
    for (i, step) in s
        .get("steps")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let command = step
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("step {i} has no command"))?;
        let params = step.get("params").cloned().unwrap_or_else(|| json!({}));
        engine
            .dispatch(doc, command, params)
            .map_err(|e| format!("step {i} ({command}): {e}"))?;
    }
    let info: Value = serde_json::from_slice(&engine.query(doc, "doc.info", json!({}))?)?;
    if let Some(expect) = s.get("expect").and_then(Value::as_object) {
        for (key, want) in expect {
            let got = info
                .get(key)
                .ok_or_else(|| format!("doc.info has no {key}"))?;
            if got != want {
                return Err(format!("expected {key} {want}, got {got}").into());
            }
        }
    }
    std::fs::create_dir_all(out_dir)?;
    let mut written = vec![];
    for out in s.get("outputs").and_then(Value::as_array).unwrap_or(&empty) {
        let rel = out
            .get("path")
            .and_then(Value::as_str)
            .ok_or("an output has no path")?;
        let path: PathBuf = out_dir.join(rel);
        match out.get("format").and_then(Value::as_str).unwrap_or("") {
            "opaint" => engine.save_document(doc, &path, SaveFormat::Opaint, false)?,
            "png" => engine.save_document(doc, &path, SaveFormat::Png, false)?,
            "jpeg" => std::fs::write(&path, jpeg(engine, doc, &info, out)?)?,
            other => return Err(format!("unknown output format {other:?}").into()),
        }
        written.push(json!(path.display().to_string()));
    }
    Ok(json!({
        "width": info["width"],
        "height": info["height"],
        "history_labels": info["history_labels"],
        "history_index": info["history_index"],
        "outputs": written,
    }))
}

/// The app's JPEG save: flattened pixels on white, the document profile and
/// resolution, quality and chroma subsampling from the output entry.
fn jpeg(engine: &Engine, doc: u64, info: &Value, out: &Value) -> Res<Vec<u8>> {
    let w = info["width"].as_u64().ok_or("doc.info width")? as u32;
    let h = info["height"].as_u64().ok_or("doc.info height")? as u32;
    let pixels = on_white(engine.query(doc, "doc.pixels", json!({}))?);
    let icc = engine.query(doc, "doc.profile_icc", json!({}))?;
    let res: Value = serde_json::from_slice(&engine.query(doc, "doc.resolution", json!({}))?)?;
    let dpi = res["dpi"].as_f64().unwrap_or(96.0);
    let mut params = json!({"format": "jpeg", "dpi": dpi});
    for key in ["jpeg_quality", "jpeg_subsampling"] {
        if let Some(v) = out.get(key) {
            params[key] = v.clone();
        }
    }
    let icc = (!icc.is_empty()).then_some(icc.as_slice());
    Ok(op_engine::save_export::encode_image(
        &params, w, h, &pixels, icc, None, None,
    )?)
}

/// JPEG has no alpha: each channel becomes (c * a + 255 * (255 - a) + 127) / 255,
/// exactly as `DocumentSaver.onWhite` in the Mac app.
pub fn on_white(mut rgba: Vec<u8>) -> Vec<u8> {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a == 255 {
            continue;
        }
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_white_matches_the_app_rule() {
        let out = on_white(vec![10, 200, 255, 255, 100, 0, 255, 128, 9, 9, 9, 0]);
        assert_eq!(&out[..4], &[10, 200, 255, 255], "opaque stays");
        // (100*128 + 255*127 + 127) / 255 = 177; (0 + 32385 + 127) / 255 = 127.
        assert_eq!(&out[4..8], &[177, 127, 255, 255]);
        assert_eq!(&out[8..], &[255, 255, 255, 255], "clear is white");
    }

    #[test]
    fn difference_counts_samples() {
        assert_eq!(difference(&[1, 2, 3, 4], &[1, 5, 2, 4]), (3, 2));
        assert_eq!(difference(&[7], &[7]), (0, 0));
    }
}
