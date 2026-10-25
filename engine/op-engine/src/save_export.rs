//! M50 save options: encodes flattened RGBA8 pixels as PNG, JPEG or GIF with
//! the options from the app's save options sheet, the document resolution and
//! the kept metadata (raw EXIF bytes and an XMP packet). The app writes the
//! bytes itself, so the same call feeds the sheet's file-size preview.
//!
//! Options (JSON): `format` "png" | "jpeg" | "gif" | "tiff" | "bmp" | "webp";
//! `png_level` 0..9; `jpeg_quality` 1..100; `jpeg_subsampling` "444" | "422" |
//! "420"; `gif_dither` bool; `tiff_compression` "none" | "lzw"; `dpi` pixels
//! per inch.
use super::{Result, bad, internal};
use op_formats::{ChromaSubsampling, Decoded, Format, Metadata, Options, PngCompression};
use serde_json::Value;

/// Encodes `rgba` (straight RGBA8, rows top first) with `params`.
pub fn encode_image(
    params: &Value,
    width: u32,
    height: u32,
    rgba: &[u8],
    icc: Option<&[u8]>,
    exif: Option<&[u8]>,
    xmp: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let format = match params.get("format").and_then(Value::as_str).unwrap_or("") {
        "png" => Format::Png,
        "jpeg" => Format::Jpeg,
        "gif" => Format::Gif,
        "tiff" => Format::Tiff,
        "bmp" => Format::Bmp,
        "webp" => Format::WebP,
        other => return Err(bad(format!("unknown export format {other}"))),
    };
    let mut options = Options::default();
    if let Some(v) = params.get("png_level") {
        let n = v
            .as_u64()
            .filter(|n| *n <= 9)
            .ok_or_else(|| bad("png_level must be 0..9"))?;
        options.png_compression = PngCompression::Level(n as u8);
    }
    if let Some(v) = params.get("jpeg_quality") {
        let n = v
            .as_u64()
            .filter(|n| (1..=100).contains(n))
            .ok_or_else(|| bad("jpeg_quality must be 1..100"))?;
        options.jpeg_quality = n as u8;
    }
    if let Some(v) = params.get("jpeg_subsampling") {
        options.jpeg_subsampling = match v.as_str() {
            Some("444") => ChromaSubsampling::S444,
            Some("422") => ChromaSubsampling::S422,
            Some("420") => ChromaSubsampling::S420,
            _ => return Err(bad("jpeg_subsampling must be 444, 422 or 420")),
        };
    }
    if let Some(v) = params.get("tiff_compression") {
        options.tiff_compression = match v.as_str() {
            Some("none") => op_formats::TiffCompression::None,
            Some("lzw") => op_formats::TiffCompression::Lzw,
            _ => return Err(bad("tiff_compression must be none or lzw")),
        };
    }
    if let Some(v) = params.get("gif_dither") {
        options.gif_dither = v
            .as_bool()
            .ok_or_else(|| bad("gif_dither must be a bool"))?;
    }
    if let Some(v) = params.get("dpi") {
        let dpi = v
            .as_f64()
            .filter(|d| d.is_finite() && (1.0..=100_000.0).contains(d))
            .ok_or_else(|| bad("dpi out of range"))?;
        options.dpi = Some(dpi);
    }
    let expected = (width as u64) * (height as u64) * 4;
    if width == 0 || height == 0 || rgba.len() as u64 != expected {
        return Err(bad("pixel count does not match the size"));
    }
    let image = Decoded {
        width,
        height,
        rgba8: rgba.to_vec(),
        icc: icc.map(<[u8]>::to_vec),
        metadata: Metadata {
            exif: exif.map(<[u8]>::to_vec),
            xmp: xmp.map(<[u8]>::to_vec),
            text: vec![],
        },
        notices: vec![],
    };
    op_formats::encode(format, &image, &options).map_err(|e| match e {
        op_formats::Error::Invalid(m) => bad(m),
        other => internal(other),
    })
}
