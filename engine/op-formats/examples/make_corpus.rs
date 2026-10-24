//! Writes the image corpus in `tests/fixtures/io` (repo root).
//!
//! Run: `cargo run -p op-formats --example make_corpus`
//! Most files come from our own encoders, the codec crates, or hand-built
//! bytes. `webp_lossy.webp` needs `cwebp` on PATH; when it is missing the
//! example says so and leaves any existing file alone.
#[path = "../tests/common/mod.rs"]
mod common;

use op_formats::{Decoded, Format, Metadata, Options, TiffCompression, encode};
use std::{fs, path::PathBuf, process::Command};

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/io")
}

fn write(name: &str, bytes: &[u8]) {
    fs::write(out_dir().join(name), bytes).unwrap();
    println!("wrote {name} ({} bytes)", bytes.len());
}

fn srgb() -> Vec<u8> {
    op_color::Profile::builtin("srgb").unwrap().bytes().to_vec()
}

fn image(width: u32, height: u32, rgba8: Vec<u8>) -> Decoded {
    Decoded {
        width,
        height,
        rgba8,
        icc: None,
        metadata: Metadata::default(),
        notices: vec![],
    }
}

/// A 16x16 picture of four flat 8x8 squares: red, green / blue, white.
fn quadrants() -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            v.extend_from_slice(match (x < 8, y < 8) {
                (true, true) => &[255, 0, 0, 255],
                (false, true) => &[0, 255, 0, 255],
                (true, false) => &[0, 0, 255, 255],
                (false, false) => &[255, 255, 255, 255],
            });
        }
    }
    v
}

fn png_files() {
    // 16-bit RGBA, 2x2, with an sRGB ICC profile.
    let mut bytes = Vec::new();
    let mut info = png::Info::with_size(2, 2);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Sixteen;
    info.icc_profile = Some(srgb().into());
    let enc = png::Encoder::with_info(&mut bytes, info).unwrap();
    let samples: [u16; 16] = [
        0x8000, 0x0000, 0xffff, 0x0001, 0x1234, 0x5678, 0x9abc, 0xffff, 0x0080, 0x0081, 0x7fff,
        0xff7f, 0xfeff, 0x0101, 0x4000, 0x8080,
    ];
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()).collect();
    let mut w = enc.write_header().unwrap();
    w.write_image_data(&data).unwrap();
    drop(w);
    write("png_rgba16.png", &bytes);

    // 8-bit palette with tRNS, 4x1.
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, 4, 1);
    enc.set_color(png::ColorType::Indexed);
    enc.set_palette(vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]);
    enc.set_trns(vec![0, 128, 255]);
    let mut w = enc.write_header().unwrap();
    w.write_image_data(&[0, 1, 2, 3]).unwrap();
    drop(w);
    write("png_palette_trns.png", &bytes);

    // 8-bit gray + alpha, 4x1.
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, 4, 1);
    enc.set_color(png::ColorType::GrayscaleAlpha);
    let mut w = enc.write_header().unwrap();
    w.write_image_data(&[0, 255, 85, 170, 170, 85, 255, 0])
        .unwrap();
    drop(w);
    write("png_gray_alpha.png", &bytes);
}

fn jpeg_files() {
    let mut base = image(16, 16, quadrants());
    base.icc = Some(srgb());
    let options = Options {
        jpeg_quality: 95,
        jpeg_subsampling: op_formats::ChromaSubsampling::S444,
        ..Options::default()
    };
    write(
        "jpeg_baseline.jpg",
        &encode(Format::Jpeg, &base, &options).unwrap(),
    );

    let rgb: Vec<u8> = quadrants()
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut bytes = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut bytes, 95);
    enc.set_progressive(true);
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    enc.encode(&rgb, 16, 16, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    write("jpeg_progressive.jpg", &bytes);

    let icc = common::cmyk_icc();
    write(
        "jpeg_cmyk_icc.jpg",
        &common::cmyk_jpeg(common::Ink::Cmyk, Some(&icc)),
    );
    write(
        "jpeg_ycck_icc.jpg",
        &common::cmyk_jpeg(common::Ink::Ycck, Some(&icc)),
    );
    write(
        "jpeg_cmyk_no_icc.jpg",
        &common::cmyk_jpeg(common::Ink::Cmyk, None),
    );

    // 16x8: left half red, right half blue, stored with EXIF orientation 6.
    let mut px = Vec::new();
    for _ in 0..8 {
        for x in 0..16 {
            px.extend_from_slice(if x < 8 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    let mut rotated = image(16, 8, px);
    rotated.metadata.exif = Some(common::exif_block(6));
    write(
        "jpeg_exif_orientation6.jpg",
        &encode(Format::Jpeg, &rotated, &options).unwrap(),
    );
}

/// A BMP with a 40-byte info header. `height` < 0 means top-down rows.
fn bmp(
    width: i32,
    height: i32,
    depth: u16,
    compression: u32,
    extra: &[u8],
    pixels: &[u8],
) -> Vec<u8> {
    let off = 54 + extra.len();
    let mut b = vec![0u8; off];
    b[..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&((off + pixels.len()) as u32).to_le_bytes());
    b[10..14].copy_from_slice(&(off as u32).to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&width.to_le_bytes());
    b[22..26].copy_from_slice(&height.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&depth.to_le_bytes());
    b[30..34].copy_from_slice(&compression.to_le_bytes());
    b[34..38].copy_from_slice(&(pixels.len() as u32).to_le_bytes());
    if depth <= 8 {
        b[46..50].copy_from_slice(&((extra.len() / 4) as u32).to_le_bytes());
    }
    b[54..].copy_from_slice(extra);
    b.extend_from_slice(pixels);
    b
}

fn bmp_files() {
    // Palette entries are B, G, R, 0: red, green, blue, white.
    let pal: Vec<u8> = [
        [0, 0, 255, 0],
        [0, 255, 0, 0],
        [255, 0, 0, 0],
        [255, 255, 255, 0],
    ]
    .concat();
    // The 4 and 8-bit files are 4x2. Bottom row first: [blue, white, red,
    // green], then the top row [red, green, blue, white]. The 1-bit file
    // uses red and green only: bottom [R, G, R, G], top [R, R, G, G].
    write(
        "bmp_1bit.bmp",
        &bmp(
            4,
            2,
            1,
            0,
            &pal[..8],
            &[0b0101_0000, 0, 0, 0, 0b0011_0000, 0, 0, 0],
        ),
    );
    write(
        "bmp_4bit.bmp",
        &bmp(4, 2, 4, 0, &pal, &[0x23, 0x01, 0, 0, 0x01, 0x23, 0, 0]),
    );
    write(
        "bmp_8bit.bmp",
        &bmp(4, 2, 8, 0, &pal, &[2, 3, 0, 1, 0, 1, 2, 3]),
    );
    // RLE8: a run of 1, an absolute block of 3 (padded), end of line, an
    // absolute block of 4, end of bitmap.
    write(
        "bmp_rle8.bmp",
        &bmp(
            4,
            2,
            8,
            1,
            &pal,
            &[1, 2, 0, 3, 3, 0, 1, 0, 0, 0, 0, 4, 0, 1, 2, 3, 0, 1],
        ),
    );
    // RLE4: bottom row as two runs of alternating pairs, end of line, top
    // row as one absolute block of 4, end of bitmap.
    write(
        "bmp_rle4.bmp",
        &bmp(
            4,
            2,
            4,
            2,
            &pal,
            &[2, 0x23, 2, 0x01, 0, 0, 0, 4, 0x01, 0x23, 0, 1],
        ),
    );
    // 24-bit top-down: rows are stored top first.
    write(
        "bmp_24bit_topdown.bmp",
        &bmp(
            2,
            -2,
            24,
            0,
            &[],
            &[0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0],
        ),
    );
    // 16-bit BITFIELDS 5-6-5, 2x1: red, blue.
    let masks: Vec<u8> = [0xf800u32, 0x07e0, 0x001f]
        .iter()
        .flat_map(|m| m.to_le_bytes())
        .collect();
    write(
        "bmp_16bit_bitfields.bmp",
        &bmp(2, 1, 16, 3, &masks, &[0x00, 0xf8, 0x1f, 0x00]),
    );

    // 24 and 32-bit from our own encoder.
    let mut px = Vec::new();
    for (i, a) in [255u8, 255, 128, 0].iter().enumerate() {
        px.extend_from_slice(&[i as u8 * 60, 255 - i as u8 * 60, 7, *a]);
    }
    let img = image(2, 2, px);
    let mut opts = Options {
        bmp_alpha: false,
        ..Options::default()
    };
    let mut opaque = img.clone();
    for p in opaque.rgba8.chunks_exact_mut(4) {
        p[3] = 255;
    }
    write(
        "bmp_24bit.bmp",
        &encode(Format::Bmp, &opaque, &opts).unwrap(),
    );
    opts.bmp_alpha = true;
    write("bmp_32bit.bmp", &encode(Format::Bmp, &img, &opts).unwrap());
}

fn gif_files() {
    // 4x2, two frames. Index 3 is transparent.
    let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
    let mut bytes = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut bytes, 4, 2, &palette).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();
        let mut first =
            gif::Frame::from_indexed_pixels(4, 2, vec![0, 1, 2, 3, 3, 2, 1, 0], Some(3));
        first.delay = 10;
        enc.write_frame(&first).unwrap();
        let mut second = gif::Frame::from_indexed_pixels(4, 2, vec![2; 8], Some(3));
        second.delay = 10;
        enc.write_frame(&second).unwrap();
    }
    write("gif_transparent_2frames.gif", &bytes);
}

fn tiff_files() {
    // 8-bit RGB, LZW, 3x1: red, green, blue.
    let mut out = std::io::Cursor::new(Vec::new());
    let mut enc = tiff::encoder::TiffEncoder::new(&mut out)
        .unwrap()
        .with_compression(tiff::encoder::Compression::Lzw);
    enc.write_image::<tiff::encoder::colortype::RGB8>(3, 1, &[255, 0, 0, 0, 255, 0, 0, 0, 255])
        .unwrap();
    write("tiff_rgb8_lzw.tif", &out.into_inner());

    // 16-bit RGB, uncompressed, 2x1.
    let mut out = std::io::Cursor::new(Vec::new());
    let mut enc = tiff::encoder::TiffEncoder::new(&mut out).unwrap();
    enc.write_image::<tiff::encoder::colortype::RGB16>(
        2,
        1,
        &[0xffff, 0x8000, 0x0000, 0x1234, 0x0080, 0xfeff],
    )
    .unwrap();
    write("tiff_rgb16.tif", &out.into_inner());

    // Our encoder: RGBA, LZW, with ICC and EXIF (EXIF IFD via tag 34665).
    let mut img = image(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 128]);
    img.icc = Some(srgb());
    img.metadata.exif = Some(common::exif_block(1));
    let opts = Options {
        tiff_compression: TiffCompression::Lzw,
        ..Options::default()
    };
    write(
        "tiff_exif_icc.tif",
        &encode(Format::Tiff, &img, &opts).unwrap(),
    );
}

fn webp_files() {
    let mut img = image(16, 16, quadrants());
    img.rgba8[3] = 0;
    img.icc = Some(srgb());
    img.metadata.exif = Some(common::exif_block(1));
    write(
        "webp_lossless.webp",
        &encode(Format::WebP, &img, &Options::default()).unwrap(),
    );

    let source = std::env::temp_dir().join("op-formats-webp-lossy-source.png");
    fs::write(
        &source,
        encode(
            Format::Png,
            &image(16, 16, quadrants()),
            &Options::default(),
        )
        .unwrap(),
    )
    .unwrap();
    let target = out_dir().join("webp_lossy.webp");
    match Command::new("cwebp")
        .args(["-quiet", "-q", "90", "-sharp_yuv"])
        .arg(&source)
        .arg("-o")
        .arg(&target)
        .status()
    {
        Ok(s) if s.success() => println!("wrote webp_lossy.webp (cwebp)"),
        Ok(s) => println!("SKIPPED webp_lossy.webp: cwebp exited with {s}"),
        Err(e) => println!("SKIPPED webp_lossy.webp: cwebp not runnable ({e})"),
    }
    fs::remove_file(source).unwrap();
}

fn main() {
    fs::create_dir_all(out_dir()).unwrap();
    png_files();
    jpeg_files();
    bmp_files();
    gif_files();
    tiff_files();
    webp_files();
}
