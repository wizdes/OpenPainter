//! M34: the clipboard. `clipboard.copy` crops to the selection with partial
//! coverage as alpha (the active layer, or the flattened image with
//! `merged`); `paste_begin` floats pixels in a move session that commits one
//! step ("Paste" or "Paste into New Layer") or cancels to the exact state
//! before; pasted pixels in another profile are converted with op-color.
use op_engine::{Engine, PasteRequest};
use op_ops::image::Resampling;
use op_tools::r#move::{Affine, MoveOptions};
use serde_json::{Value, json};

const W: u32 = 10;
const H: u32 = 8;

fn image(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend([(x * 20) as u8, (y * 25) as u8, 77, (255 - x * 9) as u8]);
        }
    }
    v
}

fn open() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(W, H, &image(W, H), None).unwrap();
    (e, id)
}

fn options() -> MoveOptions {
    MoveOptions {
        transform: Affine::identity(),
        resampling: Resampling::Nearest,
        copy: false,
    }
}

/// (x, y, width, height, rgba) from a `clipboard.copy` answer.
fn copied(e: &Engine, id: u64, merged: bool) -> (u32, u32, u32, u32, Vec<u8>) {
    let b = e
        .query(id, "clipboard.copy", json!({"merged": merged}))
        .unwrap();
    let n = |i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    (n(0), n(1), n(2), n(3), b[16..].to_vec())
}

fn layer(e: &Engine, id: u64, index: usize) -> Vec<u8> {
    let s = e.snapshot(id).unwrap();
    let mut out = Vec::new();
    let tile = s
        .state
        .read_tile(index, op_core::tiles::TileCoord { x: 0, y: 0 })
        .unwrap();
    for y in 0..s.state.height as usize {
        out.extend_from_slice(&tile[y * 256 * 4..y * 256 * 4 + s.state.width as usize * 4]);
    }
    out
}

fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}

/// A selection from x 1.5 to 5.5 and y 2 to 5: the left and right columns
/// are half covered.
fn select_half_pixels(e: &Engine, id: u64) {
    e.dispatch(
        id,
        "selection.paste",
        json!({"polygons": "{\"polygonList\":[\"1.5,2,5.5,2,5.5,5,1.5,5\"]}", "combine": "replace"}),
    )
    .unwrap();
}

#[test]
fn copy_without_selection_is_the_whole_layer() {
    let (e, id) = open();
    let (x, y, w, h, rgba) = copied(&e, id, false);
    assert_eq!((x, y, w, h), (0, 0, W, H));
    assert_eq!(rgba, image(W, H));
}

#[test]
fn copy_crops_to_the_selection_with_partial_coverage_as_alpha() {
    let (e, id) = open();
    select_half_pixels(&e, id);
    let (x, y, w, h, rgba) = copied(&e, id, false);
    assert_eq!((x, y, w, h), (1, 2, 5, 3));
    let mask = e.query(id, "doc.selection_mask", json!({})).unwrap();
    let src = image(W, H);
    for row in 0..h as usize {
        for col in 0..w as usize {
            let (sx, sy) = (x as usize + col, y as usize + row);
            let s = &src[(sy * W as usize + sx) * 4..][..4];
            let got = &rgba[(row * w as usize + col) * 4..][..4];
            // The edge columns are half covered: alpha is about half.
            let c: u32 = if col == 0 || col == 4 { 128 } else { 255 };
            let a = ((s[3] as u32 * c + 127) / 255) as u8;
            assert!(
                got[3].abs_diff(a) <= 1,
                "alpha at {sx},{sy}: {} vs {a}",
                got[3]
            );
            assert_eq!(&got[..3], &s[..3], "color at {sx},{sy}");
            if c == 255 {
                assert_eq!(got[3], s[3]);
            }
        }
    }
    // The pixelated mask says the middle columns are in.
    assert_eq!(mask[2 * W as usize + 3], 255);
}

#[test]
fn copy_merged_is_the_flattened_image() {
    let (e, id) = open();
    e.dispatch(id, "layer.add", json!({"name": "Layer 2"}))
        .unwrap();
    e.dispatch(id, "layer.set_active", json!({"index": 1}))
        .unwrap();
    e.dispatch(
        id,
        "selection.paste",
        json!({"polygons": "{\"polygonList\":[\"0,0,4,0,4,4,0,4\"]}", "combine": "replace"}),
    )
    .unwrap();
    e.dispatch(id, "selection.fill", json!({"color": "ff000080"}))
        .unwrap();
    e.dispatch(id, "selection.deselect", json!({})).unwrap();
    let flat = e.snapshot(id).unwrap().pixels(false).unwrap();
    let (x, y, w, h, rgba) = copied(&e, id, true);
    assert_eq!((x, y, w, h), (0, 0, W, H));
    assert_eq!(rgba, flat);
    // The plain copy is the new layer only.
    let (_, _, _, _, top) = copied(&e, id, false);
    assert_eq!(top, layer(&e, id, 1));
}

#[test]
fn copy_of_an_empty_selection_is_an_error() {
    let (e, id) = open();
    e.dispatch(
        id,
        "selection.paste",
        json!({"polygons": "{\"polygonList\":[\"20,20,21,20,21,21,20,21\"]}", "combine": "replace"}),
    )
    .unwrap();
    assert!(e.query(id, "clipboard.copy", json!({})).is_err());
}

#[allow(clippy::too_many_arguments)]
fn paste(
    e: &Engine,
    id: u64,
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    x: i64,
    y: i64,
    new_layer: bool,
) -> Option<String> {
    e.paste_begin(
        id,
        PasteRequest {
            width: w,
            height: h,
            rgba,
            icc: None,
            x,
            y,
            new_layer,
            options: options(),
        },
    )
    .unwrap()
    .converted_from
}

#[test]
fn paste_into_new_layer_round_trips_bytes_in_one_step() {
    let (e, id) = open();
    select_half_pixels(&e, id);
    let (_, _, w, h, rgba) = copied(&e, id, false);
    e.dispatch(id, "selection.deselect", json!({})).unwrap();
    let before = info(&e, id);
    assert_eq!(paste(&e, id, rgba.clone(), w, h, 2, 1, true), None);
    // Nothing is committed while the session is open.
    assert_eq!(info(&e, id)["history_labels"], before["history_labels"]);
    assert!(e.snapshot(id).unwrap().preview.is_some());
    e.move_finish(id).unwrap();
    let after = info(&e, id);
    let labels: Vec<_> = after["history_labels"].as_array().unwrap().clone();
    assert_eq!(labels.last().unwrap(), "Paste into New Layer");
    assert_eq!(
        labels.len(),
        before["history_labels"].as_array().unwrap().len() + 1
    );
    assert_eq!(after["layers"].as_array().unwrap().len(), 2);
    assert_eq!(after["active_layer"], 1);
    let top = layer(&e, id, 1);
    for row in 0..h as usize {
        for col in 0..w as usize {
            let i = ((1 + row) * W as usize + 2 + col) * 4;
            assert_eq!(&top[i..i + 4], &rgba[(row * w as usize + col) * 4..][..4]);
        }
    }
    // The selection is the pasted bounds.
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["bounds"], json!([2, 1, w, h]));
    let s = e.snapshot(id).unwrap();
    assert_eq!(
        s.history.commands.last().unwrap(),
        "edit.paste_into_new_layer"
    );
}

#[test]
fn paste_cancel_restores_the_exact_state() {
    let (e, id) = open();
    select_half_pixels(&e, id);
    let before = e.snapshot(id).unwrap();
    let before_info = info(&e, id);
    let before_pixels = layer(&e, id, 0);
    paste(&e, id, vec![9; 3 * 2 * 4], 3, 2, 1, 1, true);
    e.move_update(
        id,
        MoveOptions {
            transform: Affine::translation(2.0, 1.0),
            ..options()
        },
    )
    .unwrap();
    e.move_cancel(id).unwrap();
    let after = e.snapshot(id).unwrap();
    assert!(after.preview.is_none());
    assert_eq!(
        info(&e, id)["history_labels"],
        before_info["history_labels"]
    );
    assert_eq!(info(&e, id)["layers"], before_info["layers"]);
    assert_eq!(layer(&e, id, 0), before_pixels);
    assert_eq!(after.state.selection, before.state.selection);
    assert_eq!(after.revision, before.revision);
}

#[test]
fn paste_on_the_active_layer_goes_over_and_moves() {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(6, 4, &[0; 6 * 4 * 4], None).unwrap();
    // 4 x 2 pixels pasted half off the left edge; the move brings them in.
    let px: Vec<u8> = (0..8).flat_map(|i| [i * 30, 200, 10, 255 - i]).collect();
    paste(&e, id, px.clone(), 4, 2, -2, 0, false);
    e.move_update(
        id,
        MoveOptions {
            transform: Affine::translation(3.0, 1.0),
            ..options()
        },
    )
    .unwrap();
    e.move_finish(id).unwrap();
    let labels = info(&e, id)["history_labels"].clone();
    assert_eq!(labels, json!(["Open", "Paste"]));
    let s = e.snapshot(id).unwrap();
    let tile = s
        .state
        .read_tile(0, op_core::tiles::TileCoord { x: 0, y: 0 })
        .unwrap();
    for row in 0..2usize {
        for col in 0..4usize {
            let (x, y) = (1 + col, 1 + row);
            let i = (y * 256 + x) * 4;
            assert_eq!(
                &tile[i..i + 4],
                &px[(row * 4 + col) * 4..][..4],
                "at {x},{y}"
            );
        }
    }
}

#[test]
fn paste_converts_another_profile_with_op_color() {
    let e = Engine::new(None).unwrap();
    let p3 = op_color::Profile::builtin("display-p3").unwrap();
    let id = e.import_pixels(4, 4, &[0; 64], Some(&p3.bytes())).unwrap();
    let src = vec![255, 0, 0, 255, 0, 128, 255, 200];
    let converted = e
        .paste_begin(
            id,
            PasteRequest {
                width: 2,
                height: 1,
                rgba: src.clone(),
                icc: None,
                x: 0,
                y: 0,
                new_layer: true,
                options: options(),
            },
        )
        .unwrap()
        .converted_from;
    assert_eq!(converted.as_deref(), Some("sRGB"));
    e.move_finish(id).unwrap();
    let mut expected = src.clone();
    let srgb = op_color::Profile::builtin("srgb").unwrap();
    op_color::convert_rgba(
        &srgb,
        &p3,
        op_color::RenderingIntent::Perceptual,
        false,
        &mut expected,
    )
    .unwrap();
    let top = layer(&e, id, 1);
    assert_eq!(&top[..8], &expected[..]);
    // Pinned op-color numbers: sRGB red is (234, 51, 35) in Display P3.
    // (0, 128, 255) is (55, 126, 247).
    assert_eq!(&top[..8], &[234, 51, 35, 255, 55, 126, 247, 200]);
    // The same profile is never converted.
    let again = e
        .paste_begin(
            id,
            PasteRequest {
                width: 2,
                height: 1,
                rgba: src.clone(),
                icc: Some(p3.bytes().to_vec()),
                x: 0,
                y: 0,
                new_layer: false,
                options: options(),
            },
        )
        .unwrap();
    assert_eq!(again.converted_from, None);
    e.move_cancel(id).unwrap();
}

#[test]
fn paste_while_a_move_runs_is_busy_and_a_command_finishes_it() {
    let (e, id) = open();
    paste(&e, id, vec![1; 4], 1, 1, 0, 0, false);
    assert!(
        e.paste_begin(
            id,
            PasteRequest {
                width: 1,
                height: 1,
                rgba: vec![1; 4],
                icc: None,
                x: 0,
                y: 0,
                new_layer: false,
                options: options(),
            }
        )
        .is_err()
    );
    // Any command finishes the paste first (one step), then runs.
    e.dispatch(id, "selection.deselect", json!({})).unwrap();
    assert_eq!(
        info(&e, id)["history_labels"],
        json!(["Open", "Paste", "Deselect"])
    );
}

#[test]
fn profile_icc_is_srgb_for_a_document_without_one() {
    let (e, id) = open();
    let icc = e.query(id, "clipboard.profile_icc", json!({})).unwrap();
    let p = op_color::Profile::from_icc(&icc).unwrap();
    assert_eq!(p.hash(), op_color::Profile::builtin("srgb").unwrap().hash());
}

#[test]
fn decode_png_keeps_straight_alpha() {
    let px: Vec<u8> = vec![10, 20, 30, 1, 200, 100, 50, 0, 1, 2, 3, 128];
    let p3 = op_color::Profile::builtin("display-p3").unwrap().bytes();
    let png = op_engine::save_export::encode_image(
        &json!({"format": "png"}),
        3,
        1,
        &px,
        Some(&p3),
        None,
        None,
    )
    .unwrap();
    let d = op_engine::decode_png(&png).unwrap();
    assert_eq!((d.width, d.height), (3, 1));
    assert_eq!(d.pixels, px);
    assert_eq!(d.icc.as_deref(), Some(&p3[..]));
}

#[test]
fn one_undo_removes_a_paste_into_new_layer() {
    let (e, id) = open();
    paste(&e, id, vec![5; 2 * 2 * 4], 2, 2, 1, 1, true);
    e.move_finish(id).unwrap();
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    let i = info(&e, id);
    assert_eq!(i["history_index"], 0);
    assert_eq!(i["layers"].as_array().unwrap().len(), 1);
}
