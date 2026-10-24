use op_core::doc::{DocState, MaskGrid, PolygonSet, SelectionSource, SelectionState};
use op_core::history::History;
use op_core::selection::{CombineMode, RasterMode, Selection, SelectionContinuation};
use op_core::tiles::TileStore;
use std::sync::Arc;

fn rect(x: f64, y: f64, w: f64, h: f64) -> PolygonSet {
    PolygonSet::rectangle(x, y, w, h)
}

fn bytes(s: &Selection, w: u32, h: u32) -> Vec<u8> {
    s.rasterize(w, h, RasterMode::Antialiased).bytes.to_vec()
}

#[test]
fn analytic_coverage_and_pixel_centers() {
    let cases = [
        (rect(0.25, 0.25, 0.5, 0.5), 64),
        (rect(0.1, 0.0, 0.03, 1.0), 8),
        (
            PolygonSet::lasso(vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
            128,
        ),
        (
            PolygonSet::lasso(vec![(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
            128,
        ),
    ];
    for (shape, expected) in cases {
        let actual = bytes(&Selection::active_geometry(shape), 1, 1)[0];
        assert!(
            (i16::from(actual) - expected).abs() <= 1,
            "{actual} vs {expected}"
        );
    }
    assert_eq!(
        Selection::active_geometry(rect(0.0, 0.0, 1.0, 1.0))
            .rasterize(2, 1, RasterMode::Pixelated)
            .bytes
            .as_ref(),
        &[255, 0]
    );
}

#[test]
fn boolean_identities_on_50_shape_pairs() {
    let mut seed = 0x9e3779b97f4a7c15_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 500) as f64 / 100.0 - 1.0
    };
    let all = Selection::select_all(4, 4);
    for _ in 0..50 {
        let a = Selection::active_geometry(rect(next(), next(), 1.25, 1.75));
        let b = Selection::active_geometry(rect(next(), next(), 1.5, 1.1));
        let ab = a.combine(&b, CombineMode::Add, 4, 4);
        let ai = a.combine(&b, CombineMode::Intersect, 4, 4);
        let ax = a.combine(&b, CombineMode::Invert, 4, 4);
        for (actual, expected) in [
            (a.combine(&a, CombineMode::Add, 4, 4), a.clone()),
            (a.combine(&a, CombineMode::Intersect, 4, 4), a.clone()),
            (
                a.combine(&a, CombineMode::Subtract, 4, 4),
                Selection::empty(),
            ),
            (a.combine(&a, CombineMode::Invert, 4, 4), Selection::empty()),
            (ax, ab.combine(&ai, CombineMode::Subtract, 4, 4)),
            (
                all.combine(&ab, CombineMode::Subtract, 4, 4),
                all.combine(&a, CombineMode::Subtract, 4, 4).combine(
                    &all.combine(&b, CombineMode::Subtract, 4, 4),
                    CombineMode::Intersect,
                    4,
                    4,
                ),
            ),
        ] {
            for (x, y) in bytes(&actual, 4, 4).iter().zip(bytes(&expected, 4, 4)) {
                assert!((i16::from(*x) - i16::from(y)).abs() <= 1);
            }
        }
    }
}

#[test]
fn geometry_edge_cases() {
    let hole = Selection::active_geometry(rect(0.0, 0.0, 3.0, 3.0)).combine(
        &Selection::active_geometry(rect(1.0, 1.0, 1.0, 1.0)),
        CombineMode::Subtract,
        3,
        3,
    );
    assert_eq!(
        bytes(&hole, 3, 3),
        vec![255, 255, 255, 255, 0, 255, 255, 255, 255]
    );
    let touching = Selection::active_geometry(rect(0.0, 0.0, 1.0, 1.0)).combine(
        &Selection::active_geometry(rect(1.0, 0.0, 1.0, 1.0)),
        CombineMode::Add,
        2,
        1,
    );
    assert_eq!(bytes(&touching, 2, 1), vec![255, 255]);
    let bow = Selection::active_geometry(PolygonSet::lasso(vec![
        (0.0, 0.0),
        (2.0, 2.0),
        (0.0, 2.0),
        (2.0, 0.0),
    ]));
    assert_eq!(bytes(&bow, 2, 2), vec![128, 128, 128, 128]);
    assert_eq!(
        bytes(
            &Selection::active_geometry(rect(-1.0, -1.0, 2.0, 2.0)),
            2,
            2
        ),
        vec![255, 0, 0, 0]
    );
    assert_eq!(
        bytes(&Selection::active_geometry(rect(1e6, 1e6, 1.0, 1.0)), 2, 2),
        vec![0; 4]
    );
    assert_eq!(
        bytes(&Selection::active_geometry(rect(1.0, 1.0, 1.0, 1.0)), 3, 3)[4],
        255
    );
}

#[test]
fn soft_mask_combine_and_states() {
    let soft = Selection::active_coverage(MaskGrid {
        width: 2,
        height: 1,
        bytes: Arc::from([31, 207]),
    });
    let geom = Selection::active_geometry(rect(0.0, 0.0, 1.0, 1.0));
    assert_eq!(bytes(&soft, 2, 1), vec![31, 207]);
    for (mode, expected) in [
        (CombineMode::Add, vec![255, 207]),
        (CombineMode::Intersect, vec![31, 0]),
        (CombineMode::Subtract, vec![0, 207]),
        (CombineMode::Invert, vec![224, 207]),
        (CombineMode::Replace, vec![255, 0]),
    ] {
        assert_eq!(bytes(&soft.combine(&geom, mode, 2, 1), 2, 1), expected);
        assert_eq!(bytes(&soft, 2, 1), vec![31, 207]);
    }
    assert_eq!(bytes(&Selection::none(), 2, 1), vec![255, 255]);
    assert_eq!(bytes(&Selection::empty(), 2, 1), vec![0, 0]);
    assert_eq!(bytes(&Selection::select_all(2, 1), 2, 1), vec![255, 255]);
    assert_eq!(bytes(&soft.invert(2, 1), 2, 1), vec![224, 48]);
}

#[test]
fn offsets_transforms_and_history() {
    let selected = Selection::active_geometry(rect(0.0, 0.0, 1.0, 1.0));
    assert_eq!(
        bytes(&selected.offset(1.0, 0.0, 3, 1), 3, 1),
        vec![0, 255, 0]
    );
    assert_eq!(
        bytes(&selected.expand(1, 3, 3), 3, 3),
        vec![255, 255, 0, 255, 255, 0, 0, 0, 0]
    );
    assert_eq!(
        bytes(&selected.translate(1.0, 0.0, 3, 1), 3, 1),
        vec![0, 255, 0]
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 1024 * 1024).unwrap());
    let initial = DocState::new(store, 2, 1, 72.0).unwrap();
    let mut history = History::new(initial);
    let changed = history
        .current_state()
        .set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            rect(0.0, 0.0, 1.0, 1.0),
        ))));
    assert_eq!(changed.selection_revision, 1);
    let a = changed.rasterize_selection(RasterMode::Antialiased);
    let b = changed.rasterize_selection(RasterMode::Antialiased);
    assert!(Arc::ptr_eq(&a, &b));
    history.commit("select", "select", changed);
    assert!(history.undo());
    assert_eq!(history.current_state().selection, SelectionState::None);
    assert_eq!(history.current_state().selection_revision, 0);
    assert!(history.redo());
    assert_eq!(history.current_state().selection_revision, 1);
    assert!(matches!(
        history.current_state().selection,
        SelectionState::Active(_)
    ));
}

#[test]
fn continuation_previews_without_changing_base_and_commits_once() {
    let base = Selection::active_geometry(rect(0.0, 0.0, 1.0, 1.0));
    let pending = SelectionContinuation::new(
        base.clone(),
        CombineMode::Add,
        Selection::active_geometry(rect(1.0, 0.0, 1.0, 1.0)),
    );
    assert_eq!(bytes(&base, 3, 1), vec![255, 0, 0]);
    assert_eq!(bytes(&pending.preview(3, 1), 3, 1), vec![255, 255, 0]);
    let pending = pending.with_interim_transform([1.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
    assert_eq!(bytes(&pending.preview(3, 1), 3, 1), vec![0, 255, 255]);
    assert_eq!(bytes(&pending.commit(3, 1), 3, 1), vec![0, 255, 255]);
    assert_eq!(bytes(&base, 3, 1), vec![255, 0, 0]);
}

#[test]
fn ellipse_transform_contract_and_contour_cache() {
    let ellipse = PolygonSet::ellipse(0.0, 0.0, 2.0, 2.0);
    assert!(ellipse.0[0].len() >= 16);
    let selected = Selection::active_geometry(rect(0.0, 0.0, 3.0, 3.0));
    assert!(matches!(
        selected.expand(1, 3, 3).state(),
        SelectionState::Active(SelectionSource::Geometry(_))
    ));
    assert!(matches!(
        selected.contract(1, 3, 3).state(),
        SelectionState::Active(SelectionSource::Geometry(_))
    ));
    assert_eq!(
        bytes(&selected.contract(1, 3, 3), 3, 3),
        vec![0, 0, 0, 0, 255, 0, 0, 0, 0]
    );
    let turned = Selection::active_geometry(rect(0.0, 0.0, 1.0, 2.0))
        .rotate(std::f64::consts::FRAC_PI_2, (0.0, 0.0), 2, 2)
        .translate(2.0, 0.0, 2, 2);
    assert_eq!(bytes(&turned, 2, 2), vec![255, 255, 0, 0]);
    let a = selected.contours(3, 3);
    let b = selected.contours(3, 3);
    assert!(Arc::ptr_eq(&a, &b));
    let a = selected.rasterize(3, 3, RasterMode::Antialiased);
    let b = selected.rasterize(3, 3, RasterMode::Antialiased);
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(selected.revision(), 0);
    assert_eq!(selected.expand(1, 3, 3).revision(), 1);
}
