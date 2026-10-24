//! Job cancellation, job identity checks, preview commit bytes and the random
//! published-snapshot check.
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData};
use op_engine::{Engine, Error, Event, EventKind, JobTicket, Op, Published};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn engine_with_events() -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let e = Engine::new(Some(Arc::new(move |ev| {
        let _ = tx.lock().unwrap().send(ev);
    })))
    .unwrap();
    (e, rx)
}
/// A 1024x512 image: 4x2 = 8 tiles, every pixel opaque.
fn big(e: &Engine) -> u64 {
    let (w, h) = (1024u32, 512u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, 7, 255]);
        }
    }
    e.import_pixels(w, h, &px, None).unwrap()
}
fn small(e: &Engine) -> u64 {
    e.import_pixels(2, 1, &[10, 20, 30, 255, 4, 5, 6, 0], None)
        .unwrap()
}
fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
fn history_len(e: &Engine, id: u64) -> usize {
    info(e, id)["history_labels"].as_array().unwrap().len()
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
fn status(ev: &Event) -> &str {
    ev.json["status"].as_str().unwrap()
}
fn mismatch(ev: &Event) -> Vec<String> {
    ev.json["mismatch"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

// ----- Cancellation of running work -----

#[test]
fn cancel_stops_a_running_command_without_commit_and_releases_tiles() {
    let (e, rx) = engine_with_events();
    let doc = big(&e);
    let store = e.tile_store(doc).unwrap();
    let base_tiles = store.stats().tile_count;
    let before = e.query(doc, "doc.pixels", json!({})).unwrap();
    let ticket = JobTicket::new(7);
    // 8 tiles at 100 ms each: about 800 ms if it ran to the end.
    e.submit(
        doc,
        Op::Command("test.slow_invert".into(), json!({"delay_ms": 100})),
        ticket.clone(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    let mid_tiles = store.stats().tile_count;
    let start = Instant::now();
    ticket.cancel();
    let ev = wait_job(&rx, 7, Duration::from_secs(3));
    let took = start.elapsed();
    assert_eq!(status(&ev), "cancelled", "{:?}", ev.json);
    assert!(took < Duration::from_millis(300), "cancel took {took:?}");
    assert!(mid_tiles > base_tiles, "the job had written tiles mid-run");
    assert_eq!(history_len(&e, doc), 1);
    assert_eq!(e.snapshot(doc).unwrap().revision, 0);
    assert!(e.query(doc, "doc.pixels", json!({})).unwrap() == before);
    assert_eq!(store.stats().tile_count, base_tiles);
    e.close(doc).unwrap();
    assert_eq!(store.stats().tile_count, 0);
}

#[test]
fn cancel_stops_a_running_preview_render_and_publishes_nothing() {
    let (e, rx) = engine_with_events();
    let doc = big(&e);
    let store = e.tile_store(doc).unwrap();
    let base_tiles = store.stats().tile_count;
    let ticket = JobTicket::new(8);
    e.submit(
        doc,
        Op::PreviewBegin("test.slow_invert".into(), json!({"delay_ms": 100})),
        ticket.clone(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    let start = Instant::now();
    ticket.cancel();
    let ev = wait_job(&rx, 8, Duration::from_secs(3));
    let took = start.elapsed();
    assert_eq!(status(&ev), "cancelled", "{:?}", ev.json);
    assert!(took < Duration::from_millis(300), "cancel took {took:?}");
    assert!(e.snapshot(doc).unwrap().preview.is_none());
    assert_eq!(history_len(&e, doc), 1);
    assert_eq!(store.stats().tile_count, base_tiles);
    e.close(doc).unwrap();
    assert_eq!(store.stats().tile_count, 0);
}

#[test]
fn cancel_of_a_queued_command_skips_it() {
    let (e, rx) = engine_with_events();
    let doc = big(&e);
    e.submit(
        doc,
        Op::Command("test.slow_invert".into(), json!({"delay_ms": 20})),
        JobTicket::new(1),
    )
    .unwrap();
    let queued = JobTicket::new(2);
    e.submit(
        doc,
        Op::Command("layer.add".into(), json!({})),
        queued.clone(),
    )
    .unwrap();
    queued.cancel();
    assert_eq!(status(&wait_job(&rx, 1, Duration::from_secs(5))), "ok");
    assert_eq!(
        status(&wait_job(&rx, 2, Duration::from_secs(5))),
        "cancelled"
    );
    assert_eq!(e.snapshot(doc).unwrap().state.layers.len(), 1);
    assert_eq!(history_len(&e, doc), 2);
}

#[test]
fn blocking_dispatch_reports_cancelled() {
    let (e, _rx) = engine_with_events();
    let doc = big(&e);
    let ticket = JobTicket::new(3);
    ticket.cancel();
    assert_eq!(
        e.dispatch_with(doc, "image.invert_colors", json!({}), &ticket),
        Err(Error::Cancelled)
    );
    assert_eq!(history_len(&e, doc), 1);
}

// ----- Job identity: one test per identity field -----

/// Starts a preview whose render takes `delay_ms`, on ticket `t`.
fn slow_fill(e: &Engine, doc: u64, t: u64, delay_ms: u64) {
    e.submit(
        doc,
        Op::PreviewBegin(
            "test.preview_fill".into(),
            json!({"value": 99, "delay_ms": delay_ms}),
        ),
        JobTicket::new(t),
    )
    .unwrap();
}
/// Publishes a preview with a fast render, then injects a result whose identity
/// differs from the current one in `field` only. The result must be dropped and
/// name exactly that field. A matching result is the control and is published.
fn injected_single_field_mismatch_is_dropped(field: &str) {
    let (e, rx) = engine_with_events();
    let doc = small(&e);
    slow_fill(&e, doc, 1, 0);
    assert_eq!(status(&wait_job(&rx, 1, Duration::from_secs(5))), "ok");
    let shown = e
        .query(doc, "doc.pixels", json!({"preview": true}))
        .unwrap();
    assert_eq!(shown[0], 99);
    let current = e.current_identity(doc).unwrap();
    let base = e.snapshot(doc).unwrap().state.clone();
    let mut tile = base.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    tile[0] = 33;
    let result = base
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile).unwrap(),
        )
        .unwrap();
    let mut wrong = current;
    match field {
        "base_revision" => wrong.base_revision += 1,
        "target_layer" => wrong.target_layer.0 += 100,
        "selection_revision" => wrong.selection_revision += 1,
        other => panic!("unknown field {other}"),
    }
    e.inject_job_result(doc, wrong, result.clone(), JobTicket::new(2))
        .unwrap();
    let ev = wait_job(&rx, 2, Duration::from_secs(5));
    assert_eq!(status(&ev), "dropped");
    assert_eq!(mismatch(&ev), vec![field.to_owned()]);
    assert!(
        e.query(doc, "doc.pixels", json!({"preview": true}))
            .unwrap()
            == shown
    );
    assert_eq!(history_len(&e, doc), 1);
    // Control: the same result with the current identity is shown.
    e.inject_job_result(doc, current, result, JobTicket::new(3))
        .unwrap();
    assert_eq!(status(&wait_job(&rx, 3, Duration::from_secs(5))), "ok");
    assert_eq!(
        e.query(doc, "doc.pixels", json!({"preview": true}))
            .unwrap()[0],
        33
    );
    assert_eq!(history_len(&e, doc), 1);
}

#[test]
fn job_dropped_when_base_revision_changed() {
    let (e, rx) = engine_with_events();
    let doc = small(&e);
    slow_fill(&e, doc, 1, 300);
    // layer.add puts the new layer on top; the active layer stays the same.
    e.dispatch(doc, "layer.add", json!({})).unwrap();
    let ev = wait_job(&rx, 1, Duration::from_secs(5));
    assert_eq!(status(&ev), "dropped");
    let m = mismatch(&ev);
    assert!(m.contains(&"base_revision".into()), "{m:?}");
    assert!(!m.contains(&"target_layer".into()), "{m:?}");
    assert!(!m.contains(&"selection_revision".into()), "{m:?}");
    assert!(e.snapshot(doc).unwrap().preview.is_none());
    assert_eq!(e.query(doc, "doc.pixels", json!({})).unwrap()[0], 10);
    assert_eq!(history_len(&e, doc), 2);
    injected_single_field_mismatch_is_dropped("base_revision");
}

#[test]
fn job_dropped_when_target_layer_changed() {
    let (e, rx) = engine_with_events();
    let doc = small(&e);
    e.dispatch(doc, "layer.add", json!({})).unwrap();
    slow_fill(&e, doc, 1, 300);
    // Deleting the active layer makes the other layer active.
    e.dispatch(doc, "layer.delete", json!({})).unwrap();
    let ev = wait_job(&rx, 1, Duration::from_secs(5));
    assert_eq!(status(&ev), "dropped");
    let m = mismatch(&ev);
    assert!(m.contains(&"target_layer".into()), "{m:?}");
    assert!(!m.contains(&"selection_revision".into()), "{m:?}");
    assert!(e.snapshot(doc).unwrap().preview.is_none());
    assert_eq!(history_len(&e, doc), 3);
    injected_single_field_mismatch_is_dropped("target_layer");
}

#[test]
fn job_dropped_when_selection_revision_changed() {
    let (e, rx) = engine_with_events();
    let doc = small(&e);
    slow_fill(&e, doc, 1, 300);
    e.dispatch(doc, "test.set_selection", json!({})).unwrap();
    let ev = wait_job(&rx, 1, Duration::from_secs(5));
    assert_eq!(status(&ev), "dropped");
    let m = mismatch(&ev);
    assert!(m.contains(&"selection_revision".into()), "{m:?}");
    assert!(!m.contains(&"target_layer".into()), "{m:?}");
    assert!(e.snapshot(doc).unwrap().preview.is_none());
    assert_eq!(e.query(doc, "doc.pixels", json!({})).unwrap()[0], 10);
    assert_eq!(history_len(&e, doc), 2);
    injected_single_field_mismatch_is_dropped("selection_revision");
}

// ----- preview.commit gives the same bytes as the direct command -----

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn preview_commit_matches_direct_command_bytes() {
    for (png, command, params) in [
        ("plasma_alpha_512x384.png", "image.invert_colors", json!({})),
        (
            "plasma_512x384.png",
            "test.preview_fill",
            json!({"value": 77}),
        ),
    ] {
        let png = fixture(png);
        let (e, rx) = engine_with_events();
        let previewed = e.open_png(&png).unwrap();
        let direct = e.open_png(&png).unwrap();
        let original = e.query(previewed, "doc.pixels", json!({})).unwrap();
        e.submit(
            previewed,
            Op::PreviewBegin(command.into(), params.clone()),
            JobTicket::new(1),
        )
        .unwrap();
        assert_eq!(status(&wait_job(&rx, 1, Duration::from_secs(10))), "ok");
        let shown = e
            .query(previewed, "doc.pixels", json!({"preview": true}))
            .unwrap();
        assert!(e.query(previewed, "doc.pixels", json!({})).unwrap() == original);
        e.preview_commit(previewed).unwrap();
        e.dispatch(direct, command, params).unwrap();
        let committed = e.query(previewed, "doc.pixels", json!({})).unwrap();
        let expected = e.query(direct, "doc.pixels", json!({})).unwrap();
        assert!(expected != original, "{command} changed nothing");
        assert!(committed == expected, "{command}: commit bytes differ");
        assert!(shown == expected, "{command}: preview bytes differ");
        assert_eq!(history_len(&e, previewed), 2);
        assert_eq!(history_len(&e, direct), 2);
    }
}

#[test]
fn invert_keeps_alpha_and_inverts_straight_rgb() {
    let png = fixture("plasma_alpha_512x384.png");
    let e = Engine::new(None).unwrap();
    let doc = e.open_png(&png).unwrap();
    let state = e.snapshot(doc).unwrap().state.clone();
    e.dispatch(doc, "image.invert_colors", json!({})).unwrap();
    let after = e.snapshot(doc).unwrap().state.clone();
    let mut checked = 0;
    for (coord, _) in state.layers[0].tiles.iter() {
        let a = state.read_tile(0, coord).unwrap();
        let b = after.read_tile(0, coord).unwrap();
        assert_eq!(a.len(), TILE_BYTES);
        // Only pixels inside the image; the padding of edge tiles stays zero.
        let w = (state.width as usize - coord.x as usize * TILE_SIZE).min(TILE_SIZE);
        let h = (state.height as usize - coord.y as usize * TILE_SIZE).min(TILE_SIZE);
        for y in 0..h {
            for x in 0..w {
                let i = (y * TILE_SIZE + x) * 4;
                let (p, q) = (&a[i..i + 4], &b[i..i + 4]);
                assert_eq!(q, [255 - p[0], 255 - p[1], 255 - p[2], p[3]]);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 512 * 384);
}

// ----- Random operations: published snapshot equals the actor's state -----

fn assert_same(published: &Published, actor: &Published, step: usize) {
    let (p, a) = (&published.state, &actor.state);
    assert_eq!(published.revision, actor.revision, "step {step}");
    assert_eq!(
        published.history.labels, actor.history.labels,
        "step {step}"
    );
    assert_eq!(published.history.index, actor.history.index, "step {step}");
    assert_eq!(published.history.dirty, actor.history.dirty, "step {step}");
    assert_eq!((p.width, p.height), (a.width, a.height), "step {step}");
    assert_eq!(p.active_layer, a.active_layer, "step {step}");
    assert_eq!(p.revision, a.revision, "step {step}");
    assert_eq!(p.selection, a.selection, "step {step}");
    assert_eq!(p.layers.len(), a.layers.len(), "step {step}");
    for (index, (lp, la)) in p.layers.iter().zip(&a.layers).enumerate() {
        assert_eq!(lp.id, la.id, "step {step}");
        assert_eq!(lp.name, la.name, "step {step}");
        assert_eq!(lp.visible, la.visible, "step {step}");
        assert_eq!(lp.opacity, la.opacity, "step {step}");
        assert_eq!(
            lp.tiles.iter().collect::<Vec<_>>(),
            la.tiles.iter().collect::<Vec<_>>(),
            "step {step}"
        );
        for tx in 0..2 {
            let coord = TileCoord { x: tx, y: 0 };
            assert_eq!(
                p.read_tile(index, coord).unwrap(),
                a.read_tile(index, coord).unwrap(),
                "step {step}"
            );
        }
    }
}

#[test]
fn random_operations_keep_published_snapshot_equal_to_actor_state() {
    const CASES: u64 = 25;
    const STEPS: usize = 50;
    let mut total = 0;
    for case in 0..CASES {
        let e = Engine::new(None).unwrap();
        // 300x8 spans two tiles.
        let px: Vec<u8> = (0..300 * 8 * 4).map(|i| (i * 7 % 256) as u8 | 1).collect();
        let doc = e.import_pixels(300, 8, &px, None).unwrap();
        let mut seed = case.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        for step in 0..STEPS {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = e.snapshot(doc).unwrap().history.labels.len();
            let (cmd, params) = match seed % 7 {
                0 => ("layer.add", json!({})),
                1 => ("layer.delete", json!({})),
                2 => ("image.invert_colors", json!({})),
                3 => ("edit.undo", json!({})),
                4 => ("edit.redo", json!({})),
                5 => ("test.set_selection", json!({})),
                _ => ("history.jump", json!({"index": (seed >> 8) as usize % len})),
            };
            match e.dispatch(doc, cmd, params) {
                Ok(()) => {}
                // layer.delete on the last layer is disabled.
                Err(Error::BadArgument(_)) if cmd == "layer.delete" => {}
                Err(other) => panic!("case {case} step {step} {cmd}: {other}"),
            }
            let published = e.snapshot(doc).unwrap();
            let actor = e.actor_view(doc).unwrap();
            assert_same(&published, &actor, step);
            total += 1;
        }
        e.close(doc).unwrap();
    }
    assert!(total >= 1000, "ran {total} operations");
}
