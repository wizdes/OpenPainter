//! M23: Rectangle Select in every combine mode against masks built here,
//! the amend of a Rectangle Select step, the Crop tool's crop and straighten,
//! the marching-ants outline query, and undo of each.
use op_engine::Engine;
use serde_json::{Value, json};

const W: u32 = 12;
const H: u32 = 10;

fn image(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend([(x * 20) as u8, (y * 25) as u8, ((x + y) * 7) as u8, 255]);
        }
    }
    v
}

fn open() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(W, H, &image(W, H), None).unwrap();
    (e, id)
}

fn rect(e: &Engine, id: u64, r: [f64; 4], mode: &str) {
    e.dispatch(
        id,
        "selection.rectangle",
        json!({"x":r[0],"y":r[1],"width":r[2],"height":r[3],"combine":mode}),
    )
    .unwrap();
}

fn mask(e: &Engine, id: u64) -> Vec<u8> {
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    if sel["active"] != json!(true) {
        return vec![0; (W * H) as usize];
    }
    e.query(id, "doc.selection_mask", json!({})).unwrap()
}

/// A pixel is inside when its center is inside (pixelated rasterization).
fn inside(r: [f64; 4], x: u32, y: u32) -> bool {
    let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
    cx > r[0] && cx < r[0] + r[2] && cy > r[1] && cy < r[1] + r[3]
}

fn expected(f: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..H {
        for x in 0..W {
            v.push(if f(x, y) { 255 } else { 0 });
        }
    }
    v
}

const A: [f64; 4] = [2.0, 2.0, 6.0, 5.0];
const B: [f64; 4] = [5.0, 4.0, 5.0, 5.0];

/// How a mode combines "inside the old selection" and "inside the rectangle".
type Combine = fn(bool, bool) -> bool;

#[test]
fn every_mode_matches_a_mask_built_from_pixel_centers() {
    let cases: [(&str, Combine); 5] = [
        ("replace", |_, b| b),
        ("add", |a, b| a || b),
        ("subtract", |a, b| a && !b),
        ("intersect", |a, b| a && b),
        ("invert", |a, b| a != b),
    ];
    for (mode, op) in cases {
        let (e, id) = open();
        rect(&e, id, A, "replace");
        rect(&e, id, B, mode);
        assert_eq!(
            mask(&e, id),
            expected(|x, y| op(inside(A, x, y), inside(B, x, y))),
            "mode {mode}"
        );
        let info = e.snapshot(id).unwrap();
        assert_eq!(info.history.labels.len(), 3, "one step per selection");
        assert_eq!(info.history.labels[2], "Rectangle Select");
    }
}

#[test]
fn with_no_selection_every_mode_combines_with_nothing() {
    for (mode, selects) in [
        ("replace", true),
        ("add", true),
        ("subtract", false),
        ("intersect", false),
        ("invert", true),
    ] {
        let (e, id) = open();
        rect(&e, id, B, mode);
        assert_eq!(
            mask(&e, id),
            expected(|x, y| selects && inside(B, x, y)),
            "mode {mode}"
        );
    }
}

#[test]
fn the_rectangle_is_clipped_to_the_canvas() {
    let (e, id) = open();
    rect(&e, id, [-3.0, 7.0, 6.0, 20.0], "replace");
    assert_eq!(
        mask(&e, id),
        expected(|x, y| x < 3 && y >= 7),
        "clipped rectangle"
    );
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["bounds"], json!([0, 7, 3, 3]));
}

#[test]
fn amend_replaces_the_rectangle_step_against_the_earlier_selection() {
    let (e, id) = open();
    rect(&e, id, A, "replace");
    rect(&e, id, [0.0, 0.0, 2.0, 2.0], "add");
    let steps = e.snapshot(id).unwrap().history.labels.len();
    e.dispatch(
        id,
        "selection.rectangle",
        json!({"x":B[0],"y":B[1],"width":B[2],"height":B[3],"combine":"add","amend":true}),
    )
    .unwrap();
    assert_eq!(
        e.snapshot(id).unwrap().history.labels.len(),
        steps,
        "no new step"
    );
    assert_eq!(
        mask(&e, id),
        expected(|x, y| inside(A, x, y) || inside(B, x, y)),
        "the 2 x 2 rectangle is gone, B is added to A"
    );
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(mask(&e, id), expected(|x, y| inside(A, x, y)));
}

#[test]
fn amend_after_another_command_adds_a_step() {
    let (e, id) = open();
    e.dispatch(id, "selection.select_all", json!({})).unwrap();
    e.dispatch(
        id,
        "selection.rectangle",
        json!({"x":B[0],"y":B[1],"width":B[2],"height":B[3],"combine":"subtract","amend":true}),
    )
    .unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 3);
    assert_eq!(mask(&e, id), expected(|x, y| !inside(B, x, y)));
}

fn info(e: &Engine, id: u64) -> (u64, u64) {
    let s = e.snapshot(id).unwrap();
    (u64::from(s.state.width), u64::from(s.state.height))
}

#[test]
fn crop_rect_gives_the_exact_pixels_and_undo_restores_everything() {
    let (e, id) = open();
    rect(&e, id, [3.0, 2.0, 4.0, 4.0], "replace");
    let before = e.query(id, "doc.pixels", json!({})).unwrap();
    let before_mask = mask(&e, id);
    e.dispatch(
        id,
        "image.crop_rect",
        json!({"x":2,"y":1,"width":7,"height":5}),
    )
    .unwrap();
    assert_eq!(info(&e, id), (7, 5));
    let src = image(W, H);
    let mut want = Vec::new();
    for y in 1..6 {
        for x in 2..9 {
            let i = ((y * W + x) * 4) as usize;
            want.extend_from_slice(&src[i..i + 4]);
        }
    }
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), want);
    // The selection moved with the pixels.
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["bounds"], json!([1, 1, 4, 4]));
    let s = e.snapshot(id).unwrap();
    assert_eq!(s.history.labels.last().unwrap(), "Crop");
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(info(&e, id), (u64::from(W), u64::from(H)));
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), before);
    assert_eq!(mask(&e, id), before_mask);
}

#[test]
fn crop_rect_outside_the_canvas_is_refused() {
    let (e, id) = open();
    assert!(
        e.dispatch(
            id,
            "image.crop_rect",
            json!({"x":8,"y":0,"width":5,"height":2})
        )
        .is_err()
    );
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 1);
}

fn two_layer_doc() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(40, 30, &image(40, 30), None).unwrap();
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    e.dispatch(
        id,
        "tool.stroke",
        json!({"points":"5 5 1 30 20 1","width":4.0,"color":"ff0000ff"}),
    )
    .unwrap();
    (e, id)
}

#[test]
fn straighten_equals_rotate_zoom_on_each_layer_then_crop() {
    let crop = json!({"x":6,"y":4,"width":26,"height":20});
    let (a, ida) = two_layer_doc();
    a.dispatch(
        ida,
        "image.crop_rect",
        json!({"x":6,"y":4,"width":26,"height":20,"angle":5.0}),
    )
    .unwrap();
    let (b, idb) = two_layer_doc();
    for i in 0..2 {
        b.dispatch(idb, "layer.set_active", json!({"index":i}))
            .unwrap();
        b.dispatch(idb, "effect.layers.rotate_zoom", json!({"angle":5.0}))
            .unwrap();
    }
    b.dispatch(idb, "image.crop_rect", crop).unwrap();
    assert_eq!(info(&a, ida), (26, 20));
    assert_eq!(a.snapshot(ida).unwrap().history.labels.len(), 4);
    let (sa, sb) = (a.snapshot(ida).unwrap(), b.snapshot(idb).unwrap());
    for i in 0..2 {
        assert_eq!(
            op_engine_layer(&sa.state, i),
            op_engine_layer(&sb.state, i),
            "layer {i}"
        );
    }
    // Composite bytes agree too, and differ from an unrotated crop.
    assert_eq!(
        a.query(ida, "doc.pixels", json!({})).unwrap(),
        b.query(idb, "doc.pixels", json!({})).unwrap()
    );
    let (c, idc) = two_layer_doc();
    c.dispatch(
        idc,
        "image.crop_rect",
        json!({"x":6,"y":4,"width":26,"height":20}),
    )
    .unwrap();
    assert_ne!(
        a.query(ida, "doc.pixels", json!({})).unwrap(),
        c.query(idc, "doc.pixels", json!({})).unwrap()
    );
    a.dispatch(ida, "edit.undo", json!({})).unwrap();
    assert_eq!(info(&a, ida), (40, 30));
}

fn op_engine_layer(s: &op_core::doc::DocState, i: usize) -> Vec<Vec<u8>> {
    s.layers[i]
        .tiles
        .iter()
        .map(|(c, _)| s.read_tile(i, c).unwrap())
        .collect()
}

#[test]
fn selection_path_gives_rectangle_rings_and_mask_edges() {
    let (e, id) = open();
    let none: Value =
        serde_json::from_slice(&e.query(id, "doc.selection_path", json!({})).unwrap()).unwrap();
    assert_eq!(none["active"], json!(false));
    rect(&e, id, A, "replace");
    let p: Value =
        serde_json::from_slice(&e.query(id, "doc.selection_path", json!({})).unwrap()).unwrap();
    assert_eq!(p["active"], json!(true));
    let rings = p["paths"].as_array().unwrap();
    assert_eq!(rings.len(), 1);
    let mut xs: Vec<f64> = rings[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(xs.len(), 8);
    let ys: Vec<f64> = xs.iter().skip(1).step_by(2).copied().collect();
    xs = xs.iter().step_by(2).copied().collect();
    assert_eq!(xs.iter().cloned().fold(f64::MAX, f64::min), 2.0);
    assert_eq!(xs.iter().cloned().fold(f64::MIN, f64::max), 8.0);
    assert_eq!(ys.iter().cloned().fold(f64::MAX, f64::min), 2.0);
    assert_eq!(ys.iter().cloned().fold(f64::MIN, f64::max), 7.0);
    // A Magic Wand selection is a pixel mask: the outline is its edges.
    let (e, id) = {
        let e = Engine::new(None).unwrap();
        let mut px = vec![0u8; 3 * 3 * 4];
        for p in px.chunks_exact_mut(4) {
            p.copy_from_slice(&[0, 0, 0, 255]);
        }
        px[(3 + 1) * 4..(3 + 1) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
        let id = e.import_pixels(3, 3, &px, None).unwrap();
        (e, id)
    };
    e.dispatch(id, "tool.magic_wand", json!({"x":1,"y":1}))
        .unwrap();
    let p: Value =
        serde_json::from_slice(&e.query(id, "doc.selection_path", json!({})).unwrap()).unwrap();
    let seg: Vec<u64> = p["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(seg, vec![1, 1, 2, 1, 1, 2, 2, 2, 1, 1, 1, 2, 2, 1, 2, 2]);
}

#[test]
fn resolution_query_gives_the_document_dpi() {
    let (e, id) = open();
    let r: Value =
        serde_json::from_slice(&e.query(id, "doc.resolution", json!({})).unwrap()).unwrap();
    assert!(r["dpi"].as_f64().unwrap() > 0.0);
}
