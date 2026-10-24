use op_core::legacy::{
    fast_divide_short_by_byte, fast_scale_byte_by_byte, normal_apply, normal_apply_with_opacity,
};
use op_core::pixel::{PremulF32, Rgba8};

#[test]
fn rgba_storage_is_rgba_not_paint_net_bgra() {
    let p = Rgba8::from([1, 2, 3, 4]);
    assert_eq!([p.r, p.g, p.b, p.a], [1, 2, 3, 4]);
    assert_eq!(<[u8; 4]>::from(p), [1, 2, 3, 4]);
}

#[test]
fn byte_helpers_exhaustive() {
    for a in 0..=255u16 {
        for b in 0..=255u16 {
            let product = a as u32 * b as u32 + 128;
            let expected = ((product + (product >> 8)) >> 8) as u8;
            assert_eq!(fast_scale_byte_by_byte(a as u8, b as u8), expected);
            if b != 0 {
                assert_eq!(fast_divide_short_by_byte(a, b as u8), a as u32 / b as u32);
            }
        }
    }
}

#[test]
fn normal_blend_integer_stages_differ_from_float() {
    let bottom = Rgba8::from([0, 0, 0, 2]);
    let top = Rgba8::from([255, 255, 255, 64]);
    assert_eq!(normal_apply(bottom, top), Rgba8::from([251, 251, 251, 65]));
    assert_eq!(
        normal_apply_with_opacity(bottom, top, 255),
        normal_apply(bottom, top)
    );
    let b = PremulF32::from_straight(bottom);
    let t = PremulF32::from_straight(top);
    let over = PremulF32 {
        r: t.r + b.r * (1.0 - t.a),
        g: t.g + b.g * (1.0 - t.a),
        b: t.b + b.b * (1.0 - t.a),
        a: t.a + b.a * (1.0 - t.a),
    };
    assert_eq!(over.to_straight_rounded().r, 249);
}

#[test]
fn premul_round_trip_bound() {
    let mut worst = 0u8;
    let mut worst_nonzero = 0u8;
    for a in 0..=255u8 {
        for c in 0..=255u8 {
            let got = PremulF32::from_straight(Rgba8::from([c, c, c, a])).to_straight_rounded();
            worst = worst.max(c.abs_diff(got.r));
            if a > 0 {
                worst_nonzero = worst_nonzero.max(c.abs_diff(got.r));
            }
        }
    }
    // The only loss is when zero alpha erases hidden color.
    assert_eq!(worst, 255);
    assert!(
        worst_nonzero <= 1,
        "worst nonzero-alpha error: {worst_nonzero}"
    );
    for c in 0..=255u8 {
        assert_eq!(
            PremulF32::from_straight(Rgba8::from([c, c, c, 255]))
                .to_straight_rounded()
                .r,
            c
        );
    }
}

#[test]
fn boundary_weighted_64_cases() {
    // Fixed seed keeps failures reproducible. Values at both ends and the byte midpoint dominate.
    let edges = [0u8, 1, 127, 128, 254, 255];
    let mut state = 0x3cbf_62a1_u64;
    for _ in 0..64 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let a = edges[(state as usize) % edges.len()];
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let b = edges[(state as usize) % edges.len()];
        let p = Rgba8::from([a, b, 255 - a, b]);
        let out = PremulF32::from_straight(p).to_straight_rounded();
        assert_eq!(out.a, b);
        if b > 0 {
            assert!(out.r.abs_diff(a) <= 1);
            assert!(out.g.abs_diff(b) <= 1);
        }
        assert_eq!(
            fast_scale_byte_by_byte(a, b),
            (((a as u32 * b as u32 + 128) * 257) >> 16) as u8
        );
    }
}
