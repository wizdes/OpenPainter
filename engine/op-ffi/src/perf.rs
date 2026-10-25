//! Launch checks (M58): how many GPU pipelines the process has made.
use crate::{OpStatus, guard};

/// GPU pipelines made since the process started, by kind (op-gpu
/// `pipeline_stats`). The app prints these on its `OP_READY` line; launch
/// must make no effect pipeline before the canvas is ready.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpPipelineCounts {
    /// Compute pipelines for effect and resampling kernels.
    pub effect: u64,
    /// Canvas view render pipelines (one per attached view).
    pub view: u64,
    /// Mip builder render pipelines (made at the first draw below 100%).
    pub mip: u64,
}

/// Reads the process-wide pipeline counts. Needs no engine.
#[unsafe(no_mangle)]
pub extern "C" fn op_gpu_pipeline_counts(out: *mut OpPipelineCounts) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let c = op_gpu::pipeline_stats::pipeline_counts();
        // SAFETY: checked non-null; the caller owns the struct for the call.
        unsafe {
            *out = OpPipelineCounts {
                effect: c.effect,
                view: c.view,
                mip: c.mip,
            };
        }
        OpStatus::OP_OK
    })
}
