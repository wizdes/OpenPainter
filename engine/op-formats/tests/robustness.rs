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

/// M57 fuzzing (`cargo xtask fuzz webp`): WebP files under 1 KB whose RIFF
/// chunk sizes claim gigabytes. image-webp 0.2 allocated the claimed size
/// (2 to 4 GB) before reading, which aborts the process under a memory limit.
/// `decode` must refuse them before the codec allocates.
#[test]
fn webp_chunks_larger_than_the_file_are_refused_before_decoding() {
    for name in [
        "webp-oversized-chunk-1.bin",
        "webp-oversized-chunk-2.bin",
        "webp-oversized-chunk-3.bin",
        // M57 review: an animated WebP whose ANMF frame holds an ICCP
        // subchunk claiming 2.25 GiB; the outer chunks all fit.
        "webp-anmf-nested-chunk.bin",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fuzz-found")
            .join(name);
        let bytes = std::fs::read(&path).unwrap();
        let Err(error) = decode(&bytes) else {
            panic!("{name} decoded")
        };
        let error = error.to_string();
        assert!(
            error.contains("WebP chunk is larger than the file"),
            "{name}: {error}"
        );
    }
}

/// Checkpoint fuzzing on Linux (`cargo xtask fuzz`, icc target): two 568-byte
/// profiles whose `desc` tag is a 36-byte mluc that claims 33,554,433 records.
/// lcms2 allocates and zeroes 12 bytes per claimed record (about 400 MB) before
/// it reads one, which took 70 ms on the Mac and over the 1 s limit in the VM.
/// `Profile::from_icc` must refuse a record count the tag cannot hold.
#[test]
fn icc_mluc_with_more_records_than_the_tag_holds_is_refused() {
    for name in ["icc-mluc-huge-count-1.bin", "icc-mluc-huge-count-2.bin"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fuzz-found")
            .join(name);
        let bytes = std::fs::read(&path).unwrap();
        let Err(error) = op_color::Profile::from_icc(&bytes) else {
            panic!("{name} was read")
        };
        let error = error.to_string();
        assert!(
            error.contains("mluc tag claims more records than it holds"),
            "{name}: {error}"
        );
    }
    // A real mluc tag still reads: the built-in profiles carry one.
    for name in ["srgb", "display-p3", "adobe-rgb", "prophoto"] {
        let bytes = op_color::Profile::builtin(name).unwrap().bytes();
        let profile = op_color::Profile::from_icc(&bytes).unwrap();
        assert!(!profile.description().is_empty(), "{name}");
    }
}
