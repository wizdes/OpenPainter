//! M29: Ellipse Select and Lasso Select in every combine mode, the Edit menu
//! selection commands (Invert, Erase, Fill, Expand, Paste Selection and the
//! `doc.selection_polygons` query behind Copy Selection) in the no, all and
//! empty selection states, one history step each, and the Magic Wand with no
//! selection in every combine mode.
//!
//! Every expected mask is built here from pixel centers (the engine's
//! `doc.selection_mask` is the pixelated rasterization: a pixel is in when its
//! center is in), never from the engine's own selection code.
use op_engine::Engine;
use serde_json::{Value, json};

const W: u32 = 12;
const H: u32 = 10;

fn image(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            // Alpha varies so the straight-alpha fill and erase math shows.
            v.extend([
                (x * 20) as u8,
                (y * 25) as u8,
                ((x + y) * 7) as u8,
                (255 - x * 9) as u8,
            ]);
        }
    }
    v
}

fn open() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let id = e.import_pixels(W, H, &image(W, H), None).unwrap();
    (e, id)
}

fn sel(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap()
}

/// The selection mask; `None` when nothing is selected.
fn mask(e: &Engine, id: u64) -> Option<Vec<u8>> {
    (sel(e, id)["active"] == json!(true))
        .then(|| e.query(id, "doc.selection_mask", json!({})).unwrap())
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

fn steps(e: &Engine, id: u64) -> Vec<String> {
    e.snapshot(id).unwrap().history.labels.clone()
}

fn enabled(e: &Engine, id: u64, cmd: &str) -> bool {
    let list: Value =
        serde_json::from_slice(&e.query(id, "commands.list", json!({})).unwrap()).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == json!(cmd))
        .unwrap_or_else(|| panic!("{cmd} is not listed"))["enabled"]
        == json!(true)
}

fn center(x: u32, y: u32) -> (f64, f64) {
    (f64::from(x) + 0.5, f64::from(y) + 0.5)
}

const A: [f64; 4] = [2.0, 2.0, 6.0, 5.0];

fn in_rect(r: [f64; 4], x: u32, y: u32) -> bool {
    let (cx, cy) = center(x, y);
    cx > r[0] && cx < r[0] + r[2] && cy > r[1] && cy < r[1] + r[3]
}

fn rect(e: &Engine, id: u64, r: [f64; 4], mode: &str) {
    e.dispatch(
        id,
        "selection.rectangle",
        json!({"x":r[0],"y":r[1],"width":r[2],"height":r[3],"combine":mode}),
    )
    .unwrap();
}

/// Ellipse E's box. No pixel center lies within 0.15 px of its edge (checked
/// below), so the 0.05 px flattening cannot move any pixel.
const E: [f64; 4] = [0.8, 0.9, 6.4, 5.2];

/// Normalized radius of a pixel center for ellipse box `r` (1 is the edge).
fn ellipse_d(r: [f64; 4], x: u32, y: u32) -> f64 {
    let (cx, cy) = center(x, y);
    let (rx, ry) = (r[2] / 2.0, r[3] / 2.0);
    (((cx - r[0] - rx) / rx).powi(2) + ((cy - r[1] - ry) / ry).powi(2)).sqrt()
}

fn in_ellipse(r: [f64; 4], x: u32, y: u32) -> bool {
    ellipse_d(r, x, y) < 1.0
}

fn ellipse(e: &Engine, id: u64, r: [f64; 4], mode: &str) {
    e.dispatch(
        id,
        "selection.ellipse",
        json!({"x":r[0],"y":r[1],"width":r[2],"height":r[3],"combine":mode}),
    )
    .unwrap();
}

/// A self-crossing lasso (a bow tie) plus a plain triangle part, as one path.
const L: [(f64, f64); 6] = [
    (0.3, 0.2),
    (9.7, 8.6),
    (9.6, 0.4),
    (0.4, 8.7),
    (0.2, 4.1),
    (1.3, 4.4),
];

/// Even-odd point in polygon (ray casting), written here for the test.
fn in_poly(poly: &[(f64, f64)], x: u32, y: u32) -> bool {
    let (px, py) = center(x, y);
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        if (y0 > py) != (y1 > py) && px < x0 + (py - y0) * (x1 - x0) / (y1 - y0) {
            inside = !inside;
        }
    }
    inside
}

fn points(poly: &[(f64, f64)]) -> String {
    poly.iter()
        .map(|(x, y)| format!("{x} {y}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn lasso(e: &Engine, id: u64, poly: &[(f64, f64)], mode: &str) {
    e.dispatch(
        id,
        "selection.lasso",
        json!({"points":points(poly),"combine":mode}),
    )
    .unwrap();
}

type Combine = fn(bool, bool) -> bool;
type PixelTest = dyn Fn(u32, u32) -> bool;
type Setup = dyn Fn(&Engine, u64);
const MODES: [(&str, Combine); 5] = [
    ("replace", |_, b| b),
    ("add", |a, b| a || b),
    ("subtract", |a, b| a && !b),
    ("intersect", |a, b| a && b),
    ("invert", |a, b| a != b),
];

#[test]
fn ellipse_box_keeps_pixel_centers_off_the_edge() {
    for y in 0..H {
        for x in 0..W {
            let d = ellipse_d(E, x, y);
            assert!(
                (d - 1.0).abs() * (E[2].min(E[3]) / 2.0) > 0.15,
                "pixel {x},{y} is too close to the ellipse edge"
            );
        }
    }
}

#[test]
fn ellipse_every_mode_matches_pixel_centers() {
    for (mode, op) in MODES {
        let (e, id) = open();
        rect(&e, id, A, "replace");
        ellipse(&e, id, E, mode);
        assert_eq!(
            mask(&e, id).unwrap(),
            expected(|x, y| op(in_rect(A, x, y), in_ellipse(E, x, y))),
            "mode {mode}"
        );
        let labels = steps(&e, id);
        assert_eq!(labels.len(), 3, "one step per selection");
        assert_eq!(labels[2], "Ellipse Select");
    }
}

#[test]
fn lasso_every_mode_matches_even_odd_pixel_centers() {
    // The bow tie must really cross itself and leave holes for the test to mean anything.
    assert!(in_poly(&L, 8, 4) && !in_poly(&L, 5, 1) && !in_poly(&L, 5, 7));
    for (mode, op) in MODES {
        let (e, id) = open();
        rect(&e, id, A, "replace");
        lasso(&e, id, &L, mode);
        assert_eq!(
            mask(&e, id).unwrap(),
            expected(|x, y| op(in_rect(A, x, y), in_poly(&L, x, y))),
            "mode {mode}"
        );
        let labels = steps(&e, id);
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[2], "Lasso Select");
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        assert_eq!(
            mask(&e, id).unwrap(),
            expected(|x, y| in_rect(A, x, y)),
            "undo"
        );
    }
}

/// With no selection, every tool and mode combines with an empty selection:
/// Replace, Add and Invert give the new shape; Subtract and Intersect give an
/// active empty selection.
#[test]
fn with_no_selection_every_tool_and_mode_combines_with_nothing() {
    let shapes: [(&str, &PixelTest); 2] = [
        ("ellipse", &|x, y| in_ellipse(E, x, y)),
        ("lasso", &|x, y| in_poly(&L, x, y)),
    ];
    for (tool, inside) in shapes {
        for (mode, selects) in [
            ("replace", true),
            ("add", true),
            ("subtract", false),
            ("intersect", false),
            ("invert", true),
        ] {
            let (e, id) = open();
            if tool == "ellipse" {
                ellipse(&e, id, E, mode);
            } else {
                lasso(&e, id, &L, mode);
            }
            assert_eq!(
                mask(&e, id).expect("the selection is active"),
                expected(|x, y| selects && inside(x, y)),
                "{tool} {mode}"
            );
        }
    }
}

/// An image whose left 5 columns are black and the rest white.
fn two_tone() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let mut px = Vec::new();
    for _ in 0..H {
        for x in 0..W {
            let v = if x < 5 { 0 } else { 255 };
            px.extend([v, v, v, 255]);
        }
    }
    let id = e.import_pixels(W, H, &px, None).unwrap();
    (e, id)
}

#[test]
fn magic_wand_with_no_selection_combines_with_nothing_in_every_mode() {
    for (mode, selects) in [
        ("replace", true),
        ("add", true),
        ("subtract", false),
        ("intersect", false),
        ("invert", true),
    ] {
        let (e, id) = two_tone();
        e.dispatch(
            id,
            "tool.magic_wand",
            json!({"x":1,"y":1,"tolerance":0.0,"antialias":false,"combine":mode}),
        )
        .unwrap();
        assert_eq!(
            mask(&e, id).expect("the selection is active"),
            expected(|x, _| selects && x < 5),
            "magic wand {mode}"
        );
    }
}

#[test]
fn magic_wand_with_a_selection_combines_in_every_mode() {
    for (mode, op) in MODES {
        let (e, id) = two_tone();
        rect(&e, id, A, "replace");
        e.dispatch(
            id,
            "tool.magic_wand",
            json!({"x":1,"y":1,"tolerance":0.0,"antialias":false,"combine":mode}),
        )
        .unwrap();
        assert_eq!(
            mask(&e, id).unwrap(),
            expected(|x, y| op(in_rect(A, x, y), x < 5)),
            "magic wand {mode}"
        );
    }
}

#[test]
fn ellipse_amend_replaces_its_step() {
    let (e, id) = open();
    rect(&e, id, A, "replace");
    ellipse(&e, id, [0.0, 0.0, 2.0, 2.0], "add");
    let n = steps(&e, id).len();
    e.dispatch(
        id,
        "selection.ellipse",
        json!({"x":E[0],"y":E[1],"width":E[2],"height":E[3],"combine":"add","amend":true}),
    )
    .unwrap();
    assert_eq!(steps(&e, id).len(), n, "no new step");
    assert_eq!(
        mask(&e, id).unwrap(),
        expected(|x, y| in_rect(A, x, y) || in_ellipse(E, x, y))
    );
    // An amend after a rectangle step does not replace the rectangle step.
    rect(&e, id, [0.0, 0.0, 1.0, 1.0], "add");
    e.dispatch(
        id,
        "selection.ellipse",
        json!({"x":E[0],"y":E[1],"width":E[2],"height":E[3],"combine":"replace","amend":true}),
    )
    .unwrap();
    assert_eq!(steps(&e, id).len(), n + 2);
}

#[test]
fn lasso_needs_three_points() {
    let (e, id) = open();
    assert!(
        e.dispatch(
            id,
            "selection.lasso",
            json!({"points":"1 1 5 5","combine":"replace"})
        )
        .is_err()
    );
    assert!(
        e.dispatch(
            id,
            "selection.lasso",
            json!({"points":"1 1 5 x 3 3","combine":"replace"})
        )
        .is_err()
    );
    assert_eq!(steps(&e, id).len(), 1);
}

// MARK: Edit menu commands in the no, all and empty selection states.

/// The three selection states every Edit command is checked in.
fn states() -> Vec<(&'static str, Box<Setup>)> {
    vec![
        ("none", Box::new(|_: &Engine, _: u64| {})),
        (
            "all",
            Box::new(|e: &Engine, id: u64| {
                e.dispatch(id, "selection.select_all", json!({})).unwrap();
            }),
        ),
        (
            "empty",
            Box::new(|e: &Engine, id: u64| {
                rect(e, id, A, "replace");
                rect(e, id, A, "subtract");
            }),
        ),
        (
            "rect",
            Box::new(|e: &Engine, id: u64| {
                rect(e, id, A, "replace");
            }),
        ),
    ]
}

#[test]
fn invert_selection_in_each_state() {
    for (name, setup) in states() {
        let (e, id) = open();
        setup(&e, id);
        let before = steps(&e, id).len();
        let on = enabled(&e, id, "selection.invert");
        assert_eq!(on, name != "none", "{name}: enabled");
        let r = e.dispatch(id, "selection.invert", json!({}));
        if name == "none" {
            assert!(r.is_err(), "disabled with no selection");
            assert_eq!(steps(&e, id).len(), before);
            continue;
        }
        r.unwrap();
        let want = match name {
            "all" => expected(|_, _| false),
            "empty" => expected(|_, _| true),
            _ => expected(|x, y| !in_rect(A, x, y)),
        };
        assert_eq!(mask(&e, id).unwrap(), want, "{name}");
        assert_eq!(steps(&e, id).len(), before + 1, "{name}: one step");
        assert_eq!(steps(&e, id).last().unwrap(), "Invert Selection");
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        assert_eq!(steps(&e, id).len(), before + 1);
        assert_eq!(e.snapshot(id).unwrap().history.index, before - 1);
    }
}

fn pixels(e: &Engine, id: u64) -> Vec<u8> {
    e.query(id, "doc.pixels", json!({})).unwrap()
}

/// Erase: covered pixels become transparent black; the selection goes away.
#[test]
fn erase_selection_in_each_state_is_exact() {
    for (name, setup) in states() {
        let (e, id) = open();
        setup(&e, id);
        let before = steps(&e, id).len();
        let px = pixels(&e, id);
        let on = enabled(&e, id, "selection.erase");
        assert_eq!(on, name == "all" || name == "rect", "{name}: enabled");
        let r = e.dispatch(id, "selection.erase", json!({}));
        if !on {
            assert!(r.is_err(), "{name}: disabled");
            assert_eq!(pixels(&e, id), px, "{name}: pixels kept");
            assert_eq!(steps(&e, id).len(), before);
            continue;
        }
        r.unwrap();
        let src = image(W, H);
        let mut want = Vec::new();
        for y in 0..H {
            for x in 0..W {
                let i = ((y * W + x) * 4) as usize;
                if name == "all" || in_rect(A, x, y) {
                    want.extend([0, 0, 0, 0]);
                } else {
                    want.extend_from_slice(&src[i..i + 4]);
                }
            }
        }
        assert_eq!(pixels(&e, id), want, "{name}");
        assert_eq!(mask(&e, id), None, "{name}: deselected");
        assert_eq!(steps(&e, id).len(), before + 1);
        assert_eq!(steps(&e, id).last().unwrap(), "Erase Selection");
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        assert_eq!(pixels(&e, id), px, "{name}: undo");
        assert!(
            mask(&e, id).is_some(),
            "{name}: undo brings the selection back"
        );
    }
}

/// Fill: covered pixels become the color; the selection stays.
#[test]
fn fill_selection_in_each_state_is_exact() {
    let color = [0x12, 0xab, 0x34, 0xff];
    for (name, setup) in states() {
        let (e, id) = open();
        setup(&e, id);
        let before = steps(&e, id).len();
        let px = pixels(&e, id);
        let m = mask(&e, id);
        let on = enabled(&e, id, "selection.fill");
        assert_eq!(on, name == "all" || name == "rect", "{name}: enabled");
        let r = e.dispatch(id, "selection.fill", json!({"color":"12ab34ff"}));
        if !on {
            assert!(r.is_err(), "{name}: disabled");
            assert_eq!(pixels(&e, id), px);
            assert_eq!(steps(&e, id).len(), before);
            continue;
        }
        r.unwrap();
        let src = image(W, H);
        let mut want = Vec::new();
        for y in 0..H {
            for x in 0..W {
                let i = ((y * W + x) * 4) as usize;
                if name == "all" || in_rect(A, x, y) {
                    want.extend(color);
                } else {
                    want.extend_from_slice(&src[i..i + 4]);
                }
            }
        }
        assert_eq!(pixels(&e, id), want, "{name}");
        assert_eq!(mask(&e, id), m, "{name}: the selection stays");
        assert_eq!(steps(&e, id).last().unwrap(), "Fill Selection");
        assert_eq!(steps(&e, id).len(), before + 1);
        e.dispatch(id, "edit.undo", json!({})).unwrap();
        assert_eq!(pixels(&e, id), px, "{name}: undo");
    }
}

/// Straight-alpha mix at coverage t: alpha mixes linearly and color mixes by
/// alpha weight (the premultiplied mix), rounded to nearest.
fn mix(src: [u8; 4], c: [u8; 4], t: f64) -> [u8; 4] {
    let (sa, ca) = (f64::from(src[3]), f64::from(c[3]));
    let a = sa * (1.0 - t) + ca * t;
    let mut out = [0u8; 4];
    for k in 0..3 {
        out[k] = if a <= 0.0 {
            0
        } else {
            ((f64::from(src[k]) * sa * (1.0 - t) + f64::from(c[k]) * ca * t) / a).round() as u8
        };
    }
    out[3] = a.round() as u8;
    out
}

/// Half-covered pixels (a rectangle edge at x = 2.5) get coverage 128 / 255.
#[test]
fn fill_and_erase_mix_partly_covered_pixels() {
    let r = [2.5, 2.0, 3.0, 2.0];
    let t = 128.0 / 255.0;
    let src = image(W, H);
    let at = |v: &[u8], x: u32, y: u32| -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [v[i], v[i + 1], v[i + 2], v[i + 3]]
    };
    let (e, id) = open();
    rect(&e, id, r, "replace");
    e.dispatch(id, "selection.fill", json!({"color":"ff000080"}))
        .unwrap();
    let px = pixels(&e, id);
    assert_eq!(at(&px, 2, 2), mix(at(&src, 2, 2), [255, 0, 0, 128], t));
    assert_eq!(at(&px, 3, 2), [255, 0, 0, 128]);
    assert_eq!(at(&px, 5, 3), mix(at(&src, 5, 3), [255, 0, 0, 128], t));
    assert_eq!(at(&px, 6, 3), at(&src, 6, 3));
    let (e, id) = open();
    rect(&e, id, r, "replace");
    e.dispatch(id, "selection.erase", json!({})).unwrap();
    let px = pixels(&e, id);
    let s = at(&src, 2, 2);
    let a = (f64::from(s[3]) * (1.0 - t)).round() as u8;
    assert_eq!(at(&px, 2, 2), [s[0], s[1], s[2], a]);
    assert_eq!(at(&px, 3, 2), [0, 0, 0, 0]);
}

#[test]
fn erase_and_fill_change_only_the_active_layer() {
    let (e, id) = open();
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    let bottom_before = {
        e.dispatch(id, "layer.set_active", json!({"index":0}))
            .unwrap();
        pixels(&e, id)
    };
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    e.dispatch(id, "selection.select_all", json!({})).unwrap();
    e.dispatch(id, "selection.fill", json!({"color":"0000ffff"}))
        .unwrap();
    let all: Vec<u8> = (0..W * H).flat_map(|_| [0, 0, 255, 255]).collect();
    assert_eq!(pixels(&e, id), all, "the top layer covers everything");
    e.dispatch(id, "selection.erase", json!({})).unwrap();
    assert_eq!(
        pixels(&e, id),
        bottom_before,
        "the bottom layer is untouched"
    );
}

#[test]
fn expand_and_contract_in_each_state() {
    for (name, setup) in states() {
        let (e, id) = open();
        setup(&e, id);
        let before = steps(&e, id).len();
        let on = enabled(&e, id, "selection.expand");
        assert_eq!(on, name == "all" || name == "rect", "{name}: enabled");
        let r = e.dispatch(id, "selection.expand", json!({"radius":1}));
        if !on {
            assert!(r.is_err(), "{name}: disabled");
            assert_eq!(steps(&e, id).len(), before);
            continue;
        }
        r.unwrap();
        let grown = [A[0] - 1.0, A[1] - 1.0, A[2] + 2.0, A[3] + 2.0];
        let want = match name {
            "all" => expected(|_, _| true),
            _ => expected(|x, y| in_rect(grown, x, y)),
        };
        assert_eq!(mask(&e, id).unwrap(), want, "{name}: expand 1");
        assert_eq!(steps(&e, id).last().unwrap(), "Expand Selection");
        e.dispatch(id, "selection.expand", json!({"radius":-2}))
            .unwrap();
        let shrunk = [A[0] + 1.0, A[1] + 1.0, A[2] - 2.0, A[3] - 2.0];
        let want = match name {
            // The grown canvas rectangle (-1..13) shrinks to 1..11.
            "all" => expected(|x, y| x >= 1 && y >= 1 && x < W - 1 && y < H - 1),
            _ => expected(|x, y| in_rect(shrunk, x, y)),
        };
        assert_eq!(mask(&e, id).unwrap(), want, "{name}: contract 2");
        assert_eq!(steps(&e, id).last().unwrap(), "Contract Selection");
        assert_eq!(steps(&e, id).len(), before + 2, "one step each");
    }
}

#[test]
fn expand_needs_a_nonzero_radius() {
    let (e, id) = open();
    rect(&e, id, A, "replace");
    assert!(
        e.dispatch(id, "selection.expand", json!({"radius":0}))
            .is_err()
    );
}

#[test]
fn expand_grows_a_magic_wand_mask() {
    let (e, id) = two_tone();
    e.dispatch(
        id,
        "tool.magic_wand",
        json!({"x":1,"y":1,"tolerance":0.0,"antialias":false}),
    )
    .unwrap();
    e.dispatch(id, "selection.expand", json!({"radius":2}))
        .unwrap();
    assert_eq!(mask(&e, id).unwrap(), expected(|x, _| x < 7));
}

fn polygons(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.selection_polygons", json!({})).unwrap()).unwrap()
}

/// Copy Selection's text is paint.net's: `{"polygonList": ["x,y,x,y,..."]}`.
#[test]
fn selection_polygons_round_trip_through_paste_selection() {
    // No selection: an empty list, and Paste Selection of it is refused.
    let (e, id) = open();
    assert_eq!(polygons(&e, id), json!({"polygonList": []}));
    // A rectangle gives paint.net's ring.
    rect(&e, id, [3.0, 4.0, 6.0, 5.0], "replace");
    let p = polygons(&e, id);
    let ring = p["polygonList"][0].as_str().unwrap();
    let nums: Vec<f64> = ring.split(',').map(|n| n.parse().unwrap()).collect();
    assert_eq!(nums.len(), 10, "{ring}: four corners and the first again");
    assert_eq!(nums[..2], nums[8..]);
    let text = serde_json::to_string(&p).unwrap();
    // Paste it into a fresh image, in every mode against A.
    for (mode, op) in MODES {
        let (e2, id2) = open();
        rect(&e2, id2, A, "replace");
        e2.dispatch(
            id2,
            "selection.paste",
            json!({"polygons":text,"combine":mode}),
        )
        .unwrap();
        assert_eq!(
            mask(&e2, id2).unwrap(),
            expected(|x, y| op(in_rect(A, x, y), in_rect([3.0, 4.0, 6.0, 5.0], x, y))),
            "paste {mode}"
        );
        assert_eq!(steps(&e2, id2).last().unwrap(), "Paste Selection");
        assert_eq!(steps(&e2, id2).len(), 3);
    }
    // paint.net's own example text.
    let (e3, id3) = open();
    e3.dispatch(
        id3,
        "selection.paste",
        json!({"polygons":"{\n  \"polygonList\": [\n    \"3,4,9,4,9,19,3,19,3,4\"\n  ]\n}","combine":"replace"}),
    )
    .unwrap();
    assert_eq!(
        mask(&e3, id3).unwrap(),
        expected(|x, y| in_rect([3.0, 4.0, 6.0, 15.0], x, y))
    );
}

#[test]
fn magic_wand_selection_copies_as_polygons() {
    let (e, id) = two_tone();
    e.dispatch(
        id,
        "tool.magic_wand",
        json!({"x":1,"y":1,"tolerance":0.0,"antialias":false}),
    )
    .unwrap();
    let text = serde_json::to_string(&polygons(&e, id)).unwrap();
    let (e2, id2) = open();
    e2.dispatch(
        id2,
        "selection.paste",
        json!({"polygons":text,"combine":"replace"}),
    )
    .unwrap();
    assert_eq!(mask(&e2, id2).unwrap(), expected(|x, _| x < 5));
}

#[test]
fn paste_selection_refuses_bad_text() {
    let (e, id) = open();
    for bad in [
        "",
        "hello",
        "{}",
        "{\"polygonList\": []}",
        "{\"polygonList\": [\"1,2,3\"]}",
        "{\"polygonList\": [\"1,2,3,4\"]}",
        "{\"polygonList\": [\"a,b,c,d,e,f\"]}",
    ] {
        assert!(
            e.dispatch(
                id,
                "selection.paste",
                json!({"polygons":bad,"combine":"replace"})
            )
            .is_err(),
            "{bad:?}"
        );
    }
    assert_eq!(steps(&e, id).len(), 1);
}

#[test]
fn deselect_and_select_all_in_each_state() {
    for (name, setup) in states() {
        let (e, id) = open();
        setup(&e, id);
        assert_eq!(
            enabled(&e, id, "selection.deselect"),
            name != "none",
            "{name}"
        );
        let before = steps(&e, id).len();
        e.dispatch(id, "selection.select_all", json!({})).unwrap();
        assert_eq!(mask(&e, id).unwrap(), expected(|_, _| true), "{name}");
        assert_eq!(steps(&e, id).len(), before + 1);
        e.dispatch(id, "selection.deselect", json!({})).unwrap();
        assert_eq!(mask(&e, id), None);
        assert_eq!(steps(&e, id).len(), before + 2);
    }
}

#[test]
fn shapes_off_the_canvas_are_clipped_to_it() {
    let (e, id) = open();
    let poly = [(-4.0, -3.0), (6.0, -2.0), (5.5, 4.2), (-2.0, 6.1)];
    lasso(&e, id, &poly, "replace");
    assert_eq!(mask(&e, id).unwrap(), expected(|x, y| in_poly(&poly, x, y)));
    ellipse(&e, id, [8.0, 6.0, 8.0, 8.0], "add");
    let p: Value =
        serde_json::from_slice(&e.query(id, "doc.selection_path", json!({})).unwrap()).unwrap();
    for ring in p["paths"].as_array().unwrap() {
        for (i, v) in ring.as_array().unwrap().iter().enumerate() {
            let v = v.as_f64().unwrap();
            let max = if i % 2 == 0 {
                f64::from(W)
            } else {
                f64::from(H)
            };
            assert!(
                (0.0..=max).contains(&v),
                "outline point {v} is off the canvas"
            );
        }
    }
}
