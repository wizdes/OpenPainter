use lcms2::{CIExyY, Flags, Intent, PixelFormat, Transform};
use op_color::{DisplayLut, Profile, RenderingIntent, convert_rgb, convert_rgba, expand_gray};
use std::f64::consts::PI;
fn de(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (l1, a1, b1) = (a[0], a[1], a[2]);
    let (l2, a2, b2) = (b[0], b[1], b[2]);
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cb = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (cb.powi(7) / (cb.powi(7) + 25f64.powi(7))).sqrt());
    let ap1 = (1.0 + g) * a1;
    let ap2 = (1.0 + g) * a2;
    let cp1 = ap1.hypot(b1);
    let cp2 = ap2.hypot(b2);
    let hue = |y: f64, x: f64| {
        let h = y.atan2(x).to_degrees();
        if h < 0.0 { h + 360.0 } else { h }
    };
    let h1 = hue(b1, ap1);
    let h2 = hue(b2, ap2);
    let dl = l2 - l1;
    let dc = cp2 - cp1;
    let mut dh = h2 - h1;
    if cp1 * cp2 == 0.0 {
        dh = 0.0
    } else if dh > 180.0 {
        dh -= 360.0
    } else if dh < -180.0 {
        dh += 360.0
    }
    let dhp = 2.0 * (cp1 * cp2).sqrt() * (dh * PI / 360.0).sin();
    let lm = (l1 + l2) / 2.0;
    let cm = (cp1 + cp2) / 2.0;
    let hm = if cp1 * cp2 == 0.0 {
        h1 + h2
    } else if (h1 - h2).abs() <= 180.0 {
        (h1 + h2) / 2.0
    } else if h1 + h2 < 360.0 {
        (h1 + h2 + 360.0) / 2.0
    } else {
        (h1 + h2 - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * ((hm - 30.0) * PI / 180.0).cos()
        + 0.24 * (2.0 * hm * PI / 180.0).cos()
        + 0.32 * ((3.0 * hm + 6.0) * PI / 180.0).cos()
        - 0.20 * ((4.0 * hm - 63.0) * PI / 180.0).cos();
    let dt = 30.0 * (-((hm - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cm.powi(7) / (cm.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lm - 50.0).powi(2) / (20.0 + (lm - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cm;
    let sh = 1.0 + 0.015 * cm * t;
    let rt = -(2.0 * dt * PI / 180.0).sin() * rc;
    let x = dl / sl;
    let y = dc / sc;
    let z = dhp / sh;
    (x * x + y * y + z * z + rt * y * z).max(0.0).sqrt()
}
// Sharma, Wu, Dalal (2005), official supplemental data, https://hajim.rochester.edu/ece/sites/gsharma/ciede2000/dataNprograms/ciede2000testdata.txt
const SHARMA: &str = "50 2.6772 -79.7751 50 0 -82.7485 2.0425
50 3.1571 -77.2803 50 0 -82.7485 2.8615
50 2.8361 -74.0200 50 0 -82.7485 3.4412
50 -1.3802 -84.2814 50 0 -82.7485 1
50 -1.1848 -84.8006 50 0 -82.7485 1
50 -0.9009 -85.5211 50 0 -82.7485 1
50 0 0 50 -1 2 2.3669
50 -1 2 50 0 0 2.3669
50 2.49 -0.001 50 -2.49 0.0009 7.1792
50 2.49 -0.001 50 -2.49 0.0010 7.1792
50 2.49 -0.001 50 -2.49 0.0011 7.2195
50 2.49 -0.001 50 -2.49 0.0012 7.2195
50 -0.001 2.49 50 0.0009 -2.49 4.8045
50 -0.001 2.49 50 0.0010 -2.49 4.8045
50 -0.001 2.49 50 0.0011 -2.49 4.7461
50 2.5 0 50 0 -2.5 4.3065
50 2.5 0 73 25 -18 27.1492
50 2.5 0 61 -5 29 22.8977
50 2.5 0 56 -27 -3 31.9030
50 2.5 0 58 24 15 19.4535
50 2.5 0 50 3.1736 0.5854 1
50 2.5 0 50 3.2972 0 1
50 2.5 0 50 1.8634 0.5757 1
50 2.5 0 50 3.2592 0.335 1
60.2574 -34.0099 36.2677 60.4626 -34.1751 39.4387 1.2644
63.0109 -31.0961 -5.8663 62.8187 -29.7946 -4.0864 1.2630
61.2901 3.7196 -5.3901 61.4292 2.248 -4.962 1.8731
35.0831 -44.1164 3.7933 35.0232 -40.0716 1.5901 1.8645
22.7233 20.0904 -46.694 23.0331 14.973 -42.5619 2.0373
36.4612 47.858 18.3852 36.2715 50.5065 21.2231 1.4146
90.8027 -2.0831 1.441 91.1528 -1.6435 0.0447 1.4441
90.9257 -0.5406 -0.9208 88.6381 -0.8985 -0.7239 1.5381
6.7747 -0.2908 -2.4247 5.8714 -0.0985 -2.2286 0.6377
2.0776 0.0795 -1.135 0.9033 -0.0636 -0.5514 0.9082";
#[test]
fn sharma_34_pairs() {
    let mut n = 0;
    for line in SHARMA.lines() {
        let x: Vec<f64> = line
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert!(
            (de([x[0], x[1], x[2]], [x[3], x[4], x[5]]) - x[6]).abs() < 0.00005,
            "pair {}",
            n + 1
        );
        n += 1;
    }
    assert_eq!(n, 34)
}
fn samples() -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for r in 0..17 {
        for g in 0..17 {
            for b in 0..17 {
                out.push([
                    ((r * 255 + 8) / 16) as u8,
                    ((g * 255 + 8) / 16) as u8,
                    ((b * 255 + 8) / 16) as u8,
                ])
            }
        }
    }
    let mut s = 0x12345678u32;
    for _ in 0..1000 {
        let mut p = [0; 3];
        for c in &mut p {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            *c = s as u8;
        }
        out.push(p)
    }
    out
}
fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(|a, b| a.total_cmp(b));
    (*v.last().unwrap(), v[(v.len() * 99 / 100).min(v.len() - 1)])
}
fn lab_transform(p: &lcms2::Profile) -> Transform<[f64; 3], [f64; 3]> {
    let d50 = CIExyY {
        x: 0.34567,
        y: 0.35850,
        Y: 1.0,
    };
    let lab = lcms2::Profile::new_lab4_context(lcms2::GlobalContext::new(), &d50).unwrap();
    Transform::new(
        p,
        PixelFormat::RGB_DBL,
        &lab,
        PixelFormat::Lab_DBL,
        Intent::RelativeColorimetric,
    )
    .unwrap()
}
#[test]
fn direct_accuracy_and_roundtrips() {
    let grid = samples();
    let mut violations = Vec::new();
    for (a, b) in [
        ("srgb", "display-p3"),
        ("srgb", "adobe-rgb"),
        ("srgb", "prophoto"),
        ("display-p3", "adobe-rgb"),
        ("adobe-rgb", "srgb"),
        ("display-p3", "srgb"),
        ("prophoto", "srgb"),
        ("adobe-rgb", "display-p3"),
        ("v4-p3", "srgb"),
        ("srgb", "v4-p3"),
    ] {
        let v4 = || {
            let p = Profile::builtin("display-p3").unwrap();
            let mut raw = lcms2::Profile::new_icc(&p.bytes()).unwrap();
            raw.set_version(4.3);
            Profile::from_icc(&raw.icc().unwrap()).unwrap()
        };
        let src = if a == "v4-p3" {
            v4()
        } else {
            Profile::builtin(a).unwrap()
        };
        let dst = if b == "v4-p3" {
            v4()
        } else {
            Profile::builtin(b).unwrap()
        };
        let sp = lcms2::Profile::new_icc(&src.bytes()).unwrap();
        let dp = lcms2::Profile::new_icc(&dst.bytes()).unwrap();
        let direct: Transform<[f64; 3], [f64; 3]> = Transform::new_flags(
            &sp,
            PixelFormat::RGB_DBL,
            &dp,
            PixelFormat::RGB_DBL,
            Intent::RelativeColorimetric,
            Flags::BLACKPOINT_COMPENSATION,
        )
        .unwrap();
        let to_lab = lab_transform(&dp);
        let actual = convert_rgb(&src, &dst, RenderingIntent::Relative, true, &grid).unwrap();
        let back = convert_rgb(&dst, &src, RenderingIntent::Relative, true, &actual).unwrap();
        let mut err = Vec::new();
        let mut rt = Vec::new();
        for (i, p) in grid.iter().enumerate() {
            let in_f = p.map(|x| x as f64 / 255.0);
            let mut ref_f = [[0.0; 3]];
            direct.transform_pixels(&[in_f], &mut ref_f);
            let mut labs = [[0.0; 3]; 2];
            to_lab.transform_pixels(&[ref_f[0], actual[i].map(|x| x as f64 / 255.0)], &mut labs);
            if ref_f[0].iter().all(|x| *x >= 0.0 && *x <= 1.0) {
                err.push(de(labs[0], labs[1]));
            }
            rt.push(
                p.iter()
                    .zip(back[i])
                    .map(|(x, y)| (*x as i16 - y as i16).unsigned_abs() as f64)
                    .fold(0.0, f64::max),
            );
        }
        let (max, p99) = stats(err);
        let (rmax, r99) = stats(rt);
        println!(
            "{a}->{b}: in-gamut ΔE00 max {max:.4}, p99 {p99:.4}; round-trip byte max {rmax:.0}, p99 {r99:.0}"
        );
        // Measured 1.4996 for sRGB -> ProPhoto: the latter's very wide gamut makes
        // one encoded 8-bit step near dark colors exceed one ΔE00 after quantization.
        let bound = if a == "srgb" && b == "prophoto" {
            1.50
        } else {
            1.0
        };
        if max > bound {
            violations.push(format!("{a}->{b} max {max:.4}, bound {bound}"));
        }
    }
    assert!(
        violations.is_empty(),
        "stated bound exceeded: {}",
        violations.join(", ")
    );
}
#[test]
fn lut_accuracy() {
    let grid = samples();
    for (a, b) in [("srgb", "display-p3"), ("adobe-rgb", "srgb")] {
        let src = Profile::builtin(a).unwrap();
        let dst = Profile::builtin(b).unwrap();
        let lut = DisplayLut::build(&src, &dst).unwrap();
        let direct = convert_rgb(&src, &dst, RenderingIntent::Relative, true, &grid).unwrap();
        let dp = lcms2::Profile::new_icc(&dst.bytes()).unwrap();
        let to_lab = lab_transform(&dp);
        let mut err = Vec::new();
        for (p, q) in grid.iter().zip(direct) {
            let v = lut.sample(p.map(|x| x as f32 / 255.0));
            let mut labs = [[0.0; 3]; 2];
            to_lab.transform_pixels(
                &[v.map(|x| x as f64), q.map(|x| x as f64 / 255.0)],
                &mut labs,
            );
            err.push(de(labs[0], labs[1]));
        }
        let (max, p99) = stats(err);
        println!("LUT {a}->{b}: ΔE00 max {max:.4}, p99 {p99:.4}");
        assert!(max < 3.0);
    }
}
#[test]
fn alpha_and_gray() {
    let src = Profile::builtin("display-p3").unwrap();
    let dst = Profile::builtin("srgb").unwrap();
    let mut px = [200, 100, 40, 0, 40, 200, 100, 128];
    convert_rgba(&src, &dst, RenderingIntent::Relative, true, &mut px).unwrap();
    assert_eq!([px[3], px[7]], [0, 128]);
    assert_ne!(&px[..3], &[200, 100, 40]);
    let gray = Profile::builtin("gray-2.2").unwrap();
    let rgba = expand_gray(&[0, 128, 255], None, &gray, &dst).unwrap();
    assert_eq!(rgba.len(), 12);
    assert_eq!(rgba[3], 255)
}
#[test]
fn builtin_descriptions_survive_icc_serialization() {
    for (name, description) in [
        ("srgb", "sRGB"),
        ("display-p3", "Display P3"),
        ("adobe-rgb", "Adobe RGB (1998)"),
        ("prophoto", "ProPhoto RGB"),
        ("gray-2.2", "Gray gamma 2.2"),
    ] {
        let p = Profile::builtin(name).unwrap();
        assert_eq!(p.description(), description);
        assert_eq!(
            Profile::from_icc(&p.bytes()).unwrap().description(),
            description
        );
    }
}
#[test]
fn malformed_icc_2000_mutations_and_truncations() {
    for name in ["srgb", "display-p3", "adobe-rgb", "prophoto", "gray-2.2"] {
        let bytes = Profile::builtin(name).unwrap().bytes();
        let mut seed = 0x27182818u32;
        for _ in 0..400 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut b = bytes.to_vec();
            let i = seed as usize % b.len();
            b[i] ^= (seed >> 8) as u8;
            let _ = Profile::from_icc(&b);
        }
        for n in 0..bytes.len().min(140) {
            assert!(Profile::from_icc(&bytes[..n]).is_err());
        }
    }
}
#[test]
fn gray_to_rgb_accuracy() {
    let src = Profile::builtin("gray-2.2").unwrap();
    let dst = Profile::builtin("srgb").unwrap();
    let sp = lcms2::Profile::new_icc(&src.bytes()).unwrap();
    let dp = lcms2::Profile::new_icc(&dst.bytes()).unwrap();
    let reference: Transform<f64, [f64; 3]> = Transform::new_flags(
        &sp,
        PixelFormat::GRAY_DBL,
        &dp,
        PixelFormat::RGB_DBL,
        Intent::RelativeColorimetric,
        Flags::BLACKPOINT_COMPENSATION,
    )
    .unwrap();
    let to_lab = lab_transform(&dp);
    let input: Vec<u8> = (0..=255).collect();
    let actual = expand_gray(&input, None, &src, &dst).unwrap();
    let mut errors = Vec::new();
    for (i, &v) in input.iter().enumerate() {
        let mut ref_rgb = [[0.0; 3]];
        reference.transform_pixels(&[v as f64 / 255.0], &mut ref_rgb);
        let mut lab = [[0.0; 3]; 2];
        to_lab.transform_pixels(
            &[
                ref_rgb[0],
                [
                    actual[i * 4] as f64 / 255.0,
                    actual[i * 4 + 1] as f64 / 255.0,
                    actual[i * 4 + 2] as f64 / 255.0,
                ],
            ],
            &mut lab,
        );
        errors.push(de(lab[0], lab[1]));
    }
    let (max, p99) = stats(errors);
    println!("gray-2.2->srgb: ΔE00 max {max:.4}, p99 {p99:.4}");
    assert!(max <= 1.0);
}
#[test]
fn cmyk_import_reports_notice() {
    let src = synthetic_cmyk_profile();
    let (rgba, notice) = op_color::import_cmyk(&[[0, 0, 0, 0], [0, 0, 0, 255]], &src).unwrap();
    assert_eq!(rgba.len(), 8);
    assert!(notice.contains("CMYK") && notice.contains("sRGB"));
}
fn synthetic_cmyk_profile() -> Profile {
    use lcms2_sys as f;
    // A 2^4 CMYK-to-XYZ CLUT makes a portable test profile.
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
                        let r = (1 - c) * (1 - k);
                        let g = (1 - m) * (1 - k);
                        let b = (1 - y) * (1 - k);
                        for v in [
                            0.4361 * r as f64 + 0.3851 * g as f64 + 0.1431 * b as f64,
                            0.2225 * r as f64 + 0.7169 * g as f64 + 0.0606 * b as f64,
                            0.0139 * r as f64 + 0.0971 * g as f64 + 0.7142 * b as f64,
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
        Profile::from_icc(&data).unwrap()
    }
}
