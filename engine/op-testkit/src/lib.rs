//! PNG fixtures, image comparisons, reviewed goldens, and test tolerances.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Component, Path, PathBuf};

/// An image with straight, eight-bit RGBA pixels in row-major order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn load_png(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref())
            .with_context(|| format!("open {}", path.as_ref().display()))?;
        let mut decoder = png::Decoder::new(BufReader::new(file));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer)?;
        let source = &buffer[..info.buffer_size()];
        let mut data = Vec::with_capacity(pixel_len(info.width, info.height)?);
        match info.color_type {
            png::ColorType::Rgba => data.extend_from_slice(source),
            png::ColorType::Rgb => {
                for pixel in source.chunks_exact(3) {
                    data.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
            png::ColorType::Grayscale => {
                for &gray in source {
                    data.extend_from_slice(&[gray, gray, gray, 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for pixel in source.chunks_exact(2) {
                    data.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
                }
            }
            other => bail!("unsupported decoded PNG color type: {other:?}"),
        }
        let image = Self {
            width: info.width,
            height: info.height,
            data,
        };
        image.validate()?;
        Ok(image)
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(path.as_ref())
            .with_context(|| format!("create {}", path.as_ref().display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.data)?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let wanted = pixel_len(self.width, self.height)?;
        if self.data.len() != wanted {
            bail!(
                "RGBA data length is {}, expected {wanted} for {}x{}",
                self.data.len(),
                self.width,
                self.height
            );
        }
        Ok(())
    }
}

fn pixel_len(width: u32, height: u32) -> Result<usize> {
    let len = u64::from(width) * u64::from(height) * 4;
    usize::try_from(len).context("image dimensions exceed addressable memory")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Tolerance {
    pub max_channel_delta: u8,
    pub max_pixels_over: u64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            max_channel_delta: 1,
            max_pixels_over: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub passed: bool,
    pub max_delta: u8,
    pub failing_pixels: u64,
    pub first_failures: Vec<(u32, u32)>,
    pub message: String,
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}; max delta {}, failing pixels {}, first failures {:?}",
            if self.passed { "PASS" } else { "FAIL" },
            self.message,
            self.max_delta,
            self.failing_pixels,
            self.first_failures
        )
    }
}

pub fn compare(expected: &RgbaImage, actual: &RgbaImage, tol: Tolerance) -> Comparison {
    if expected.width != actual.width || expected.height != actual.height {
        return Comparison {
            passed: false,
            max_delta: 0,
            failing_pixels: 0,
            first_failures: Vec::new(),
            message: format!(
                "size mismatch: expected {}x{}, actual {}x{}",
                expected.width, expected.height, actual.width, actual.height
            ),
        };
    }
    if let Err(error) = expected.validate().and_then(|_| actual.validate()) {
        return Comparison {
            passed: false,
            max_delta: 0,
            failing_pixels: 0,
            first_failures: Vec::new(),
            message: format!("invalid image: {error}"),
        };
    }
    let mut result = Comparison {
        passed: true,
        max_delta: 0,
        failing_pixels: 0,
        first_failures: Vec::new(),
        message: String::new(),
    };
    for (index, (left, right)) in expected
        .data
        .chunks_exact(4)
        .zip(actual.data.chunks_exact(4))
        .enumerate()
    {
        if left[3] == 0 && right[3] == 0 {
            continue;
        }
        let delta = (0..4)
            .map(|channel| left[channel].abs_diff(right[channel]))
            .max()
            .unwrap_or(0);
        result.max_delta = result.max_delta.max(delta);
        if delta > tol.max_channel_delta {
            result.failing_pixels += 1;
            if result.first_failures.len() < 10 {
                let position = index as u64;
                result.first_failures.push((
                    (position % u64::from(expected.width)) as u32,
                    (position / u64::from(expected.width)) as u32,
                ));
            }
        }
    }
    result.passed = result.failing_pixels <= tol.max_pixels_over;
    result.message = format!(
        "tolerance: channel delta ≤ {}, pixels over ≤ {}",
        tol.max_channel_delta, tol.max_pixels_over
    );
    result
}

/// Find the repository root by its `engine/` and `docs/` children.
pub fn repo_root() -> Result<PathBuf> {
    for parent in Path::new(env!("CARGO_MANIFEST_DIR")).ancestors() {
        if parent.join("engine").is_dir() && parent.join("docs").is_dir() {
            return Ok(parent.to_path_buf());
        }
    }
    bail!(
        "cannot find repository root above {}",
        env!("CARGO_MANIFEST_DIR")
    )
}

pub fn assert_golden(name: &str, actual: &RgbaImage, tol: Tolerance) {
    let relative = Path::new(name);
    assert!(
        !name.is_empty()
            && relative
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "golden name must be a nonempty relative path without traversal"
    );
    let root = repo_root().expect("locate repository root");
    let golden = root.join("tests/goldens").join(format!("{name}.png"));
    if !golden.exists() {
        if std::env::var("OP_BLESS").as_deref() == Ok("1") {
            actual.save_png(&golden).expect("write blessed golden");
            return;
        }
        panic!(
            "missing golden {}; create it with OP_BLESS=1 cargo test -p op-testkit",
            golden.display()
        );
    }
    let expected = RgbaImage::load_png(&golden).expect("load golden PNG");
    let comparison = compare(&expected, actual, tol);
    if comparison.passed {
        return;
    }
    let base = root.join("tests/out").join(relative);
    let stem = base.to_string_lossy();
    expected
        .save_png(format!("{stem}.expected.png"))
        .expect("write expected image");
    actual
        .save_png(format!("{stem}.actual.png"))
        .expect("write actual image");
    let diff = difference_image(&expected, actual);
    diff.save_png(format!("{stem}.diff.png"))
        .expect("write difference image");
    panic!("golden {name}: {comparison}");
}

fn difference_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    let width = expected.width.max(actual.width);
    let height = expected.height.max(actual.height);
    let mut data = vec![0; pixel_len(width, height).expect("diff dimensions")];
    for y in 0..height {
        for x in 0..width {
            let offset = ((u64::from(y) * u64::from(width) + u64::from(x)) * 4) as usize;
            let left = pixel_at(expected, x, y);
            let right = pixel_at(actual, x, y);
            for channel in 0..3 {
                data[offset + channel] = match (left, right) {
                    (Some(a), Some(b)) => a[channel].abs_diff(b[channel]).saturating_mul(16),
                    _ => 255,
                };
            }
            data[offset + 3] = 255;
        }
    }
    RgbaImage {
        width,
        height,
        data,
    }
}

fn pixel_at(image: &RgbaImage, x: u32, y: u32) -> Option<&[u8]> {
    if x >= image.width || y >= image.height {
        return None;
    }
    let offset = ((u64::from(y) * u64::from(image.width) + u64::from(x)) * 4) as usize;
    image.data.get(offset..offset + 4)
}

#[derive(Debug, Deserialize)]
pub struct Tolerances {
    #[serde(default)]
    pub tests: BTreeMap<String, ToleranceEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ToleranceEntry {
    pub max_channel_delta: u8,
    pub max_pixels_over: u64,
    pub reason: String,
}

impl Tolerances {
    pub fn load() -> Result<Self> {
        let path = repo_root()?.join("tests/tolerances.toml");
        Self::parse(&fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?)
    }

    pub fn parse(contents: &str) -> Result<Self> {
        let tolerances: Self = toml::from_str(contents).context("parse tolerances.toml")?;
        for (id, entry) in &tolerances.tests {
            if entry.reason.trim().is_empty() {
                bail!("tolerance {id} needs a reason");
            }
        }
        Ok(tolerances)
    }

    pub fn get(&self, id: &str) -> Tolerance {
        self.tests
            .get(id)
            .map_or_else(Tolerance::default, |entry| Tolerance {
                max_channel_delta: entry.max_channel_delta,
                max_pixels_over: entry.max_pixels_over,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixel: [u8; 4]) -> RgbaImage {
        RgbaImage {
            width: 1,
            height: 1,
            data: pixel.to_vec(),
        }
    }

    #[test]
    fn identical_images_pass() {
        let img = image([5, 8, 13, 255]);
        assert!(compare(&img, &img, Tolerance::default()).passed);
    }

    #[test]
    fn one_channel_step_passes_default_tolerance() {
        assert!(
            compare(
                &image([5, 8, 13, 255]),
                &image([6, 8, 13, 255]),
                Tolerance::default()
            )
            .passed
        );
    }

    #[test]
    fn two_channel_steps_fail_default_tolerance() {
        let result = compare(
            &image([5, 8, 13, 255]),
            &image([7, 8, 13, 255]),
            Tolerance::default(),
        );
        assert!(!result.passed);
        assert_eq!(result.max_delta, 2);
        assert_eq!(result.failing_pixels, 1);
        assert_eq!(result.first_failures, vec![(0, 0)]);
    }

    #[test]
    fn transparent_colors_are_ignored() {
        assert!(
            compare(
                &image([0, 0, 0, 0]),
                &image([255, 42, 19, 0]),
                Tolerance::default()
            )
            .passed
        );
    }

    #[test]
    fn size_mismatch_fails_without_panicking() {
        let result = compare(
            &image([0, 0, 0, 0]),
            &RgbaImage {
                width: 2,
                height: 1,
                data: vec![0; 8],
            },
            Tolerance::default(),
        );
        assert!(!result.passed);
        assert!(result.message.contains("size mismatch"));
    }

    #[test]
    fn tolerance_without_reason_is_rejected() {
        let error = Tolerances::parse("[tests.blur]\nmax_channel_delta = 2\nmax_pixels_over = 0\n")
            .unwrap_err();
        assert!(format!("{error:#}").contains("missing field `reason`"));
    }

    #[test]
    fn png_round_trip_with_relative_filename() {
        let original_dir = std::env::current_dir().unwrap();
        let scratch = std::env::temp_dir().join(format!("op-testkit-{}", std::process::id()));
        fs::create_dir_all(&scratch).unwrap();
        std::env::set_current_dir(&scratch).unwrap();
        let path = "round-trip.png";
        let image = image([12, 34, 56, 78]);
        image.save_png(path).unwrap();
        let loaded = RgbaImage::load_png(path).unwrap();
        fs::remove_file(path).unwrap();
        std::env::set_current_dir(original_dir).unwrap();
        fs::remove_dir(scratch).unwrap();
        assert_eq!(loaded, image);
    }
}
