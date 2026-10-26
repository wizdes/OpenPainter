// Release review, fix round 3 (engine side). Item 6: the layer and pixel
// limits come back as their own error kind ("limit") with the plain limit
// text, so the app can show it. Item 7: a selection that is stored as a mask
// is refused when the 16 GiB pixel total has no room for that mask, so every
// image stays savable. Item 8: the test-only commands are on in this crate's
// own tests (the `test-commands` feature) and nowhere else.
use op_engine::{Engine, Error, JobTicket, Op, PasteRequest, finished_json};
use op_tools::r#move::{Affine, MoveOptions};
use serde_json::{Value, json};

fn run(e: &Engine, id: u64, command: &str, params: Value) -> op_engine::Result<()> {
    e.run_op(id, Op::Command(command.into(), params), &JobTicket::new(0))
        .map(|_| ())
}

fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}

fn paste_new_layer() -> PasteRequest {
    PasteRequest {
        width: 2,
        height: 1,
        rgba: [0, 255, 0, 255].repeat(2),
        icc: None,
        x: 0,
        y: 0,
        new_layer: true,
        options: MoveOptions {
            transform: Affine::identity(),
            copy: false,
            resampling: op_ops::image::Resampling::Bilinear,
        },
    }
}

/// The error is a limit, says `text`, and has no "bad argument" or
/// "internal error" prefix; the JobFinished payload carries the same.
fn assert_limit(result: op_engine::Result<()>, text: &str, why: &str) {
    let err = result.expect_err(why);
    assert_eq!(err.status_name(), "limit", "{why}: {err:?}");
    let shown = err.to_string();
    assert!(shown.contains(text), "{why}: {shown}");
    assert!(!shown.starts_with("bad argument"), "{why}: {shown}");
    assert!(!shown.starts_with("internal error"), "{why}: {shown}");
    let payload = finished_json::<()>(&Err(err));
    assert_eq!(payload["status"], json!("limit"), "{why}");
    assert_eq!(payload["error"], json!(shown), "{why}");
}

// MARK: Item 6

#[test]
fn the_1000_layer_limit_is_a_limit_error_with_its_text() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(4, 4, 72.0, [0, 0, 0, 0]).unwrap();
    for _ in 1..1000 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    let text = "at most 1000 layers";
    assert_limit(run(&e, id, "layer.add", json!({})), text, "Add New Layer");
    assert_limit(
        run(&e, id, "layer.duplicate", json!({})),
        text,
        "Duplicate Layer",
    );
    assert_limit(
        run(
            &e,
            id,
            "layer.import_from_file",
            json!({"path":"/nonexistent/a.png"}),
        ),
        text,
        "Import from File",
    );
    assert_limit(
        e.paste_begin(id, paste_new_layer()).map(|_| ()),
        text,
        "Paste into New Layer",
    );
    assert_eq!(info(&e, id)["layers"].as_array().unwrap().len(), 1000);
}

#[test]
fn the_16_gib_total_is_a_limit_error_with_its_text() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(16384, 16384, 72.0, [0, 0, 0, 0]).unwrap();
    for _ in 1..15 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    let text = "more than 16 GiB";
    assert_limit(run(&e, id, "layer.add", json!({})), text, "Add New Layer");
    assert_limit(
        run(&e, id, "layer.duplicate", json!({})),
        text,
        "Duplicate Layer",
    );
    assert_limit(
        e.paste_begin(id, paste_new_layer()).map(|_| ()),
        text,
        "Paste into New Layer",
    );
}

#[test]
fn canvas_size_and_resize_past_the_16_gib_total_are_limit_errors() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(10000, 9000, 72.0, [0, 0, 0, 0]).unwrap();
    for _ in 1..43 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    let text = "more than 16 GiB";
    assert_limit(
        run(
            &e,
            id,
            "image.canvas_size",
            json!({"width":10000,"height":10000,"anchor":"center"}),
        ),
        text,
        "Canvas Size",
    );
    assert_limit(
        run(
            &e,
            id,
            "image.resize",
            json!({"width":10000,"height":10000,"maintain_aspect":false}),
        ),
        text,
        "Resize",
    );
    assert_eq!(info(&e, id)["height"], json!(9000));
}

#[test]
fn other_errors_keep_their_kinds() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(4, 4, 72.0, [0, 0, 0, 0]).unwrap();
    let err = run(&e, id, "layer.delete", json!({})).unwrap_err();
    assert_eq!(err.status_name(), "bad_argument", "{err:?}");
    assert!(matches!(err, Error::BadArgument(_)));
}

// MARK: Item 7

/// 2072 x 2072 x 4 bytes is 17,172,736 bytes a layer: 1,000 layers hold
/// 17,172,736,000 bytes, under 16 GiB (17,179,869,184), and one more layer
/// or a stored selection mask would go past it.
const FULL: (u32, u32, usize) = (2072, 2072, 1000);

fn mask_param(width: u32, height: u32) -> Value {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // Every pixel half selected (128 = 0x80): base64 of 80 80 80 is "gICA".
    let n = width as usize * height as usize;
    assert_eq!(n % 3, 1, "the padding below assumes one byte left over");
    let mut out = "gICA".repeat(n / 3);
    out.push(A[(0x80 >> 2) as usize] as char);
    out.push(A[((0x80 & 3) << 4) as usize] as char);
    out.push_str("==");
    json!({ "mask": out })
}

fn wand() -> Value {
    json!({"x":0,"y":0,"tolerance":0.5,"contiguous":true,"alpha":"straight",
           "sampling":"layer","antialias":false,"combine":"replace"})
}

#[test]
fn a_stored_mask_is_refused_when_the_16_gib_total_has_no_room_for_it() {
    assert!(op_core::doc::check_pixel_budget(FULL.2 - 1, FULL.0, FULL.1).is_ok());
    assert!(op_core::doc::check_pixel_budget(FULL.2, FULL.0, FULL.1).is_err());
    let e = Engine::new(None).unwrap();
    let id = e.test_open_unbudgeted(FULL.0, FULL.1, FULL.2).unwrap();
    assert_eq!(info(&e, id)["layers"].as_array().unwrap().len(), FULL.2);
    let text = "16 GiB";
    // A mask selection (Select Foreground) is refused; the image keeps no
    // selection and no step.
    assert_limit(
        run(&e, id, "selection.from_mask", mask_param(FULL.0, FULL.1)),
        text,
        "Select Foreground",
    );
    // The Magic Wand stores a mask: it is refused when it starts.
    assert_limit(
        e.preview_begin(id, "tool.magic_wand", wand()).map(|_| ()),
        text,
        "Magic Wand",
    );
    assert_eq!(info(&e, id)["session_open"], json!(false));
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["active"], json!(false), "no selection was kept");
    assert_eq!(e.snapshot(id).unwrap().history.labels, vec!["Open"]);
    // A shape selection is kept as its outline (no mask), so it still works.
    run(
        &e,
        id,
        "selection.rectangle",
        json!({"x":1.0,"y":1.0,"width":10.0,"height":10.0}),
    )
    .unwrap();
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["active"], json!(true));
}

#[test]
fn a_stored_mask_is_kept_when_the_total_has_room_for_it() {
    let e = Engine::new(None).unwrap();
    let id = e.test_open_unbudgeted(FULL.0, FULL.1, FULL.2 - 1).unwrap();
    run(&e, id, "selection.from_mask", mask_param(FULL.0, FULL.1)).unwrap();
    let sel: Value =
        serde_json::from_slice(&e.query(id, "doc.selection", json!({})).unwrap()).unwrap();
    assert_eq!(sel["active"], json!(true));
}

// MARK: Item 8

#[test]
fn the_test_commands_are_on_in_this_crates_tests() {
    // The app's library (op-ffi, built alone) does not turn the feature on;
    // the Mac test `testTheShippedLibraryRejectsTheTestPanicHook` checks it.
    const { assert!(cfg!(feature = "test-commands")) };
    let e = Engine::new(None).unwrap();
    let id = e.new_document(4, 4, 72.0, [0, 0, 0, 0]).unwrap();
    e.dispatch(id, "test.panic_on_finish", json!({})).unwrap();
}

#[test]
fn doc_layer_limit_names_the_limit_that_stops_a_new_layer() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(4, 4, 72.0, [0, 0, 0, 0]).unwrap();
    let limit = |e: &Engine| -> Value {
        serde_json::from_slice::<Value>(&e.query(id, "doc.layer_limit", json!({})).unwrap())
            .unwrap()["limit"]
            .clone()
    };
    assert_eq!(limit(&e), Value::Null);
    for _ in 1..1000 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    assert!(
        limit(&e)
            .as_str()
            .is_some_and(|t| t.contains("at most 1000 layers")),
        "{}",
        limit(&e)
    );
}
