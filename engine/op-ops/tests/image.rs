use op_ops::image::{Resampling, resize};

#[test]
fn nearest_preserves_hidden_color_and_partial_alpha() {
    let src = [200, 30, 40, 0, 10, 20, 30, 128];
    assert_eq!(
        resize(&src, 2, 1, 4, 1, Resampling::Nearest, false),
        [
            200, 30, 40, 0, 200, 30, 40, 0, 10, 20, 30, 128, 10, 20, 30, 128
        ]
    );
}

#[test]
fn filtering_uses_premultiplied_alpha_and_gamma() {
    let src = [255, 0, 0, 0, 0, 0, 255, 255];
    let out = resize(&src, 2, 1, 1, 1, Resampling::SuperSampling, false);
    assert_eq!(out, [0, 0, 255, 128]);
    let contrast = [0, 0, 0, 255, 255, 255, 255, 255];
    assert!(
        resize(&contrast, 2, 1, 1, 1, Resampling::SuperSampling, true)[0]
            > resize(&contrast, 2, 1, 1, 1, Resampling::SuperSampling, false)[0]
    );
}

#[test]
fn resize_24_mp_half_reports_time() {
    let source = vec![128u8; 4000 * 6000 * 4];
    let start = std::time::Instant::now();
    let output = resize(&source, 4000, 6000, 2000, 3000, Resampling::Bicubic, false);
    println!(
        "24 MP to 50% bicubic CPU: {:.3} s",
        start.elapsed().as_secs_f64()
    );
    assert_eq!(output.len(), 2000 * 3000 * 4);
}

#[test]
fn nearest_matches_legacy_oracle() {
    use op_formats::png::decode_png;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src =
        decode_png(&std::fs::read(root.join("tests/fixtures/plasma_alpha_512x384.png")).unwrap())
            .unwrap();
    for (label, w, h) in [
        ("0.5x", 256, 192),
        ("0.37x", 189, 142),
        ("1.6x", 819, 614),
        ("3x", 1536, 1152),
    ] {
        let reference = decode_png(
            &std::fs::read(root.join(format!(
                "tests/oracle/legacy336/resample/NearestNeighbor_{label}.png"
            )))
            .unwrap(),
        )
        .unwrap();
        let got = resize(
            &src.pixels,
            src.width,
            src.height,
            w,
            h,
            Resampling::Nearest,
            false,
        );
        assert_eq!((reference.width, reference.height), (w, h));
        assert_eq!(got, reference.pixels, "{label}");
    }
}

#[test]
fn filtered_modes_match_legacy_oracle_tolerance() {
    use op_formats::png::decode_png;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src =
        decode_png(&std::fs::read(root.join("tests/fixtures/plasma_alpha_512x384.png")).unwrap())
            .unwrap();
    for (mode, name) in [
        (Resampling::Bilinear, "Bilinear"),
        (Resampling::Bicubic, "Bicubic"),
        (Resampling::SuperSampling, "SuperSampling"),
    ] {
        for (label, w, h) in [
            ("0.5x", 256, 192),
            ("0.37x", 189, 142),
            ("1.6x", 819, 614),
            ("3x", 1536, 1152),
        ] {
            let reference = decode_png(
                &std::fs::read(root.join(format!(
                    "tests/oracle/legacy336/resample/{name}_{label}.png"
                )))
                .unwrap(),
            )
            .unwrap();
            let got = op_ops::image::legacy::resize(&src.pixels, src.width, src.height, w, h, mode);
            let max = got
                .iter()
                .zip(&reference.pixels)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            let mismatches = got
                .iter()
                .zip(&reference.pixels)
                .filter(|(a, b)| a.abs_diff(**b) > 1)
                .count();
            assert!(
                max <= 1,
                "{name} {label}: max {max}, mismatches {mismatches}"
            );
        }
    }
}

#[test]
fn one_pixel_and_strong_reduction_keep_valid_channels() {
    let src: Vec<u8> = (0..4000)
        .flat_map(|i| [i as u8, 255, 7, if i % 2 == 0 { 0 } else { 200 }])
        .collect();
    for mode in [
        Resampling::Nearest,
        Resampling::Bilinear,
        Resampling::Bicubic,
        Resampling::SuperSampling,
    ] {
        let tiny = op_ops::image::legacy::resize(&src, 4000, 1, 7, 1, mode);
        assert_eq!(tiny.len(), 7 * 4);
        let one = op_ops::image::legacy::resize(&src, 4000, 1, 1, 1, mode);
        assert_eq!(one.len(), 4);
    }
    let source_2d = src.repeat(8);
    let reduced =
        op_ops::image::legacy::resize(&source_2d, 4000, 8, 7, 1, Resampling::SuperSampling);
    assert_eq!(reduced.len(), 7 * 4);
    assert!(reduced.chunks_exact(4).all(|p| p[3] > 0));
}
