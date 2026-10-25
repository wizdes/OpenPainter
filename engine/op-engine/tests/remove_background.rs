//! M53: Remove Background. The Mac side computes a foreground mask (8-bit,
//! one byte per document pixel) and sends it as base64:
//! - `selection.from_mask` replaces the selection with a Coverage selection
//!   holding the mask bytes exactly (soft values survive), one history step.
//! - `layer.remove_background` multiplies the active layer's alpha by the
//!   mask, one history step; other layers stay as they were.
//! - `layer.pixels` is the active layer's straight RGBA (the Vision input).
//!
//! Every expected value is computed here from the inputs, never from the
//! engine's own code.
use op_engine::{Engine, Error, JobTicket};
use serde_json::{Value, json};

const W: u32 = 37; // not a tile multiple, so edge tiles are covered
const H: u32 = 29;

fn image() -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..H {
        for x in 0..W {
            v.extend([
                (x * 6) as u8,
                (y * 8) as u8,
                ((x + y) * 3) as u8,
                (255 - (x + y) * 3) as u8,
            ]);
        }
    }
    v
}

/// A mask with every value 0..=255 somewhere, plus solid 0 and 255 areas.
fn soft_mask() -> Vec<u8> {
    let mut m = Vec::new();
    for y in 0..H {
        for x in 0..W {
            m.push(if x < 4 {
                0
            } else if x > W - 5 {
                255
            } else {
                ((y * W + x) % 256) as u8
            });
        }
    }
    m
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn open() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(W, H, &image(), None).unwrap();
    (e, id)
}

fn steps(e: &Engine, id: u64) -> Vec<String> {
    e.snapshot(id).unwrap().history.labels.clone()
}

fn selection_active(e: &Engine, id: u64) -> bool {
    let v: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    v["active"] == json!(true)
}

fn sel_mask(e: &Engine, id: u64) -> Vec<u8> {
    e.query(id, "doc.selection_mask", json!({})).unwrap()
}

fn layer_pixels(e: &Engine, id: u64, params: Value) -> Vec<u8> {
    e.query(id, "layer.pixels", params).unwrap()
}

/// Straight alpha times coverage, rounded; alpha 0 is transparent black.
fn multiplied(src: &[u8], mask: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (px, &m) in src.chunks(4).zip(mask) {
        let a = (f64::from(px[3]) * f64::from(m) / 255.0).round() as u8;
        if a == 0 {
            out.extend([0, 0, 0, 0]);
        } else {
            out.extend([px[0], px[1], px[2], a]);
        }
    }
    out
}

#[test]
fn layer_pixels_is_the_active_layer_in_straight_rgba() {
    let (e, id) = open();
    assert_eq!(layer_pixels(&e, id, json!({})), image());
    assert_eq!(layer_pixels(&e, id, json!({"index": 0})), image());
    assert!(e.query(id, "layer.pixels", json!({"index": 3})).is_err());
}

#[test]
fn select_foreground_keeps_every_soft_value_in_one_step() {
    let (e, id) = open();
    // An existing selection is replaced, not combined.
    e.dispatch(
        id,
        "selection.rectangle",
        json!({"x":0.0,"y":0.0,"width":3.0,"height":3.0}),
    )
    .unwrap();
    let before = steps(&e, id).len();
    let mask = soft_mask();
    e.dispatch(id, "selection.from_mask", json!({"mask": base64(&mask)}))
        .unwrap();
    assert_eq!(sel_mask(&e, id), mask, "coverage bytes are kept exactly");
    let soft = sel_mask(&e, id)
        .iter()
        .filter(|&&c| (1..=254).contains(&c))
        .count();
    assert!(soft >= 254, "soft values survive: {soft}");
    assert_eq!(steps(&e, id).len(), before + 1);
    assert_eq!(steps(&e, id).last().unwrap(), "Select Foreground");
    assert_eq!(layer_pixels(&e, id, json!({})), image(), "pixels unchanged");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(steps(&e, id).len(), before + 1, "undo keeps the redo step");
    assert_eq!(e.snapshot(id).unwrap().history.index, before - 1);
    let rect: Vec<u8> = (0..H)
        .flat_map(|y| (0..W).map(move |x| if x < 3 && y < 3 { 255 } else { 0 }))
        .collect();
    assert_eq!(sel_mask(&e, id), rect, "undo brings the rectangle back");
}

#[test]
fn remove_background_multiplies_alpha_in_one_step() {
    let (e, id) = open();
    let before = steps(&e, id).len();
    let mask = soft_mask();
    e.dispatch(
        id,
        "layer.remove_background",
        json!({"mask": base64(&mask)}),
    )
    .unwrap();
    assert_eq!(layer_pixels(&e, id, json!({})), multiplied(&image(), &mask));
    assert_eq!(steps(&e, id).len(), before + 1);
    assert_eq!(steps(&e, id).last().unwrap(), "Remove Background");
    assert!(!selection_active(&e, id), "no selection is made");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(layer_pixels(&e, id, json!({})), image(), "undo restores");
    e.dispatch(id, "edit.redo", json!({})).unwrap();
    assert_eq!(layer_pixels(&e, id, json!({})), multiplied(&image(), &mask));
}

#[test]
fn remove_background_changes_only_the_active_layer() {
    let (e, id) = open();
    e.dispatch(id, "layer.add", json!({"name": "Top"})).unwrap();
    // A new layer goes on top but does not become active.
    e.dispatch(id, "layer.set_active", json!({"index": 1}))
        .unwrap();
    e.dispatch(id, "selection.select_all", json!({})).unwrap();
    e.dispatch(id, "selection.fill", json!({"color": "3366ccff"}))
        .unwrap();
    e.dispatch(id, "selection.deselect", json!({})).unwrap();
    let top = layer_pixels(&e, id, json!({"index": 1}));
    assert_eq!(&top[..4], &[0x33, 0x66, 0xcc, 0xff]);
    e.dispatch(id, "layer.set_active", json!({"index": 0}))
        .unwrap();
    assert_eq!(layer_pixels(&e, id, json!({})), image(), "active is 0");
    let mask = soft_mask();
    e.dispatch(
        id,
        "layer.remove_background",
        json!({"mask": base64(&mask)}),
    )
    .unwrap();
    assert_eq!(
        layer_pixels(&e, id, json!({"index": 0})),
        multiplied(&image(), &mask)
    );
    assert_eq!(layer_pixels(&e, id, json!({"index": 1})), top);
}

#[test]
fn a_mask_of_the_wrong_size_or_bad_text_changes_nothing() {
    let (e, id) = open();
    let before = steps(&e, id).len();
    let short = vec![255u8; (W * H - 1) as usize];
    for cmd in ["selection.from_mask", "layer.remove_background"] {
        for mask in [base64(&short), "not base64!".to_owned(), String::new()] {
            let r = e.dispatch(id, cmd, json!({"mask": mask}));
            assert!(
                matches!(r, Err(Error::BadArgument(_))),
                "{cmd} {mask:?}: {r:?}"
            );
        }
        assert!(e.dispatch(id, cmd, json!({})).is_err(), "{cmd}: no mask");
    }
    assert_eq!(steps(&e, id).len(), before);
    assert_eq!(layer_pixels(&e, id, json!({})), image());
    assert!(!selection_active(&e, id));
}

#[test]
fn an_all_zero_mask_selects_nothing_and_is_refused() {
    // Select Foreground with no foreground would make an empty selection;
    // the Mac side says "no foreground found" instead, and the engine refuses.
    let (e, id) = open();
    let zero = vec![0u8; (W * H) as usize];
    let r = e.dispatch(id, "selection.from_mask", json!({"mask": base64(&zero)}));
    assert!(matches!(r, Err(Error::BadArgument(_))), "{r:?}");
    assert!(!selection_active(&e, id));
}

#[test]
fn a_cancelled_run_commits_nothing() {
    let (e, id) = open();
    let before = steps(&e, id).len();
    for cmd in ["selection.from_mask", "layer.remove_background"] {
        let ticket = JobTicket::new(7);
        ticket.cancel();
        let r = e.dispatch_with(id, cmd, json!({"mask": base64(&soft_mask())}), &ticket);
        assert_eq!(r, Err(Error::Cancelled), "{cmd}");
    }
    assert_eq!(steps(&e, id).len(), before);
    assert_eq!(layer_pixels(&e, id, json!({})), image());
    assert!(!selection_active(&e, id));
}
