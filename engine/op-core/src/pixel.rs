use serde::{Deserialize, Serialize};

/// Stored straight-alpha pixel. OpenPainter stores RGBA; paint.net ColorBgra stores BGRA.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl From<[u8; 4]> for Rgba8 {
    fn from(v: [u8; 4]) -> Self {
        Self {
            r: v[0],
            g: v[1],
            b: v[2],
            a: v[3],
        }
    }
}
impl From<Rgba8> for [u8; 4] {
    fn from(v: Rgba8) -> Self {
        [v.r, v.g, v.b, v.a]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PremulF32 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
impl PremulF32 {
    pub fn from_straight(c: Rgba8) -> Self {
        let a = c.a as f32 / 255.0;
        Self {
            r: c.r as f32 / 255.0 * a,
            g: c.g as f32 / 255.0 * a,
            b: c.b as f32 / 255.0 * a,
            a,
        }
    }
    /// Use only for mixing or filtering. Zero alpha loses hidden straight color.
    pub fn to_straight_rounded(self) -> Rgba8 {
        if self.a <= 0.0 {
            return Rgba8::default();
        }
        Rgba8 {
            r: round_unit_to_u8(self.r / self.a),
            g: round_unit_to_u8(self.g / self.a),
            b: round_unit_to_u8(self.b / self.a),
            a: round_unit_to_u8(self.a),
        }
    }
}
pub fn round_unit_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}
pub fn round_unit_to_u8_f64(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

/// Every operation must declare this arithmetic contract (ARCHITECTURE section 3):
/// alpha form, working space, each precision and quantization stage, border,
/// target behavior, and reference version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub alpha_form: AlphaForm,
    pub working_space: WorkingSpace,
    pub stages: Vec<Stage>,
    pub border: Border,
    pub target: Target,
    pub reference: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaForm {
    Straight,
    Premultiplied,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkingSpace {
    Encoded,
    Linear,
    Custom(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub precision: Precision,
    pub quantize_after: bool,
    pub rounding: RoundingRule,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precision {
    U8,
    F32,
    F64,
    I32,
    I64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundingRule {
    HalfUp,
    Truncate,
    Floor,
    Ceiling,
    Custom(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Border {
    Clamp,
    Wrap,
    Mirror,
    Transparent,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Legacy336,
    Pdn51,
    Spec,
}
