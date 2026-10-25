//! M36 Color Picker sample query: `doc.sample_color` averages a clipped square
//! window from the active layer or the composite image.
use op_engine::Engine;
use serde_json::{Value, json};

/// A 300 x 260 image (it spans 2 x 2 tiles) where pixel (x, y) is
/// `[x % 256, y % 256, 7, 255]`.
fn doc(e: &Engine) -> u64 {
    let (w, h) = (300u32, 260u32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[(x % 256) as u8, (y % 256) as u8, 7, 255]);
        }
    }
    e.import_pixels(w, h, &px, None).unwrap()
}
fn sample(e: &Engine, id: u64, p: Value) -> Value {
    serde_json::from_slice(&e.query(id, "doc.sample_color", p).unwrap()).unwrap()
}
fn hex(c: [u8; 4]) -> Value {
    json!(format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]))
}
/// The expected average of the clipped window, computed straight from the
/// pixel formula (all pixels opaque).
fn expected(x: i64, y: i64, size: i64, w: i64, h: i64) -> ([u8; 4], i64, [i64; 4]) {
    let half = size / 2;
    let (x0, y0) = ((x - half).max(0), (y - half).max(0));
    let (x1, y1) = ((x + half + 1).min(w), (y + half + 1).min(h));
    let (mut sr, mut sg, mut n) = (0i64, 0i64, 0i64);
    for py in y0..y1 {
        for px in x0..x1 {
            sr += px % 256;
            sg += py % 256;
            n += 1;
        }
    }
    let r = |s: i64| ((2 * s + n) / (2 * n)) as u8;
    ([r(sr), r(sg), 7, 255], n, [x0, y0, x1 - x0, y1 - y0])
}

#[test]
fn single_pixel_and_tile_seams() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    let s = sample(&e, id, json!({"x":10,"y":20,"size":1}));
    assert_eq!(s["color"], hex([10, 20, 7, 255]));
    assert_eq!(s["count"], json!(1));
    // A window across the tile seam at x = 256 and y = 256 reads all four tiles.
    for size in [3i64, 11, 51] {
        let s = sample(&e, id, json!({"x":256,"y":255,"size":size}));
        let (c, n, win) = expected(256, 255, size, 300, 260);
        assert_eq!(s["color"], hex(c), "size {size}");
        assert_eq!(s["count"], json!(n));
        assert_eq!(s["window"], json!(win));
    }
}

#[test]
fn window_is_clipped_at_all_four_edges_and_corners() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    let (w, h) = (300i64, 260i64);
    let points = [
        (0, 130),
        (w - 1, 130),
        (150, 0),
        (150, h - 1),
        (0, 0),
        (w - 1, 0),
        (0, h - 1),
        (w - 1, h - 1),
        (1, 2),
    ];
    for size in [1i64, 3, 5, 7, 9, 11, 31, 51] {
        for (x, y) in points {
            let s = sample(&e, id, json!({"x":x,"y":y,"size":size,"sampling":"image"}));
            let (c, n, win) = expected(x, y, size, w, h);
            assert_eq!(s["color"], hex(c), "({x},{y}) size {size}");
            assert_eq!(s["count"], json!(n), "({x},{y}) size {size}");
            assert_eq!(s["window"], json!(win));
        }
    }
    // The corner window of a 5 x 5 sample holds only the 3 x 3 pixels inside.
    assert_eq!(
        sample(&e, id, json!({"x":0,"y":0,"size":5}))["count"],
        json!(9)
    );
}

#[test]
fn layer_and_image_sampling_differ() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.add", json!({"name":"Top"})).unwrap();
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    // Fill the whole new layer with half-transparent blue.
    e.dispatch(
        id,
        "tool.paint_bucket",
        json!({"x":0,"y":0,"tolerance":0.0,"contiguous":false,"sampling":"layer",
               "antialias":false,"color":"0000ff80","blend":"normal"}),
    )
    .unwrap();
    let layer = sample(&e, id, json!({"x":40,"y":40,"size":1,"sampling":"layer"}));
    assert_eq!(layer["color"], hex([0, 0, 255, 128]));
    let image = sample(&e, id, json!({"x":40,"y":40,"size":1,"sampling":"image"}));
    let c = image["color"].as_str().unwrap().to_owned();
    assert_ne!(c, "0000ff80");
    assert!(c.ends_with("ff"), "the composite is opaque: {c}");
    // Layer 0 still reads its own pixel.
    e.dispatch(id, "layer.set_active", json!({"index":0}))
        .unwrap();
    let bottom = sample(&e, id, json!({"x":40,"y":40,"size":1,"sampling":"layer"}));
    assert_eq!(bottom["color"], hex([40, 40, 7, 255]));
    // Sampling an empty layer gives transparent. Added from layer 0, it lands
    // just above it, at index 1 (item 13, paint.net placement).
    e.dispatch(id, "layer.add", json!({"name":"Empty"}))
        .unwrap();
    e.dispatch(id, "layer.set_active", json!({"index":1}))
        .unwrap();
    let empty = sample(&e, id, json!({"x":40,"y":40,"size":5,"sampling":"layer"}));
    assert_eq!(empty["color"], json!("00000000"));
}

#[test]
fn bad_arguments_are_rejected() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    for p in [
        json!({"x":-1,"y":0,"size":1}),
        json!({"x":300,"y":0,"size":1}),
        json!({"x":0,"y":260,"size":1}),
        json!({"x":0,"y":0,"size":4}),
        json!({"x":0,"y":0,"size":1,"sampling":"canvas"}),
        json!({"y":0}),
    ] {
        assert!(e.query(id, "doc.sample_color", p.clone()).is_err(), "{p}");
    }
}
