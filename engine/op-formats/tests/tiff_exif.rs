mod common;
use common::{exif_block, exif_text};
use op_formats::{Decoded, Format, Metadata, Options, decode, encode};

fn image_with_exif(exif: Vec<u8>) -> Decoded {
    Decoded {
        width: 2,
        height: 1,
        rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255],
        icc: None,
        metadata: Metadata {
            exif: Some(exif),
            ..Metadata::default()
        },
        notices: vec![],
    }
}

/// Walk IFD0 of a little-endian TIFF and return (tag, type, count, value field).
fn ifd0(b: &[u8]) -> Vec<(u16, u16, u32, u32)> {
    assert_eq!(&b[..4], b"II*\0");
    let u16at = |p: usize| u16::from_le_bytes([b[p], b[p + 1]]);
    let u32at = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    let off = u32at(4) as usize;
    (0..u16at(off) as usize)
        .map(|i| {
            let p = off + 2 + i * 12;
            (u16at(p), u16at(p + 2), u32at(p + 4), u32at(p + 8))
        })
        .collect()
}

#[test]
fn tiff_writes_exif_ifd_pointed_to_by_tag_34665() {
    for compression in [
        op_formats::TiffCompression::None,
        op_formats::TiffCompression::Lzw,
    ] {
        let options = Options {
            tiff_compression: compression,
            ..Options::default()
        };
        let bytes = encode(Format::Tiff, &image_with_exif(exif_block(1)), &options).unwrap();
        let entries = ifd0(&bytes);
        let pointer = entries.iter().find(|e| e.0 == 34665).expect("tag 34665");
        assert!(matches!(pointer.1, 4 | 13) && pointer.2 == 1, "{pointer:?}");
        assert!(
            entries.iter().all(|e| e.0 != 65000),
            "private EXIF tag must not be written"
        );
        assert!(entries.iter().any(|e| e.0 == 271), "Make is in IFD0");

        let exif = exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(&bytes))
            .unwrap();
        assert_eq!(
            exif_text(&exif, exif::Tag::Make).as_deref(),
            Some("\"OpenPainter\"")
        );
        assert_eq!(
            exif_text(&exif, exif::Tag::DateTimeOriginal).as_deref(),
            Some("2026-10-24 12:34:56")
        );
        assert_eq!(
            exif_text(&exif, exif::Tag::ExifVersion).as_deref(),
            Some("2.32")
        );
        assert_eq!(
            exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0)),
            Some(1)
        );
    }
}

#[test]
fn tiff_exif_reads_back_through_decode() {
    let bytes = encode(
        Format::Tiff,
        &image_with_exif(exif_block(1)),
        &Options::default(),
    )
    .unwrap();
    let read = decode(&bytes).unwrap();
    let block = read.metadata.exif.expect("EXIF block");
    let exif = exif::Reader::new().read_raw(block).unwrap();
    assert_eq!(
        exif_text(&exif, exif::Tag::Make).as_deref(),
        Some("\"OpenPainter\"")
    );
    assert_eq!(
        exif_text(&exif, exif::Tag::DateTimeOriginal).as_deref(),
        Some("2026-10-24 12:34:56")
    );
    assert_eq!(
        exif_text(&exif, exif::Tag::ExifVersion).as_deref(),
        Some("2.32")
    );
}

#[test]
fn tiff_orientation_in_ifd0_is_applied_and_reset() {
    let bytes = encode(
        Format::Tiff,
        &image_with_exif(exif_block(6)),
        &Options::default(),
    )
    .unwrap();
    let read = decode(&bytes).unwrap();
    // Orientation 6 turns the 2x1 image into 1x2.
    assert_eq!((read.width, read.height), (1, 2));
    assert_eq!(read.rgba8, [10, 20, 30, 255, 40, 50, 60, 255]);
    let exif = exif::Reader::new()
        .read_raw(read.metadata.exif.unwrap())
        .unwrap();
    assert_eq!(
        exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0)),
        Some(1)
    );
}

#[test]
fn tiff_without_exif_has_no_exif_block() {
    let mut image = image_with_exif(vec![]);
    image.metadata.exif = None;
    let read = decode(&encode(Format::Tiff, &image, &Options::default()).unwrap()).unwrap();
    assert_eq!(read.metadata.exif, None);
    assert!(read.notices.is_empty(), "{:?}", read.notices);
}

#[test]
fn tiff_rejects_exif_that_is_not_a_tiff_structure() {
    let image = image_with_exif(b"not exif".to_vec());
    assert!(encode(Format::Tiff, &image, &Options::default()).is_err());
}
