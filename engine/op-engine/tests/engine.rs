use op_engine::{Engine, Error};
use serde_json::{Value, json};
fn fresh() -> (Engine, u64) {
    let e = Engine::new(None).unwrap();
    let id = e
        .import_pixels(2, 1, &[10, 20, 30, 255, 4, 5, 6, 0], None)
        .unwrap();
    (e, id)
}
fn info(e: &Engine, id: u64) -> Value {
    serde_json::from_slice(&e.query(id, "doc.info", json!({})).unwrap()).unwrap()
}
#[test]
fn commands_pixels_history_and_validation() {
    let (e, id) = fresh();
    assert_eq!(
        e.query(id, "doc.pixels", json!({})).unwrap()[..4],
        [10, 20, 30, 255]
    );
    assert!(matches!(
        e.dispatch(id, "history.jump", json!({"index":-1})),
        Err(Error::BadArgument(_))
    ));
    assert!(matches!(
        e.dispatch(id, "layer.add", json!({"extra":1})),
        Err(Error::BadArgument(_))
    ));
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    assert_eq!(
        e.query(id, "doc.pixels", json!({})).unwrap()[..4],
        [245, 235, 225, 255]
    );
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(
        e.query(id, "doc.pixels", json!({})).unwrap()[..4],
        [10, 20, 30, 255]
    );
    e.dispatch(id, "edit.redo", json!({})).unwrap();
    assert_eq!(info(&e, id)["history_index"], 1);
    e.dispatch(id, "history.jump", json!({"index":0})).unwrap();
    assert_eq!(info(&e, id)["dirty"], false);
    e.close(id).unwrap();
    assert!(matches!(e.snapshot(id), Err(Error::NoSuchDoc)));
}
#[test]
fn previews_commit_once_and_cancel_without_history() {
    let (e, id) = fresh();
    let initial = e.query(id, "doc.pixels", json!({})).unwrap();
    e.preview_begin(id, "image.invert_colors", json!({}))
        .unwrap();
    for _ in 0..100 {
        e.preview_update(id, json!({})).unwrap();
    }
    e.preview_commit(id).unwrap();
    assert_eq!(info(&e, id)["history_labels"].as_array().unwrap().len(), 2);
    let direct = e.query(id, "doc.pixels", json!({})).unwrap();
    e.preview_begin(id, "image.invert_colors", json!({}))
        .unwrap();
    e.preview_cancel(id).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), direct);
    assert_eq!(info(&e, id)["history_labels"].as_array().unwrap().len(), 2);
    e.dispatch(id, "edit.undo", json!({})).unwrap();
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), initial);
}
#[test]
fn stale_preview_generation_and_edit_cannot_publish() {
    let (e, id) = fresh();
    e.preview_begin(
        id,
        "test.preview_fill",
        json!({"value": 40, "delay_ms": 200}),
    )
    .unwrap();
    e.preview_update(id, json!({"value": 80, "delay_ms": 0}))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(
        e.query(id, "doc.pixels", json!({"preview":true})).unwrap()[0],
        80
    );
    assert_eq!(info(&e, id)["history_labels"].as_array().unwrap().len(), 1);
    e.preview_update(id, json!({"value": 120, "delay_ms": 200}))
        .unwrap();
    e.dispatch(id, "layer.add", json!({})).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(e.snapshot(id).unwrap().preview.is_none());
    assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap()[0], 10);
    assert_eq!(info(&e, id)["history_labels"].as_array().unwrap().len(), 2);
}

#[test]
fn command_and_worker_panic_keep_snapshot_readable() {
    for worker in [false, true] {
        let (e, id) = fresh();
        let original = e.query(id, "doc.pixels", json!({})).unwrap();
        if worker {
            e.preview_begin(id, "test.panic_preview", json!({}))
                .unwrap();
            for _ in 0..100 {
                if e.is_read_only(id).unwrap() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        } else {
            assert_eq!(e.dispatch(id, "test.panic", json!({})), Err(Error::Panic));
        }
        assert!(e.is_read_only(id).unwrap());
        assert_eq!(e.dispatch(id, "layer.add", json!({})), Err(Error::ReadOnly));
        assert_eq!(e.query(id, "doc.pixels", json!({})).unwrap(), original);
    }
}
#[test]
fn random_command_sequences_keep_published_summary_consistent() {
    let (e, id) = fresh();
    let mut seed = 1u64;
    for _ in 0..1000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let current = info(&e, id);
        let len = current["history_labels"].as_array().unwrap().len();
        let cmd = match seed % 4 {
            0 => "layer.add",
            1 => "edit.undo",
            2 => "edit.redo",
            _ => "history.jump",
        };
        let params = if cmd == "history.jump" {
            json!({"index":seed as usize%len})
        } else {
            json!({})
        };
        e.dispatch(id, cmd, params).unwrap();
        let p = e.snapshot(id).unwrap();
        assert_eq!(
            p.history.index,
            info(&e, id)["history_index"].as_u64().unwrap() as usize
        );
        assert!(p.history.index < p.history.labels.len());
        let (actor_state, actor_index, actor_len, actor_revision) = e.actor_probe(id).unwrap();
        assert_eq!(p.history.index, actor_index);
        assert_eq!(p.history.labels.len(), actor_len);
        assert_eq!(p.revision, actor_revision);
        assert_eq!(p.state.active_layer, actor_state.active_layer);
        assert_eq!(p.state.revision, actor_state.revision);
        assert_eq!(
            p.state.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
            actor_state.layers.iter().map(|l| l.id).collect::<Vec<_>>()
        );
        for index in 0..p.state.layers.len() {
            let coord = op_core::tiles::TileCoord { x: 0, y: 0 };
            assert_eq!(
                p.state.read_tile(index, coord).unwrap(),
                actor_state.read_tile(index, coord).unwrap()
            );
        }
    }
}
#[test]
fn all_tiles_release_after_close() {
    let (e, id) = fresh();
    let store = e.tile_store(id).unwrap();
    e.dispatch(id, "image.invert_colors", json!({})).unwrap();
    e.close(id).unwrap();
    assert_eq!(store.stats().tile_count, 0);
    assert!(matches!(e.snapshot(id), Err(Error::NoSuchDoc)));
}
#[test]
fn close_waits_for_in_flight_preview_job() {
    let (engine, id) = fresh();
    engine
        .preview_begin(id, "test.slow_invert", json!({"delay_ms": 250}))
        .unwrap();
    let start = std::time::Instant::now();
    engine.close(id).unwrap();
    assert!(start.elapsed() >= std::time::Duration::from_millis(200));
}
