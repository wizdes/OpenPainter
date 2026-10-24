// Port of src/Core/Utility.cs lines 2047-2102 and 2990-3271;
// src/Core/ColorBgra.cs lines 214-216 and 352-461;
// src/Data/UserBlendOps.Generated.cs line 579 (Normal scalar Apply and opacity Apply).
// https://github.com/rivy/OpenPDN/blob/cca476b0df2a2f70996e6b9486ec45327631568c/src/Core/Utility.cs
// https://github.com/rivy/OpenPDN/blob/cca476b0df2a2f70996e6b9486ec45327631568c/src/Core/ColorBgra.cs
// https://github.com/rivy/OpenPDN/blob/cca476b0df2a2f70996e6b9486ec45327631568c/src/Data/UserBlendOps.Generated.cs
// MIT License, Copyright dotPDN LLC.
use crate::pixel::Rgba8;
include!("legacy_table.rs");

pub fn fast_scale_byte_by_byte(a: u8, b: u8) -> u8 {
    let r1 = a as u32 * b as u32 + 0x80;
    (((r1 >> 8) + r1) >> 8) as u8
}
pub fn fast_divide_short_by_byte(n: u16, d: u8) -> u32 {
    let [m, a, s] = MAS_TABLE[d as usize];
    ((n as u64 * m as u64 + a as u64) >> s) as u32
}
pub fn clamp_to_byte_f32(x: f32) -> u8 {
    x.clamp(0.0, 255.0) as u8
}
pub fn clamp_to_byte_f64(x: f64) -> u8 {
    x.clamp(0.0, 255.0) as u8
}
pub fn clamp_to_byte_i32(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}
pub fn lerp_f32(from: f32, to: f32, frac: f32) -> f32 {
    from + frac * (to - from)
}
pub fn lerp_f64(from: f64, to: f64, frac: f64) -> f64 {
    from + frac * (to - from)
}

pub fn new_alpha(c: Rgba8, a: u8) -> Rgba8 {
    Rgba8 { a, ..c }
}
pub fn blend(ca: Rgba8, cb: Rgba8, cb_alpha: u8) -> Rgba8 {
    let ca_a = fast_scale_byte_by_byte(255 - cb_alpha, ca.a) as u32;
    let cb_a = fast_scale_byte_by_byte(cb_alpha, cb.a) as u32;
    let total = ca_a + cb_a;
    if total == 0 {
        return Rgba8::default();
    }
    let channel = |a: u8, b: u8| ((a as u32 * ca_a + b as u32 * cb_a) / total) as u8;
    Rgba8 {
        r: channel(ca.r, cb.r),
        g: channel(ca.g, cb.g),
        b: channel(ca.b, cb.b),
        a: total as u8,
    }
}
pub fn lerp_color_f32(from: Rgba8, to: Rgba8, frac: f32) -> Rgba8 {
    let ch = |a: u8, b: u8| clamp_to_byte_f32(lerp_f32(a as f32, b as f32, frac));
    Rgba8 {
        r: ch(from.r, to.r),
        g: ch(from.g, to.g),
        b: ch(from.b, to.b),
        a: ch(from.a, to.a),
    }
}
pub fn lerp_color_f64(from: Rgba8, to: Rgba8, frac: f64) -> Rgba8 {
    let ch = |a: u8, b: u8| clamp_to_byte_f64(lerp_f64(a as f64, b as f64, frac));
    Rgba8 {
        r: ch(from.r, to.r),
        g: ch(from.g, to.g),
        b: ch(from.b, to.b),
        a: ch(from.a, to.a),
    }
}
pub fn blend_colors4_w16ip(colors: [Rgba8; 4], weights: [u32; 4]) -> Rgba8 {
    let af: u64 = colors
        .iter()
        .zip(weights)
        .map(|(c, w)| c.a as u64 * w as u64)
        .sum();
    let a = ((af + 32768) >> 16) as u8;
    if a == 0 {
        return Rgba8::default();
    }
    let ch = |select: fn(Rgba8) -> u8| -> u8 {
        (colors
            .iter()
            .zip(weights)
            .map(|(c, w)| c.a as u64 * select(*c) as u64 * w as u64)
            .sum::<u64>()
            / af) as u8
    };
    Rgba8 {
        r: ch(|c| c.r),
        g: ch(|c| c.g),
        b: ch(|c| c.b),
        a,
    }
}
fn divide_blend(n: u32, d: u32) -> u8 {
    let [m, a, s] = MAS_TABLE[d as usize];
    (((n as u64 * m as u64 + a as u64) >> s) as u32) as u8
}
pub fn normal_apply(lhs: Rgba8, rhs: Rgba8) -> Rgba8 {
    normal_apply_with_opacity(lhs, rhs, 255)
}
pub fn normal_apply_with_opacity(lhs: Rgba8, rhs: Rgba8, opacity: u8) -> Rgba8 {
    let lhs_a = lhs.a;
    let rhs_a = fast_scale_byte_by_byte(rhs.a, opacity);
    let y = fast_scale_byte_by_byte(lhs_a, 255 - rhs_a) as u32;
    let total = y + rhs_a as u32;
    if total == 0 {
        return Rgba8::default();
    }
    let x = fast_scale_byte_by_byte(lhs_a, rhs_a) as u32;
    let z = rhs_a as u32 - x;
    let ch = |l: u8, r: u8| divide_blend(l as u32 * y + r as u32 * z + r as u32 * x, total);
    Rgba8 {
        r: ch(lhs.r, rhs.r),
        g: ch(lhs.g, rhs.g),
        b: ch(lhs.b, rhs.b),
        a: total as u8,
    }
}
