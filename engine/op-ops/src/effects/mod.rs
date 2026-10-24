pub mod auto_level;
pub mod black_and_white;
pub mod brightness_contrast;
pub mod content_aware;
pub mod hue_saturation;
pub mod invert_alpha;
pub mod invert_colors;
pub mod posterize;
pub mod sepia;
pub mod temperature_tint;

pub(crate) fn intensity([r, g, b, _]: [u8; 4]) -> u8 {
    ((7471 * u32::from(b) + 38470 * u32::from(g) + 19595 * u32::from(r)) >> 16) as u8
}
