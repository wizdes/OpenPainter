//! Decodes every committed file in `tests/fixtures/io` (repo root).
//! Regenerate the files with `cargo run -p op-formats --example make_corpus`.
mod common;
use common::{assert_close, cmyk_icc, exif_text, naive, patch_pixels, through_op_color};
use op_formats::{Decoded, decode};
use std::path::PathBuf;

const R: [u8; 4] = [255, 0, 0, 255];
const G: [u8; 4] = [0, 255, 0, 255];
const B: [u8; 4] = [0, 0, 255, 255];
const W: [u8; 4] = [255, 255, 255, 255];

const FILES: [&str; 24] = [
    "png_rgba16.png",
    "png_palette_trns.png",
    "png_gray_alpha.png",
    "jpeg_baseline.jpg",
    "jpeg_progressive.jpg",
    "jpeg_cmyk_icc.jpg",
    "jpeg_ycck_icc.jpg",
    "jpeg_cmyk_no_icc.jpg",
    "jpeg_exif_orientation6.jpg",
    "bmp_1bit.bmp",
    "bmp_4bit.bmp",
    "bmp_8bit.bmp",
    "bmp_rle8.bmp",
    "bmp_rle4.bmp",
    "bmp_24bit_topdown.bmp",
    "bmp_16bit_bitfields.bmp",
    "bmp_24bit.bmp",
    "bmp_32bit.bmp",
    "gif_transparent_2frames.gif",
    "tiff_rgb8_lzw.tif",
    "tiff_rgb16.tif",
    "tiff_exif_icc.tif",
    "webp_lossless.webp",
    "webp_lossy.webp",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/io")
}

fn load(name: &str) -> Decoded {
    let bytes = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn pixels(image: &Decoded) -> Vec<[u8; 4]> {
    image
        .rgba8
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn pixel(image: &Decoded, x: u32, y: u32) -> [u8; 4] {
    let p = ((y * image.width + x) * 4) as usize;
    image.rgba8[p..p + 4].try_into().unwrap()
}

fn icc_description(image: &Decoded) -> Option<String> {
    image.icc.as_ref().map(|icc| {
        op_color::Profile::from_icc(icc)
            .unwrap()
            .description()
            .to_string()
    })
}

fn exif(image: &Decoded) -> exif::Exif {
    exif::Reader::new()
        .read_raw(image.metadata.exif.clone().expect("EXIF block"))
        .unwrap()
}

/// The four 8x8 squares of the 16x16 test picture: red, green / blue, white.
fn assert_quadrants(image: &Decoded, tolerance: i32, label: &str) {
    assert_eq!((image.width, image.height), (16, 16), "{label}");
    let got = [(4, 4), (12, 4), (4, 12), (12, 12)].map(|(x, y)| pixel(image, x, y));
    assert_close(&got, &[R, G, B, W], tolerance, label);
}

#[test]
fn corpus_is_complete_and_small() {
    let mut total = 0;
    for name in FILES {
        total += std::fs::metadata(dir().join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .len();
    }
    assert!(total < 3 * 1024 * 1024, "corpus is {total} bytes");
}

#[test]
fn png_variants() {
    let image = load("png_rgba16.png");
    assert_eq!((image.width, image.height), (2, 2));
    // floor(v * 255 / 65535 + 0.5) per sample.
    assert_eq!(
        pixels(&image),
        [
            [128, 0, 255, 0],
            [18, 86, 154, 255],
            [0, 1, 127, 255],
            [254, 1, 64, 128]
        ]
    );
    assert_eq!(icc_description(&image).as_deref(), Some("sRGB"));

    let image = load("png_palette_trns.png");
    assert_eq!(
        pixels(&image),
        [
            [255, 0, 0, 0],
            [0, 255, 0, 128],
            [0, 0, 255, 255],
            [255, 255, 0, 255]
        ]
    );

    let image = load("png_gray_alpha.png");
    assert_eq!(
        pixels(&image),
        [
            [0, 0, 0, 255],
            [85, 85, 85, 170],
            [170, 170, 170, 85],
            [255, 255, 255, 0]
        ]
    );
}

#[test]
fn jpeg_variants() {
    let image = load("jpeg_baseline.jpg");
    assert_quadrants(&image, 4, "baseline");
    assert_eq!(icc_description(&image).as_deref(), Some("sRGB"));
    assert!(image.notices.is_empty(), "{:?}", image.notices);

    let image = load("jpeg_progressive.jpg");
    assert_quadrants(&image, 6, "progressive");
    assert_eq!(image.icc, None);

    let want = through_op_color(&cmyk_icc());
    for (name, tolerance) in [("jpeg_cmyk_icc.jpg", 3), ("jpeg_ycck_icc.jpg", 8)] {
        let image = load(name);
        assert_eq!((image.width, image.height), (32, 8), "{name}");
        assert_close(&patch_pixels(&image.rgba8), &want, tolerance, name);
        assert_eq!(icc_description(&image).as_deref(), Some("sRGB"), "{name}");
        assert!(image.notices[0].contains("perceptual"), "{name}");
    }

    let image = load("jpeg_cmyk_no_icc.jpg");
    assert_close(&patch_pixels(&image.rgba8), &naive(), 3, "cmyk no icc");
    assert!(image.notices[0].contains("no ICC profile"));

    // Stored 16x8 (left red, right blue) with orientation 6: shown 8x16,
    // red on top and blue below.
    let image = load("jpeg_exif_orientation6.jpg");
    assert_eq!((image.width, image.height), (8, 16));
    assert_close(
        &[pixel(&image, 4, 3), pixel(&image, 4, 12)],
        &[R, B],
        4,
        "orientation 6",
    );
    let exif = exif(&image);
    assert_eq!(
        exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0)),
        Some(1)
    );
    assert_eq!(
        exif_text(&exif, exif::Tag::Make).as_deref(),
        Some("\"OpenPainter\"")
    );
}

#[test]
fn bmp_variants() {
    let image = load("bmp_1bit.bmp");
    assert_eq!((image.width, image.height), (4, 2));
    assert_eq!(pixels(&image), [R, R, G, G, R, G, R, G]);
    for name in [
        "bmp_4bit.bmp",
        "bmp_8bit.bmp",
        "bmp_rle8.bmp",
        "bmp_rle4.bmp",
    ] {
        let image = load(name);
        assert_eq!((image.width, image.height), (4, 2), "{name}");
        assert_eq!(pixels(&image), [R, G, B, W, B, W, R, G], "{name}");
    }
    assert_eq!(pixels(&load("bmp_24bit_topdown.bmp")), [R, G, B, W]);
    assert_eq!(pixels(&load("bmp_16bit_bitfields.bmp")), [R, B]);
    assert_eq!(
        pixels(&load("bmp_24bit.bmp")),
        [
            [0, 255, 7, 255],
            [60, 195, 7, 255],
            [120, 135, 7, 255],
            [180, 75, 7, 255]
        ]
    );
    assert_eq!(
        pixels(&load("bmp_32bit.bmp")),
        [
            [0, 255, 7, 255],
            [60, 195, 7, 255],
            [120, 135, 7, 128],
            [180, 75, 7, 0]
        ]
    );
}

#[test]
fn gif_first_frame_with_transparency() {
    let image = load("gif_transparent_2frames.gif");
    assert_eq!((image.width, image.height), (4, 2));
    let p = pixels(&image);
    assert_eq!(&p[..3], &[R, G, B]);
    assert_eq!(p[3][3], 0, "transparent index");
    assert_eq!(p[4][3], 0, "transparent index");
    assert_eq!(&p[5..], &[B, G, R]);
}

#[test]
fn tiff_variants() {
    assert_eq!(pixels(&load("tiff_rgb8_lzw.tif")), [R, G, B]);
    assert_eq!(
        pixels(&load("tiff_rgb16.tif")),
        [[255, 128, 0, 255], [18, 0, 254, 255]]
    );
    let image = load("tiff_exif_icc.tif");
    assert_eq!(pixels(&image), [[10, 20, 30, 255], [40, 50, 60, 128]]);
    assert_eq!(icc_description(&image).as_deref(), Some("sRGB"));
    let exif = exif(&image);
    assert_eq!(
        exif_text(&exif, exif::Tag::DateTimeOriginal).as_deref(),
        Some("2026-10-24 12:34:56")
    );
}

#[test]
fn webp_variants() {
    let image = load("webp_lossless.webp");
    assert_eq!(pixel(&image, 0, 0), [255, 0, 0, 0], "hidden color kept");
    assert_quadrants(
        &Decoded {
            rgba8: {
                let mut v = image.rgba8.clone();
                v[3] = 255;
                v
            },
            ..image.clone()
        },
        0,
        "lossless",
    );
    assert_eq!(icc_description(&image).as_deref(), Some("sRGB"));
    assert_eq!(
        exif_text(&exif(&image), exif::Tag::Make).as_deref(),
        Some("\"OpenPainter\"")
    );

    let image = load("webp_lossy.webp");
    assert_quadrants(&image, 12, "lossy");
}
