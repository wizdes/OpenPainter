use op_formats::png::{DecodedImage, decode_png, encode_png};

#[test]
fn rgba_round_trip_preserves_hidden_color() {
    let image = DecodedImage {
        width: 2,
        height: 1,
        pixels: vec![13, 24, 35, 0, 255, 1, 2, 128],
        icc: None,
    };
    let encoded = encode_png(&image).unwrap();
    assert_eq!(decode_png(&encoded).unwrap(), image);
    for end in 0..encoded.len() {
        assert!(decode_png(&encoded[..end]).is_err(), "truncated at {end}");
    }
    let mut corrupt = encoded;
    corrupt[20] ^= 1;
    assert!(decode_png(&corrupt).is_err());
}

#[test]
fn rejects_16_bit() {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 1, 1);
    encoder.set_depth(png::BitDepth::Sixteen);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[0; 8]).unwrap();
    drop(writer);
    assert!(
        decode_png(&out)
            .unwrap_err()
            .to_string()
            .contains("unsupported in M04")
    );
}

#[test]
fn iccp_survives_round_trip() {
    let image = DecodedImage {
        width: 1,
        height: 1,
        pixels: vec![4, 5, 6, 0],
        icc: Some(b"dummy ICC profile bytes".to_vec()),
    };
    assert_eq!(decode_png(&encode_png(&image).unwrap()).unwrap(), image);
}

#[test]
fn expands_common_8_bit_inputs() {
    for (color, source, expected) in [
        (png::ColorType::Rgb, vec![2, 3, 4], vec![2, 3, 4, 255]),
        (png::ColorType::Grayscale, vec![6], vec![6, 6, 6, 255]),
        (png::ColorType::GrayscaleAlpha, vec![7, 8], vec![7, 7, 7, 8]),
        (png::ColorType::Indexed, vec![0], vec![9, 10, 11, 12]),
    ] {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 1, 1);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_color(color);
        if color == png::ColorType::Indexed {
            enc.set_palette(vec![9, 10, 11]);
            enc.set_trns(vec![12]);
        }
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&source).unwrap();
        drop(writer);
        assert_eq!(decode_png(&out).unwrap().pixels, expected, "{color:?}");
    }
}

#[test]
fn rejects_oversize_and_wrong_pixel_length() {
    let image = DecodedImage {
        width: u32::MAX,
        height: u32::MAX,
        pixels: vec![],
        icc: None,
    };
    assert!(encode_png(&image).is_err());
    let image = DecodedImage {
        width: 1,
        height: 1,
        pixels: vec![1],
        icc: None,
    };
    assert!(encode_png(&image).is_err());
}
