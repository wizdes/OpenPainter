//! M31: the live Magic Wand and Paint Bucket session on the shared preview
//! stack (`preview.begin` / `preview.update` / `preview.commit` /
//! `preview.cancel` with `tool.magic_wand` or `tool.paint_bucket`).
//!
//! paint.net 5 keeps a wand selection or a bucket fill live after the click:
//! the tolerance and the other options can change and the click point can be
//! dragged, and the result is re-made from the original click until Finish
//! (https://www.getpaint.net/doc/latest/MagicWand.html,
//! https://www.getpaint.net/doc/latest/PaintBucket.html). Here the session is
//! a preview: updates merge into the session's parameters and re-run the flood
//! on the unchanged document, so the committed result always equals one plain
//! command with the final parameters.
//!
//! Rules while a session is open, as for the Line and Text sessions: any other
//! command finishes (commits) it first, and `edit.undo` drops it.

/// True for the commands whose preview session these rules cover.
pub(super) fn is_flood(id: &str) -> bool {
    matches!(id, "tool.magic_wand" | "tool.paint_bucket")
}

/// paint.net's History names.
pub(super) fn history_label(id: &str) -> Option<&'static str> {
    match id {
        "tool.magic_wand" => Some("Magic Wand"),
        "tool.paint_bucket" => Some("Paint Bucket"),
        _ => None,
    }
}
