use op_engine::Engine;
use op_ops::image::Resampling;
use op_tools::r#move::{Affine, MoveKind, MoveOptions};

fn engine() -> (Engine, u64, Vec<u8>) {
    let e = Engine::new(None).unwrap();
    let pixels: Vec<u8> = (0..64)
        .flat_map(|i| [i as u8, (i * 3) as u8, 99, 255])
        .collect();
    let id = e.import_pixels(8, 8, &pixels, None).unwrap();
    (e, id, pixels)
}
fn options() -> MoveOptions {
    MoveOptions {
        transform: Affine::identity(),
        resampling: Resampling::Nearest,
        copy: false,
    }
}
#[test]
fn one_session_many_updates_commits_once() {
    let (e, id, _) = engine();
    e.move_begin(id, MoveKind::Pixels, options()).unwrap();
    for i in 0..100 {
        e.move_update(
            id,
            MoveOptions {
                transform: Affine::translation(i as f64 % 3.0, 0.0),
                ..options()
            },
        )
        .unwrap();
    }
    let before = e
        .snapshot(id)
        .unwrap()
        .preview
        .as_ref()
        .unwrap()
        .read_tile(0, op_core::tiles::TileCoord { x: 0, y: 0 })
        .unwrap();
    e.move_finish(id).unwrap();
    let snapshot = e.snapshot(id).unwrap();
    assert_eq!(snapshot.history.labels, ["Open", "Move Selected Pixels"]);
    assert_eq!(
        before,
        snapshot
            .state
            .read_tile(0, op_core::tiles::TileCoord { x: 0, y: 0 })
            .unwrap()
    );
}
#[test]
fn finished_moves_are_separate_steps() {
    let (e, id, _) = engine();
    for _ in 0..100 {
        e.move_begin(id, MoveKind::Selection, options()).unwrap();
        e.move_finish(id).unwrap();
    }
    assert_eq!(e.snapshot(id).unwrap().history.labels.len(), 101);
}
#[test]
fn identity_and_outline_only() {
    let (e, id, pixels) = engine();
    e.move_begin(id, MoveKind::Pixels, options()).unwrap();
    e.move_finish(id).unwrap();
    assert_eq!(
        e.query(id, "doc.pixels", serde_json::json!({})).unwrap(),
        pixels
    );
    e.move_begin(
        id,
        MoveKind::Selection,
        MoveOptions {
            transform: Affine::translation(1.0, 0.0),
            ..options()
        },
    )
    .unwrap();
    e.move_finish(id).unwrap();
    assert_eq!(
        e.query(id, "doc.pixels", serde_json::json!({})).unwrap(),
        pixels
    );
}

#[test]
fn cancellation_discards_preview() {
    let (e, id, pixels) = engine();
    e.move_begin(
        id,
        MoveKind::Pixels,
        MoveOptions {
            transform: Affine::translation(2.0, 0.0),
            ..options()
        },
    )
    .unwrap();
    assert_ne!(
        e.query(id, "doc.pixels", serde_json::json!({"preview":true}))
            .unwrap(),
        pixels
    );
    e.move_cancel(id).unwrap();
    assert_eq!(e.snapshot(id).unwrap().history.labels, ["Open"]);
    assert_eq!(
        e.query(id, "doc.pixels", serde_json::json!({})).unwrap(),
        pixels
    );
}

#[test]
fn layer_change_finishes_the_live_move_first() {
    let (e, id, _) = engine();
    e.move_begin(
        id,
        MoveKind::Pixels,
        MoveOptions {
            transform: Affine::translation(1.0, 0.0),
            ..options()
        },
    )
    .unwrap();
    e.dispatch(id, "layer.add", serde_json::json!({"name":"Top"}))
        .unwrap();
    assert_eq!(
        e.snapshot(id).unwrap().history.labels,
        ["Open", "Move Selected Pixels", "Add New Layer"]
    );
    assert!(e.move_finish(id).is_err());
}
