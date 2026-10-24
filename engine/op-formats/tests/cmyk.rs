mod common;
use common::{Ink, assert_close, cmyk_icc, cmyk_jpeg, naive, patch_pixels, through_op_color};
use op_formats::decode;

#[test]
fn cmyk_and_ycck_jpeg_with_icc_use_op_color() {
    let icc = cmyk_icc();
    let want = through_op_color(&icc);
    let srgb = op_color::Profile::builtin("srgb").unwrap().bytes().to_vec();
    // YCCK keeps ink as YCbCr. Pure cyan clips Cr at 255 and returns as 254
    // ink, which the steep sRGB curve near black turns into about 6 levels.
    for (kind, tolerance) in [(Ink::Cmyk, 3), (Ink::Ycck, 8)] {
        let image = decode(&cmyk_jpeg(kind, Some(&icc))).unwrap();
        assert_eq!((image.width, image.height), (32, 8));
        assert_close(
            &patch_pixels(&image.rgba8),
            &want,
            tolerance,
            &format!("{kind:?}"),
        );
        assert_eq!(image.icc.as_deref(), Some(&srgb[..]), "{kind:?}");
        assert_eq!(image.notices.len(), 1, "{kind:?}: {:?}", image.notices);
        let notice = &image.notices[0];
        assert!(
            notice.contains("perceptual") && notice.contains("black point"),
            "{notice}"
        );
    }
}

#[test]
fn cmyk_and_ycck_jpeg_without_icc_use_naive_conversion() {
    let want = naive();
    for kind in [Ink::Cmyk, Ink::Ycck] {
        let image = decode(&cmyk_jpeg(kind, None)).unwrap();
        assert_close(&patch_pixels(&image.rgba8), &want, 3, &format!("{kind:?}"));
        assert_eq!(image.icc, None, "{kind:?}");
        assert_eq!(image.notices.len(), 1, "{kind:?}: {:?}", image.notices);
        assert!(
            image.notices[0].contains("no ICC profile"),
            "{}",
            image.notices[0]
        );
    }
}

#[test]
fn cmyk_jpeg_with_rgb_icc_falls_back_with_notice() {
    let srgb = op_color::Profile::builtin("srgb").unwrap().bytes().to_vec();
    let image = decode(&cmyk_jpeg(Ink::Cmyk, Some(&srgb))).unwrap();
    assert_close(&patch_pixels(&image.rgba8), &naive(), 3, "rgb icc");
    assert_eq!(image.icc, None);
    assert!(
        image.notices[0].contains("not usable"),
        "{}",
        image.notices[0]
    );
}
