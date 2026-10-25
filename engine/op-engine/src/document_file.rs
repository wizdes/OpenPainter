//! M19 document lifecycle: File > New, opening and saving `.opaint`, the
//! flattened PNG export, the saved-revision marker and Revert.
//!
//! The saved marker is `op_core::history::History`'s saved step: a save marks
//! the current step, and `doc.info` reports `dirty` whenever the current step
//! is a different one (undo back to the saved step clears it; a new edit after
//! undo cuts the saved step off, so the mark stays until the next save).
use super::{Actor, Engine, Error, EventKind, Message, Result, bad, flatten, internal};
use op_core::doc::DocState;
use op_core::history::History;
use op_core::tiles::{TILE_BYTES, TILE_SIZE, TileCoord, TileData, TileStore};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The formats the engine writes itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    /// The native layered file (all layers, names, opacity, blend modes,
    /// visibility, resolution, selection and profile).
    Opaint,
    /// The flattened image with the document profile as its iCCP chunk.
    Png,
}
impl SaveFormat {
    pub fn from_name(name: &str) -> Result<Self> {
        match name {
            "opaint" => Ok(Self::Opaint),
            "png" => Ok(Self::Png),
            _ => Err(bad(format!("unknown save format {name}"))),
        }
    }
}

pub(crate) enum FileOp {
    Save {
        path: PathBuf,
        format: SaveFormat,
        mark_saved: bool,
    },
    MarkSaved(u64),
    Replace(DocState),
    /// M57: no step is saved (a document recovered after a crash).
    MarkUnsaved,
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn store() -> Result<Arc<TileStore>> {
    Ok(Arc::new(
        TileStore::new(std::env::temp_dir(), 64 * 1024 * 1024).map_err(internal)?,
    ))
}

/// Writes `bytes` to a temporary file next to `path`, syncs it and renames it
/// over `path`, so a failed save never leaves a half-written image.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| bad("the path has no file name"))?;
    let (temp, mut file) = loop {
        let n = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{}.{}.{n}.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(f) => break (temp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(internal(format!("cannot write {}: {e}", path.display()))),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| internal(format!("cannot write {}: {e}", path.display())))
}

impl Engine {
    /// File > New: one layer named "Background" filled with `background`
    /// (straight RGBA8; paint.net's White, Secondary color or Transparent).
    pub fn new_document(
        &self,
        width: u32,
        height: u32,
        dpi: f64,
        background: [u8; 4],
    ) -> Result<u64> {
        if width == 0 || height == 0 || width > 100_000 || height > 100_000 {
            return Err(bad("image size out of range"));
        }
        if (width as u64) * (height as u64) * 4 > 1024 * 1024 * 1024 {
            return Err(bad("image too large"));
        }
        if !dpi.is_finite() || !(1.0..=100_000.0).contains(&dpi) {
            return Err(bad("resolution out of range"));
        }
        let store = store()?;
        let mut state = DocState::new(store.clone(), width, height, dpi).map_err(internal)?;
        if background != [0, 0, 0, 0] {
            let mut tile = vec![0; TILE_BYTES];
            for p in tile.chunks_exact_mut(4) {
                p.copy_from_slice(&background);
            }
            for ty in 0..height.div_ceil(TILE_SIZE as u32) {
                for tx in 0..width.div_ceil(TILE_SIZE as u32) {
                    state = state
                        .write_tile(
                            0,
                            TileCoord {
                                x: tx as i32,
                                y: ty as i32,
                            },
                            TileData::try_from(tile.clone()).map_err(internal)?,
                        )
                        .map_err(internal)?;
                }
            }
        }
        self.spawn_document(state, store)
    }

    /// Opens a `.opaint` file. Tiles go to a store in the temp folder.
    pub fn open_opaint(&self, path: impl AsRef<Path>) -> Result<u64> {
        let store = store()?;
        let loaded = op_formats::opaint::load_opaint_with_store(path.as_ref(), store.clone())
            .map_err(|e| match e {
                op_formats::opaint::Error::Io(e) => internal(e),
                other => bad(other),
            })?;
        self.spawn_document(loaded.doc, store)
    }

    /// Writes the current history step to `path`. With `mark_saved` (Save and
    /// Save As) that step becomes the saved one; Save a Copy passes false.
    pub fn save_document(
        &self,
        id: u64,
        path: impl AsRef<Path>,
        format: SaveFormat,
        mark_saved: bool,
    ) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let result = self.call(id, |r| {
            Message::File(
                FileOp::Save {
                    path,
                    format,
                    mark_saved,
                },
                r,
            )
        })?;
        // M57: the error text for the app (`doc.last_file_error`), and a clean
        // save lets the recovery writer delete its record at once.
        self.note_file_result(id, &result);
        result
    }

    /// M57: marks the document unsaved (no step is the saved one).
    pub fn mark_unsaved(&self, id: u64) -> Result<()> {
        self.call(id, |r| Message::File(FileOp::MarkUnsaved, r))?
    }

    /// Marks the current step saved after the app wrote the file itself
    /// (ImageIO formats). `revision` is the `doc.info` revision the app read
    /// the pixels at; if the document changed since, this returns Busy and the
    /// mark stays.
    pub fn mark_saved(&self, id: u64, revision: u64) -> Result<()> {
        let result = self.call(id, |r| Message::File(FileOp::MarkSaved(revision), r))?;
        self.note_file_result(id, &result);
        result
    }

    /// Revert: `id` takes `source`'s current content as a fresh history (one
    /// step, saved). Both documents stay open; the caller closes `source`.
    pub fn replace_content(&self, id: u64, source: u64) -> Result<()> {
        let state = self.snapshot(source)?.state.clone();
        self.call(id, |r| Message::File(FileOp::Replace(state), r))?
    }
}

impl Actor {
    pub(crate) fn file_op(&mut self, op: FileOp) -> Result<()> {
        match op {
            FileOp::Save {
                path,
                format,
                mark_saved,
            } => {
                let state = self.history.current_state();
                match format {
                    SaveFormat::Opaint => op_formats::opaint::save_opaint(state, &path)
                        .map_err(|e| internal(format!("cannot save {}: {e}", path.display())))?,
                    SaveFormat::Png => {
                        let image = op_formats::png::DecodedImage {
                            width: state.width,
                            height: state.height,
                            pixels: flatten(state)?,
                            icc: state.profile.as_deref().map(<[u8]>::to_vec),
                        };
                        let bytes = op_formats::png::encode_png(&image).map_err(internal)?;
                        write_atomic(&path, &bytes)?;
                    }
                }
                if mark_saved {
                    self.mark_current_saved();
                }
                Ok(())
            }
            FileOp::MarkSaved(revision) => {
                if revision != self.revision {
                    return Err(Error::Busy);
                }
                self.mark_current_saved();
                Ok(())
            }
            FileOp::MarkUnsaved => {
                self.history.mark_unsaved();
                self.publish(None);
                self.event(EventKind::HistoryChanged, 0, json!({"saved":false}));
                Ok(())
            }
            FileOp::Replace(state) => {
                self.preview = None;
                self.stroke = None;
                self.move_session = None;
                self.gradient_session = None;
                self.line_session = None;
                self.shape_session = None;
                self.text_session = None;
                self.history = History::new(state);
                self.revision += 1;
                self.selection_revision += 1;
                self.publish(None);
                self.event(EventKind::DocChanged, 0, json!({"revision":self.revision}));
                self.event(EventKind::HistoryChanged, 0, json!({}));
                self.event(EventKind::LayersChanged, 0, json!({}));
                Ok(())
            }
        }
    }

    fn mark_current_saved(&mut self) {
        let was = self.history.is_dirty();
        self.history.mark_saved();
        // Only the dirty flag changes: a preview or tool session on screen stays.
        let mut view = (*self.published.load_full()).clone();
        view.history.dirty = self.history.is_dirty();
        self.published.store(Arc::new(view));
        if was {
            self.event(EventKind::HistoryChanged, 0, json!({"saved":true}));
        }
    }
}
