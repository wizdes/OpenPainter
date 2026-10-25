//! Read-only answers about the document's color profile for the Color Profile
//! dialog (M27): `doc.profile` (JSON) and `doc.profile_icc` (the raw ICC bytes,
//! empty when the document has no profile and is treated as sRGB).
use op_core::doc::DocState;
use serde_json::{Value, json};

fn space_name(space: lcms2::ColorSpaceSignature) -> &'static str {
    use lcms2::ColorSpaceSignature as S;
    match space {
        S::RgbData => "RGB",
        S::GrayData => "Gray",
        S::CmykData => "CMYK",
        S::LabData => "Lab",
        S::XYZData => "XYZ",
        _ => "Other",
    }
}

/// `{"embedded", "description", "color_space", "icc_len"}`. A document with no
/// profile reports `embedded: false`, description "sRGB" and space "RGB".
pub fn profile_info(state: &DocState) -> Value {
    match &state.profile {
        None => {
            json!({"embedded": false, "description": "sRGB", "color_space": "RGB", "icc_len": 0})
        }
        Some(bytes) => match op_color::Profile::from_icc(bytes) {
            Ok(p) => json!({
                "embedded": true,
                "description": p.description(),
                "color_space": space_name(p.color_space()),
                "icc_len": bytes.len(),
            }),
            Err(e) => json!({
                "embedded": true,
                "description": format!("Invalid ICC profile: {e}"),
                "color_space": "Other",
                "icc_len": bytes.len(),
            }),
        },
    }
}

pub fn profile_icc(state: &DocState) -> Vec<u8> {
    state
        .profile
        .as_deref()
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}
