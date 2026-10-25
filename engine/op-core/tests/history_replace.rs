//! `History::replace_current`: a change that is not an edit (M20: choosing the
//! active layer) keeps the step list, the labels and the dirty flag.
use op_core::doc::DocState;
use op_core::history::History;
use op_core::tiles::TileStore;
use std::sync::Arc;

#[test]
fn replace_current_keeps_steps_labels_and_dirty_flag() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TileStore::new(dir.path(), 64 << 20).unwrap());
    let state = DocState::new(store, 8, 8, 96.0).unwrap();
    let two = state.add_layer(1, "Two").unwrap();
    let mut h = History::new(state);
    h.commit("Add", "layer.add", two.clone());
    assert!(h.is_dirty());
    h.undo();
    assert!(!h.is_dirty());
    // At the saved step: replacing the state keeps the document clean.
    let chosen = h.current_state().set_active_layer(0).unwrap();
    h.replace_current(chosen);
    assert!(!h.is_dirty());
    assert_eq!(h.len(), 2, "redo step kept");
    h.redo();
    let top = h.current_state().set_active_layer(1).unwrap();
    h.replace_current(top);
    assert_eq!(h.len(), 2);
    assert_eq!(h.label(1), Some("Add"));
    assert_eq!(h.command_id(1), Some("layer.add"));
    assert_eq!(h.current_state().active_layer, 1);
    assert!(h.is_dirty());
    // Other steps keep their own state.
    h.undo();
    assert_eq!(h.current_state().layers.len(), 1);
    h.mark_saved();
    h.redo();
    h.replace_current(h.current_state().set_active_layer(0).unwrap());
    assert!(
        h.is_dirty(),
        "a different step than the saved one stays dirty"
    );
}
