//! Minimal PNG I/O. Pixels are straight RGBA8, including colors under zero alpha.
use std::borrow::Cow;
use std::fmt;
use std::io::Cursor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub icc: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct Error(String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn err(e: impl fmt::Display) -> Error {
    Error(e.to_string())
}
fn image_len(w: u32, h: u32) -> Result<usize> {
    let n = (w as u64)
        .checked_mul(h as u64)
        .and_then(|x| x.checked_mul(4))
        .ok_or_else(|| err("PNG dimensions overflow"))?;
    if n > 1024 * 1024 * 1024 || n > usize::MAX as u64 {
        return Err(err("PNG image exceeds 1 GiB"));
    }
    Ok(n as usize)
}
pub fn decode_png(bytes: &[u8]) -> Result<DecodedImage> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 1024 * 1024 * 1024 + 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(err)?;
    let (width, height, depth) = {
        let i = reader.info();
        (i.width, i.height, i.bit_depth)
    };
    let len = image_len(width, height)?;
    if depth == png::BitDepth::Sixteen {
        return Err(err("16-bit PNG unsupported in M04"));
    }
    if depth != png::BitDepth::Eight && reader.output_color_type().1 != png::BitDepth::Eight {
        return Err(err("PNG bit depth unsupported in M04"));
    }
    let output_size = reader
        .output_buffer_size()
        .ok_or_else(|| err("PNG output size overflow"))?;
    if output_size > 1024 * 1024 * 1024 {
        return Err(err("PNG output exceeds 1 GiB"));
    }
    let mut raw = vec![0; output_size];
    let output = reader.next_frame(&mut raw).map_err(err)?;
    let embedded_icc = reader.info().icc_profile.as_ref().map(|p| p.to_vec());
    let mut pixels = Vec::with_capacity(len);
    let mut gray_expanded = false;
    match output.color_type {
        png::ColorType::Rgba => pixels.extend_from_slice(&raw[..output.buffer_size()]),
        png::ColorType::Rgb => {
            for p in raw[..output.buffer_size()].chunks_exact(3) {
                pixels.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            if let Some(icc) = &embedded_icc {
                let src = op_color::Profile::from_icc(icc).map_err(err)?;
                let dst = op_color::Profile::builtin("srgb").map_err(err)?;
                pixels = op_color::expand_gray(&raw[..output.buffer_size()], None, &src, &dst)
                    .map_err(err)?;
                gray_expanded = true;
            } else {
                for &g in &raw[..output.buffer_size()] {
                    pixels.extend_from_slice(&[g, g, g, 255]);
                }
            }
        }
        png::ColorType::GrayscaleAlpha => {
            if let Some(icc) = &embedded_icc {
                let src = op_color::Profile::from_icc(icc).map_err(err)?;
                let dst = op_color::Profile::builtin("srgb").map_err(err)?;
                let gray: Vec<u8> = raw[..output.buffer_size()]
                    .chunks_exact(2)
                    .map(|p| p[0])
                    .collect();
                let alpha: Vec<u8> = raw[..output.buffer_size()]
                    .chunks_exact(2)
                    .map(|p| p[1])
                    .collect();
                pixels = op_color::expand_gray(&gray, Some(&alpha), &src, &dst).map_err(err)?;
                gray_expanded = true;
            } else {
                for p in raw[..output.buffer_size()].chunks_exact(2) {
                    pixels.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
                }
            }
        }
        _ => return Err(err("PNG color type unsupported in M04")),
    }
    if pixels.len() != len {
        return Err(err("PNG pixel count mismatch"));
    }
    reader.finish().map_err(err)?;
    let icc = if gray_expanded {
        Some(
            op_color::Profile::builtin("srgb")
                .map_err(err)?
                .bytes()
                .to_vec(),
        )
    } else {
        embedded_icc
    };
    Ok(DecodedImage {
        width,
        height,
        pixels,
        icc,
    })
}
pub fn encode_png(image: &DecodedImage) -> Result<Vec<u8>> {
    let len = image_len(image.width, image.height)?;
    if image.pixels.len() != len {
        return Err(err("RGBA pixel count mismatch"));
    }
    let mut out = Vec::new();
    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = image.icc.as_deref().map(Cow::Borrowed);
    let encoder = png::Encoder::with_info(&mut out, info).map_err(err)?;
    let mut writer = encoder.write_header().map_err(err)?;
    writer.write_image_data(&image.pixels).map_err(err)?;
    writer.finish().map_err(err)?;
    Ok(out)
}

/// `encode_png` straight into `out`, 64 rows at a time, asking `stop` between
/// bands (M57: a recovery writer that was told to stop does not finish a
/// long encode first). The bytes decode to the same pixels as `encode_png`'s.
pub fn encode_png_to<W: std::io::Write>(
    out: W,
    image: &DecodedImage,
    stop: impl Fn() -> bool,
) -> Result<()> {
    let len = image_len(image.width, image.height)?;
    if image.pixels.len() != len {
        return Err(err("RGBA pixel count mismatch"));
    }
    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = image.icc.as_deref().map(Cow::Borrowed);
    let encoder = png::Encoder::with_info(out, info).map_err(err)?;
    let mut writer = encoder.write_header().map_err(err)?;
    {
        let mut stream = writer.stream_writer().map_err(err)?;
        let band = image.width as usize * 4 * 64;
        for chunk in image.pixels.chunks(band) {
            if stop() {
                return Err(err("save cancelled"));
            }
            std::io::Write::write_all(&mut stream, chunk).map_err(err)?;
        }
        stream.finish().map_err(err)?;
    }
    Ok(())
}

/// The resolution in pixels per inch from a PNG's pHYs chunk (M50), when the
/// chunk gives meters as its unit. Pixels per meter × 0.0254 = pixels per
/// inch. The writer stores round(dpi / 0.0254), so when a whole number of dpi
/// gives exactly this pixels-per-meter value that whole number is returned
/// (72 dpi is 2835 px/m, which is 72.009 dpi); otherwise the value is rounded
/// to 2 decimals. A pHYs with no unit (aspect ratio only) gives None.
pub fn png_dpi(bytes: &[u8]) -> Option<f64> {
    let mut p = 8usize;
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    while p.checked_add(12)? <= bytes.len() {
        let n = u32::from_be_bytes(bytes[p..p + 4].try_into().ok()?) as usize;
        let kind = &bytes[p + 4..p + 8];
        let data = bytes.get(p + 8..p.checked_add(8)?.checked_add(n)?)?;
        if kind == b"pHYs" {
            if n != 9 || data[8] != 1 {
                return None;
            }
            let x = u32::from_be_bytes(data[0..4].try_into().ok()?);
            if x == 0 {
                return None;
            }
            let exact = x as f64 * 0.0254;
            let whole = exact.round();
            if (whole / 0.0254).round() == x as f64 {
                return Some(whole);
            }
            return Some((exact * 100.0).round() / 100.0);
        }
        if kind == b"IDAT" {
            return None;
        }
        p = p.checked_add(12)?.checked_add(n)?;
    }
    None
}
