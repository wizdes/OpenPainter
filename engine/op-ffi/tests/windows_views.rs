#![cfg(windows)]

use op_ffi::*;
use std::ptr;

#[test]
fn hwnd_attach_rejects_a_non_window() {
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!engine.is_null());
    let mut doc = 0;
    assert_eq!(
        op_import_pixels(
            engine,
            1,
            1,
            [20u8, 40, 60, 255].as_ptr(),
            4,
            ptr::null(),
            0,
            &mut doc
        ),
        OpStatus::OP_OK,
    );
    let mut view = 0;
    let result = op_view_attach(engine, doc, 1usize as *mut _, &mut view);
    assert_eq!(result, OpStatus::OP_ERR_BAD_ARGUMENT);
    assert_eq!(view, 0);
    op_engine_free(engine);
}
