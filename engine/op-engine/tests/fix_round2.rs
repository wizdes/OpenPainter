// Release review, fix round 2 (engine side). Item 2 and 5: `doc.sync` and
// `doc.finish_sessions` are queued commands that answer after the work
// queued before them. Item 3: recovery records keep the original's content
// type. Item 4: starting a move (or a stroke) while text, a line or a shape
// is open keeps that open work. Item 7: the 16 GiB pixel total is enforced
// when layers are added or the canvas grows. Item 8: Add New Layer,
// Duplicate Layer and Import from File turn off at the limits. Item 9: a
// panic while a save finishes the open sessions makes the image read-only.
use op_core::doc::ToolCompositeMode;
use op_engine::recovery::{RecordHooks, RecoveryOptions};
use op_engine::{Engine, Error, Event, EventKind, JobTicket, Op, PasteRequest, SaveFormat};
use op_tools::r#move::{Affine, MoveKind, MoveOptions};
use op_tools::{BrushSettings, PressureMode, StrokeSample, Tool};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// Red text "Hi" as three covered pixels at (1, 1), (2, 1) and (3, 1).
fn text_begin() -> Value {
    let run =
        json!({"kind":"coverage","x":0,"y":0,"width":3,"height":1,"data":b64(&[255, 255, 255])});
    json!({"origin":[1,1],"runs":[run],"primary":[255,0,0,255],"blend":"normal","text":"Hi","caret":2})
}

fn blue(e: &Engine) -> u64 {
    e.import_pixels(8, 8, &[0, 0, 255, 255].repeat(64), None)
        .unwrap()
}

fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}

fn labels(e: &Engine, id: u64) -> Vec<String> {
    e.snapshot(id).unwrap().history.labels.clone()
}

fn pixel(px: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    px[(y * w + x) * 4..(y * w + x) * 4 + 4].try_into().unwrap()
}

fn opened_pixels(path: &std::path::Path) -> Vec<u8> {
    let e = Engine::new(None).unwrap();
    let id = e.open_opaint(path).unwrap();
    e.query(id, "doc.pixels", json!({})).unwrap()
}

fn engine_with_events() -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let e = Engine::new(Some(Arc::new(move |ev| {
        let _ = tx.lock().unwrap().send(ev);
    })))
    .unwrap();
    (e, rx)
}

fn wait_job(rx: &Receiver<Event>, ticket: u64, limit: Duration) -> Event {
    let end = Instant::now() + limit;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let ev = rx
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("no JobFinished for ticket {ticket} within {limit:?}"));
        if matches!(ev.kind, EventKind::JobFinished) && ev.ticket == ticket {
            return ev;
        }
    }
}

fn move_options(dx: f64, dy: f64) -> MoveOptions {
    MoveOptions {
        transform: Affine::translation(dx, dy),
        resampling: op_ops::image::Resampling::Nearest,
        copy: false,
    }
}

// MARK: Item 4

#[test]
fn a_move_started_while_text_is_open_keeps_the_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("moved.opaint");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    e.move_begin(id, MoveKind::Pixels, move_options(0.0, 0.0))
        .unwrap();
    e.move_update(id, move_options(0.0, 2.0)).unwrap();
    let shown = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert_eq!(
        pixel(&shown, 8, 1, 3),
        [255, 0, 0, 255],
        "the move shows the text"
    );
    e.save_document(id, &path, SaveFormat::Opaint, true)
        .unwrap();
    let saved = opened_pixels(&path);
    assert_eq!(
        pixel(&saved, 8, 1, 3),
        [255, 0, 0, 255],
        "the saved file has the moved text"
    );
    assert_eq!(pixel(&saved, 8, 3, 3), [255, 0, 0, 255]);
    assert_eq!(saved, shown, "the file holds what the screen showed");
    assert_eq!(labels(&e, id), ["Open", "Text", "Move Selected Pixels"]);
}

#[test]
fn a_move_started_while_an_empty_text_box_is_open_drops_the_box() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let mut b = text_begin();
    b["runs"] = json!([]);
    b["text"] = json!("");
    b["caret"] = json!(0);
    e.dispatch(id, "tool.text.begin", b).unwrap();
    e.move_begin(id, MoveKind::Pixels, move_options(0.0, 0.0))
        .unwrap();
    e.move_update(id, move_options(1.0, 0.0)).unwrap();
    e.finish_sessions(id).unwrap();
    assert_eq!(labels(&e, id), ["Open", "Move Selected Pixels"]);
}

#[test]
fn a_move_started_while_a_line_is_open_keeps_the_line() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    e.dispatch(
        id,
        "tool.line.begin",
        json!({"points":[[0,1],[7,1]],"primary":[0,255,0,255],"width":1.0}),
    )
    .unwrap();
    let line = e.query(id, "doc.pixels", json!({"preview":true})).unwrap();
    assert!(pixel(&line, 8, 3, 1)[1] > 0, "the line shows");
    e.move_begin(id, MoveKind::Pixels, move_options(0.0, 0.0))
        .unwrap();
    e.move_update(id, move_options(0.0, 3.0)).unwrap();
    e.finish_sessions(id).unwrap();
    assert_eq!(
        labels(&e, id),
        ["Open", "Line/Curve", "Move Selected Pixels"]
    );
    let px = e.query(id, "doc.pixels", json!({})).unwrap();
    assert_eq!(
        pixel(&px, 8, 3, 4),
        pixel(&line, 8, 3, 1),
        "the line moved down with the layer"
    );
}

#[test]
fn a_stroke_started_while_a_line_is_open_keeps_the_line() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    e.dispatch(
        id,
        "tool.line.begin",
        json!({"points":[[0,1],[7,1]],"primary":[0,255,0,255],"width":1.0}),
    )
    .unwrap();
    let brush = BrushSettings {
        tool: Tool::Paintbrush,
        width: 2.0,
        color: [255, 255, 255, 255],
        antialias: false,
        spacing: 0.1,
        pressure: PressureMode::None,
        blend: ToolCompositeMode::Normal,
    };
    e.stroke_begin(id, brush, 7).unwrap();
    e.stroke_append(id, &[StrokeSample::mouse(5.5, 6.5, 0)])
        .unwrap();
    e.stroke_commit(id).unwrap();
    e.finish_sessions(id).unwrap();
    assert_eq!(labels(&e, id), ["Open", "Line/Curve", "Paintbrush"]);
}

// MARK: Items 2 and 5

#[test]
fn doc_sync_answers_after_the_queued_commands_and_works_with_open_text() {
    let (e, rx) = engine_with_events();
    let id = e
        .import_pixels(1024, 512, &[9, 9, 9, 255].repeat(1024 * 512), None)
        .unwrap();
    let slow = Instant::now();
    e.submit(
        id,
        Op::Command("test.slow_invert".into(), json!({"delay_ms":100})),
        JobTicket::new(1),
    )
    .unwrap();
    e.submit(
        id,
        Op::Command("doc.sync".into(), json!({})),
        JobTicket::new(2),
    )
    .unwrap();
    let ev = wait_job(&rx, 2, Duration::from_secs(20));
    assert_eq!(ev.json["status"], "ok");
    assert!(
        slow.elapsed() >= Duration::from_millis(700),
        "it waited for the slow command"
    );
    assert_eq!(labels(&e, id).len(), 2, "the slow command ran first");
    // An open text session does not make the barrier fail (other commands are busy then).
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    e.submit(
        id,
        Op::Command("doc.sync".into(), json!({})),
        JobTicket::new(3),
    )
    .unwrap();
    assert_eq!(
        wait_job(&rx, 3, Duration::from_secs(5)).json["status"],
        "ok"
    );
    assert_eq!(info(&e, id)["dirty"], json!(true));
    assert_eq!(
        info(&e, id)["session_open"],
        json!(true),
        "the barrier leaves the text open"
    );
}

#[test]
fn doc_finish_sessions_is_a_queued_command() {
    let (e, rx) = engine_with_events();
    let id = blue(&e);
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    e.submit(
        id,
        Op::Command("doc.finish_sessions".into(), json!({})),
        JobTicket::new(5),
    )
    .unwrap();
    assert_eq!(
        wait_job(&rx, 5, Duration::from_secs(5)).json["status"],
        "ok"
    );
    assert_eq!(labels(&e, id), ["Open", "Text"]);
    assert_eq!(info(&e, id)["session_open"], json!(false));
}

#[test]
fn doc_dirty_is_the_edited_flag_alone() {
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let read = |e: &Engine| -> Value {
        serde_json::from_slice(&e.query(id, "doc.dirty", json!({})).unwrap()).unwrap()
    };
    assert_eq!(read(&e)["dirty"], json!(false));
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    assert_eq!(read(&e)["dirty"], json!(true), "open text counts");
    assert_eq!(read(&e)["revision"], info(&e, id)["revision"]);
}

// MARK: Item 9

#[test]
fn a_panic_while_save_finishes_the_open_sessions_makes_the_image_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.opaint");
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    e.dispatch(id, "tool.text.begin", text_begin()).unwrap();
    e.dispatch(id, "test.panic_on_finish", json!({})).unwrap();
    assert_eq!(
        e.save_document(id, &path, SaveFormat::Opaint, true),
        Err(Error::Panic)
    );
    assert!(!path.exists(), "nothing was written");
    assert!(
        e.is_read_only(id).unwrap(),
        "read-only, as after a command panic"
    );
    assert_eq!(info(&e, id)["read_only"], json!(true));
    assert_eq!(
        e.run_op(
            id,
            Op::Command("image.invert_colors".into(), json!({})),
            &JobTicket::new(0)
        ),
        Err(Error::ReadOnly)
    );
}

// MARK: Item 3

#[test]
fn a_recovery_record_keeps_the_original_content_type() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("rec");
    let original = root.path().join("photo.png");
    std::fs::write(&original, b"really a camera raw file").unwrap();
    let url = format!("file://{}", original.display());
    let e = Engine::new(None).unwrap();
    let id = blue(&e);
    let options = RecoveryOptions {
        settle: Duration::from_secs(60),
        max_interval: Duration::from_secs(600),
        hooks: RecordHooks::default(),
    };
    e.recovery_attach_typed(
        id,
        &dir,
        Some(url.clone()),
        Some("com.adobe.raw-image".into()),
        options.clone(),
    )
    .unwrap();
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    assert!(e.recovery_flush(id).unwrap().is_some());
    e.recovery_detach(id, false);
    let found = op_engine::recovery::scan(root.path());
    assert_eq!(found.len(), 1);
    let j = found[0].to_json();
    assert_eq!(j["original_url"], json!(url));
    assert_eq!(j["original_type"], json!("com.adobe.raw-image"));
    // Recover keeps it for the next record.
    let e2 = Engine::new(None).unwrap();
    let (back, manifest) = e2.recovery_open(&dir, options).unwrap();
    assert_eq!(
        manifest.original_type.as_deref(),
        Some("com.adobe.raw-image")
    );
    e2.dispatch(back, "image.invert_colors", json!({})).unwrap();
    e2.recovery_flush(back).unwrap();
    e2.recovery_detach(back, false);
    let again = op_engine::recovery::scan(root.path());
    assert_eq!(
        again[0].to_json()["original_type"],
        json!("com.adobe.raw-image")
    );
}

// MARK: Items 7 and 8

fn enabled(e: &Engine, id: u64, command: &str) -> bool {
    let list: Value =
        serde_json::from_slice(&e.query(id, "commands.list", json!({})).unwrap()).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == command)
        .unwrap_or_else(|| panic!("{command} is listed"))["enabled"]
        .as_bool()
        .unwrap()
}

fn run(e: &Engine, id: u64, command: &str, params: Value) -> op_engine::Result<()> {
    e.run_op(id, Op::Command(command.into(), params), &JobTicket::new(0))
        .map(|_| ())
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

/// 16384 x 16384 is 1 GiB of pixels per layer: 15 layers plus the slot kept
/// for a selection mask fill the 16 GiB an .opaint file holds.
#[test]
fn layers_stop_at_the_16_gib_pixel_total() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(16384, 16384, 72.0, [0, 0, 0, 0]).unwrap();
    for _ in 1..15 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    assert_eq!(info(&e, id)["layers"].as_array().unwrap().len(), 15);
    for command in ["layer.add", "layer.duplicate", "layer.import_from_file"] {
        assert!(
            !enabled(&e, id, command),
            "{command} is off at the pixel total"
        );
    }
    let err = run(&e, id, "layer.add", json!({})).unwrap_err().to_string();
    assert!(err.contains("16 GiB"), "{err}");
    let err = run(&e, id, "layer.duplicate", json!({}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("16 GiB"), "{err}");
    let Err(err) = e.paste_begin(id, paste_new_layer()) else {
        panic!("Paste into New Layer is refused");
    };
    let err = err.to_string();
    assert!(err.contains("16 GiB"), "{err}");
    assert_eq!(info(&e, id)["layers"].as_array().unwrap().len(), 15);
    // One layer fewer turns them on again.
    run(&e, id, "layer.delete", json!({})).unwrap();
    assert!(enabled(&e, id, "layer.add"));
    assert!(enabled(&e, id, "layer.duplicate"));
    assert!(enabled(&e, id, "layer.import_from_file"));
}

#[test]
fn layer_commands_are_off_at_1000_layers() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(4, 4, 72.0, [0, 0, 0, 0]).unwrap();
    for _ in 1..1000 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    for command in ["layer.add", "layer.duplicate", "layer.import_from_file"] {
        assert!(
            !enabled(&e, id, command),
            "{command} is off at 1,000 layers"
        );
    }
    let err = run(&e, id, "layer.add", json!({})).unwrap_err().to_string();
    assert!(err.contains("at most 1000 layers"), "{err}");
    run(&e, id, "layer.delete", json!({})).unwrap();
    assert!(enabled(&e, id, "layer.add"));
}

/// 10000 x 9000 is 360 MB per layer: 43 layers plus the mask slot hold
/// 15.84 GB. Growing the canvas to 10000 x 9700 (16.69 GB) would fit; to
/// 10000 x 10000 (17.6 GB, past 16 GiB = 17.18 GB) it is refused, by Canvas
/// Size and by Resize, and the image stays as it was.
#[test]
fn canvas_size_and_resize_stop_at_the_16_gib_pixel_total() {
    let e = Engine::new(None).unwrap();
    let id = e.new_document(10000, 9000, 72.0, [0, 0, 0, 0]).unwrap();
    for _ in 1..43 {
        run(&e, id, "layer.add", json!({})).unwrap();
    }
    let err = run(
        &e,
        id,
        "image.canvas_size",
        json!({"width":10000,"height":10000,"anchor":"center"}),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("16 GiB"), "{err}");
    let err = run(
        &e,
        id,
        "image.resize",
        json!({"width":10000,"height":10000,"maintain_aspect":false}),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("16 GiB"), "{err}");
    assert_eq!(info(&e, id)["height"], json!(9000));
    // The rule at the edge: 10000 x 9700 still fits, 10000 x 10000 does not.
    assert!(op_core::doc::check_pixel_budget(43, 10000, 9700).is_ok());
    assert!(op_core::doc::check_pixel_budget(43, 10000, 10000).is_err());
    assert!(op_core::doc::check_pixel_budget(15, 16384, 16384).is_ok());
    assert!(op_core::doc::check_pixel_budget(16, 16384, 16384).is_err());
}
