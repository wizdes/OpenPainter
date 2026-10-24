use op_formats::{
    Decoded, Format, Metadata, Options, PngCompression, TiffCompression, decode, encode,
};

fn sample() -> Decoded {
    Decoded {
        width: 2,
        height: 1,
        rgba8: vec![12, 34, 56, 255, 78, 90, 123, 255],
        icc: None,
        metadata: Metadata::default(),
        notices: vec![],
    }
}

#[test]
fn png_new_api_round_trip_and_truncation() {
    let image = sample();
    let bytes = encode(Format::Png, &image, &Options::default()).unwrap();
    assert_eq!(decode(&bytes).unwrap().rgba8, image.rgba8);
    for cut in (0..bytes.len()).step_by((bytes.len() / 20).max(1)) {
        assert!(decode(&bytes[..cut]).is_err(), "cut {cut}");
    }
}

#[test]
fn png_compression_levels() {
    let image = sample();
    for level in 0..=9 {
        let options = Options {
            png_compression: PngCompression::Level(level),
            ..Options::default()
        };
        assert_eq!(
            decode(&encode(Format::Png, &image, &options).unwrap())
                .unwrap()
                .rgba8,
            image.rgba8
        );
    }
    let options = Options {
        png_compression: PngCompression::Level(10),
        ..Options::default()
    };
    assert!(encode(Format::Png, &image, &options).is_err());
}

#[test]
fn png_text_chunks() {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder
        .add_text_chunk("plain".into(), "hello".into())
        .unwrap();
    encoder
        .add_ztxt_chunk("compressed".into(), "world".into())
        .unwrap();
    encoder.add_itxt_chunk("utf8".into(), "☀".into()).unwrap();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[1, 2, 3, 4]).unwrap();
    drop(writer);
    let metadata = decode(&bytes).unwrap().metadata;
    assert!(metadata.text.contains(&("plain".into(), "hello".into())));
    assert!(
        metadata
            .text
            .contains(&("compressed".into(), "world".into()))
    );
    assert!(metadata.text.contains(&("utf8".into(), "☀".into())));
}

#[test]
fn png_16_bit_rounding() {
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, 1, 1);
    enc.set_depth(png::BitDepth::Sixteen);
    enc.set_color(png::ColorType::Rgba);
    let mut writer = enc.write_header().unwrap();
    writer
        .write_image_data(&[0x80, 0, 0, 0, 0xff, 0xff, 0, 1])
        .unwrap();
    drop(writer);
    assert_eq!(decode(&bytes).unwrap().rgba8, [128, 0, 255, 0]);
}

#[test]
fn lossless_formats_and_truncated_inputs() {
    let image = sample();
    for format in [
        Format::Png,
        Format::Bmp,
        Format::Gif,
        Format::Tiff,
        Format::WebP,
    ] {
        let bytes = encode(format, &image, &Options::default()).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.rgba8, image.rgba8, "{format:?}");
        for i in 0..20 {
            let cut = bytes.len() * i / 20;
            assert!(decode(&bytes[..cut]).is_err(), "{format:?} cut {cut}");
        }
    }
}

#[test]
fn lossless_formats_keep_hidden_color() {
    let mut image = sample();
    image.rgba8[3] = 0;
    for format in [Format::Png, Format::Bmp, Format::Tiff, Format::WebP] {
        let read = decode(&encode(format, &image, &Options::default()).unwrap()).unwrap();
        assert_eq!(read.rgba8, image.rgba8, "{format:?}");
    }
}

#[test]
fn bmp_24_and_tiff_lzw_round_trip() {
    let image = sample();
    let bmp = encode(
        Format::Bmp,
        &image,
        &Options {
            bmp_alpha: false,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(decode(&bmp).unwrap().rgba8, image.rgba8);
    let tiff = encode(
        Format::Tiff,
        &image,
        &Options {
            tiff_compression: TiffCompression::Lzw,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(decode(&tiff).unwrap().rgba8, image.rgba8);
}

fn exif_with_orientation(orientation: u16) -> Vec<u8> {
    let mut exif = b"II\x2a\x00\x08\x00\x00\x00\x01\x00".to_vec();
    exif.extend_from_slice(&274u16.to_le_bytes());
    exif.extend_from_slice(&3u16.to_le_bytes());
    exif.extend_from_slice(&1u32.to_le_bytes());
    exif.extend_from_slice(&orientation.to_le_bytes());
    exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    exif
}

#[test]
fn all_exif_orientations() {
    let source = Decoded {
        width: 2,
        height: 3,
        rgba8: (1..=6).flat_map(|v| [v, 0, 0, 255]).collect(),
        ..sample()
    };
    let expected: [&[u8]; 8] = [
        &[1, 2, 3, 4, 5, 6],
        &[2, 1, 4, 3, 6, 5],
        &[6, 5, 4, 3, 2, 1],
        &[5, 6, 3, 4, 1, 2],
        &[1, 3, 5, 2, 4, 6],
        &[5, 3, 1, 6, 4, 2],
        &[6, 4, 2, 5, 3, 1],
        &[2, 4, 6, 1, 3, 5],
    ];
    for orientation in 1..=8 {
        let mut image = source.clone();
        image.metadata.exif = Some(exif_with_orientation(orientation));
        let bytes = encode(Format::Png, &image, &Options::default()).unwrap();
        let decoded = decode(&bytes).unwrap();
        let ids: Vec<u8> = decoded.rgba8.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(
            ids,
            expected[(orientation - 1) as usize],
            "orientation {orientation}"
        );
        assert_eq!(decoded.metadata.exif.unwrap()[18], 1);
    }
}

#[test]
fn metadata_round_trip_png_webp_and_jpeg() {
    let mut image = sample();
    image.metadata.exif = Some(exif_with_orientation(1));
    image.metadata.xmp = Some(b"<xmp>hello</xmp>".to_vec());
    image.icc = Some(op_color::Profile::builtin("srgb").unwrap().bytes().to_vec());
    for format in [Format::Png, Format::Tiff, Format::WebP, Format::Jpeg] {
        let bytes = encode(format, &image, &Options::default()).unwrap();
        let read = decode(&bytes).unwrap();
        assert_eq!(read.metadata.exif, image.metadata.exif, "{format:?}");
        assert_eq!(read.metadata.xmp, image.metadata.xmp, "{format:?}");
        assert_eq!(read.icc, image.icc, "{format:?}");
    }
}

#[test]
fn jpeg_quality_95_psnr() {
    let image = Decoded {
        width: 32,
        height: 32,
        rgba8: (0..32 * 32)
            .flat_map(|i| {
                let x = (i % 32) as u8;
                let y = (i / 32) as u8;
                [x * 4, y * 4, 60, 255]
            })
            .collect(),
        ..sample()
    };
    let options = Options {
        jpeg_quality: 95,
        ..Options::default()
    };
    let decoded = decode(&encode(Format::Jpeg, &image, &options).unwrap()).unwrap();
    let mse: f64 = image
        .rgba8
        .chunks_exact(4)
        .zip(decoded.rgba8.chunks_exact(4))
        .flat_map(|(a, b)| (0..3).map(|c| ((a[c] as f64) - (b[c] as f64)).powi(2)))
        .sum::<f64>()
        / (32.0 * 32.0 * 3.0);
    let psnr = 10.0 * (255.0 * 255.0 / mse).log10();
    assert!(psnr > 40.0, "PSNR {psnr:.1} dB");
}

#[test]
fn gif_transparent_pixel() {
    let mut image = sample();
    image.rgba8[3] = 0;
    let decoded = decode(&encode(Format::Gif, &image, &Options::default()).unwrap()).unwrap();
    assert_eq!(decoded.rgba8[3], 0);
    assert_eq!(decoded.rgba8[7], 255);
}

#[test]
fn gif_exact_palette_for_small_images() {
    let image = Decoded {
        width: 16,
        height: 1,
        rgba8: (0..16)
            .flat_map(|i| [i * 13, 255 - i * 11, i * 7, 255])
            .collect(),
        ..sample()
    };
    let read = decode(&encode(Format::Gif, &image, &Options::default()).unwrap()).unwrap();
    assert_eq!(read.rgba8, image.rgba8);
}

#[test]
fn gif_median_cut_is_deterministic() {
    let image = Decoded {
        width: 300,
        height: 1,
        rgba8: (0..300)
            .flat_map(|i| [i as u8, (i / 2) as u8, (i * 7) as u8, 255])
            .collect(),
        ..sample()
    };
    let a = encode(Format::Gif, &image, &Options::default()).unwrap();
    assert_eq!(a, encode(Format::Gif, &image, &Options::default()).unwrap());
    assert_eq!(decode(&a).unwrap().width, 300);
}

#[test]
fn tiff_16_bit_rounding() {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut encoder = tiff::encoder::TiffEncoder::new(&mut out).unwrap();
    encoder
        .write_image::<tiff::encoder::colortype::RGBA16>(1, 1, &[32768, 0, 65535, 1])
        .unwrap();
    let bytes = out.into_inner();
    assert_eq!(decode(&bytes).unwrap().rgba8, [128, 0, 255, 0]);
}

#[test]
fn six_decoder_mutation_checks() {
    let image = sample();
    for format in [
        Format::Png,
        Format::Jpeg,
        Format::Bmp,
        Format::Gif,
        Format::Tiff,
        Format::WebP,
    ] {
        let mut bytes = encode(format, &image, &Options::default()).unwrap();
        let mut seed = 0x1234_5678u32;
        for _ in 0..128 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let p = seed as usize % bytes.len();
            let old = bytes[p];
            bytes[p] ^= (seed >> 8) as u8;
            let _ = decode(&bytes);
            bytes[p] = old;
        }
    }
}

#[test]
fn jpeg_truncated_at_twenty_cut_points() {
    let bytes = encode(Format::Jpeg, &sample(), &Options::default()).unwrap();
    for i in 0..20 {
        let cut = bytes.len() * i / 20;
        assert!(decode(&bytes[..cut]).is_err(), "JPEG cut {cut}");
    }
}

fn bmp_case(
    depth: u16,
    compression: u32,
    height: i32,
    palette: &[[u8; 4]],
    masks: &[u32],
    data: &[u8],
) -> Vec<u8> {
    let off = 54 + palette.len() * 4 + masks.len() * 4;
    let mut b = vec![0u8; off];
    b[..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&((off + data.len()) as u32).to_le_bytes());
    b[10..14].copy_from_slice(&(off as u32).to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&2u32.to_le_bytes());
    b[22..26].copy_from_slice(&height.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&depth.to_le_bytes());
    b[30..34].copy_from_slice(&compression.to_le_bytes());
    b[46..50].copy_from_slice(&(palette.len() as u32).to_le_bytes());
    for (i, m) in masks.iter().enumerate() {
        b[54 + i * 4..58 + i * 4].copy_from_slice(&m.to_le_bytes());
    }
    for (i, p) in palette.iter().enumerate() {
        let q = 54 + masks.len() * 4 + i * 4;
        b[q..q + 4].copy_from_slice(p);
    }
    b.extend_from_slice(data);
    b
}

#[test]
fn bmp_depth_rle_and_bitfields_cases() {
    let palette = [[0, 0, 255, 0], [0, 255, 0, 0]];
    let cases: Vec<(Vec<u8>, [u8; 2])> = vec![
        (bmp_case(1, 0, 1, &palette, &[], &[0x40, 0, 0, 0]), [255, 0]),
        (bmp_case(4, 0, 1, &palette, &[], &[0x01, 0, 0, 0]), [255, 0]),
        (bmp_case(8, 0, 1, &palette, &[], &[0, 1, 0, 0]), [255, 0]),
        (
            bmp_case(24, 0, -1, &[], &[], &[0, 0, 255, 0, 255, 0, 0, 0]),
            [255, 0],
        ),
        (
            bmp_case(32, 0, 1, &[], &[], &[0, 0, 255, 255, 0, 255, 0, 255]),
            [255, 0],
        ),
        (bmp_case(8, 1, 1, &palette, &[], &[2, 0, 0, 1]), [255, 255]),
        (bmp_case(4, 2, 1, &palette, &[], &[2, 0x01, 0, 1]), [255, 0]),
        (
            bmp_case(
                16,
                3,
                1,
                &[],
                &[0x7c00, 0x03e0, 0x001f],
                &[0, 0x7c, 0xe0, 0x03],
            ),
            [255, 0],
        ),
    ];
    for (bytes, red) in cases {
        let read = decode(&bytes).unwrap();
        assert_eq!([read.rgba8[0], read.rgba8[4]], red);
    }
}
