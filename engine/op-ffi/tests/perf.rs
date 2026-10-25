//! M58: `op_gpu_pipeline_counts` reads the process-wide pipeline counts.
use op_ffi::*;

#[test]
fn pipeline_counts_need_no_engine_and_reject_null() {
    assert_eq!(
        op_gpu_pipeline_counts(std::ptr::null_mut()),
        OpStatus::OP_ERR_BAD_ARGUMENT
    );
    let mut c = OpPipelineCounts {
        effect: 99,
        view: 99,
        mip: 99,
    };
    assert_eq!(op_gpu_pipeline_counts(&mut c), OpStatus::OP_OK);
    // Nothing in this test binary has made a pipeline yet.
    assert_eq!(c, OpPipelineCounts::default());
}
