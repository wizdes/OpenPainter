use op_color::{Profile, convert_rgb_float, display_table};

#[test]
fn same_profile_table_is_identity() {
    let srgb = Profile::builtin("srgb").unwrap();
    let n = 33u32;
    let t = display_table(&srgb, &srgb, n).unwrap();
    assert_eq!(t.len(), (n * n * n) as usize);
    for (i, c) in t.iter().enumerate() {
        let i = i as u32;
        let want = [i % n, i / n % n, i / (n * n)].map(|v| v as f32 / (n - 1) as f32);
        for k in 0..3 {
            assert!(
                (c[k] - want[k]).abs() < 1e-3,
                "entry {i}: {c:?} vs {want:?}"
            );
        }
    }
}

#[test]
fn table_entries_equal_the_float_transform_at_grid_points() {
    let p3 = Profile::builtin("display-p3").unwrap();
    let srgb = Profile::builtin("srgb").unwrap();
    let t = display_table(&p3, &srgb, 5).unwrap();
    // Entry r + g*n + b*n*n is the input (r, g, b) / (n - 1).
    let probe = [[1.0f32, 0.0, 0.0], [0.25, 0.5, 0.75], [0.0, 1.0, 0.0]];
    let exact = convert_rgb_float(&p3, &srgb, &probe).unwrap();
    for (rgb, want) in probe.iter().zip(&exact) {
        let idx = rgb.map(|v| (v * 4.0).round() as usize);
        let got = t[idx[0] + idx[1] * 5 + idx[2] * 25];
        assert_eq!(got, *want, "{rgb:?}");
    }
    // P3 red is outside sRGB: clamped to the gamut.
    assert_eq!(exact[0][0], 1.0);
    assert!(exact[0][1] == 0.0 && exact[0][2] == 0.0, "{:?}", exact[0]);
}

#[test]
fn refuses_non_rgb_profiles_and_bad_sizes() {
    let srgb = Profile::builtin("srgb").unwrap();
    let gray = Profile::builtin("gray-2.2").unwrap();
    assert!(display_table(&gray, &srgb, 33).is_err());
    assert!(display_table(&srgb, &gray, 33).is_err());
    assert!(display_table(&srgb, &srgb, 1).is_err());
}
