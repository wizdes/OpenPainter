// Release review item 6: valid 16-bit PNGs open as 8-bit with the engine's
// rounding, floor(v * 255 / 65535 + 0.5), and keep their ICC profile.
use op_formats::png::decode_png;

fn round16(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32767) / 65535) as u8
}

fn encode16(color: png::ColorType, samples: &[u16], icc: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut info = png::Info::with_size(2, 1);
    info.color_type = color;
    info.bit_depth = png::BitDepth::Sixteen;
    info.icc_profile = icc.map(|p| std::borrow::Cow::Owned(p.to_vec()));
    let mut w = png::Encoder::with_info(&mut out, info)
        .unwrap()
        .write_header()
        .unwrap();
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()).collect();
    w.write_image_data(&bytes).unwrap();
    w.finish().unwrap();
    out
}

const A: u16 = 0x8080;
const B: u16 = 0x0101;
const C: u16 = 0xFF7F;
const D: u16 = 0x1234;

#[test]
fn rgba16_rgb16_gray16_and_gray_alpha16_decode_with_engine_rounding() {
    let r = round16;
    let cases: [(png::ColorType, Vec<u16>, Vec<u8>); 4] = [
        (
            png::ColorType::Rgba,
            vec![A, B, C, D, D, C, B, A],
            vec![r(A), r(B), r(C), r(D), r(D), r(C), r(B), r(A)],
        ),
        (
            png::ColorType::Rgb,
            vec![A, B, C, D, C, B],
            vec![r(A), r(B), r(C), 255, r(D), r(C), r(B), 255],
        ),
        (
            png::ColorType::Grayscale,
            vec![A, D],
            vec![r(A), r(A), r(A), 255, r(D), r(D), r(D), 255],
        ),
        (
            png::ColorType::GrayscaleAlpha,
            vec![A, C, D, B],
            vec![r(A), r(A), r(A), r(C), r(D), r(D), r(D), r(B)],
        ),
    ];
    for (color, samples, expected) in cases {
        let image = decode_png(&encode16(color, &samples, None))
            .unwrap_or_else(|e| panic!("{color:?}: {e}"));
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixels, expected, "{color:?}");
    }
}

#[test]
fn a_16_bit_png_keeps_its_icc_profile() {
    let icc = op_color::Profile::builtin("display-p3")
        .or_else(|_| op_color::Profile::builtin("srgb"))
        .unwrap()
        .bytes()
        .to_vec();
    let image = decode_png(&encode16(
        png::ColorType::Rgb,
        &[A, B, C, D, C, B],
        Some(&icc),
    ))
    .unwrap();
    assert_eq!(image.icc.as_deref(), Some(&icc[..]));
    assert_eq!(
        &image.pixels[..4],
        [round16(A), round16(B), round16(C), 255]
    );
}
