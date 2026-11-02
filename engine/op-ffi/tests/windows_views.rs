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

#[test]
fn panel_and_overlay_reject_bad_arguments_without_a_surface() {
    let engine = op_engine_new(ptr::null(), None, ptr::null_mut());
    assert!(!engine.is_null());
    let mut doc = 0;
    assert_eq!(op_doc_new(engine, 1600, 1000, 96.0, 0xffffffff, &mut doc), OpStatus::OP_OK);
    let mut view = 0;
    assert_eq!(op_view_attach_panel(engine, doc, ptr::null_mut(), &mut view), OpStatus::OP_ERR_BAD_ARGUMENT);
    assert_eq!(op_view_attach_panel(engine, doc + 1, 1usize as *mut _, &mut view), OpStatus::OP_ERR_NO_SUCH_DOC);
    assert_eq!(op_view_attach_panel(engine, doc, 1usize as *mut _, ptr::null_mut()), OpStatus::OP_ERR_BAD_ARGUMENT);
    assert_eq!(op_view_set_overlay(engine, 99, ptr::null()), OpStatus::OP_ERR_NO_SUCH_DOC);
    op_engine_free(engine);
}

#[test]
fn overlay_math_uses_document_points_and_dash_offset() {
    let overlay = OpViewOverlay {
        ants: OpOverlayRect { x: 10.0, y: 20.0, width: 40.0, height: 30.0 },
        crop: OpOverlayRect { x: 5.0, y: 5.0, width: 80.0, height: 70.0 },
        caret: OpOverlayRect { x: 15.0, y: 25.0, width: 2.0, height: 12.0 },
        dash_offset: 0.0,
    };
    assert_eq!(overlay_sample(&overlay, 15.0, 25.0), OverlayHit::Caret);
    assert_eq!(overlay_sample(&overlay, 2.0, 2.0), OverlayHit::CropDim);
    assert_eq!(overlay_sample(&overlay, 10.0, 22.0), OverlayHit::AntWhite);
    let moved = OpViewOverlay { dash_offset: 4.0, ..overlay };
    assert_eq!(overlay_sample(&moved, 10.0, 22.0), OverlayHit::AntBlack);
    assert_eq!(overlay_sample(&overlay, 25.0, 35.0), OverlayHit::None);
}

#[test]
fn windows_overlay_shader_compiles_without_a_surface() {
    let (_instance, adapter) = op_gpu::windows_adapter().expect("DX12 adapter");
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("overlay shader test"),
        required_limits: adapter.limits(),
        ..Default::default()
    })).expect("DX12 device");
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _renderer = op_gpu::presenter::ViewRenderer::new(&device, op_gpu::win32::FORMAT);
    let _ = device.poll(wgpu::PollType::Poll);
    assert!(pollster::block_on(device.pop_error_scope()).is_none());
}
