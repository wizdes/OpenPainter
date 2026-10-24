//! Helpers shared by the integration tests and the corpus generator example.
#![allow(dead_code)]

/// A small CMYK input profile: a 2^4 CMYK-to-XYZ lookup table, built with lcms2.
/// It is made here so no third-party profile has to be committed.
pub fn cmyk_icc() -> Vec<u8> {
    use lcms2_sys as f;
    // SAFETY: every lcms2 handle is checked for null and freed before return.
    unsafe {
        let h = f::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        assert!(!h.is_null());
        f::cmsSetProfileVersion(h, 2.1);
        f::cmsSetDeviceClass(h, f::ProfileClassSignature::InputClass);
        f::cmsSetColorSpace(h, f::ColorSpaceSignature::CmykData);
        f::cmsSetPCS(h, f::ColorSpaceSignature::XYZData);
        let white = f::CIEXYZ {
            X: 0.9642,
            Y: 1.0,
            Z: 0.8249,
        };
        assert_ne!(
            f::cmsWriteTag(
                h,
                f::TagSignature::MediaWhitePointTag,
                &white as *const _ as *const _
            ),
            0
        );
        let pipe = f::cmsPipelineAlloc(std::ptr::null_mut(), 4, 3);
        let mut table = Vec::new();
        for c in 0..2 {
            for m in 0..2 {
                for y in 0..2 {
                    for k in 0..2 {
                        let r = ((1 - c) * (1 - k)) as f64;
                        let g = ((1 - m) * (1 - k)) as f64;
                        let b = ((1 - y) * (1 - k)) as f64;
                        for v in [
                            0.4361 * r + 0.3851 * g + 0.1431 * b,
                            0.2225 * r + 0.7169 * g + 0.0606 * b,
                            0.0139 * r + 0.0971 * g + 0.7142 * b,
                        ] {
                            table.push((v * 65535.0).round() as u16)
                        }
                    }
                }
            }
        }
        let stage = f::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, 4, 3, table.as_ptr());
        assert!(!stage.is_null());
        assert_ne!(
            f::cmsPipelineInsertStage(pipe, f::StageLoc::AT_END, stage),
            0
        );
        assert_ne!(
            f::cmsWriteTag(h, f::TagSignature::AToB0Tag, pipe as *const _),
            0
        );
        f::cmsPipelineFree(pipe);
        let mut len = 0u32;
        assert_ne!(f::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut len), 0);
        let mut data = vec![0; len as usize];
        assert_ne!(
            f::cmsSaveProfileToMem(h, data.as_mut_ptr() as *mut _, &mut len),
            0
        );
        f::cmsCloseProfile(h);
        data
    }
}

/// Ink patches used by the CMYK tests and the CMYK corpus files (C, M, Y, K, 0 = no ink).
pub const INKS: [[u8; 4]; 4] = [
    [0, 0, 0, 0],
    [255, 0, 0, 0],
    [0, 128, 255, 0],
    [20, 40, 60, 200],
];

/// Which 4-channel layout a CMYK JPEG stores.
#[derive(Clone, Copy, Debug)]
pub enum Ink {
    Cmyk,
    Ycck,
}

/// A 32x8 JPEG made of four flat 8x8 ink patches (one per entry of `INKS`).
/// Flat 8x8 blocks keep JPEG loss to about one level per channel.
pub fn cmyk_jpeg(kind: Ink, icc: Option<&[u8]>) -> Vec<u8> {
    let mut ink = Vec::new();
    for _ in 0..8 {
        for patch in INKS {
            for _ in 0..8 {
                ink.extend_from_slice(&patch);
            }
        }
    }
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, 100);
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
    if let Some(icc) = icc {
        enc.add_icc_profile(icc).unwrap();
    }
    let color = match kind {
        Ink::Cmyk => jpeg_encoder::ColorType::Cmyk,
        Ink::Ycck => jpeg_encoder::ColorType::CmykAsYcck,
    };
    enc.encode(&ink, 32, 8, color).unwrap();
    out
}

/// The center pixel of each 8x8 patch.
pub fn patch_pixels(rgba: &[u8]) -> Vec<[u8; 4]> {
    (0..4)
        .map(|i| {
            let p = (4 * 32 + i * 8 + 4) * 4;
            [rgba[p], rgba[p + 1], rgba[p + 2], rgba[p + 3]]
        })
        .collect()
}

pub fn assert_close(got: &[[u8; 4]], want: &[[u8; 4]], tolerance: i32, label: &str) {
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let d = (g[c] as i32 - w[c] as i32).abs();
            assert!(
                d <= tolerance,
                "{label} patch {i}: got {g:?}, want {w:?} (ink {:?})",
                INKS[i]
            );
        }
    }
}

pub fn through_op_color(icc: &[u8]) -> Vec<[u8; 4]> {
    let profile = op_color::Profile::from_icc(icc).unwrap();
    let (rgba, _) = op_color::import_cmyk(&INKS, &profile).unwrap();
    rgba.chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// Naive conversion documented in `op_formats::decode`: R = 255 * (1 - C) * (1 - K).
pub fn naive() -> Vec<[u8; 4]> {
    INKS.iter()
        .map(|&[c, m, y, k]| {
            let f = |v: u8| ((255 - v as u32) * (255 - k as u32) + 127) / 255;
            [f(c) as u8, f(m) as u8, f(y) as u8, 255]
        })
        .collect()
}

/// A raw EXIF block (TIFF structure) with Make and Orientation in IFD0 and
/// DateTimeOriginal plus ExifVersion in the EXIF IFD.
pub fn exif_block(orientation: u16) -> Vec<u8> {
    use exif::{Field, In, Tag, Value};
    let make = Field {
        tag: Tag::Make,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![b"OpenPainter".to_vec()]),
    };
    let orient = Field {
        tag: Tag::Orientation,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![orientation]),
    };
    let date = Field {
        tag: Tag::DateTimeOriginal,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![b"2026:10:24 12:34:56".to_vec()]),
    };
    let version = Field {
        tag: Tag::ExifVersion,
        ifd_num: In::PRIMARY,
        value: Value::Undefined(b"0232".to_vec(), 0),
    };
    let mut writer = exif::experimental::Writer::new();
    for f in [&make, &orient, &date, &version] {
        writer.push_field(f);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    writer.write(&mut out, true).unwrap();
    out.into_inner()
}

/// Read one EXIF field as text through kamadak-exif.
pub fn exif_text(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    exif.get_field(tag, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string())
}
