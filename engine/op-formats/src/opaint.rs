//! OpenRaster-compatible native document files.
use crate::png::{DecodedImage, decode_png, encode_png};
use op_core::doc::{
    DocState, LayerBlendMode, MaskGrid, PolygonSet, SelectionSource, SelectionState,
};
use op_core::tiles::{TILE_SIZE, TileCoord, TileData, TileStore};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MIME: &[u8] = b"image/openraster";
const NS: &str = "https://openpainter.dev/ns/1";
const MAX_ZIP: u64 = 4 * 1024 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
type XmlAttrs = BTreeMap<String, String>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("file I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid .opaint: {0}")]
    Invalid(String),
    #[error("unsupported .opaint version {0}")]
    Version(String),
}
pub type Result<T> = std::result::Result<T, Error>;
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn other(error: impl std::fmt::Display) -> Error {
    invalid(error.to_string())
}

/// Save one document by replacing the target only after the complete ZIP is synced.
pub fn save_opaint(doc: &DocState, path: impl AsRef<Path>) -> Result<()> {
    save_impl(doc, path.as_ref(), |_| Ok(()), false)
}

fn save_impl(
    doc: &DocState,
    path: &Path,
    mut checkpoint: impl FnMut(usize) -> Result<()>,
    keep_temp: bool,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| invalid("target has no file name"))?;
    let (temp, file) = loop {
        let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{}.{}.{n}.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| {
        let mut zip = ZipWriter::new(file);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("mimetype", stored).map_err(other)?;
        zip.write_all(MIME)?;
        checkpoint(1)?;
        let xml = make_xml(doc)?;
        zip.start_file("stack.xml", deflated).map_err(other)?;
        zip.write_all(xml.as_bytes())?;
        checkpoint(2)?;
        for (index, _) in doc.layers.iter().enumerate() {
            let image = layer_image(doc, index)?;
            let png = encode_png(&image).map_err(other)?;
            zip.start_file(format!("data/layer{index}.png"), deflated)
                .map_err(other)?;
            zip.write_all(&png)?;
            checkpoint(3 + index)?;
        }
        if let Some(profile) = &doc.profile {
            zip.start_file("data/profile.icc", deflated)
                .map_err(other)?;
            zip.write_all(profile)?;
        }
        if let SelectionState::Active(SelectionSource::Coverage(mask)) = &doc.selection {
            if mask.width != doc.width || mask.height != doc.height {
                return Err(invalid("selection mask size mismatch"));
            }
            let mut pixels = Vec::with_capacity(mask.bytes.len() * 4);
            for &v in mask.bytes.iter() {
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
            let png = encode_png(&DecodedImage {
                width: doc.width,
                height: doc.height,
                pixels,
                icc: None,
            })
            .map_err(other)?;
            zip.start_file("data/selection.png", deflated)
                .map_err(other)?;
            zip.write_all(&png)?;
        }
        let merged = composite(doc)?;
        zip.start_file("mergedimage.png", deflated).map_err(other)?;
        zip.write_all(&encode_png(&merged).map_err(other)?)?;
        let thumb = thumbnail(&merged);
        zip.start_file("Thumbnails/thumbnail.png", deflated)
            .map_err(other)?;
        zip.write_all(&encode_png(&thumb).map_err(other)?)?;
        let file = zip.finish().map_err(other)?;
        file.sync_all()?;
        checkpoint(usize::MAX)?;
        if !keep_temp {
            fs::rename(&temp, path)?;
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() && !keep_temp {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn layer_image(doc: &DocState, index: usize) -> Result<DecodedImage> {
    let len = image_len(doc.width, doc.height)?;
    let mut pixels = vec![0; len];
    for ty in 0..doc.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..doc.width.div_ceil(TILE_SIZE as u32) {
            let tile = doc
                .read_tile(
                    index,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                )
                .map_err(other)?;
            let valid_w = (doc.width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
            let valid_h = (doc.height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
            for y in 0..valid_h {
                let from = y * TILE_SIZE * 4;
                let to = ((ty as usize * TILE_SIZE + y) * doc.width as usize
                    + tx as usize * TILE_SIZE)
                    * 4;
                pixels[to..to + valid_w * 4].copy_from_slice(&tile[from..from + valid_w * 4]);
            }
        }
    }
    Ok(DecodedImage {
        width: doc.width,
        height: doc.height,
        pixels,
        icc: None,
    })
}

fn image_len(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(invalid("empty canvas"));
    }
    let n = u64::from(width) * u64::from(height) * 4;
    if n > 1024 * 1024 * 1024 {
        return Err(invalid("canvas exceeds PNG limit"));
    }
    Ok(n as usize)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn mode_name(mode: LayerBlendMode) -> (&'static str, &'static str) {
    use LayerBlendMode::*;
    match mode {
        Normal => ("Normal", "svg:src-over"),
        Multiply => ("Multiply", "svg:multiply"),
        Additive => ("Additive", "svg:plus"),
        ColorBurn => ("ColorBurn", "svg:color-burn"),
        ColorDodge => ("ColorDodge", "svg:color-dodge"),
        Reflect => ("Reflect", "svg:color-dodge"),
        Glow => ("Glow", "svg:color-dodge"),
        Overlay => ("Overlay", "svg:overlay"),
        Difference => ("Difference", "svg:difference"),
        Negation => ("Negation", "svg:difference"),
        Lighten => ("Lighten", "svg:lighten"),
        Darken => ("Darken", "svg:darken"),
        Screen => ("Screen", "svg:screen"),
        Xor => ("Xor", "svg:difference"),
    }
}
fn parse_mode(s: &str) -> Option<LayerBlendMode> {
    use LayerBlendMode::*;
    Some(match s {
        "Normal" => Normal,
        "Multiply" => Multiply,
        "Additive" => Additive,
        "ColorBurn" => ColorBurn,
        "ColorDodge" => ColorDodge,
        "Reflect" => Reflect,
        "Glow" => Glow,
        "Overlay" => Overlay,
        "Difference" => Difference,
        "Negation" => Negation,
        "Lighten" => Lighten,
        "Darken" => Darken,
        "Screen" => Screen,
        "Xor" => Xor,
        _ => return None,
    })
}
fn ora_mode(s: &str) -> LayerBlendMode {
    use LayerBlendMode::*;
    match s {
        "svg:multiply" => Multiply,
        "svg:plus" => Additive,
        "svg:color-burn" => ColorBurn,
        "svg:color-dodge" => ColorDodge,
        "svg:overlay" => Overlay,
        "svg:difference" => Difference,
        "svg:lighten" => Lighten,
        "svg:darken" => Darken,
        "svg:screen" => Screen,
        _ => Normal,
    }
}
fn make_xml(doc: &DocState) -> Result<String> {
    image_len(doc.width, doc.height)?;
    if doc.layers.is_empty() || doc.active_layer >= doc.layers.len() {
        return Err(invalid("invalid layer stack"));
    }
    let metadata = serde_json::to_string(doc.metadata.as_ref()).map_err(other)?;
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><image version=\"0.0.6\" w=\"{}\" h=\"{}\" xres=\"{}\" yres=\"{}\" xmlns:op=\"{NS}\" op:version=\"1.0\" op:dpi=\"{}\" op:metadata=\"{}\" op:active-layer=\"{}\"",
        doc.width,
        doc.height,
        doc.dpi.round().max(1.0),
        doc.dpi.round().max(1.0),
        doc.dpi,
        escape(&metadata),
        doc.layers[doc.active_layer].id.0
    );
    if doc.profile.is_some() {
        xml.push_str(" op:profile=\"data/profile.icc\"");
    }
    match &doc.selection {
        SelectionState::None => xml.push_str(" op:selection=\"none\""),
        SelectionState::Active(SelectionSource::Coverage(_)) => {
            xml.push_str(" op:selection=\"coverage\" op:selection-src=\"data/selection.png\"")
        }
        SelectionState::Active(SelectionSource::Geometry(poly)) => {
            let json = serde_json::to_string(&poly.0).map_err(other)?;
            xml.push_str(&format!(
                " op:selection=\"geometry\" op:selection-paths=\"{}\"",
                escape(&json)
            ));
        }
    }
    xml.push_str("><stack>");
    for (i, layer) in doc.layers.iter().enumerate() {
        let (mode, composite) = mode_name(layer.blend);
        xml.push_str(&format!("<layer src=\"data/layer{i}.png\" name=\"{}\" visibility=\"{}\" opacity=\"{}\" x=\"0\" y=\"0\" composite-op=\"{composite}\" op:blend=\"{mode}\" op:id=\"{}\"/>", escape(&layer.name), if layer.visible { "visible" } else { "hidden" }, f64::from(layer.opacity) / 255.0, layer.id.0));
    }
    xml.push_str("</stack></image>");
    Ok(xml)
}

pub struct LoadedOpaint {
    pub doc: DocState,
    pub notice: Option<String>,
}
/// Load a native file, returning a notice when its minor version is newer.
pub fn load_opaint_with_notice(path: impl AsRef<Path>) -> Result<LoadedOpaint> {
    let path = path.as_ref();
    let mut zip = ZipArchive::new(File::open(path)?).map_err(other)?;
    if zip.is_empty() {
        return Err(invalid("missing mimetype"));
    }
    let mut names = HashSet::new();
    let mut total = 0_u64;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(other)?;
        let name = entry.name();
        check_path(name)?;
        if !names.insert(name.to_owned()) {
            return Err(invalid("duplicate entry path"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| invalid("ZIP size overflow"))?;
        if total > MAX_ZIP {
            return Err(invalid("ZIP exceeds 4 GiB"));
        }
        if i == 0 && (name != "mimetype" || entry.compression() != CompressionMethod::Stored) {
            return Err(invalid("first entry must be stored mimetype"));
        }
    }
    for required_name in ["stack.xml", "mergedimage.png", "Thumbnails/thumbnail.png"] {
        if !names.contains(required_name) {
            return Err(invalid(format!("missing {required_name}")));
        }
    }
    let mime = read_entry(&mut zip, "mimetype", 64)?;
    if mime != MIME {
        return Err(invalid("wrong mimetype"));
    }
    let xml = read_entry(&mut zip, "stack.xml", 16 * 1024 * 1024)?;
    let (root, layers) = parse_xml(&xml)?;
    let version = required(&root, "op:version")?;
    let (major, minor) = version
        .split_once('.')
        .ok_or_else(|| invalid("bad format version"))?;
    let major: u32 = major.parse().map_err(|_| invalid("bad format version"))?;
    let minor: u32 = minor.parse().map_err(|_| invalid("bad format version"))?;
    if major > 1 {
        return Err(Error::Version(version.to_owned()));
    }
    if major != 1 {
        return Err(invalid("unsupported format version"));
    }
    let notice = (minor > 0)
        .then(|| format!("newer .opaint minor version {version}; unknown fields may be ignored"));
    let width: u32 = parse_required(&root, "w")?;
    let height: u32 = parse_required(&root, "h")?;
    let raw_len = image_len(width, height)?;
    if layers.is_empty() {
        return Err(invalid("empty layer stack"));
    }
    let dpi: f64 = parse_required(&root, "op:dpi")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let store = Arc::new(TileStore::with_default_budget(parent).map_err(other)?);
    let mut doc = DocState::new(store, width, height, dpi).map_err(other)?;
    for i in 1..layers.len() {
        doc = doc.add_layer(i, "").map_err(other)?;
    }
    let mut ids = HashSet::new();
    for (i, attrs) in layers.iter().enumerate() {
        let src = required(attrs, "src")?;
        check_path(src)?;
        if !src.starts_with("data/") || !src.ends_with(".png") {
            return Err(invalid("invalid layer source"));
        }
        let bytes = read_entry(&mut zip, src, raw_len as u64 + 1024 * 1024)?;
        let image = decode_png(&bytes).map_err(other)?;
        if image.width != width || image.height != height {
            return Err(invalid("layer PNG size mismatch"));
        }
        for ty in 0..height.div_ceil(TILE_SIZE as u32) {
            for tx in 0..width.div_ceil(TILE_SIZE as u32) {
                let mut tile = vec![0; TILE_SIZE * TILE_SIZE * 4];
                let valid_w = (width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
                let valid_h = (height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
                for y in 0..valid_h {
                    let from = ((ty as usize * TILE_SIZE + y) * width as usize
                        + tx as usize * TILE_SIZE)
                        * 4;
                    let to = y * TILE_SIZE * 4;
                    tile[to..to + valid_w * 4]
                        .copy_from_slice(&image.pixels[from..from + valid_w * 4]);
                }
                doc = doc
                    .write_tile(
                        i,
                        TileCoord {
                            x: tx as i32,
                            y: ty as i32,
                        },
                        TileData::try_from(tile).map_err(other)?,
                    )
                    .map_err(other)?;
            }
        }
        doc = doc
            .set_layer_name(i, attrs.get("name").cloned().unwrap_or_default())
            .map_err(other)?;
        doc = doc
            .set_layer_visible(i, attrs.get("visibility").is_none_or(|v| v == "visible"))
            .map_err(other)?;
        let opacity = attrs
            .get("opacity")
            .map_or(Ok(1.0), |s| s.parse::<f64>().map_err(other))?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(invalid("invalid opacity"));
        }
        doc = doc
            .set_layer_opacity(i, (opacity * 255.0).round() as u8)
            .map_err(other)?;
        let blend = attrs.get("op:blend").map_or_else(
            || {
                Ok(ora_mode(
                    attrs
                        .get("composite-op")
                        .map_or("svg:src-over", String::as_str),
                ))
            },
            |s| parse_mode(s).ok_or_else(|| invalid("unknown blend mode")),
        )?;
        doc = doc.set_layer_blend(i, blend).map_err(other)?;
        if attrs.get("x").is_some_and(|v| v != "0") || attrs.get("y").is_some_and(|v| v != "0") {
            return Err(invalid("nonzero layer offset unsupported"));
        }
        let id: u64 = parse_required(attrs, "op:id")?;
        if id == 0 || !ids.insert(id) {
            return Err(invalid("duplicate or zero layer id"));
        }
        Arc::make_mut(&mut doc.layers[i]).id = op_core::doc::LayerId(id);
        doc.reserve_layer_id(op_core::doc::LayerId(id));
    }
    if let Some(profile_path) = root.get("op:profile") {
        check_path(profile_path)?;
        doc = doc.set_profile(Some(Arc::from(read_entry(
            &mut zip,
            profile_path,
            16 * 1024 * 1024,
        )?)));
    }
    if let Some(metadata) = root.get("op:metadata") {
        let map: BTreeMap<String, String> = serde_json::from_str(metadata).map_err(other)?;
        for (key, value) in map {
            doc = doc.set_metadata(key, value);
        }
    }
    let active: u64 = parse_required(&root, "op:active-layer")?;
    let active_index = doc
        .layers
        .iter()
        .position(|l| l.id.0 == active)
        .ok_or_else(|| invalid("active layer missing"))?;
    doc = doc.set_active_layer(active_index).map_err(other)?;
    match root
        .get("op:selection")
        .map(String::as_str)
        .unwrap_or("none")
    {
        "none" => {}
        "geometry" => {
            let value: Value =
                serde_json::from_str(required(&root, "op:selection-paths")?).map_err(other)?;
            let paths = parse_paths(&value)?;
            doc = doc.set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
                PolygonSet(paths),
            ))));
        }
        "coverage" => {
            let src = required(&root, "op:selection-src")?;
            check_path(src)?;
            let png = read_entry(&mut zip, src, raw_len as u64 + 1024 * 1024)?;
            let mask = decode_png(&png).map_err(other)?;
            if mask.width != width || mask.height != height {
                return Err(invalid("selection PNG size mismatch"));
            }
            let bytes = mask.pixels.chunks_exact(4).map(|p| p[0]).collect();
            doc = doc.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
                MaskGrid::new(width, height, bytes)
                    .ok_or_else(|| invalid("selection size mismatch"))?,
            ))));
        }
        _ => return Err(invalid("unknown selection type")),
    }
    Ok(LoadedOpaint { doc, notice })
}
pub fn load_opaint(path: impl AsRef<Path>) -> Result<DocState> {
    Ok(load_opaint_with_notice(path)?.doc)
}

fn check_path(path: &str) -> Result<()> {
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|c| c == ".." || c == "." || c.is_empty())
        || path.contains(':')
    {
        return Err(invalid("unsafe ZIP entry path"));
    }
    Ok(())
}
fn read_entry(zip: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let entry = zip.by_name(name).map_err(other)?;
    if entry.size() > limit {
        return Err(invalid(format!("{name} exceeds size limit")));
    }
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid(format!("{name} exceeds size limit")));
    }
    Ok(bytes)
}
fn required<'a>(attrs: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    attrs
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| invalid(format!("missing {name}")))
}
fn parse_required<T: std::str::FromStr>(attrs: &BTreeMap<String, String>, name: &str) -> Result<T> {
    required(attrs, name)?
        .parse()
        .map_err(|_| invalid(format!("invalid {name}")))
}
fn attributes(start: &BytesStart<'_>) -> Result<BTreeMap<String, String>> {
    let mut attrs = BTreeMap::new();
    for attr in start.attributes() {
        let attr = attr.map_err(other)?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(other)?
            .to_owned();
        let value = attr.unescape_value().map_err(other)?.into_owned();
        if attrs.insert(key, value).is_some() {
            return Err(invalid("duplicate XML attribute"));
        }
    }
    Ok(attrs)
}
fn parse_xml(bytes: &[u8]) -> Result<(XmlAttrs, Vec<XmlAttrs>)> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut depth = 0;
    let mut root = None;
    let mut layers = Vec::new();
    loop {
        match reader.read_event().map_err(other)? {
            Event::Start(start) => {
                match (depth, start.name().as_ref()) {
                    (0, b"image") if root.is_none() => root = Some(attributes(&start)?),
                    (1, b"stack") => {}
                    _ => return Err(invalid("unexpected XML element")),
                }
                depth += 1;
            }
            Event::Empty(start) if depth == 2 && start.name().as_ref() == b"layer" => {
                layers.push(attributes(&start)?)
            }
            Event::End(end) => {
                if depth == 0
                    || !matches!((depth, end.name().as_ref()), (2, b"stack") | (1, b"image"))
                {
                    return Err(invalid("bad XML nesting"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.unescape().map_err(other)?.trim().is_empty() => {
                return Err(invalid("unexpected XML text"));
            }
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) | Event::Text(_) => {}
            _ => return Err(invalid("unsupported XML content")),
        }
    }
    if depth != 0 {
        return Err(invalid("truncated XML"));
    }
    let root = root.ok_or_else(|| invalid("missing image element"))?;
    if required(&root, "xmlns:op")? != NS {
        return Err(invalid("wrong op namespace"));
    }
    Ok((root, layers))
}
fn parse_paths(value: &Value) -> Result<Vec<Vec<(f64, f64)>>> {
    let paths = value
        .as_array()
        .ok_or_else(|| invalid("invalid geometry paths"))?;
    let mut result = Vec::new();
    for path in paths {
        let points = path
            .as_array()
            .ok_or_else(|| invalid("invalid geometry path"))?;
        let mut out = Vec::new();
        for point in points {
            let pair = point
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or_else(|| invalid("invalid geometry point"))?;
            let x = pair[0]
                .as_f64()
                .ok_or_else(|| invalid("invalid geometry x"))?;
            let y = pair[1]
                .as_f64()
                .ok_or_else(|| invalid("invalid geometry y"))?;
            if !x.is_finite() || !y.is_finite() {
                return Err(invalid("non-finite geometry"));
            }
            out.push((x, y));
        }
        result.push(out);
    }
    Ok(result)
}

fn composite(doc: &DocState) -> Result<DecodedImage> {
    let mut output = vec![0; image_len(doc.width, doc.height)?];
    for ty in 0..doc.height.div_ceil(TILE_SIZE as u32) {
        for tx in 0..doc.width.div_ceil(TILE_SIZE as u32) {
            let mut acc = vec![[0.0_f32; 4]; TILE_SIZE * TILE_SIZE];
            for (i, layer) in doc.layers.iter().enumerate().rev() {
                if !layer.visible {
                    continue;
                }
                let tile = doc
                    .read_tile(
                        i,
                        TileCoord {
                            x: tx as i32,
                            y: ty as i32,
                        },
                    )
                    .map_err(other)?;
                for (pixel, src) in acc.iter_mut().zip(tile.chunks_exact(4)) {
                    let sa = src[3] as f32 / 255.0 * layer.opacity as f32 / 255.0;
                    if sa == 0.0 {
                        continue;
                    }
                    let ba = pixel[3];
                    for c in 0..3 {
                        let s = src[c] as f32 / 255.0;
                        let b = if ba > 0.0 { pixel[c] / ba } else { 0.0 };
                        pixel[c] = sa * (1.0 - ba) * s
                            + (1.0 - sa) * pixel[c]
                            + sa * ba * blend(layer.blend, b, s);
                    }
                    pixel[3] = sa + ba * (1.0 - sa);
                }
            }
            let valid_w = (doc.width - tx * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
            let valid_h = (doc.height - ty * TILE_SIZE as u32).min(TILE_SIZE as u32) as usize;
            for y in 0..valid_h {
                for x in 0..valid_w {
                    let p = acc[y * TILE_SIZE + x];
                    if p[3] == 0.0 {
                        continue;
                    }
                    let to = ((ty as usize * TILE_SIZE + y) * doc.width as usize
                        + tx as usize * TILE_SIZE
                        + x)
                        * 4;
                    for c in 0..3 {
                        output[to + c] = op_core::pixel::round_unit_to_u8(p[c] / p[3]);
                    }
                    output[to + 3] = op_core::pixel::round_unit_to_u8(p[3]);
                }
            }
        }
    }
    Ok(DecodedImage {
        width: doc.width,
        height: doc.height,
        pixels: output,
        icc: None,
    })
}
fn blend(mode: LayerBlendMode, b: f32, s: f32) -> f32 {
    use LayerBlendMode::*;
    match mode {
        Normal => s,
        Multiply => b * s,
        Additive => (b + s).min(1.0),
        ColorBurn => {
            if s == 0.0 {
                0.0
            } else {
                (1.0 - (1.0 - b) / s).max(0.0)
            }
        }
        ColorDodge => {
            if s == 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        Reflect => {
            if s == 1.0 {
                1.0
            } else {
                (b * b / (1.0 - s)).min(1.0)
            }
        }
        Glow => {
            if b == 1.0 {
                1.0
            } else {
                (s * s / (1.0 - b)).min(1.0)
            }
        }
        Overlay => {
            if b < 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        Difference => (b - s).abs(),
        Negation => 1.0 - (1.0 - b - s).abs(),
        Lighten => b.max(s),
        Darken => b.min(s),
        Screen => b + s - b * s,
        Xor => {
            (op_core::pixel::round_unit_to_u8(b) ^ op_core::pixel::round_unit_to_u8(s)) as f32
                / 255.0
        }
    }
}
fn thumbnail(merged: &DecodedImage) -> DecodedImage {
    let scale = (merged.width.max(merged.height) as f64 / 256.0).max(1.0);
    let width = (f64::from(merged.width) / scale).round().max(1.0) as u32;
    let height = (f64::from(merged.height) / scale).round().max(1.0) as u32;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let sx = ((f64::from(x) * scale) as u32).min(merged.width - 1);
            let sy = ((f64::from(y) * scale) as u32).min(merged.height - 1);
            let from = (sy as usize * merged.width as usize + sx as usize) * 4;
            let to = (y as usize * width as usize + x as usize) * 4;
            pixels[to..to + 4].copy_from_slice(&merged.pixels[from..from + 4]);
        }
    }
    DecodedImage {
        width,
        height,
        pixels,
        icc: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_and_interrupted_save_preserve_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(TileStore::with_default_budget(dir.path()).unwrap());
        let doc = DocState::new(store, 2, 2, 72.0).unwrap();
        let path = dir.path().join("atomic.opaint");
        save_opaint(&doc, &path).unwrap();
        let old = fs::read(&path).unwrap();
        let error = save_impl(
            &doc,
            &path,
            |stage| {
                if stage == 3 {
                    Err(invalid("injected writer failure"))
                } else {
                    Ok(())
                }
            },
            false,
        );
        assert!(error.is_err());
        assert_eq!(fs::read(&path).unwrap(), old);
        save_impl(&doc, &path, |_| Ok(()), true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), old);
        assert!(
            fs::read_dir(dir.path()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );
        assert!(load_opaint(&path).is_ok());
    }
}
