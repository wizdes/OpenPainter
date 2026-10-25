// Release review (before v1.0) engine checks. Item 1: an open tool session
// (text, paste, shape, ...) counts as an edit, a save writes it as its own
// step, and a recovery record holds what the screen shows.
use op_engine::recovery::{RecordHooks, RecoveryOptions};
use op_engine::{Engine, PasteRequest, SaveFormat};
use op_tools::r#move::{Affine, MoveOptions};
use serde_json::{Value, json};
use std::time::Duration;

fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = u32::from(c[0]) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn text_begin() -> Value {
    let run =
        json!({"kind":"coverage","x":0,"y":0,"width":3,"height":1,"data":b64(&[255, 255, 255])});
    json!({"origin":[1,1],"runs":[run],"primary":[255,0,0,255],"blend":"normal","text":"Hi","caret":2})
}

fn run(e: &Engine, id: u64, command: &str, params: Value) -> op_engine::Result<()> {
    e.run_op(
        id,
        op_engine::Op::Command(command.into(), params),
        &op_engine::JobTicket::new(0),
    )
    .map(|_| ())
}

fn blue(e: &Engine) -> u64 {
    e.import_pixels(8, 4, &[0, 0, 255, 255].repeat(8 * 4), None)
        .unwrap()
}

fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}

fn saved_clean(e: &Engine, id: u64, path: &std::path::Path) {
    e.save_document(id, path, SaveFormat::Opaint, true).unwrap();
    assert_eq!(info(e, id)["dirty"], json!(false));
}

fn opened_pixels(path: &std::path::Path) -> Vec<u8> {
    let e = Engine::new(None).unwrap();
    let id = e.open_opaint(path).unwrap();
    e.query(id, "doc.pixels", json!({})).unwrap()
}

#[test]
fn open_text_counts_as_an_edit_and_save_writes_it_as_its_own_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.opaint");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    saved_clean(&e, id, &path);
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    let shown = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    let i = info(&e, id);
    assert_eq!(i["dirty"], json!(true), "open text marks the image edited");
    assert_eq!(i["session_open"], json!(true));
    e.save_document(id, &path, SaveFormat::Opaint, true)
        .unwrap();
    let i = info(&e, id);
    assert_eq!(i["dirty"], json!(false));
    assert_eq!(
        i["history_labels"].as_array().unwrap().last().unwrap(),
        "Text"
    );
    assert_eq!(opened_pixels(&path), shown, "the file holds the text");
    assert_eq!(&shown[(8 + 1) * 4..(8 + 1) * 4 + 4], [255, 0, 0, 255]);
}

#[test]
fn an_empty_text_box_is_not_an_edit_and_stays_open() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let mut b = text_begin();
    b["runs"] = json!([]);
    b["text"] = json!("");
    b["caret"] = json!(0);
    e.dispatch(id, "tool.text.begin", b).unwrap();
    assert_eq!(info(&e, id)["dirty"], json!(false));
    e.finish_sessions(id).unwrap();
    // Still open: an update works and nothing was committed.
    e.dispatch(id, "tool.text.update", json!({"origin":[2,2]}))
        .unwrap();
    assert_eq!(info(&e, id)["history_labels"].as_array().unwrap().len(), 1);
}

#[test]
fn open_paste_counts_as_an_edit_and_finish_sessions_commits_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.opaint");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    saved_clean(&e, id, &path);
    e.paste_begin(
        id,
        PasteRequest {
            width: 2,
            height: 1,
            rgba: [0, 255, 0, 255].repeat(2),
            icc: None,
            x: 0,
            y: 0,
            new_layer: false,
            options: MoveOptions {
                transform: Affine::identity(),
                copy: false,
                resampling: op_ops::image::Resampling::Bilinear,
            },
        },
    )
    .unwrap();
    assert_eq!(
        info(&e, id)["dirty"],
        json!(true),
        "an open paste is an edit"
    );
    e.finish_sessions(id).unwrap();
    let i = info(&e, id);
    assert_eq!(i["session_open"], json!(false));
    assert_eq!(
        i["history_labels"].as_array().unwrap().last().unwrap(),
        "Paste"
    );
    assert_eq!(i["dirty"], json!(true));
}

#[test]
fn open_shape_is_saved_by_a_png_save_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.png");
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(40, 40, &[0, 0, 255, 255].repeat(1600), None)
        .unwrap();
    e.dispatch(id, "tool.shape.begin", json!({"shape":"rectangle","box":[[0,0],[39,39]],"primary":[0,255,0,255],"secondary":[0,0,255,255],"mode":"fill"})).unwrap();
    assert_eq!(info(&e, id)["dirty"], json!(true));
    let shown = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    e.save_document(id, &path, SaveFormat::Png, true).unwrap();
    assert_eq!(info(&e, id)["dirty"], json!(false));
    let e2 = Engine::new(None).unwrap();
    let back = e2.open_png(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(e2.query(back, "doc.pixels", json!({})).unwrap(), shown);
}

#[test]
fn the_recovery_record_holds_the_open_text() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("rec");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let options = RecoveryOptions {
        settle: Duration::from_secs(60),
        max_interval: Duration::from_secs(600),
        hooks: RecordHooks::default(),
    };
    e.recovery_attach(id, &dir, None, options.clone()).unwrap();
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    let shown = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert!(e.recovery_flush(id).unwrap().is_some());
    // Typing more changes the record too (same revision, new preview).
    e.dispatch(id, "tool.text.update", json!({"origin":[3,2]}))
        .unwrap();
    let moved = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert_ne!(moved, shown);
    e.recovery_flush(id).unwrap();
    e.recovery_detach(id, false);
    let e2 = Engine::new(None).unwrap();
    let (rec, _) = e2.recovery_open(&dir, options).unwrap();
    assert_eq!(e2.query(rec, "doc.pixels", json!({})).unwrap(), moved);
}

/// Item 13: Add New Layer puts the layer just above the active one and makes
/// it active (paint.net), so the next paint goes onto the new layer.
#[test]
fn add_new_layer_goes_above_the_active_layer_and_becomes_active() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    e.dispatch(id, "layer.add", json!({"name":"Two"})).unwrap();
    e.dispatch(id, "layer.add", json!({"name":"Three"}))
        .unwrap();
    // Choose the bottom layer, then add: the new one lands just above it.
    e.dispatch(id, "layer.set_active", json!({"index":0}))
        .unwrap();
    e.dispatch(id, "layer.add", json!({"name":"Mid"})).unwrap();
    let i = info(&e, id);
    let names: Vec<_> = i["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Background", "Mid", "Two", "Three"]);
    assert_eq!(i["active_layer"], json!(1), "the new layer is active");
}

/// Item 11: a selection whose outline would get a point that is not a finite
/// number is refused at the command, and the image stays as it was.
#[test]
fn a_selection_with_a_non_finite_point_is_refused() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    // A Move Selection scaled past f64 is refused too; the move stays open.
    run(
        &e,
        id,
        "selection.rectangle",
        json!({"x":1,"y":1,"width":4,"height":2}),
    )
    .unwrap();
    e.move_begin(
        id,
        op_tools::r#move::MoveKind::Selection,
        MoveOptions {
            transform: Affine::identity(),
            copy: false,
            resampling: op_ops::image::Resampling::Bilinear,
        },
    )
    .unwrap();
    let huge = MoveOptions {
        transform: Affine([1e308, 0.0, 0.0, 1e-308, 0.0, 0.0]),
        copy: false,
        resampling: op_ops::image::Resampling::Bilinear,
    };
    let steps = info(&e, id)["history_labels"].as_array().unwrap().len();
    assert!(e.move_update(id, huge).is_err());
    e.move_finish(id).unwrap();
    // The refused transform was not kept: Finish committed an unmoved
    // selection, and every saved point is finite.
    assert_eq!(
        info(&e, id)["history_labels"].as_array().unwrap().len(),
        steps + 1
    );
    let dir = tempfile::tempdir().unwrap();
    saved_clean(&e, id, &dir.path().join("sel.opaint"));
}

/// Item 12b: an amended Rectangle Select right after a save makes a new
/// step, so the image becomes edited instead of changing the saved step.
#[test]
fn an_amend_right_after_a_save_marks_the_image_edited() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.opaint");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let rect = |w: u32, amend: bool| json!({"x":0,"y":0,"width":w,"height":2,"amend":amend});
    run(&e, id, "selection.rectangle", rect(2, false)).unwrap();
    saved_clean(&e, id, &path);
    run(&e, id, "selection.rectangle", rect(5, true)).unwrap();
    assert_eq!(info(&e, id)["dirty"], json!(true));
    // Undo goes back to the saved selection.
    run(&e, id, "edit.undo", json!({})).unwrap();
    assert_eq!(info(&e, id)["dirty"], json!(false));
}

/// Item 10 (engine PNG writer): a save through a symlink replaces the
/// target, keeps the link, and keeps the target's permissions.
#[test]
fn a_png_save_through_a_symlink_keeps_the_link_and_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.png");
    let link = dir.path().join("link.png");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    e.save_document(id, &real, SaveFormat::Png, false).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    run(&e, id, "image.invert_colors", json!({})).unwrap();
    e.save_document(id, &link, SaveFormat::Png, true).unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let e2 = Engine::new(None).unwrap();
    let back = e2.open_png(&std::fs::read(&real).unwrap()).unwrap();
    assert_eq!(
        e2.query(back, "doc.pixels", json!({})).unwrap()[..4],
        [255, 255, 0, 255]
    );
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

fn kept_metadata(e: &Engine, id: u64) -> Value {
    let v: Value =
        serde_json::from_slice(&e.query(id, "doc.image_metadata", json!({})).unwrap()).unwrap();
    v["text"].clone()
}

/// Item 5: the app's kept metadata travels with the document through an
/// .opaint save and a recovery record, and setting it adds no step.
#[test]
fn kept_image_metadata_survives_opaint_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.opaint");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let text = "bplist00 \"EXIF\" & <XMP> é".repeat(1000);
    e.set_image_metadata(id, text.clone()).unwrap();
    let i = info(&e, id);
    assert_eq!(i["dirty"], json!(false), "setting it is not an edit");
    assert_eq!(i["history_labels"].as_array().unwrap().len(), 1);
    // Undo past later edits keeps it (it is on every step).
    run(&e, id, "image.invert_colors", json!({})).unwrap();
    e.save_document(id, &path, SaveFormat::Opaint, true)
        .unwrap();
    let e2 = Engine::new(None).unwrap();
    let back = e2.open_opaint(&path).unwrap();
    assert_eq!(kept_metadata(&e2, back), json!(text));
    // A recovery record holds it too.
    let rec_dir = dir.path().join("rec");
    let options = RecoveryOptions {
        settle: Duration::from_secs(60),
        max_interval: Duration::from_secs(600),
        hooks: RecordHooks::default(),
    };
    e2.recovery_attach(back, &rec_dir, None, options.clone())
        .unwrap();
    run(&e2, back, "image.invert_colors", json!({})).unwrap();
    e2.recovery_flush(back).unwrap();
    e2.recovery_detach(back, false);
    let e3 = Engine::new(None).unwrap();
    let (rec, _) = e3.recovery_open(&rec_dir, options).unwrap();
    assert_eq!(kept_metadata(&e3, rec), json!(text));
    run(&e3, rec, "edit.undo", json!({})).ok();
    assert_eq!(kept_metadata(&e3, rec), json!(text));
}
