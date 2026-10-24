//! Inputs found by `examples/fuzz.rs` that once reached a panic inside a codec.
use op_formats::decode;

/// tiff 0.10.3 indexes an empty SampleFormat list (decoder/image.rs:153).
/// `decode` must still return an error rather than panic.
#[test]
fn tiff_with_empty_sample_format_is_an_error() {
    let mut out = std::io::Cursor::new(Vec::new());
    tiff::encoder::TiffEncoder::new(&mut out)
        .unwrap()
        .write_image::<tiff::encoder::colortype::RGB8>(1, 1, &[1, 2, 3])
        .unwrap();
    let mut bytes = out.into_inner();
    assert_eq!(&bytes[..4], b"II*\0");
    let u16at = |b: &[u8], p: usize| u16::from_le_bytes([b[p], b[p + 1]]);
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let entry = (0..u16at(&bytes, ifd) as usize)
        .map(|i| ifd + 2 + i * 12)
        .find(|&p| u16at(&bytes, p) == 339)
        .expect("SampleFormat entry");
    bytes[entry + 4..entry + 8].copy_from_slice(&0u32.to_le_bytes());
    assert!(decode(&bytes).is_err());
}
