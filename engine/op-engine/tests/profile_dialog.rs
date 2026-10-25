//! M27: the queries the Color Profile dialog reads, and the fixed sRGB to
//! Display P3 numbers the Mac dialog test compares against (within 1/255).
use op_engine::Engine;
use serde_json::{Value, json};

/// The two pixels the engine profile tests use (`engine.rs`), then pure red and
/// a saturated sky blue, so the check covers opaque gamut-edge colors too
/// (4 x 1, straight alpha).
const IMAGE: [u8; 16] = [
    100, 120, 80, 128, 80, 140, 100, 0, 255, 0, 0, 255, 30, 200, 240, 255,
];
/// `op_color::convert_rgba(sRGB, Display P3, relative, bpc)` of `IMAGE`. The
/// Mac test `ColorProfileTests.testConvertMatchesEngineNumbers` uses these.
const SRGB_TO_P3_RELATIVE_BPC: [u8; 16] = [
    104, 119, 84, 128, 94, 139, 103, 0, 234, 51, 35, 255, 95, 197, 236, 255,
];
/// `doc.pixels` clears the color of fully transparent pixels.
fn visible(mut px: [u8; 16]) -> [u8; 16] {
    for p in px.chunks_exact_mut(4) {
        if p[3] == 0 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        }
    }
    px
}

fn profile(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.profile", json!({})).unwrap()).unwrap()
}
fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}

#[test]
fn op_color_gives_the_pinned_srgb_to_p3_numbers() {
    let src = op_color::Profile::builtin("srgb").unwrap();
    let dst = op_color::Profile::builtin("display-p3").unwrap();
    let mut px = IMAGE.to_vec();
    op_color::convert_rgba(
        &src,
        &dst,
        op_color::RenderingIntent::Relative,
        true,
        &mut px,
    )
    .unwrap();
    assert_eq!(px, SRGB_TO_P3_RELATIVE_BPC);
}

#[test]
fn convert_command_equals_op_color_and_profile_queries_follow() {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(4, 1, &IMAGE, None).unwrap();
    let p = profile(&e, id);
    assert_eq!(p["embedded"], false);
    assert_eq!(p["description"], "sRGB");
    assert_eq!(p["color_space"], "RGB");
    assert_eq!(info(&e, id)["profile_embedded"], false);
    assert!(
        e.query(id, "doc.profile_icc", json!({}))
            .unwrap()
            .is_empty()
    );

    e.dispatch(
        id,
        "image.convert_profile",
        json!({"profile":"display-p3","intent":"relative","bpc":true}),
    )
    .unwrap();
    assert_eq!(
        e.query(id, "doc.pixels", json!({})).unwrap(),
        visible(SRGB_TO_P3_RELATIVE_BPC)
    );
    let raw = e
        .actor_probe(id)
        .unwrap()
        .0
        .read_tile(0, op_core::tiles::TileCoord { x: 0, y: 0 })
        .unwrap();
    assert_eq!(&raw[..16], &SRGB_TO_P3_RELATIVE_BPC, "the stored bytes");
    let p = profile(&e, id);
    assert_eq!(p["embedded"], true);
    assert_eq!(p["description"], "Display P3");
    assert_eq!(p["color_space"], "RGB");
    assert_eq!(info(&e, id)["profile_embedded"], true);
    let icc = e.query(id, "doc.profile_icc", json!({})).unwrap();
    assert_eq!(
        icc,
        op_color::Profile::builtin("display-p3")
            .unwrap()
            .bytes()
            .to_vec()
    );
    assert_eq!(p["icc_len"], icc.len());
}
