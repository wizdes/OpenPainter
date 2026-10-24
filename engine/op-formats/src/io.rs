use crate::{Error, Result, bmp, checked_len, codec, exif_ifd};
use std::{borrow::Cow, io::Cursor};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    pub text: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub icc: Option<Vec<u8>>,
    pub metadata: Metadata,
    pub notices: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    WebP,
}
#[derive(Clone, Copy, Debug, Default)]
pub enum ChromaSubsampling {
    #[default]
    S420,
    S422,
    S444,
}
#[derive(Clone, Copy, Debug, Default)]
pub enum PngCompression {
    Fast,
    #[default]
    Default,
    Best,
    Level(u8),
}
#[derive(Clone, Copy, Debug, Default)]
pub enum TiffCompression {
    #[default]
    None,
    Lzw,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub jpeg_quality: u8,
    pub jpeg_subsampling: ChromaSubsampling,
    pub png_compression: PngCompression,
    pub tiff_compression: TiffCompression,
    pub bmp_alpha: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            jpeg_quality: 90,
            jpeg_subsampling: ChromaSubsampling::S420,
            png_compression: PngCompression::Default,
            tiff_compression: TiffCompression::None,
            bmp_alpha: true,
        }
    }
}
impl Decoded {
    fn new(width: u32, height: u32, rgba8: Vec<u8>) -> Result<Self> {
        if rgba8.len() != checked_len(width, height)? {
            return Err(Error::Invalid("pixel count mismatch"));
        }
        Ok(Self {
            width,
            height,
            rgba8,
            icc: None,
            metadata: Metadata::default(),
            notices: vec![],
        })
    }
}
fn expand(raw: &[u8], channels: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() / channels * 4);
    for p in raw.chunks_exact(channels) {
        match channels {
            1 => out.extend_from_slice(&[p[0], p[0], p[0], 255]),
            2 => out.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
            3 => out.extend_from_slice(&[p[0], p[1], p[2], 255]),
            4 => out.extend_from_slice(p),
            _ => {}
        }
    }
    out
}
/// Convert a 16-bit sample with floor(v * 255 / 65535 + 0.5).
fn round16(v: u16) -> u8 {
    ((v as u32 * 255 + 32767) / 65535) as u8
}

pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    let result = std::panic::catch_unwind(|| {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            decode_png(bytes)
        } else if bytes.starts_with(b"\xff\xd8") {
            decode_jpeg(bytes)
        } else if bytes.starts_with(b"BM") {
            bmp::decode(bytes)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            decode_gif(bytes)
        } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
            decode_tiff(bytes)
        } else if bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            decode_webp(bytes)
        } else {
            Err(Error::Invalid("unknown image format"))
        }
    })
    .map_err(|_| Error::Invalid("image decoder rejected corrupt data"))?;
    let mut image = result?;
    let orientation = image
        .metadata
        .exif
        .as_ref()
        .and_then(|b| exif_orientation(b))
        .unwrap_or(1);
    if orientation > 1 {
        orient(&mut image, orientation);
        if let Some(exif) = &mut image.metadata.exif {
            reset_orientation(exif);
        }
    }
    Ok(image)
}

pub fn encode(format: Format, image: &Decoded, options: &Options) -> Result<Vec<u8>> {
    if image.rgba8.len() != checked_len(image.width, image.height)? {
        return Err(Error::Invalid("pixel count mismatch"));
    }
    match format {
        Format::Png => encode_png(image, options),
        Format::Jpeg => encode_jpeg(image, options),
        Format::Bmp => bmp::encode(image, options.bmp_alpha),
        Format::Gif => encode_gif(image),
        Format::Tiff => encode_tiff(image, options),
        Format::WebP => encode_webp(image),
    }
}

fn decode_png(bytes: &[u8]) -> Result<Decoded> {
    let mut dec = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 1_073_741_824 + 1_048_576,
        },
    );
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().map_err(codec)?;
    let (w, h, depth) = {
        let i = reader.info();
        (i.width, i.height, i.bit_depth)
    };
    checked_len(w, h)?;
    let output_size = reader
        .output_buffer_size()
        .ok_or(Error::Invalid("PNG output size"))?;
    if output_size > 1_073_741_824 {
        return Err(Error::Invalid("PNG output exceeds 1 GiB"));
    }
    let mut buf = vec![0; output_size];
    let frame = reader.next_frame(&mut buf).map_err(codec)?;
    let n = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return Err(Error::Invalid("PNG color type")),
    };
    let raw = &buf[..frame.buffer_size()];
    let rgba8 = if depth == png::BitDepth::Sixteen {
        let values: Vec<u8> = raw
            .chunks_exact(2)
            .map(|p| round16(u16::from_be_bytes([p[0], p[1]])))
            .collect();
        expand(&values, n)
    } else {
        expand(raw, n)
    };
    let mut out = Decoded::new(w, h, rgba8)?;
    let info = reader.info();
    out.icc = info.icc_profile.as_ref().map(|v| v.to_vec());
    out.metadata.exif = png_chunk(bytes, b"eXIf").map(|v| v.to_vec());
    for t in &info.uncompressed_latin1_text {
        out.metadata.text.push((t.keyword.clone(), t.text.clone()));
    }
    for t in &info.compressed_latin1_text {
        let mut t = t.clone();
        t.decompress_text_with_limit(16 * 1024 * 1024)
            .map_err(codec)?;
        out.metadata
            .text
            .push((t.keyword.clone(), t.get_text().map_err(codec)?));
    }
    for t in &info.utf8_text {
        let mut t = t.clone();
        t.decompress_text_with_limit(16 * 1024 * 1024)
            .map_err(codec)?;
        let value = t.get_text().map_err(codec)?;
        if t.keyword == "XML:com.adobe.xmp" {
            out.metadata.xmp = Some(value.into_bytes());
        } else {
            out.metadata.text.push((t.keyword.clone(), value));
        }
    }
    reader.finish().map_err(codec)?;
    if n <= 2
        && let Some(icc) = &out.icc
    {
        let src = op_color::Profile::from_icc(icc).map_err(codec)?;
        let dst = op_color::Profile::builtin("srgb").map_err(codec)?;
        let gray: Vec<u8> = out.rgba8.chunks_exact(4).map(|p| p[0]).collect();
        let alpha: Vec<u8> = out.rgba8.chunks_exact(4).map(|p| p[3]).collect();
        out.rgba8 = op_color::expand_gray(&gray, Some(&alpha), &src, &dst).map_err(codec)?;
        out.icc = Some(dst.bytes().to_vec());
    }
    Ok(out)
}
fn png_chunk<'a>(bytes: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    let mut p = 8usize;
    while p + 12 <= bytes.len() {
        let n = u32::from_be_bytes(bytes[p..p + 4].try_into().ok()?) as usize;
        let end = p.checked_add(n)?.checked_add(12)?;
        if end > bytes.len() {
            return None;
        }
        if &bytes[p + 4..p + 8] == kind {
            return Some(&bytes[p + 8..p + 8 + n]);
        }
        p = end;
    }
    None
}
fn encode_png(image: &Decoded, opt: &Options) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = image.icc.as_deref().map(Cow::Borrowed);
    info.exif_metadata = image.metadata.exif.as_deref().map(Cow::Borrowed);
    for (k, v) in &image.metadata.text {
        info.uncompressed_latin1_text
            .push(png::text_metadata::TEXtChunk::new(k.clone(), v.clone()));
    }
    if let Some(xmp) = &image.metadata.xmp {
        info.utf8_text.push(png::text_metadata::ITXtChunk::new(
            "XML:com.adobe.xmp",
            String::from_utf8_lossy(xmp).into_owned(),
        ));
    }
    let mut encoder = png::Encoder::with_info(&mut out, info).map_err(codec)?;
    let compression = match opt.png_compression {
        PngCompression::Fast => png::DeflateCompression::FdeflateUltraFast,
        PngCompression::Default => png::DeflateCompression::Level(6),
        PngCompression::Best => png::DeflateCompression::Level(9),
        PngCompression::Level(0) => png::DeflateCompression::NoCompression,
        PngCompression::Level(n @ 1..=9) => png::DeflateCompression::Level(n),
        PngCompression::Level(_) => {
            return Err(Error::Invalid("PNG compression level must be 0..9"));
        }
    };
    encoder.set_deflate_compression(compression);
    let mut writer = encoder.write_header().map_err(codec)?;
    writer.write_image_data(&image.rgba8).map_err(codec)?;
    writer.finish().map_err(codec)?;
    Ok(out)
}
fn decode_jpeg(bytes: &[u8]) -> Result<Decoded> {
    if !bytes.ends_with(b"\xff\xd9") {
        return Err(Error::Invalid("truncated JPEG"));
    }
    let mut dec = zune_jpeg::JpegDecoder::new(Cursor::new(bytes));
    dec.decode_headers().map_err(codec)?;
    let info = dec.info().ok_or(Error::Invalid("JPEG header"))?;
    let (w, h) = (info.width as u32, info.height as u32);
    checked_len(w, h)?;
    let icc = dec.icc_profile();
    let exif = dec.exif().cloned();
    let xmp = dec.xmp().cloned();
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    let input_space = dec.input_colorspace();
    if let Some(space @ (ColorSpace::CMYK | ColorSpace::YCCK)) = input_space {
        // Ask for the stored channels unchanged and convert them here.
        let options = dec.options().jpeg_set_out_colorspace(space);
        dec.set_options(options);
        let raw = dec.decode().map_err(codec)?;
        if raw.len() != checked_len(w, h)? {
            return Err(Error::Invalid("CMYK JPEG pixel count"));
        }
        let ink: Vec<[u8; 4]> = if space == ColorSpace::YCCK {
            raw.chunks_exact(4).map(ycck_to_ink).collect()
        } else {
            raw.chunks_exact(4)
                .map(|p| [255 - p[0], 255 - p[1], 255 - p[2], 255 - p[3]])
                .collect()
        };
        let (rgba8, icc, notice) = cmyk_to_srgb(&ink, icc.as_deref())?;
        let mut out = Decoded::new(w, h, rgba8)?;
        out.icc = icc;
        out.metadata.exif = exif;
        out.metadata.xmp = xmp;
        out.notices.push(notice);
        return Ok(out);
    }
    let raw = dec.decode().map_err(codec)?;
    let channels = raw.len() / ((w as usize) * (h as usize));
    if !matches!(channels, 1 | 3 | 4) {
        return Err(Error::Invalid("JPEG channels"));
    }
    let mut out = Decoded::new(w, h, expand(&raw, channels))?;
    out.icc = icc;
    out.metadata.exif = exif;
    out.metadata.xmp = xmp;
    Ok(out)
}
/// Turn one stored YCCK sample into ink amounts (0 = no ink).
///
/// Adobe YCCK keeps (255 - C, 255 - M, 255 - Y) as YCbCr, which decodes to
/// R = C ink, G = M ink, B = Y ink, and stores K inverted like Adobe CMYK.
fn ycck_to_ink(p: &[u8]) -> [u8; 4] {
    let (y, cb, cr) = (p[0] as f32, p[1] as f32 - 128.0, p[2] as f32 - 128.0);
    let clamp = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    [
        clamp(y + 1.402 * cr),
        clamp(y - 0.344_136 * cb - 0.714_136 * cr),
        clamp(y + 1.772 * cb),
        255 - p[3],
    ]
}
/// Convert ink amounts to sRGB.
///
/// With a usable CMYK ICC profile this goes through op-color (perceptual
/// intent, black point compensation) and the result is tagged sRGB. Without
/// one it uses the naive formula R = 255 * (1 - C) * (1 - K) (and the same for
/// M with G and Y with B), which ignores ink behavior and can be visibly off;
/// the result has no ICC profile and is meant to be read as sRGB. Both paths
/// add a notice. JPEG CMYK is read with the Adobe convention (inverted
/// samples), which is what Photoshop and most CMYK JPEG writers produce.
fn cmyk_to_srgb(ink: &[[u8; 4]], icc: Option<&[u8]>) -> Result<(Vec<u8>, Option<Vec<u8>>, String)> {
    let problem = match icc {
        None => "the file has no ICC profile".to_string(),
        Some(bytes) => {
            match op_color::Profile::from_icc(bytes).and_then(|p| op_color::import_cmyk(ink, &p)) {
                Ok((rgba8, notice)) => {
                    let srgb = op_color::Profile::builtin("srgb").map_err(codec)?;
                    return Ok((rgba8, Some(srgb.bytes().to_vec()), notice));
                }
                Err(e) => format!("the embedded ICC profile is not usable for CMYK ({e})"),
            }
        }
    };
    let mut rgba8 = Vec::with_capacity(ink.len() * 4);
    for &[c, m, y, k] in ink {
        let f = |v: u8| (((255 - v as u32) * (255 - k as u32) + 127) / 255) as u8;
        rgba8.extend_from_slice(&[f(c), f(m), f(y), 255]);
    }
    Ok((
        rgba8,
        None,
        format!(
            "CMYK image converted to sRGB with the naive formula R = (1 - C)(1 - K) because {problem}; colors are approximate"
        ),
    ))
}
fn encode_jpeg(image: &Decoded, opt: &Options) -> Result<Vec<u8>> {
    if !(1..=100).contains(&opt.jpeg_quality) {
        return Err(Error::Invalid("JPEG quality must be 1..100"));
    }
    let w = u16::try_from(image.width).map_err(|_| Error::Invalid("JPEG width"))?;
    let h = u16::try_from(image.height).map_err(|_| Error::Invalid("JPEG height"))?;
    let rgb: Vec<u8> = image
        .rgba8
        .chunks_exact(4)
        .flat_map(|p| p[..3].to_vec())
        .collect();
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, opt.jpeg_quality);
    enc.set_sampling_factor(match opt.jpeg_subsampling {
        ChromaSubsampling::S420 => jpeg_encoder::SamplingFactor::R_4_2_0,
        ChromaSubsampling::S422 => jpeg_encoder::SamplingFactor::R_4_2_2,
        ChromaSubsampling::S444 => jpeg_encoder::SamplingFactor::R_4_4_4,
    });
    if let Some(icc) = &image.icc {
        enc.add_icc_profile(icc).map_err(codec)?;
    }
    if let Some(exif) = &image.metadata.exif {
        let mut p = b"Exif\0\0".to_vec();
        p.extend_from_slice(exif);
        enc.add_app_segment(1, &p).map_err(codec)?;
    }
    if let Some(xmp) = &image.metadata.xmp {
        let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        p.extend_from_slice(xmp);
        enc.add_app_segment(1, &p).map_err(codec)?;
    }
    enc.encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .map_err(codec)?;
    Ok(out)
}
fn decode_gif(bytes: &[u8]) -> Result<Decoded> {
    let mut opt = gif::DecodeOptions::new();
    opt.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opt.read_info(Cursor::new(bytes)).map_err(codec)?;
    let (w, h) = (dec.width() as u32, dec.height() as u32);
    checked_len(w, h)?;
    let frame = dec
        .read_next_frame()
        .map_err(codec)?
        .ok_or(Error::Invalid("empty GIF"))?;
    let mut out = Decoded::new(w, h, vec![0; checked_len(w, h)?])?;
    for y in 0..frame.height as usize {
        for x in 0..frame.width as usize {
            let dx = x + frame.left as usize;
            let dy = y + frame.top as usize;
            if dx >= w as usize || dy >= h as usize {
                return Err(Error::Invalid("GIF frame bounds"));
            }
            let src = (y * frame.width as usize + x) * 4;
            let dst = (dy * w as usize + dx) * 4;
            out.rgba8[dst..dst + 4].copy_from_slice(&frame.buffer[src..src + 4]);
        }
    }
    Ok(out)
}
fn encode_gif(image: &Decoded) -> Result<Vec<u8>> {
    let w = u16::try_from(image.width).map_err(|_| Error::Invalid("GIF width"))?;
    let h = u16::try_from(image.height).map_err(|_| Error::Invalid("GIF height"))?;
    let transparent = image.rgba8.chunks_exact(4).any(|p| p[3] < 128);
    let first = usize::from(transparent);
    let max_colors = 256 - first;
    let mut exact = std::collections::BTreeSet::<[u8; 3]>::new();
    for p in image.rgba8.chunks_exact(4).filter(|p| p[3] >= 128) {
        exact.insert([p[0], p[1], p[2]]);
        if exact.len() > max_colors {
            break;
        }
    }
    let palette_colors: Vec<[u8; 3]> = if exact.len() <= max_colors {
        exact.into_iter().collect()
    } else {
        // Median cut over 5-bit color bins bounds memory and gives stable output.
        let mut bins = std::collections::BTreeMap::<u16, (u64, u64, u64, u64)>::new();
        for p in image.rgba8.chunks_exact(4).filter(|p| p[3] >= 128) {
            let key = ((p[0] as u16 >> 3) << 10) | ((p[1] as u16 >> 3) << 5) | (p[2] as u16 >> 3);
            let v = bins.entry(key).or_default();
            v.0 += p[0] as u64;
            v.1 += p[1] as u64;
            v.2 += p[2] as u64;
            v.3 += 1;
        }
        let colors: Vec<[u8; 3]> = bins
            .values()
            .map(|v| [(v.0 / v.3) as u8, (v.1 / v.3) as u8, (v.2 / v.3) as u8])
            .collect();
        let mut buckets = vec![colors];
        while buckets.len() < max_colors {
            let Some((index, axis)) = buckets
                .iter()
                .enumerate()
                .filter(|(_, b)| b.len() > 1)
                .map(|(i, b)| {
                    let ranges = std::array::from_fn::<_, 3, _>(|c| {
                        b.iter().map(|p| p[c]).max().unwrap()
                            - b.iter().map(|p| p[c]).min().unwrap()
                    });
                    let axis = (0..3).max_by_key(|&c| ranges[c]).unwrap();
                    (i, axis)
                })
                .max_by_key(|&(i, axis)| {
                    let b = &buckets[i];
                    b.iter().map(|p| p[axis]).max().unwrap()
                        - b.iter().map(|p| p[axis]).min().unwrap()
                })
            else {
                break;
            };
            buckets[index].sort_by_key(|p| (p[axis], p[(axis + 1) % 3], p[(axis + 2) % 3]));
            let midpoint = buckets[index].len() / 2;
            let other = buckets[index].split_off(midpoint);
            buckets.push(other);
        }
        buckets
            .iter()
            .map(|b| {
                std::array::from_fn(|c| {
                    (b.iter().map(|p| p[c] as u32).sum::<u32>() / b.len() as u32) as u8
                })
            })
            .collect()
    };
    let mut palette = if transparent {
        vec![0, 0, 0]
    } else {
        Vec::new()
    };
    for rgb in &palette_colors {
        palette.extend_from_slice(rgb);
    }
    if palette.is_empty() {
        palette.extend_from_slice(&[0, 0, 0]);
    }
    let mut indexed = Vec::with_capacity((w as usize) * (h as usize));
    let mut cache = std::collections::BTreeMap::<[u8; 3], u8>::new();
    for p in image.rgba8.chunks_exact(4) {
        if p[3] < 128 {
            indexed.push(0);
            continue;
        }
        let rgb = [p[0], p[1], p[2]];
        let index = *cache.entry(rgb).or_insert_with(|| {
            let (i, _) = palette_colors
                .iter()
                .enumerate()
                .min_by_key(|(_, q)| {
                    (0..3)
                        .map(|c| {
                            let d = rgb[c] as i32 - q[c] as i32;
                            (d * d) as u32
                        })
                        .sum::<u32>()
                })
                .unwrap();
            (i + first) as u8
        });
        indexed.push(index);
    }
    let frame = gif::Frame::from_palette_pixels(w, h, indexed, palette, transparent.then_some(0));
    let mut out = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut out, w, h, &[]).map_err(codec)?;
        enc.write_frame(&frame).map_err(codec)?;
    }
    Ok(out)
}
fn decode_tiff(bytes: &[u8]) -> Result<Decoded> {
    let mut dec = tiff::decoder::Decoder::new(Cursor::new(bytes)).map_err(codec)?;
    let (w, h) = dec.dimensions().map_err(codec)?;
    checked_len(w, h)?;
    let color = dec.colortype().map_err(codec)?;
    let channels = match color {
        tiff::ColorType::Gray(8 | 16) => 1,
        tiff::ColorType::GrayA(8 | 16) => 2,
        tiff::ColorType::RGB(8 | 16) => 3,
        tiff::ColorType::RGBA(8 | 16) => 4,
        _ => return Err(Error::Invalid("TIFF color type")),
    };
    let icc = dec
        .find_tag(tiff::tags::Tag::Unknown(34675))
        .ok()
        .flatten()
        .and_then(|v| v.into_u8_vec().ok());
    let mut notices = Vec::new();
    let exif = match tiff_exif(bytes) {
        Ok(exif) => exif,
        Err(e) => {
            notices.push(format!(
                "TIFF EXIF metadata could not be read and was dropped: {e}"
            ));
            None
        }
    };
    let xmp = dec
        .find_tag(tiff::tags::Tag::Unknown(700))
        .ok()
        .flatten()
        .and_then(|v| v.into_u8_vec().ok());
    let raw = match dec.read_image().map_err(codec)? {
        tiff::decoder::DecodingResult::U8(v) => v,
        tiff::decoder::DecodingResult::U16(v) => v.into_iter().map(round16).collect(),
        _ => return Err(Error::Invalid("TIFF sample type")),
    };
    let mut out = Decoded::new(w, h, expand(&raw, channels))?;
    out.icc = icc;
    out.metadata.exif = exif;
    out.metadata.xmp = xmp;
    out.notices = notices;
    Ok(out)
}
/// Build a standalone EXIF block from a TIFF file: the picture metadata tags
/// of IFD0 (`IFD0_METADATA`) plus the EXIF and GPS sub-directories.
fn tiff_exif(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let entries: Vec<_> = exif_ifd::parse(bytes)?
        .into_iter()
        .filter(|e| {
            exif_ifd::IFD0_METADATA.contains(&e.tag)
                || (matches!(e.tag, exif_ifd::EXIF_IFD | exif_ifd::GPS_IFD) && e.sub.is_some())
        })
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    exif_ifd::serialize(&entries).map(Some)
}

/// A TIFF value written with its original field type `K`.
/// `data` holds the value bytes in native order, as the tiff encoder expects.
struct RawValue<const K: u16> {
    count: usize,
    data: Vec<u8>,
}
const fn tiff_type(kind: u16) -> tiff::tags::Type {
    use tiff::tags::Type;
    match kind {
        1 => Type::BYTE,
        2 => Type::ASCII,
        3 => Type::SHORT,
        4 => Type::LONG,
        5 => Type::RATIONAL,
        6 => Type::SBYTE,
        7 => Type::UNDEFINED,
        8 => Type::SSHORT,
        9 => Type::SLONG,
        10 => Type::SRATIONAL,
        11 => Type::FLOAT,
        12 => Type::DOUBLE,
        _ => Type::IFD,
    }
}
const fn tiff_type_len(kind: u16) -> u8 {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        5 | 10 | 12 => 8,
        _ => 4,
    }
}
impl<const K: u16> tiff::encoder::TiffValue for RawValue<K> {
    const BYTE_LEN: u8 = tiff_type_len(K);
    const FIELD_TYPE: tiff::tags::Type = tiff_type(K);
    fn count(&self) -> usize {
        self.count
    }
    fn data(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.data)
    }
}
fn write_raw_tag<W: std::io::Write + std::io::Seek, T: tiff::encoder::TiffKind>(
    dir: &mut tiff::encoder::DirectoryEncoder<'_, W, T>,
    e: &exif_ifd::Entry,
) -> Result<()> {
    let mut data = e.data.clone();
    if cfg!(target_endian = "big") {
        let (_, unit) = exif_ifd::kind_size(e.kind).ok_or(Error::Invalid("EXIF field type"))?;
        for c in data.chunks_exact_mut(unit) {
            c.reverse();
        }
    }
    let tag = tiff::tags::Tag::Unknown(e.tag);
    let count = e.count as usize;
    macro_rules! put {
        ($($k:literal)*) => {
            match e.kind {
                $($k => dir.write_tag(tag, RawValue::<$k> { count, data }),)*
                _ => return Err(Error::Invalid("EXIF field type")),
            }
        };
    }
    put!(1 2 3 4 5 6 7 8 9 10 11 12).map_err(codec)
}
/// Write a sub-directory (and the sub-directories it points to) outside the
/// image chain and return its file offset.
fn write_tiff_subdir<W: std::io::Write + std::io::Seek>(
    enc: &mut tiff::encoder::TiffEncoder<W>,
    entries: &[exif_ifd::Entry],
) -> Result<u32> {
    let mut pointers = Vec::new();
    for e in entries {
        if let Some(sub) = &e.sub {
            pointers.push((e.tag, write_tiff_subdir(enc, sub)?));
        }
    }
    let mut dir = enc.extra_directory().map_err(codec)?;
    for e in entries.iter().filter(|e| e.sub.is_none()) {
        write_raw_tag(&mut dir, e)?;
    }
    for (tag, offset) in pointers {
        dir.write_tag(tiff::tags::Tag::Unknown(tag), offset)
            .map_err(codec)?;
    }
    let offsets = dir.finish_with_offsets().map_err(codec)?;
    u32::try_from(offsets.pointer.0).map_err(|_| Error::Invalid("TIFF offset"))
}
fn encode_tiff(image: &Decoded, opt: &Options) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let compression = match opt.tiff_compression {
        TiffCompression::None => tiff::encoder::Compression::Uncompressed,
        TiffCompression::Lzw => tiff::encoder::Compression::Lzw,
    };
    // EXIF goes the standard TIFF way: picture metadata tags join IFD0, and the
    // EXIF and GPS directories are written apart and pointed to by tags 34665
    // and 34853. Other IFD0 tags of the EXIF block (such as resolution) are
    // owned by the TIFF image itself and are not copied.
    let exif = match &image.metadata.exif {
        Some(bytes) => exif_ifd::parse(bytes)?,
        None => Vec::new(),
    };
    let mut enc = tiff::encoder::TiffEncoder::new(&mut out)
        .map_err(codec)?
        .with_compression(compression);
    let mut pointers = Vec::new();
    for e in &exif {
        if let (exif_ifd::EXIF_IFD | exif_ifd::GPS_IFD, Some(sub)) = (e.tag, &e.sub) {
            pointers.push((e.tag, write_tiff_subdir(&mut enc, sub)?));
        }
    }
    let mut img = enc
        .new_image::<tiff::encoder::colortype::RGBA8>(image.width, image.height)
        .map_err(codec)?;
    if let Some(icc) = &image.icc {
        img.encoder()
            .write_tag(tiff::tags::Tag::Unknown(34675), icc.as_slice())
            .map_err(codec)?;
    }
    for e in exif
        .iter()
        .filter(|e| e.sub.is_none() && exif_ifd::IFD0_METADATA.contains(&e.tag))
    {
        write_raw_tag(img.encoder(), e)?;
    }
    for (tag, offset) in pointers {
        img.encoder()
            .write_tag(tiff::tags::Tag::Unknown(tag), offset)
            .map_err(codec)?;
    }
    if let Some(xmp) = &image.metadata.xmp {
        img.encoder()
            .write_tag(tiff::tags::Tag::Unknown(700), xmp.as_slice())
            .map_err(codec)?;
    }
    img.write_data(&image.rgba8).map_err(codec)?;
    let _ = enc;
    Ok(out.into_inner())
}
fn decode_webp(bytes: &[u8]) -> Result<Decoded> {
    let mut dec = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(codec)?;
    let (w, h) = dec.dimensions();
    checked_len(w, h)?;
    let alpha = dec.has_alpha();
    let icc = dec.icc_profile().map_err(codec)?;
    let exif = dec.exif_metadata().map_err(codec)?;
    let xmp = dec.xmp_metadata().map_err(codec)?;
    let mut raw = vec![
        0;
        dec.output_buffer_size()
            .ok_or(Error::Invalid("WebP size"))?
    ];
    dec.read_image(&mut raw).map_err(codec)?;
    let mut out = Decoded::new(w, h, expand(&raw, if alpha { 4 } else { 3 }))?;
    out.icc = icc;
    out.metadata.exif = exif;
    out.metadata.xmp = xmp;
    Ok(out)
}
fn encode_webp(image: &Decoded) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = image_webp::WebPEncoder::new(&mut out);
    if let Some(icc) = &image.icc {
        enc.set_icc_profile(icc.clone());
    }
    if let Some(exif) = &image.metadata.exif {
        enc.set_exif_metadata(exif.clone());
    }
    if let Some(xmp) = &image.metadata.xmp {
        enc.set_xmp_metadata(xmp.clone());
    }
    enc.encode(
        &image.rgba8,
        image.width,
        image.height,
        image_webp::ColorType::Rgba8,
    )
    .map_err(codec)?;
    Ok(out)
}
fn orientation_offset(bytes: &[u8]) -> Option<(usize, bool)> {
    let b = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    let base = bytes.len() - b.len();
    let le = match b.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16at = |p: usize| -> Option<u16> {
        let x = b.get(p..p + 2)?;
        Some(if le {
            u16::from_le_bytes([x[0], x[1]])
        } else {
            u16::from_be_bytes([x[0], x[1]])
        })
    };
    let u32at = |p: usize| -> Option<u32> {
        let x = b.get(p..p + 4)?;
        Some(if le {
            u32::from_le_bytes(x.try_into().ok()?)
        } else {
            u32::from_be_bytes(x.try_into().ok()?)
        })
    };
    if u16at(2)? != 42 {
        return None;
    }
    let dir = u32at(4)? as usize;
    let count = u16at(dir)? as usize;
    for i in 0..count.min(1024) {
        let p = dir.checked_add(2 + i * 12)?;
        if u16at(p)? == 274 && u16at(p + 2)? == 3 && u32at(p + 4)? == 1 {
            return Some((base + p + 8, le));
        }
    }
    None
}
fn exif_orientation(bytes: &[u8]) -> Option<u16> {
    let exif = exif::Reader::new().read_raw(bytes.to_vec()).ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    let value = field.value.get_uint(0)? as u16;
    if (1..=8).contains(&value) {
        Some(value)
    } else {
        None
    }
}
fn reset_orientation(bytes: &mut [u8]) {
    if let Some((p, le)) = orientation_offset(bytes)
        && let Some(v) = bytes.get_mut(p..p + 2)
    {
        v.copy_from_slice(&if le {
            1u16.to_le_bytes()
        } else {
            1u16.to_be_bytes()
        });
    }
}
fn orient(image: &mut Decoded, orientation: u16) {
    let (w, h) = (image.width, image.height);
    let (nw, nh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut pixels = vec![0; image.rgba8.len()];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => (x, y),
            };
            let src = ((y * w + x) * 4) as usize;
            let dst = ((dy * nw + dx) * 4) as usize;
            pixels[dst..dst + 4].copy_from_slice(&image.rgba8[src..src + 4]);
        }
    }
    image.width = nw;
    image.height = nh;
    image.rgba8 = pixels;
}
