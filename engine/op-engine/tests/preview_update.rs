//! What a dialog's preview shows between an update and its new render.
use op_engine::Engine;
use serde_json::json;
use std::time::{Duration, Instant};

fn doc(e: &Engine) -> u64 {
    let mut px = vec![0u8; 40 * 24 * 4];
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.copy_from_slice(&[(i % 251) as u8, 40, 200, 255]);
    }
    e.import_pixels(40, 24, &px, None).unwrap()
}

fn wait(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    f()
}

/// `update` published no preview until the new render came back, so a frame drawn in between
/// showed the document without the change, on each step of a slider drag. A preview must stay on
/// screen until the next one replaces it.
#[test]
fn a_preview_update_keeps_the_last_picture_until_the_new_one_is_ready() {
    let e = Engine::new(None).unwrap();
    let id = doc(&e);
    e.dispatch(id, "layer.duplicate", json!({})).unwrap();
    let props = |opacity: u64| json!({"index":1,"name":"Dragged","visible":true,"blend":"difference","opacity":opacity});
    e.preview_begin(id, "layer.properties", props(200)).unwrap();
    assert!(wait(|| e.snapshot(id).unwrap().preview.is_some()));
    for opacity in 0..200u64 {
        e.preview_update(id, props(opacity)).unwrap();
        for _ in 0..50 {
            assert!(
                e.snapshot(id).unwrap().preview.is_some(),
                "the preview went away after the update to opacity {opacity}"
            );
        }
    }
    // The last value does come through.
    assert!(wait(|| {
        e.snapshot(id)
            .unwrap()
            .preview
            .as_ref()
            .is_some_and(|p| p.layers[1].opacity == 199)
    }));
    e.preview_cancel(id).unwrap();
}
