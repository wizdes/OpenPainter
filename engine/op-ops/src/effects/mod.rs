pub mod artistic_common;
pub mod auto_level;
pub mod black_and_white;
pub mod brightness_contrast;
pub mod content_aware;
pub mod curves;
pub mod edge_detect;
pub mod emboss;
pub mod exposure;
pub mod fragment;
pub mod gaussian_blur;
pub mod highlights_shadows;
pub mod hue_saturation;
pub mod ink_sketch;
pub mod invert_alpha;
pub mod invert_colors;
pub mod levels;
pub mod median_blur;
pub mod motion_blur;
pub mod oil_painting;
pub mod outline;
pub mod pencil_sketch;
pub mod posterize;
pub mod radial_blur;
pub mod relief;
pub mod sepia;
pub mod stylize_common;
pub mod temperature_tint;
pub mod zoom_blur;

pub(crate) fn intensity([r, g, b, _]: [u8; 4]) -> u8 {
    ((7471 * u32::from(b) + 38470 * u32::from(g) + 19595 * u32::from(r)) >> 16) as u8
}
