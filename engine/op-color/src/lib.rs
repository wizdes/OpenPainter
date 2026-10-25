//! ICC profiles, transforms, and a document-to-display lookup table.
mod hash;
use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Flags, InfoType, Intent, Locale, PixelFormat,
    ToneCurve, Transform,
};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use thiserror::Error;
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid ICC profile: {0}")]
    InvalidProfile(String),
    #[error("unsupported profile color space")]
    ColorSpace,
    #[error("invalid pixel buffer")]
    Pixels,
    #[error("color transform failed: {0}")]
    Transform(String),
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug)]
pub struct Profile {
    bytes: Arc<[u8]>,
    hash: [u8; 32],
    description: String,
    space: ColorSpaceSignature,
}
impl Profile {
    pub fn from_icc(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 132 || bytes.len() > 16 * 1024 * 1024 || &bytes[36..40] != b"acsp" {
            return Err(Error::InvalidProfile("bad ICC header or size".into()));
        }
        if u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize != bytes.len() {
            return Err(Error::InvalidProfile(
                "ICC size does not match header".into(),
            ));
        }
        let count = u32::from_be_bytes(bytes[128..132].try_into().unwrap()) as usize;
        if count == 0 {
            return Err(Error::InvalidProfile("ICC profile has no tags".into()));
        }
        let table_end = count
            .checked_mul(12)
            .and_then(|n| n.checked_add(132))
            .ok_or_else(|| Error::InvalidProfile("ICC tag table overflows".into()))?;
        if table_end > bytes.len() {
            return Err(Error::InvalidProfile("ICC tag table is truncated".into()));
        }
        for tag in bytes[132..table_end].chunks_exact(12) {
            let offset = u32::from_be_bytes(tag[4..8].try_into().unwrap()) as usize;
            let length = u32::from_be_bytes(tag[8..12].try_into().unwrap()) as usize;
            if offset < table_end
                || length == 0
                || offset
                    .checked_add(length)
                    .is_none_or(|end| end > bytes.len())
            {
                return Err(Error::InvalidProfile("ICC tag points outside data".into()));
            }
            // lcms2 allocates and zeroes 12 bytes per claimed mluc record before
            // it reads any, so a tiny tag can claim gigabytes of records.
            let data = &bytes[offset..offset + length];
            if data.starts_with(b"mluc") {
                let records = data.get(8..12).map_or(u64::MAX, |n| {
                    u64::from(u32::from_be_bytes(n.try_into().unwrap()))
                });
                if records.saturating_mul(12).saturating_add(16) > length as u64 {
                    return Err(Error::InvalidProfile(
                        "ICC mluc tag claims more records than it holds".into(),
                    ));
                }
            }
        }
        let p = lcms2::Profile::new_icc(bytes).map_err(|e| Error::InvalidProfile(e.to_string()))?;
        let hash = hash::sha256(bytes);
        let description = p
            .info(InfoType::Description, Locale::none())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Embedded ICC profile".into());
        Ok(Self {
            bytes: Arc::from(bytes),
            hash,
            description,
            space: p.color_space(),
        })
    }
    pub fn builtin(name: &str) -> Result<Self> {
        let d65 = xy(0.3127, 0.3290);
        let d50 = xy(0.34567, 0.35850);
        let (mut p, name) = match name.to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
            "srgb" => (lcms2::Profile::new_srgb(), "sRGB"),
            "display-p3" | "p3" => {
                let c = ToneCurve::new_parametric(
                    4,
                    &[
                        2.4,
                        1.0 / 1.055,
                        0.055 / 1.055,
                        1.0 / 12.92,
                        0.04045,
                        0.0,
                        0.0,
                    ],
                )
                .map_err(invalid)?;
                (
                    rgb(&d65, [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)], &c)?,
                    "Display P3",
                )
            }
            "adobe-rgb" | "adobe-rgb-1998" => {
                let c = ToneCurve::new(563.0 / 256.0);
                (
                    rgb(&d65, [(0.640, 0.330), (0.210, 0.710), (0.150, 0.060)], &c)?,
                    "Adobe RGB (1998)",
                )
            }
            "prophoto" | "prophoto-rgb" => {
                let c = ToneCurve::new_parametric(
                    4,
                    &[1.8, 1.0, 0.0, 1.0 / 16.0, 16.0 / 512.0, 0.0, 0.0],
                )
                .map_err(invalid)?;
                (
                    rgb(
                        &d50,
                        [(0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)],
                        &c,
                    )?,
                    "ProPhoto RGB",
                )
            }
            "gray-2.2" | "gray-gamma-2.2" => (
                lcms2::Profile::new_gray(&d50, &ToneCurve::new(2.2)).map_err(invalid)?,
                "Gray gamma 2.2",
            ),
            _ => {
                return Err(Error::InvalidProfile(format!(
                    "unknown built-in profile {name}"
                )));
            }
        };
        let mut label = lcms2::MLU::new(1);
        if !label.set_text_ascii(name, Locale::new("en_US"))
            || !p.write_tag(
                lcms2::TagSignature::ProfileDescriptionTag,
                lcms2::Tag::MLU(&label),
            )
        {
            return Err(Error::InvalidProfile("could not label built-in ICC".into()));
        }
        Self::from_icc(&p.icc().map_err(invalid)?)
    }
    pub fn bytes(&self) -> Arc<[u8]> {
        self.bytes.clone()
    }
    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn color_space(&self) -> ColorSpaceSignature {
        self.space
    }
    fn open(&self) -> Result<lcms2::Profile> {
        lcms2::Profile::new_icc(&self.bytes).map_err(invalid)
    }
}
fn invalid(e: impl std::fmt::Display) -> Error {
    Error::InvalidProfile(e.to_string())
}
fn xy(x: f64, y: f64) -> CIExyY {
    CIExyY { x, y, Y: 1.0 }
}
fn rgb(w: &CIExyY, p: [(f64, f64); 3], c: &ToneCurve) -> Result<lcms2::Profile> {
    let p = CIExyYTRIPLE {
        Red: xy(p[0].0, p[0].1),
        Green: xy(p[1].0, p[1].1),
        Blue: xy(p[2].0, p[2].1),
    };
    lcms2::Profile::new_rgb(w, &p, &[c, c, c]).map_err(invalid)
}
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum RenderingIntent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
}
impl RenderingIntent {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "perceptual" => Ok(Self::Perceptual),
            "relative" => Ok(Self::Relative),
            "saturation" => Ok(Self::Saturation),
            "absolute" => Ok(Self::Absolute),
            _ => Err(Error::Transform(format!("unknown intent {s}"))),
        }
    }
    fn lcms(self) -> Intent {
        match self {
            Self::Perceptual => Intent::Perceptual,
            Self::Relative => Intent::RelativeColorimetric,
            Self::Saturation => Intent::Saturation,
            Self::Absolute => Intent::AbsoluteColorimetric,
        }
    }
}
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Key {
    src: [u8; 32],
    dst: [u8; 32],
    intent: RenderingIntent,
    bpc: bool,
    input: u32,
    output: u32,
}
enum CachedTransform {
    Rgb(Transform<[u8; 3], [u8; 3]>),
    Gray(Transform<u8, [u8; 3]>),
    Cmyk(Transform<[u8; 4], [u8; 3]>),
}
struct Cache {
    map: HashMap<Key, CachedTransform>,
    order: VecDeque<Key>,
}
impl Cache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }
    fn insert(&mut self, key: Key, transform: CachedTransform) {
        if self.order.len() == 32
            && let Some(old) = self.order.pop_front()
        {
            self.map.remove(&old);
        }
        self.order.push_back(key);
        self.map.insert(key, transform);
    }
    fn rgb(
        &mut self,
        src: &Profile,
        dst: &Profile,
        intent: RenderingIntent,
        bpc: bool,
        pixels: &[[u8; 3]],
    ) -> Result<Vec<[u8; 3]>> {
        if src.space != ColorSpaceSignature::RgbData || dst.space != ColorSpaceSignature::RgbData {
            return Err(Error::ColorSpace);
        }
        let key = Key {
            src: src.hash,
            dst: dst.hash,
            intent,
            bpc,
            input: PixelFormat::RGB_8.0,
            output: PixelFormat::RGB_8.0,
        };
        if !self.map.contains_key(&key) {
            let flags = if bpc {
                Flags::BLACKPOINT_COMPENSATION
            } else {
                Flags::default()
            };
            let t = Transform::new_flags(
                &src.open()?,
                PixelFormat::RGB_8,
                &dst.open()?,
                PixelFormat::RGB_8,
                intent.lcms(),
                flags,
            )
            .map_err(|e| Error::Transform(e.to_string()))?;
            self.insert(key, CachedTransform::Rgb(t));
        }
        let mut out = vec![[0; 3]; pixels.len()];
        match self.map.get(&key) {
            Some(CachedTransform::Rgb(t)) => t.transform_pixels(pixels, &mut out),
            _ => return Err(Error::Transform("RGB cache entry mismatch".into())),
        }
        Ok(out)
    }
    fn gray(&mut self, src: &Profile, dst: &Profile, gray: &[u8]) -> Result<Vec<[u8; 3]>> {
        let key = Key {
            src: src.hash,
            dst: dst.hash,
            intent: RenderingIntent::Perceptual,
            bpc: true,
            input: PixelFormat::GRAY_8.0,
            output: PixelFormat::RGB_8.0,
        };
        if !self.map.contains_key(&key) {
            let t = Transform::new_flags(
                &src.open()?,
                PixelFormat::GRAY_8,
                &dst.open()?,
                PixelFormat::RGB_8,
                Intent::Perceptual,
                Flags::BLACKPOINT_COMPENSATION,
            )
            .map_err(|e| Error::Transform(e.to_string()))?;
            self.insert(key, CachedTransform::Gray(t));
        }
        let mut out = vec![[0; 3]; gray.len()];
        match self.map.get(&key) {
            Some(CachedTransform::Gray(t)) => t.transform_pixels(gray, &mut out),
            _ => return Err(Error::Transform("gray cache entry mismatch".into())),
        }
        Ok(out)
    }
    fn cmyk(&mut self, src: &Profile, dst: &Profile, cmyk: &[[u8; 4]]) -> Result<Vec<[u8; 3]>> {
        let key = Key {
            src: src.hash,
            dst: dst.hash,
            intent: RenderingIntent::Perceptual,
            bpc: true,
            input: PixelFormat::CMYK_8.0,
            output: PixelFormat::RGB_8.0,
        };
        if !self.map.contains_key(&key) {
            let t = Transform::new_flags(
                &src.open()?,
                PixelFormat::CMYK_8,
                &dst.open()?,
                PixelFormat::RGB_8,
                Intent::Perceptual,
                Flags::BLACKPOINT_COMPENSATION,
            )
            .map_err(|e| Error::Transform(e.to_string()))?;
            self.insert(key, CachedTransform::Cmyk(t));
        }
        let mut out = vec![[0; 3]; cmyk.len()];
        match self.map.get(&key) {
            Some(CachedTransform::Cmyk(t)) => t.transform_pixels(cmyk, &mut out),
            _ => return Err(Error::Transform("CMYK cache entry mismatch".into())),
        }
        Ok(out)
    }
}
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache::new());}
pub fn convert_rgb(
    src: &Profile,
    dst: &Profile,
    intent: RenderingIntent,
    bpc: bool,
    pixels: &[[u8; 3]],
) -> Result<Vec<[u8; 3]>> {
    CACHE.with(|c| c.borrow_mut().rgb(src, dst, intent, bpc, pixels))
}
pub fn convert_rgba(
    src: &Profile,
    dst: &Profile,
    intent: RenderingIntent,
    bpc: bool,
    rgba: &mut [u8],
) -> Result<()> {
    if !rgba.len().is_multiple_of(4) {
        return Err(Error::Pixels);
    }
    let input: Vec<_> = rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect();
    let output = convert_rgb(src, dst, intent, bpc, &input)?;
    for (p, rgb) in rgba.chunks_exact_mut(4).zip(output) {
        p[..3].copy_from_slice(&rgb)
    }
    Ok(())
}
pub fn expand_gray(
    gray: &[u8],
    alpha: Option<&[u8]>,
    src: &Profile,
    dst: &Profile,
) -> Result<Vec<u8>> {
    if src.space != ColorSpaceSignature::GrayData
        || dst.space != ColorSpaceSignature::RgbData
        || alpha.is_some_and(|a| a.len() != gray.len())
    {
        return Err(Error::Pixels);
    }
    let out = CACHE.with(|c| c.borrow_mut().gray(src, dst, gray))?;
    let mut rgba = Vec::with_capacity(gray.len() * 4);
    for (i, p) in out.iter().enumerate() {
        rgba.extend_from_slice(&[p[0], p[1], p[2], alpha.map_or(255, |a| a[i])])
    }
    Ok(rgba)
}
pub fn import_cmyk(cmyk: &[[u8; 4]], src: &Profile) -> Result<(Vec<u8>, String)> {
    if src.space != ColorSpaceSignature::CmykData {
        return Err(Error::ColorSpace);
    }
    let dst = Profile::builtin("srgb")?;
    let out = CACHE.with(|c| c.borrow_mut().cmyk(src, &dst, cmyk))?;
    let mut rgba = Vec::with_capacity(cmyk.len() * 4);
    for p in out {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 255])
    }
    Ok((
        rgba,
        "CMYK image converted to sRGB using perceptual intent and black point compensation".into(),
    ))
}
pub struct DisplayLut {
    values: Vec<[f32; 3]>,
}
impl DisplayLut {
    pub fn build(src: &Profile, dst: &Profile) -> Result<Self> {
        let mut input = Vec::with_capacity(33 * 33 * 33);
        for r in 0..33 {
            for g in 0..33 {
                for b in 0..33 {
                    input.push([
                        ((r * 255 + 16) / 32) as u8,
                        ((g * 255 + 16) / 32) as u8,
                        ((b * 255 + 16) / 32) as u8,
                    ])
                }
            }
        }
        let output = convert_rgb(src, dst, RenderingIntent::Relative, true, &input)?;
        Ok(Self {
            values: output
                .into_iter()
                .map(|p| {
                    [
                        p[0] as f32 / 255.0,
                        p[1] as f32 / 255.0,
                        p[2] as f32 / 255.0,
                    ]
                })
                .collect(),
        })
    }
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let p = rgb.map(|v| (v.clamp(0.0, 1.0) * 32.0).min(31.99999));
        let lo = p.map(|v| v.floor() as usize);
        let t = p.map(|v| v - v.floor());
        let mut out = [0.0; 3];
        for dr in 0..2 {
            for dg in 0..2 {
                for db in 0..2 {
                    let w = (if dr == 0 { 1.0 - t[0] } else { t[0] })
                        * (if dg == 0 { 1.0 - t[1] } else { t[1] })
                        * (if db == 0 { 1.0 - t[2] } else { t[2] });
                    let v = self.values[((lo[0] + dr) * 33 + lo[1] + dg) * 33 + lo[2] + db];
                    for c in 0..3 {
                        out[c] += w * v[c]
                    }
                }
            }
        }
        out
    }
}
/// lcms2 float transform of RGB colors (relative colorimetric, no black point
/// compensation), clamped to 0..1. Both profiles must be RGB.
pub fn convert_rgb_float(src: &Profile, dst: &Profile, rgb: &[[f32; 3]]) -> Result<Vec<[f32; 3]>> {
    if src.space != ColorSpaceSignature::RgbData || dst.space != ColorSpaceSignature::RgbData {
        return Err(Error::ColorSpace);
    }
    let t: Transform<[f32; 3], [f32; 3]> = Transform::new(
        &src.open()?,
        PixelFormat::RGB_FLT,
        &dst.open()?,
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
    )
    .map_err(|e| Error::Transform(e.to_string()))?;
    let mut out = vec![[0f32; 3]; rgb.len()];
    t.transform_pixels(rgb, &mut out);
    Ok(out
        .into_iter()
        .map(|c| c.map(|v| v.clamp(0.0, 1.0)))
        .collect())
}
/// The document -> display table the canvas presenter uses (M09 decision): an
/// `n`-per-axis grid of float outputs from [`convert_rgb_float`]. Entry
/// `r + g * n + b * n * n` holds the output for the input `(r, g, b) / (n - 1)`.
pub fn display_table(src: &Profile, dst: &Profile, n: u32) -> Result<Vec<[f32; 3]>> {
    if !(2..=129).contains(&n) {
        return Err(Error::Pixels);
    }
    let step = 1.0 / (n - 1) as f32;
    let mut input = Vec::with_capacity((n * n * n) as usize);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                input.push([r as f32 * step, g as f32 * step, b as f32 * step]);
            }
        }
    }
    convert_rgb_float(src, dst, &input)
}
