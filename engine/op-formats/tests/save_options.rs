//! M50 save options in the codecs: the resolution in PNG pHYs and JPEG JFIF,
//! GIF dithering, and PNG metadata chunks next to exact pixels.
use op_formats::{Decoded, Format, Metadata, Options, decode, encode, png::png_dpi};

fn gradient(w: u32, h: u32) -> Decoded {
    let mut rgba8 = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            rgba8.extend_from_slice(&[
                (x * 255 / (w - 1)) as u8,
                (y * 255 / (h - 1)) as u8,
                ((x + y) % 256) as u8,
                if x == 0 { 0 } else { 255 },
            ]);
        }
    }
    Decoded {
        width: w,
        height: h,
        rgba8,
        icc: None,
        metadata: Metadata::default(),
        notices: vec![],
    }
}

/// The JFIF APP0 density: (units, x, y).
fn jfif_density(bytes: &[u8]) -> Option<(u8, u16, u16)> {
    let p = bytes.windows(5).position(|w| w == b"JFIF\0")?;
    let d = &bytes[p + 5..];
    Some((
        d[2],
        u16::from_be_bytes([d[3], d[4]]),
        u16::from_be_bytes([d[5], d[6]]),
    ))
}

#[test]
fn png_writes_phys_and_reads_it_back() {
    let image = gradient(16, 8);
    for dpi in [72.0, 96.0, 300.0, 600.0] {
        let options = Options {
            dpi: Some(dpi),
            ..Options::default()
        };
        let bytes = encode(Format::Png, &image, &options).unwrap();
        assert_eq!(png_dpi(&bytes), Some(dpi), "{dpi} dpi");
        assert_eq!(decode(&bytes).unwrap().rgba8, image.rgba8, "pixels exact");
    }
    let none = encode(Format::Png, &image, &Options::default()).unwrap();
    assert_eq!(png_dpi(&none), None, "no resolution asked: no pHYs");
    let bad = Options {
        dpi: Some(0.0),
        ..Options::default()
    };
    assert!(encode(Format::Png, &image, &bad).is_err());
}

#[test]
fn png_keeps_exif_and_xmp_chunks_with_exact_pixels() {
    let mut image = gradient(9, 7);
    // A tiny big-endian EXIF block: IFD0 with one ASCII Make tag "OP".
    let exif = vec![
        b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 1, 15, 0, 2, 0, 0, 0, 3, b'O', b'P', 0, 0, 0, 0, 0, 0,
    ];
    image.metadata.exif = Some(exif.clone());
    image.metadata.xmp = Some(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>".to_vec());
    let bytes = encode(Format::Png, &image, &Options::default()).unwrap();
    let back = decode(&bytes).unwrap();
    assert_eq!(back.rgba8, image.rgba8);
    assert_eq!(back.metadata.exif, Some(exif));
    assert_eq!(back.metadata.xmp, image.metadata.xmp);
}

#[test]
fn jpeg_writes_the_density_in_inches() {
    let image = gradient(16, 16);
    let options = Options {
        dpi: Some(300.0),
        ..Options::default()
    };
    let bytes = encode(Format::Jpeg, &image, &options).unwrap();
    assert_eq!(jfif_density(&bytes), Some((1, 300, 300)));
}

#[test]
fn gif_dither_spreads_error_and_plain_maps_to_nearest() {
    // 300 distinct colors force a reduced palette.
    let mut image = gradient(60, 40);
    for p in image.rgba8.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let plain = encode(Format::Gif, &image, &Options::default()).unwrap();
    let dithered = encode(
        Format::Gif,
        &image,
        &Options {
            gif_dither: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_ne!(plain, dithered, "dithering changes the indices");
    let a = decode(&plain).unwrap().rgba8;
    let b = decode(&dithered).unwrap().rgba8;
    // Dithering keeps the average color of a region closer to the source.
    let mean_err = |out: &[u8]| -> f64 {
        let mut total = 0.0;
        let n = 10;
        for by in 0..4 {
            for bx in 0..6 {
                for c in 0..3 {
                    let (mut s, mut o) = (0i64, 0i64);
                    for y in by * n..(by + 1) * n {
                        for x in bx * n..(bx + 1) * n {
                            let i = (y * 60 + x) * 4 + c;
                            s += image.rgba8[i] as i64;
                            o += out[i] as i64;
                        }
                    }
                    total += ((s - o) as f64 / (n * n) as f64).abs();
                }
            }
        }
        total
    };
    assert!(
        mean_err(&b) <= mean_err(&a),
        "dithered {} vs plain {}",
        mean_err(&b),
        mean_err(&a)
    );
}

#[test]
fn gif_dither_keeps_transparency() {
    let image = gradient(20, 10);
    let bytes = encode(
        Format::Gif,
        &image,
        &Options {
            gif_dither: true,
            ..Options::default()
        },
    )
    .unwrap();
    let back = decode(&bytes).unwrap();
    for y in 0..10 {
        assert_eq!(back.rgba8[y * 20 * 4 + 3], 0, "column 0 stays transparent");
        assert_eq!(back.rgba8[(y * 20 + 1) * 4 + 3], 255);
    }
}
