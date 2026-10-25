//! Process-wide counts of the GPU pipelines this crate has created (M58).
//!
//! Launch must stay lazy (ARCHITECTURE section 9): the canvas view pipeline is
//! made when the first view is attached, and every other pipeline (effect
//! kernels, the mip builder) is made the first time it is used. The app prints
//! these counts with its `OP_READY` line, and `scripts/launch-time.sh` fails
//! when an effect kernel was compiled before the canvas was ready.
use std::sync::atomic::{AtomicU64, Ordering};

static EFFECT: AtomicU64 = AtomicU64::new(0);
static VIEW: AtomicU64 = AtomicU64::new(0);
static MIP: AtomicU64 = AtomicU64::new(0);

/// Pipelines created since the process started, by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineCounts {
    /// Compute pipelines for effect and resampling kernels.
    pub effect: u64,
    /// Canvas view render pipelines (one per attached view).
    pub view: u64,
    /// Mip builder render pipelines (one per view that zoomed below 100%).
    pub mip: u64,
}

/// The counts now.
pub fn pipeline_counts() -> PipelineCounts {
    PipelineCounts {
        effect: EFFECT.load(Ordering::SeqCst),
        view: VIEW.load(Ordering::SeqCst),
        mip: MIP.load(Ordering::SeqCst),
    }
}

pub(crate) fn note_effect() {
    EFFECT.fetch_add(1, Ordering::SeqCst);
}
pub(crate) fn note_view() {
    VIEW.fetch_add(1, Ordering::SeqCst);
}
pub(crate) fn note_mip() {
    MIP.fetch_add(1, Ordering::SeqCst);
}
