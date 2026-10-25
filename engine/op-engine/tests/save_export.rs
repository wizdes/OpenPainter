//! M50 save options: `save_export::encode_image` (the save sheet's encoder and
//! its size preview) and the resolution kept through import and PNG open.
use op_engine::Engine;
use op_engine::save_export::encode_image;
use serde_json::{Value, json};

fn dpi(e: &Engine, id: u64) -> f64 {
    let v: Value =
        serde_json::from_slice(&e.query(id, "doc.resolution", json!({})).unwrap()).unwrap();
    v["dpi"].as_f64().unwrap()
}

fn pixels(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            // Partly transparent colors must survive a lossless save exactly
            // (the engine stores zero-alpha pixels as 0, 0, 0, 0, so alpha
            // starts at 1 here).
            v.extend_from_slice(&[
                (x * 37) as u8,
                (y * 53) as u8,
                (x ^ y) as u8,
                (x * 9) as u8 | 1,
            ]);
        }
    }
    v
}

#[test]
fn png_at_every_level_keeps_every_pixel_byte_and_the_resolution() {
    let e = Engine::new(None).unwrap();
    let rgba = pixels(31, 17);
    let mut sizes = vec![];
    for level in 0..=9 {
        let bytes = encode_image(
            &json!({"format":"png","png_level":level,"dpi":300.0}),
            31,
            17,
            &rgba,
            None,
            None,
            None,
        )
        .unwrap();
        sizes.push(bytes.len());
        let id = e.open_png(&bytes).unwrap();
        assert!(
            e.query(id, "doc.pixels", json!({})).unwrap() == rgba,
            "level {level}: pixels differ"
        );
        assert_eq!(dpi(&e, id), 300.0, "pHYs read back at level {level}");
        e.close(id).unwrap();
    }
    assert!(
        sizes[9] < sizes[0],
        "level 9 is smaller than stored: {sizes:?}"
    );
}

#[test]
fn png_without_phys_opens_at_96_dpi() {
    let e = Engine::new(None).unwrap();
    let rgba = pixels(4, 4);
    let bytes = encode_image(&json!({"format":"png"}), 4, 4, &rgba, None, None, None).unwrap();
    let id = e.open_png(&bytes).unwrap();
    assert_eq!(dpi(&e, id), 96.0);
}

#[test]
fn jpeg_quality_and_subsampling_change_the_size() {
    let mut rgba = pixels(64, 64);
    for p in rgba.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let size = |q: u32, s: &str| {
        encode_image(
            &json!({"format":"jpeg","jpeg_quality":q,"jpeg_subsampling":s}),
            64,
            64,
            &rgba,
            None,
            None,
            None,
        )
        .unwrap()
        .len()
    };
    assert!(size(95, "422") > size(50, "422"));
    assert!(size(50, "422") > size(10, "422"));
    assert!(size(90, "444") > size(90, "420"));
}

#[test]
fn bad_options_are_refused() {
    let rgba = pixels(2, 2);
    for p in [
        json!({"format":"nope"}),
        json!({"format":"png","png_level":10}),
        json!({"format":"jpeg","jpeg_quality":0}),
        json!({"format":"jpeg","jpeg_subsampling":"411"}),
        json!({"format":"png","dpi":0.0}),
        json!({"format":"gif","gif_dither":"yes"}),
    ] {
        assert!(
            encode_image(&p, 2, 2, &rgba, None, None, None).is_err(),
            "{p}"
        );
    }
    assert!(encode_image(&json!({"format":"png"}), 3, 2, &rgba, None, None, None).is_err());
}

#[test]
fn import_pixels_at_keeps_the_resolution() {
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels_at(2, 2, &pixels(2, 2), None, 240.0)
        .unwrap();
    assert_eq!(dpi(&e, id), 240.0);
    assert!(e.import_pixels_at(2, 2, &pixels(2, 2), None, 0.0).is_err());
    let plain = e.import_pixels(2, 2, &pixels(2, 2), None).unwrap();
    assert_eq!(dpi(&e, plain), 96.0);
}
