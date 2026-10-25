//! Undo and redo by switching immutable document snapshots.
use crate::doc::{DocError, DocState, ResidentView};
use thiserror::Error;

struct HistoryStep {
    label: String,
    command_id: String,
    state: DocState,
    identity: u64,
}

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("history index is outside the list")]
    Index,
    #[error(transparent)]
    Document(#[from] DocError),
}
pub type Result<T> = std::result::Result<T, HistoryError>;

pub struct History {
    steps: Vec<HistoryStep>,
    current: usize,
    saved: Option<u64>,
    next_identity: u64,
}
impl History {
    pub fn new(state: DocState) -> Self {
        Self {
            steps: vec![HistoryStep {
                label: "Open".into(),
                command_id: "document.open".into(),
                state,
                identity: 0,
            }],
            current: 0,
            saved: Some(0),
            next_identity: 1,
        }
    }
    /// The number of steps, including step 0 (the opened document).
    pub fn len(&self) -> usize {
        self.steps.len()
    }
    /// Always false: step 0 exists from the start.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
    pub fn label(&self, index: usize) -> Option<&str> {
        self.steps.get(index).map(|step| step.label.as_str())
    }
    pub fn command_id(&self, index: usize) -> Option<&str> {
        self.steps.get(index).map(|step| step.command_id.as_str())
    }
    pub fn state(&self, index: usize) -> Option<&DocState> {
        self.steps.get(index).map(|step| &step.state)
    }
    pub fn current_index(&self) -> usize {
        self.current
    }
    pub fn current_state(&self) -> &DocState {
        &self.steps[self.current].state
    }
    pub fn commit(
        &mut self,
        label: impl Into<String>,
        command_id: impl Into<String>,
        new_state: DocState,
    ) {
        self.steps.truncate(self.current + 1);
        let identity = self.next_identity;
        self.next_identity = self.next_identity.wrapping_add(1);
        self.steps.push(HistoryStep {
            label: label.into(),
            command_id: command_id.into(),
            state: new_state,
            identity,
        });
        self.current = self.steps.len() - 1;
    }
    /// Replaces the current step's state in place: no new step, and the step
    /// keeps its label, command id and saved identity, so the dirty flag does not
    /// change. For changes that are not edits, such as choosing the active layer.
    pub fn replace_current(&mut self, state: DocState) {
        self.steps[self.current].state = state;
    }
    pub fn undo(&mut self) -> bool {
        if self.current == 0 {
            false
        } else {
            self.current -= 1;
            true
        }
    }
    pub fn redo(&mut self) -> bool {
        if self.current + 1 == self.steps.len() {
            false
        } else {
            self.current += 1;
            true
        }
    }
    pub fn jump(&mut self, index: usize) -> Result<()> {
        if index >= self.steps.len() {
            return Err(HistoryError::Index);
        }
        self.current = index;
        Ok(())
    }
    /// Jumps to `index` and pins the viewport tiles of that step. If loading fails, the
    /// current step does not change. The view keeps the history borrowed; read the new
    /// state through `ResidentView::state` until the view is dropped.
    pub fn jump_checked(
        &mut self,
        index: usize,
        rect: (u32, u32, u32, u32),
    ) -> Result<ResidentView<'_>> {
        let step = self.steps.get(index).ok_or(HistoryError::Index)?;
        let view = step.state.ensure_resident(rect.0, rect.1, rect.2, rect.3)?;
        self.current = index;
        Ok(view)
    }
    /// Pins the viewport tiles of the current step until the returned view is dropped.
    pub fn ensure_resident(&self, rect: (u32, u32, u32, u32)) -> Result<ResidentView<'_>> {
        Ok(self
            .current_state()
            .ensure_resident(rect.0, rect.1, rect.2, rect.3)?)
    }
    /// Sets (or with None removes) one metadata entry on every step, in
    /// place: no step is added and the saved step stays the saved one.
    pub fn set_metadata_everywhere(&mut self, key: &str, value: Option<String>) {
        for step in &mut self.steps {
            let map = std::sync::Arc::make_mut(&mut step.state.metadata);
            match &value {
                Some(v) => {
                    map.insert(key.to_owned(), v.clone());
                }
                None => {
                    map.remove(key);
                }
            }
        }
    }
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.steps[self.current].identity);
    }
    /// No step is the saved one, so the document shows as unsaved until the
    /// next save (M57: a document recovered after a crash).
    pub fn mark_unsaved(&mut self) {
        self.saved = None;
    }
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.steps[self.current].identity)
    }
}
